# AGENTS.md • Contrato Operativo para Agentes de IA y Desarrolladores

> [!IMPORTANT]
> Este archivo define **únicamente las reglas de ingeniería, flujo Git, validación, despliegue y guardrails técnicos** del proyecto sobre Debian 13 (`x86_64`).
> La arquitectura, tecnologías, seguridad y manual operativo viven en **[docs/](./docs)** (fuente única de verdad).

---

## 1. Identidad del Entorno y Premisa Cero-Supuestos

* **Sistema Operativo Base:** Debian 13 (Trixie) GNU/Linux `x86_64` (instalación mínima por consola o con escritorio).
* **Premisa de Entorno Mínimo y Auto-Aprovisionamiento (OBLIGATORIA):** Toda nueva funcionalidad, servicio, script o módulo debe asumir que el usuario **únicamente** actualizó Debian (`apt update && apt upgrade`) e instaló `curl` y `wget`. **Prohibido** asumir paquetes preinstalados (`smartmontools`, `hdparm`, `btrfs-progs`, `lvm2`, `parted`, `acl`, `samba`, `nginx`, `auditd`, `aide`, `nut`, `sudo`, etc.). Cualquier dependencia requerida debe **auto-instalarse de forma idempotente y desatendida** (`DEBIAN_FRONTEND=noninteractive apt-get install -y`) en [install.sh](./install.sh) (tanto binario precompilado como `--build-from-source`) y en el flujo de actualización (`nas update`), manteniendo degradación segura en tiempo de ejecución si aún falta en nodos previos.
* **Organización / Grupo de Trabajo:** `TEAM-JOFRATO`.
* **Identificadores NetBIOS por Defecto:** `SRV-NAS` (`ARCHIVOS`), `SRV-BKP` (`BACKUP`).
* **Placeholders de Laboratorio:** IP `<host>` (Subred `<red>`), usuario `<usuario>` / `root`, contraseña `<contraseña>` (nunca persistir credenciales reales).

---

## 2. Reglas de Git, CI y Coexistencia Multi-Agente (OBLIGATORIAS)

1. **Commits atómicos 1:1 en español (sin firma en commits, solo tags):**
   * **REGLA ESTRICTA:** debe existir **un commit independiente por cada archivo modificado**. Si cambian 3 archivos, se registran 3 commits separados.
   * Mensajes convencionales en español: `feat:`, `fix:`, `docs:`, `refactor:`, `ci:`.
   * **Política de firma GPG:** no se firman los commits (`commit.gpgsign = false`); únicamente se firman con GPG los *tags* de release (`tag.gpgsign = true`).
2. **Integración Continua sin supresiones:**
   * Cada `push` debe pasar `.github/workflows/ci.yml`. **Prohibido** usar `# shellcheck disable`, `-e` globales en módulos importables o relajar assertions para silenciar advertencias.
3. **Validación local obligatoria previa a `git push` (5 puertas):**
   1. **BATS:** `PATH="$PATH:/usr/sbin" bats --print-output-on-failure tests/*.bats`
   2. **Bash:** `find src scripts packaging -type f -name "*.sh" -print0 | xargs -0 shellcheck -x` y `shellcheck -x install.sh` y `bash -n install.sh`
   3. **JavaScript:** `find crates/nas-web/assets/js -type f -name "*.js" -print0 | xargs -0 -n1 node --check`
   4. **Rust:** `cargo fmt --all --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings` y `cargo test --workspace --locked`
   5. **Empaquetado y paridad:** `bash packaging/verify.sh`, `python3 scripts/parity_extract.py --check` y `python3 scripts/qa_ui_audit.py --static`
4. **Coexistencia multi-agente:**
   * Trabajar en **worktree propio** (`git worktree add`) y con **`CARGO_TARGET_DIR` separado**.
   * **Prohibido** editar en paralelo archivos compartidos de alto tráfico: `Cargo.lock`, `AGENTS.md`, `README.md`, `crates/nas-web/assets/js/app.js`, `crates/nas-web/templates/layout.html`, `crates/nas-web/src/routes/mod.rs`, `crates/nas-cli/src/commands/mod.rs`, `crates/nas-cli/src/main.rs`, `src/core/{deploy,uninstall,updater}.sh`.
   * Ejecutar `git pull --rebase` antes de commitear y **prohibido** reiniciar o detener `nas-web`, `smbd` o `nmbd` desde tests en paralelo.

