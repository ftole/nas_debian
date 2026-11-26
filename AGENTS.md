# AGENTS.md • Contrato Operativo para Agentes de IA y Desarrolladores

> [!IMPORTANT]
> Reglas obligatorias de ingeniería, seguridad, Git, CI, recursos y despliegue sobre **Debian 13 (Trixie) `x86_64`**.
> La arquitectura, tecnologías, seguridad y manual operativo viven en **[docs/](./docs)** (fuente única de verdad).

---

## 1. Entorno Base y Principios Inviolables

* **Cero-Supuestos y Auto-Aprovisionamiento:** asumir que el usuario solo ejecutó `apt update && apt upgrade` e instaló `curl` y `wget`. Prohibido asumir paquetes preinstalados (`smartmontools`, `hdparm`, `btrfs-progs`, `lvm2`, `parted`, `acl`, `samba`, `nginx`, `auditd`, `aide`, `nut`, `sudo`, etc.). Toda dependencia debe auto-instalarse de forma idempotente y desatendida (`DEBIAN_FRONTEND=noninteractive apt-get install -y`) en [install.sh](./install.sh) y en `nas update`, manteniendo degradación segura en tiempo de ejecución si aún falta.
* **Remediación sin Mutilación Funcional:** mitigar brechas endureciendo controles sin borrar, ocultar ni degradar funcionalidades legítimas, demostrando con pruebas automatizadas tanto el bloqueo del abuso como el uso legítimo.
* **Identidad y Placeholders:** grupo `TEAM-JOFRATO`; NetBIOS `SRV-NAS` (`ARCHIVOS`), `SRV-BKP` (`BACKUP`); placeholders `<host>`, `<red>`, `<usuario>`, `<contraseña>` (nunca persistir credenciales reales).
* **Acceso Remoto Intocable:** prohibido alterar autenticación SSH o reglas UFW/anti-lockout salvo instrucción explícita del usuario.

---

## 2. Seguridad, Código Rust / Web y Contratos

1. **Helpers `root`, Secretos y Symlinks:**
   * Toda ejecución externa pasa por `nas-exec` (`CommandSpec` con `argv` explícito y `.timeout(...)`); **prohibido** `sh -c` o interpolación de shell.
   * Contraseñas, tokens y claves viajan únicamente por `stdin` (`SecretInput` + `Runner::with_secret`); nunca en `argv`, entorno ni logs.
   * Todo helper `root` (`nas helper *`) debe validar rutas antes y después de `canonicalize`, rechazar symlinks en ancestros y contenedores internos (`.trash`, `.meta`, `.nas_staging`), crear archivos con `O_EXCL` (`create_new(true)`) y **nunca** invocar `chattr`/`lsattr`/`chmod`/`chown` sobre symlinks.
   * Escrituras en `/etc/samba/smb.conf`, `/etc/sudoers.d/*`, `/etc/nginx/*`, `/etc/cron.d/*`, `/etc/fstab` o runners: escribir temporal en el mismo FS (`O_EXCL`), validar sintaxis (`testparm -s`, `visudo -cf`, `nginx -t`, `bash -n`) y reemplazar atómicamente (`rename`).
2. **Backend Rust (`crates/*`):**
   * `#![deny(missing_docs)]` en `nas-engine` (documentar con `///` todo símbolo/campo `pub`).
   * **Cero** `unwrap()`/`expect()`/`panic!` fuera de `#[cfg(test)]`; propagar `NasResult<T>`.
   * I/O bloqueante en `nas-web` siempre vía `state.run_system_blocking(...)` (nunca bloquear Tokio).
   * Toda ruta mutante (`POST`/`PUT`/`DELETE`) exige extractor de rol explícito (`AuthenticatedUser`/`AdminUser`/`SuperAdminUser`), CSRF, auditoría (`SUCCESS`/`FAILED` sin secretos) y *rate-limit* acotado en memoria si valida credenciales o dispara I/O costoso.
   * **Austeridad de dependencias:** prohibido añadir crates a `Cargo.toml` si puede resolverse con `std` o `[workspace.dependencies]`.
3. **Frontend 100% Offline, XSS-Safe y Paridad UI:**
   * **Prohibido** usar CDNs externos; librerías locales únicamente en `crates/nas-web/assets/vendor/` respetando CSP.
   * **Prohibido** inyectar datos dinámicos en `innerHTML` sin `escapeHtml()` (preferir `textContent`) o usar `eval`/`new Function`.
   * Cambios en `crates/nas-web/assets/js/**/*.js` o `crates/nas-web/templates/*.html` requieren regenerar paridad (`python3 scripts/parity_extract.py --write`) y validar `--check` y `qa_ui_audit.py --static`.