---

## 3. Protocolo de Despliegue Local y Vigencia Documental

1. **Despliegue local inmediato (`crates/nas-web`):** HTML, CSS y JS están embebidos en el binario (`include_str!` / `include_bytes!`). Editar código fuente no cambia el servicio activo hasta compilar y reiniciar con `sudo ./scripts/deploy_local.sh` (o `--all` para todos los binarios).
2. **Caché de cliente:** `/js/app.js` usa `Cache-Control: public, max-age=300`; recargar navegador con `Ctrl + Shift + R`.
3. **Uso reservado de `install.sh`:** no usar `install.sh --build-from-source` para iteración diaria; queda reservado a aprovisionamiento inicial o reinstalación completa.
4. **Confirmaciones obligatorias:** tras cualquier cambio, preguntar explícitamente al usuario si desea:
   * ejecutar el despliegue local (pidiéndole validar primero que no haya trabajo pendiente de otro desarrollador en ramas o *worktrees*), y
   * posteriormente realizar *merge* a `main` y `git push` (nunca guardar contraseñas `sudo` ni de GPG).
5. **Vigencia documental:** ante cambios en CLI, API/vistas web, `packaging/`, backups o seguridad, revisar y actualizar en el mismo bloque [docs/manual.md](./docs/manual.md), [docs/seguridad.md](./docs/seguridad.md), [docs/arquitectura.md](./docs/arquitectura.md), [docs/lab_y_desarrollo.md](./docs/lab_y_desarrollo.md), [docs/deuda_tecnica.md](./docs/deuda_tecnica.md), [README.md](./README.md) y este `AGENTS.md` (con commits `docs:` atómicos por archivo), retirando advertencias ya resueltas.

---

## 4. Protección para Discos de Grado Consumidor y Resiliencia Eléctrica (OBLIGATORIAS)

1. **Aislamiento del disco del SO (`/` y `/var`):** prohibido alojar escrituras temporales masivas (staging ZIP/cargas, `CARGO_TARGET_DIR`, volcados) sobre el SSD raíz cuando exista almacenamiento secundario; priorizar `NAS_DATA_ROOT/.nas_staging` y mantener cuotas estrictas (`packaging/99-nas-journald.conf` `SystemMaxUse=256M`, `logrotate`, `auditd`).
2. **Cero tormentas recursivas de metadatos ni opciones destructivas de desgaste:** prohibido lanzar `chattr -R -i`, `chmod -R` o `chown -R` indiscriminados sobre árboles históricos de backups (verificar por muestra `lsattr -d` y aplicar `+i` solo al contenedor raíz). Prohibido usar `autodefrag` en Btrfs con snapshots/hardlinks, evitar `commit=2` y `discard=async` continuo (preferir `commit=5,errors=remount-ro` + `fstrim.timer` periódico) y **prohibido** desactivar barreras/journal (`nobarrier`, `data=writeback`).
3. **Límites cgroup estrictos en tareas de mantenimiento:** toda unidad transitoria o pesada (`nas-backup-dedup-migration` vía `systemd-run`, scrubs, reindexados) debe declarar `Nice=19`, `IOSchedulingClass=idle`, `IOWeight=10`, `MemoryHigh`, `MemoryMax` y `TasksMax` acotados. No fijar `vm.vfs_cache_pressure` agresivamente bajo (`< 50–100`; valor vigente `100`).
4. **Arranque y red tolerantes a fallos:** toda entrada secundaria en `/etc/fstab` (`/srv/nas`) debe incluir `nofail,x-systemd.device-timeout=15s`; el despliegue debe enmascarar `sleep.target`, `suspend.target`, `hibernate.target` y `hybrid-sleep.target` y fijar `net.ipv4.conf.*.rp_filter = 2`.
5. **Pre-chequeo S.M.A.R.T. y disciplina de carga:** antes de formatear, extender volúmenes o ejecutar migraciones masivas, comprobar que el volumen no esté en solo lectura (`ro`) y consultar `smartctl -H` (si está disponible) abortando ante `FAILED`. **Prohibido** compilar `--release` o estresar discos en paralelo a backups/migraciones sin `nice -n 10 ionice -c 2 -n 7`.
6. **Infraestructura low-cost y requisito documentado (sin suposiciones físicas):** la plataforma está diseñada para operar sobre hardware común/económico sin asumir SSD con PLP ni UPS presentes; no obstante, para producción con discos de consumo **se documenta como requisito indispensable** operar con UPS/No-Break (`nut`/`apcupsd` cuando aplique), supresor de picos/regulador y recomendar RAID-1 o SSD con PLP (*Power Loss Protection*) para `/`.