4. **Retrocompatibilidad de Contratos:** cambios en JSON (`/api/*`, `nas --json`), SQLite (`nas.sqlite`) o metadatos (`.trash/.meta/*`, `current_version.json`) deben ser aditivos (`#[serde(default)]`, migraciones idempotentes) sin romper nodos ni clientes previos.
5. **Pruebas Seguras en Servidor Activo:** prohibido que `cargo test` o `bats` muten `/srv/nas`, `/etc/*`, `/var/lib/nas`, reinicien servicios reales (`nas-web`, `smbd`, `nmbd`) o dependan de internet. Todo directorio temporal de test debe limpiarse con un *guard* RAII (`Drop`).

---

## 3. Protección de Discos de Consumo, RAM (`tmpfs`) y Almacenamiento

1. **Aislamiento de `/`, `/var` y `/tmp` (`tmpfs`):** en Debian 13 `/tmp` reside en RAM/swap. **Prohibido** alojar `CARGO_TARGET_DIR`, cachés de agentes (`opencode`, `agy`, `bun`), staging ZIP o volcados en `/` o `/tmp`; priorizar almacenamiento secundario (`NAS_DATA_ROOT/.nas_staging`, `/srv/...`) y cuotas estrictas (`packaging/99-nas-journald.conf` `SystemMaxUse=256M`, `logrotate`, `auditd`). Revisar `free -h` y `df -h /tmp` antes de compilar `--release`.
2. **Cero Tormentas de Metadatos ni Alteración de Permisos:** prohibido lanzar `chattr -R -i`, `chmod -R`, `chown -R` o `setfacl` masivos sobre recursos compartidos o árboles de respaldo (en borrado/papelera privilegiada usar `rename` O(1) y *guards* puntuales `lsattr -d`/Btrfs `ro` con `Drop`). Prohibido `autodefrag` en Btrfs con snapshots/hardlinks, `commit=2`, `discard=async` continuo, `nobarrier` y `data=writeback` (usar `commit=5,errors=remount-ro` + `fstrim.timer`).
3. **Respaldos y Rutas Protegidas (`/dashboard#files`):** carpetas raíz `BACKUPS_HISTORICOS`, `BACKUPS_PERMANENTES`, más `LOGS_BACKUP`, `.nas_staging`, `.tmp_zip`, `.trash` y `unidades/<id>` siempre bloqueadas; solo administradores con credenciales OS (`root`/`sudo`) pueden depurar subárboles internos de `BACKUPS_*` sin alterar permisos/ACLs.
4. **Almacenamiento (`/dashboard#storage`):** soporta Btrfs (`single`/`raid1`) y LVM/ext4 (sin `mdadm` ni *hot-spare* automático). Formatear, limpiar o extender exige confirmación textual (`SI-FORMATEAR`/`SI-DESTRUIR-DATOS`), preserva `is_os_disk()` y comprueba estado `ro` + `smartctl -H` (abortando ante `FAILED`).
5. **Límites `cgroup`, Arranque y Carga:** tareas pesadas (`systemd-run`, scrubs, deduplicación) declaran `Nice=19`, `IOSchedulingClass=idle`, `IOWeight=10`, `MemoryHigh`, `MemoryMax` y `TasksMax`; mantener `vm.vfs_cache_pressure=100`, `net.ipv4.conf.*.rp_filter=2`, `nofail,x-systemd.device-timeout=15s` en `/etc/fstab` secundario y enmascarar `sleep`/`suspend`/`hibernate`/`hybrid-sleep`. Compilar `--release` con `nice -n 10 ionice -c 2 -n 7` y `CARGO_BUILD_JOBS` adaptativo. No asumir SSD PLP ni UPS en código, pero documentarlos como requisito en producción.

---

## 4. Git, Multi-Agente, 5 Puertas CI y Releases