---

## 5. Guardrails de Almacenamiento (`/dashboard#storage`) y Paridad UI

* **Pool = volumen:** no se implementa RAID por software ni hot-spare en esta fase; la UI no debe prometer redundancia automática.
* **Confirmaciones destructivas e `is_os_disk()`:** formatear, limpiar o extender exige confirmación textual (`SI-FORMATEAR` / `SI-DESTRUIR-DATOS`) y preserva la protección del disco del SO (`is_os_disk()` / `protected`).
* **Backup binario previo al deploy local:** copiar `/usr/local/bin/nas-web` a `.prev` antes de desplegar y validar post-deploy (`systemctl is-active nas-web` y `curl -fsS http://127.0.0.1:8080/healthz`).
* **Paridad UI:** cualquier cambio en `crates/nas-web/assets/js/app.js` o `layout.html` requiere validar `parity_extract.py --check` y `qa_ui_audit.py --static`.

---

## 6. Mapa Rápido del Repositorio y Documentación

* **Workspace Rust (`crates/*`):** `nas-core`, `nas-exec`, `nas-audit`, `nas-inventory`, `nas-ssh`, `nas-engine` (lógica de sistema, almacenamiento, Samba, usuarios, backups, `deploy/*`, `helpers/*`), `nas-cli` (`nas`), `nas-web` (`axum` + Slate UI embebido), `nas-tui` (`ratatui`), `nasctl` y `nas-dev`.
* **Empaquetado e instalador:** [install.sh](./install.sh), [packaging/](./packaging) (`install.sh`, `nas-web.service`, `nginx-nas-web.conf`, `99-nas-tuning.conf`, `99-nas-journald.conf`, helpers root) y [scripts/deploy_local.sh](./scripts/deploy_local.sh).
* **Shims y pruebas:** [src/](./src) (shims de retrocompatibilidad y utilidades lib), [tests/](./tests) (suites BATS y contrato UI), [.github/workflows/ci.yml](./.github/workflows/ci.yml).
* **Documentación viva (`docs/`):** [arquitectura.md](./docs/arquitectura.md) • [tecnologias.md](./docs/tecnologias.md) • [seguridad.md](./docs/seguridad.md) • [manual.md](./docs/manual.md) • [lab_y_desarrollo.md](./docs/lab_y_desarrollo.md) • [deuda_tecnica.md](./docs/deuda_tecnica.md).

---

## 7. Owners por Crate / Módulo

| Crate / Módulo | Responsabilidad | Owner |
| :--- | :--- | :--- |
| `nas-web` | Panel web, assets embebidos, paridad UI | `@frontend` |
| `nas-engine` (`storage`) | Discos, LVM, Btrfs, formatos, salud SMART/`ro` | `@storage` |
| `nas-engine` (`backups`) | `rsync`, snapshots, retención, deduplicación | `@backups` |
| `nas-engine` (`users`/`samba`) | Usuarios, grupos, ACLs, Active Directory | `@identity` |
| `nas-engine` (`deploy`) | Aprovisionamiento, `fstab`, sysctl, helpers | `@infra` |
| `nas-cli` / `nasctl` / `nas-dev` | CLI `nas`, ejecución multi-host y sync dev | `@cli` |
| `nas-tui` | Asistente visual de terminal | `@tui` |
| `nas-exec`, `nas-audit`, `nas-core` | Ejecución sin shell, auditoría SHA-256, `ResourceBudget` | `@core` |
| Bash shims (`src/core/*.sh`) | Retrocompatibilidad y tests BATS | `@infra` |
| Paridad/CI (`scripts/`, `tests/`) | Contrato UI, empaquetado y pipelines | `@qa` |