1. **Pre-flight y Coexistencia Multi-Agente:** antes de editar, revisar `git status`, `git branch -vv` y `git worktree list`. Trabajar en **worktree/rama propia** con `CARGO_TARGET_DIR` separado (fuera de `/tmp`). Prohibido editar en paralelo archivos de alto tráfico (`Cargo.toml`, `Cargo.lock`, `AGENTS.md`, `README.md`, `tests/parity/*`, `crates/nas-web/templates/layout.html`, `crates/nas-web/assets/js/app.js`, `crates/nas-web/src/routes/mod.rs`, `crates/nas-cli/src/main.rs`, `crates/nas-cli/src/helper/dispatch.rs`, `src/core/{deploy,uninstall,updater}.sh`) y prohibido hacer refactors cosméticos fuera del alcance pedido.
2. **Commits Atómicos 1:1 en Español (sin firma GPG):** un commit independiente por cada archivo modificado (`git -c commit.gpgsign=false commit -m "<tipo>: ..."`) con prefijos `feat:`, `fix:`, `refactor:`, `test:`, `docs:`, `ci:`, `chore:`. Ejecutar `git fetch origin && git rebase origin/main` antes del merge.
3. **5 Puertas CI Obligatorias (alineadas a `.github/workflows/ci.yml`; prohibido `# shellcheck disable`, `-e` globales en módulos importables o relajar assertions):**
   1. **Bash & Seguridad:** `find src scripts packaging -type f -name "*.sh" -print0 | xargs -0 -n1 bash -n && bash -n install.sh`, `find src scripts packaging -type f -name "*.sh" -print0 | xargs -0 shellcheck -x && shellcheck -x install.sh packaging/nas-{accounts,backup-manage,samba-apply,share-acl,share-dir,storage-apply,terminal}` y cero `StrictHostKeyChecking=no` en `src`, `install.sh`, `scripts`.
   2. **BATS:** `PATH="$PATH:/usr/sbin" bats --print-output-on-failure tests/*.bats`
   3. **JavaScript:** `find crates/nas-web/assets/js -type f -name "*.js" -print0 | xargs -0 -n1 node --check` *(si `node` falta en el host, reportarlo explícitamente; corre en CI)*.
   4. **Rust:** `cargo fmt --all --check && cargo clippy --workspace --all-targets --locked -- -D warnings && cargo test --workspace --locked` *(más `cargo build --workspace --release --locked` si cambian crates/dependencias)*.
   5. **Empaquetado y Paridad:** `bash packaging/verify.sh && python3 scripts/parity_extract.py --check && python3 scripts/qa_ui_audit.py --static`
4. **Releases (`vX.Y.Z`):** solo los tags llevan firma GPG (`git tag -s vX.Y.Z`). Un release requiere sincronizar `Cargo.toml` + `Cargo.lock` (commits 1:1 sin firma); subir el tag dispara `.github/workflows/release.yml` (firma `Ed25519` vía `NAS_RELEASE_KEY`). **Prohibido** crear o subir tags sin orden explícita del usuario.

---

## 5. Despliegue Local, Documentación y Cierre (*Definition of Done*)

1. **Despliegue Local:** los assets web van embebidos (`include_str!`/`include_bytes!`); usar siempre `sudo ./scripts/deploy_local.sh` (o `--all`), que respalda `.prev`, reinicia `nas-web` y valida `/healthz`. Recargar navegador con `Ctrl + Shift + R` (`Cache-Control: max-age=300`). Reservar `install.sh --build-from-source` a instalación inicial o reinstalación.
2. **Vigencia Documental Dirigida:** actualizar únicamente los documentos de [docs/](./docs) afectados (commits `docs:` 1:1): CLI/API/UI → [manual.md](./docs/manual.md); auth/sudoers/hardening → [seguridad.md](./docs/seguridad.md); crates/módulos → [arquitectura.md](./docs/arquitectura.md); CI/desarrollo/releases → [lab_y_desarrollo.md](./docs/lab_y_desarrollo.md); pendientes resueltos → [deuda_tecnica.md](./docs/deuda_tecnica.md).
3. **Confirmaciones Obligatorias y Cierre:** una tarea solo está cerrada con pruebas positivas + negativas, las 5 puertas en verde y `git status` limpio. Antes de ejecutar `deploy_local.sh`, hacer *merge* a `main` o `git push`, pedir confirmación explícita al usuario (validando que no haya trabajo pendiente en otros *worktrees* y sin almacenar jamás contraseñas `sudo`/GPG).

---

## 6. Referencias Clave

* **Crates:** `crates/{nas-core,nas-exec,nas-audit,nas-inventory,nas-ssh,nas-engine,nas-cli,nas-web,nas-tui,nasctl,nas-dev}`
* **Empaquetado / CI:** [install.sh](./install.sh) • [packaging/](./packaging) • [scripts/deploy_local.sh](./scripts/deploy_local.sh) • [.github/workflows/ci.yml](./.github/workflows/ci.yml) • [.github/workflows/release.yml](./.github/workflows/release.yml)
* **Docs:** [arquitectura.md](./docs/arquitectura.md) • [tecnologias.md](./docs/tecnologias.md) • [seguridad.md](./docs/seguridad.md) • [manual.md](./docs/manual.md) • [lab_y_desarrollo.md](./docs/lab_y_desarrollo.md) • [deuda_tecnica.md](./docs/deuda_tecnica.md)
