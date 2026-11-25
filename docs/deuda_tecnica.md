# Deuda Técnica, Bugs Latentes y Plan de Remediación (`v0.1.13+`)

> [!IMPORTANT]
> **Auditoría código por código del workspace completo** (11 crates Rust, frontend HTML/JS, `packaging/`, `install.sh`, `src/`, `scripts/`, `tests/` y `.github/workflows/`) actualizada tras el cierre de las **4 fases de remediación** sobre `v0.1.13`.
> Este documento actúa como **registro vivo de deuda técnica**, complementando la línea base histórica [docs/auditoria.md](./auditoria.md) (`0f7a272`).

---

## 1. Resumen Ejecutivo e Inventario Auditado

| Área Inspeccionada | Volumen Aproximado | Estado General |
| :--- | :--- | :--- |
| **Crates Base y Orquestación** (`nas-core`, `nas-exec`, `nas-audit`, `nas-inventory`, `nas-ssh`, `nas-tui`, `nasctl`, `nas-dev`, `nas-cli`) | 39 archivos (~10,200 líneas) | Sólido (`#![forbid(unsafe_code)]`, `CommandSpec` sin shell, `ResourceBudget`, auditoría O(1) en cola, contratos JSON/exit codes consistentes, TTY `Drop` en `nas-tui`, `nas-dev sync` con `--strip-components=1`). |
| **Motor del Sistema** (`crates/nas-engine/src/**`) | 55 archivos (~20,400 líneas) | Endurecido en seguridad (`backup-manage`, `restore`, `share`, `domain`, `is_os_disk()` Btrfs/LVM/RAID), retención Btrfs RO y timeouts `mkfs`/permanentes 600–900s; `backup_dedup.rs` pendiente de modularización futura. |
| **Panel Web y Frontend** (`crates/nas-web/src/**`, `templates/`, `assets/js/**`) | Backend `axum` + `layout.html` + 28 JS | Auth/CSRF/PBKDF2 robusto; ZIP64 + descarga streaming, XSS papelera y `unidades/<id>` cerrados, `files_upload` sin orden/TOCTOU, preview SVG/`.docx` aislado y panel de seguridad en vivo (`GET /api/security/status`). |
| **Empaquetado, Bash Legacy, Scripts y CI** (`install.sh`, `packaging/`, `src/`, `scripts/`, `tests/`, `.github/workflows/`) | 3 shims Bash + scripts Python + 8 suites BATS | `packaging/` alineado vía `include_str!`, dependencias Debian 13 mínimo completas, exclusiones AIDE `!/...` verificadas, `ci.yml` con `deploy_shim.bats` y `shellcheck` en wrappers, y `release.yml` con tests + `release.pub` previo al tarball. |

### Conteo de Hallazgos Activos
- **Críticos:** `0` activos (`DT-C01`–`DT-C07` y `C-01`–`C-03` resueltos)
- **Altos:** `0` activos (`DT-A01`–`DT-A20` resueltos; `DT-A10` acota RAM en `backup_dedup.rs`; `DT-A21` reducido a bloques BATS retrocompatibles)
- **Medios:** `1` parcial (`DT-M01` duplicación residual de variantes `is_os_disk` y dos motores `tasks/runner.rs` vs `backup_spec/render.rs`; `DT-M02`–`DT-M15` resueltos)
- **Bajos / Deuda Estructural:** `5` (`DT-B01` monolitos restantes, `DT-B02`–`DT-B03` aliases/código muerto menor, `DT-B05` nomenclatura `Arquivos`, `DT-B07`–`DT-B08` cobertura JS/E2E y CLI)

---

## 2. Hallazgos de Severidad CRÍTICA

| ID | Título | Ubicación Verificada | Riesgo Real |
| :--- | :--- | :--- | :--- |
| **DT-C01** | **[RESUELTO]** `FileRoot::resolve_dir` bloquea rutas protegidas y `nas-web.service` retira `AmbientCapabilities` | `crates/nas-web/src/files.rs`, `routes/files/mutations.rs`, `packaging/nas-web.service` | `FileRoot::resolve_dir` e `is_protected_path` bloquean subidas/escrituras a `.trash`, `.nas_staging`, `LOGS_BACKUP`, `BACKUPS_*` y `unidades/<id>/...`, y `nas-web.service` opera con `AmbientCapabilities=` vacío. |
| **DT-C02** | **[RESUELTO]** `install.sh` exige manifiesto firmado, valida SHA-256 + Ed25519 con `openssl` y reporta `ed25519_verified` verídico | `install.sh` | Exige descarga del manifiesto `.json`, valida `SHA-256` y firma `Ed25519` vía `openssl pkeyutl -rawin` antes de descomprimir y marca `ed25519_verified: false` en `--build-from-source`. |
| **DT-C03** | **[RESUELTO]** ZIP64 (`large_file(true)`) y descarga directa por token en streaming sin `res.blob()` en RAM | `crates/nas-web/src/files/zip.rs`, `routes/files.rs`, `assets/js/modules/files/actions.js` | `SimpleFileOptions` habilita `.large_file(true)` para >4 GiB y `actions.js` descarga el ZIP preparado mediante `/api/files/zip?token=...` sin almacenar el archivo completo en memoria del navegador. |
| **DT-C04** | **[RESUELTO]** `cleanup()` en runners de backup ahora usa `umount \|\| umount -f \|\| umount -l` | `crates/nas-engine/src/backup_spec/render.rs:113-115` | Evita bloqueos en `D-state` reteniendo los `flock` (FD 8 y 9) si el servidor remoto Windows cae durante `rsync`. |
| **DT-C05** | **[RESUELTO]** `run_backup_task` aplica límites cgroup `ResourceBudget` y desacopla el fallback sin systemd | `crates/nas-engine/src/helpers/backup/control.rs` | La rama `systemd-run` aplica límites cgroup y propaga errores; el fallback sin systemd inicia el proceso desacoplado (`Stdio::null`) sin `timeout(10s)` bloqueante y `stop_backup_task` evita colisión de prefijo. |
| **DT-C06** | **[RESUELTO]** `purge_recycle_in` y `nas-recycle-purge` podan árboles de backups y logs | `crates/nas-engine/src/helpers/share.rs`, `deploy/artifacts.rs` | Tanto Rust (`recycle_find_args`) como el cron fallback usan `-prune` sobre `BACKUPS_HISTORICOS`, `BACKUPS_PERMANENTES`, `LOGS_BACKUP` y `.nas_staging`, purgando únicamente `.recycle` de shares y unidades adicionales. |
| **DT-C07** | **[RESUELTO]** `nas-dev onboard` acota `sudoers` sin `rm`/`mv`/`tar`/`rsync` y `sync` crea directorios antes de subir | `crates/nas-dev/src/remote.rs`, `crates/nas-dev/src/sync.rs` | `onboard` entrega propiedad de `{web}` y `{state}/dev-sync` al usuario de desarrollo y restringe `sudoers` a `nginx -t`, `test` acotado y `systemctl`; `sync` crea directorios antes de `scp` y limpia su workdir temporal. |

---

## 3. Hallazgos de Severidad ALTA

### 3.1 Seguridad, Credenciales y Validación de Entradas
- **`DT-A01` [RESUELTO] — `test_ssh_conn` restringe rutas de clave a `/etc/backup-credentials/` y redacta secretos:** `crates/nas-engine/src/helpers/backup/conn.rs` solo acepta rutas canónicas dentro de `/etc/backup-credentials/` o contenido PEM validado y aplica `Runner::with_secret` en SSH y CIFS.
- **`DT-A02` [RESUELTO] — `BackupSpec` restringe `KEY_FILE` a `/etc/backup-credentials/` y `render.rs` lo entrecomilla:** `crates/nas-engine/src/backup_spec.rs` y `backup_spec/render.rs` bloquean rutas arbitrarias/espacios/inyección `-o` y entrecomillan `'$KEY_FILE'`.
- **`DT-A03` [RESUELTO] — Temporales en `/tmp` creados con `O_EXCL` (`create_new(true)`) y modo `0600`/`0700`:** `deploy/web.rs`, `deploy/samba/wsdd2.rs`, `helpers/backup/conn.rs`, `nas-dev/src/sync.rs` y `commands/deploy/credentials.rs` usan `write_unique_temp_file` o `DirBuilder` único con limpieza al salir.
- **`DT-A04` [RESUELTO] — `restore_backup` canonicaliza origen/destino, adquiere `TaskLock` y soporta claves SSH Ed25519:** `crates/nas-engine/src/helpers/backup/restore.rs` exige origen real dentro de `BACKUPS_*`, verifica symlinks antes de crear destinos, adquiere `/var/lock/backup_<id>.lock`, usa timeout configurable (6h por defecto) y soporta llaves Ed25519 además de `sshpass`.
- **`DT-A05` [RESUELTO] — `update/remote.rs` valida `repo`, `tag_name`, URL HTTPS y coincidencia de versión:** `crates/nas-engine/src/update/remote.rs`.
- **`DT-A06` [RESUELTO] — `nas-ssh::validate_private_key` inspecciona cabecera binaria `openssh-key-v1` y `OpenSsh` añade `--`:** `crates/nas-ssh/src/lib.rs` detecta cifrado `bcrypt`/PKCS#8 sin falsos positivos en comentarios y pasa `--` antes de destino en `ssh`/`scp`.
- **`DT-A07` [RESUELTO] — `nas-cli helper` acota `stdin`, propaga fallos de `systemd-run` y exige `stdin` para contraseñas:** `crates/nas-cli/src/helper/dispatch.rs` acota lectura de `stdin` (1 MiB / 8 KiB) con error explícito, evita truncamiento de salida >255, no oculta errores de `systemd-run` y exige `stdin` (`-`) en `CreateUser`/`UpdateUser`.

### 3.2 Almacenamiento, Resiliencia ante Apagones y Migración Deduplicada
- **`DT-A08` [RESUELTO] — `deploy/storage.rs::update_fstab` y `helpers/storage/units.rs` ahora delegan en `deploy/artifacts.rs::update_fstab`:** ambos aplican `fstab_options_for_target` (`nofail,x-systemd.device-timeout=15s`, retirando `autodefrag`/`discard=async`) con temporales `O_EXCL` (`write_unique_temp_file`) y respaldo `.bak.<epoch>`, además de saneamiento bajo demanda vía `sanitize_fstab` (`/api/storage/fstab-sanitize`).
- **`DT-A09` [RESUELTO] — S.M.A.R.T. y write-cache bajo `nas-web` con respaldo `sudo` acotado:** `crates/nas-engine/src/storage.rs` (`check_block_health`, `inspect_write_cache`) limpian `[/@subvol]` y, cuando corren sin root, consultan vía `/usr/local/bin/nas helper storage-apply smart-health|write-cache-status` (`SudoMode::Passwordless`) autorizado en `sudoers`.
- **`DT-A10` [PARCIAL] — `backup_dedup.rs` ya protege `ro=true` / `+i` con guarda RAII y recuperación tras apagón:** `ContainerProtectionGuard` y `.nas_dedup_guard.json` (`recover_interrupted_dedup_guard`) restauran `ro=true` y `chattr +i` ante fallo, `Drop` o corte eléctrico previo; queda pendiente la cota de cubos cuando `divisor >= 4096` en `collect_files_in_container_partitioned` (`969-990`).
- **`DT-A11` [RESUELTO] — Eliminado desgaste recursivo y `FULL_*` huérfanos en congelación:** `helpers/backup.rs` y `helpers/backup/permanent.rs` aplican `chown` no recursivo al contenedor, `dir_stats` acota `seen` en memoria (`250_000` inodos), `delete_permanent_backup` y `backup_spec/render.rs` retiran `chattr -i` del contenedor primero (con fallback recursivo solo si falla en legados) y la congelación prepara en `.freezing_*` antes de renombrar a `FULL_*`.
- **`DT-A12` [RESUELTO] — `nas-audit` endurecido frente a apagones y permisos:** `crates/nas-audit/src/lib.rs` ejecuta `sync_data()` en cada `append()`, recorta de forma segura solo una última línea truncada antes de sellar (`trim_truncated_tail_unlocked` / `recover_truncated_tail`), reporta explícitamente el truncamiento eléctrico en `verify()` y falla de inmediato ante `PermissionDenied` en `AuditLockGuard`.

### 3.3 Bugs Funcionales en Panel Web, CLI y Orquestación
- **`DT-A13` [RESUELTO] — Papelera y explorador completos en unidades adicionales (`unidades/<id>`):** `crates/nas-web/src/routes/files.rs::file_root_for` y `assets/js/modules/files/trash.js` propagan `root` en todas las operaciones de listar, restaurar, borrar y vaciar papelera.
- **`DT-A14` [RESUELTO] — Tarjetas `superadmin` y tokens SSR en `layout.html`:** `views.rs` puebla `active_*` y `asset_version_stamp()` válido, `layout.html` limpia tokens PHP residuales y `app.js::applyRoleUi()` controla `.role-card[data-role="superadmin|admin"]`.
- **`DT-A15` [RESUELTO] — Subida multipart independiente del orden de campos y sin TOCTOU:** `routes/files/mutations.rs` resuelve el destino tras recibir los campos, usa `StagedCleanupGuard` y promoción exclusiva `hard_link`/`O_EXCL`.
- **`DT-A16` [RESUELTO] — Papelera (`trash`) endurecida contra XSS, truncamiento e IDs:** `trash.js` escapa `data-id` y conserva el ID completo como cadena; `files/trash.rs` valida rutas protegidas al restaurar.
- **`DT-A17` [RESUELTO] — Vista previa SVG saneada y visor `.docx` en `iframe sandbox`:** `assets/js/modules/files/preview.js` sanea SVG vía `DOMParser` y aísla `.docx` en `<iframe sandbox="">` sin debilitar `/api/files/raw`.
- **`DT-A18` [RESUELTO] — `user_update` exige contraseña al activar acceso web y lee `/etc/nas/superadmin`:** `crates/nas-web/src/routes/users.rs`.
- **`DT-A19` [RESUELTO] — `nas-cli deploy` y `nas-cli update` con detección completa y SemVer:** `commands/deploy.rs` valida `has_critical_metadata` y propaga errores; `commands/update.rs` compara SemVer contra versión desplegada y exige pares `--manifest`/`--artifact`.
- **`DT-A20` [RESUELTO] — `nas-inventory::Inventory::resolve` valida rol, entorno, puerto y usuario explícito:** `crates/nas-inventory/src/lib.rs`.
- **`DT-A21` [PARCIAL] — `src/core/uninstall.sh` retiró `80-udisks2-hide-os.rules`;** se conservan los bloques de referencia de `deploy.sh`/`updater.sh` requeridos por las suites BATS vigentes.

---

## 4. Hallazgos de Severidad MEDIA

- **`DT-M01` [PARCIAL] — Duplicación crítica entre módulos de sistema:**
  - *Resuelto:* validadores de dispositivo, nombres de usuario/grupo/share/unidad, cálculo `parent_disk`/`first_partition_path` y `RESERVED_SYSTEM_USERS` centralizados en `crates/nas-engine/src/validation.rs` (con `OnceLock<Regex>`); `nas-cli` (`services`, `diag`, `storage`) delega directamente en `nas_engine`.
  - *Pendiente:* `is_os_disk` (4 variantes), `update_fstab` (3 variantes), parsers de `df -Pk`/IP primaria, dos motores de ejecución de backups divergentes (`tasks/runner.rs` vs `backup_spec/render.rs`) y 3 parsers distintos de `backup_<id>.sh`.
- **`DT-M02` [RESUELTO] — `tasks/runner.rs` limpia `.inprogress_*` y resuelve pool sin `[subvol]`:** `crates/nas-engine/src/tasks/runner.rs` purga `.inprogress_*` tanto al inicio (`clean_orphan_inprogress_dirs`) como al fallar (`remove_dir_all(task.staging_path(now))`), añade `umount -f`/`-l` ante caída CIFS y recorta `[subvol]` en `destination_pool_key`.
- **`DT-M03` [RESUELTO] — `nas-exec::command.rs` combina `SudoMode::WithPassword` + `stdin_data` y evita bloqueos con nietos en timeout:** `crates/nas-exec/src/command.rs` y `redact.rs` ordenan secretos por longitud descendente, redactan `CommandOutput::command` y acotan la espera de pipes tras timeout.
- **`DT-M04` [RESUELTO] — SQLite en `db.rs` ordena `CREATE TABLE` antes de `ALTER TABLE web_audit`:** `crates/nas-web/src/db.rs`.
- **`DT-M05` [RESUELTO] — Endpoint `/api/terminal/history` exige `SuperAdminUser`:** `crates/nas-web/src/routes/terminal.rs:295`.
- **`DT-M06` [RESUELTO] — AD (`ou`/`user`/`password`), ACL modal y rollback `smb.conf` corregidos:** `routes/domain.rs`, `users/permissions.js`, `routes/shares/access.rs`.
- **`DT-M07` [RESUELTO] — Caché no bloqueante en almacenamiento/backups:** `legacy/storage.rs` refresca en segundo plano sin retener `system_ops`.
- **`DT-M08` [RESUELTO] — Escritura atómica y rollback de `smb.conf` cubiertos.**
- **`DT-M09` [RESUELTO] — `helpers/terminal.rs` verifica códigos de salida y ejecuta `tmux has-session` como el usuario propietario:** `crates/nas-engine/src/helpers/terminal.rs`.
- **`DT-M10` [RESUELTO] — `nas-tui` restaura TTY vía `Drop`, usa `NasPaths::from_env` y no cierra al tabular sobre `Quit`:** `crates/nas-tui/src/{main.rs,app.rs}`.
- **`DT-M11` [RESUELTO] — Contratos `--json` consistentes (`ApiResponse::err`) y recuperación ante mutex poison en `nasctl`:** `nas-core/src/contract.rs`, `nas-cli`, `nasctl`, `nas-dev`.
- **`DT-M12` [RESUELTO] — `btrfs scrub` y `fstrim` para volúmenes adicionales en `/srv/nas/unidades/*`:** `helpers/storage/{pools,units,maintenance}.rs`.
- **`DT-M13` [RESUELTO] — Healthcheck `update` valida código HTTP exacto (`200`/`302`):** `crates/nas-cli/src/commands/update/rollback.rs`.
- **`DT-M14` [RESUELTO] — `ci.yml` ejecuta `deploy_shim.bats` y `shellcheck` en wrappers `packaging/nas-*`; `qa_live_runner.py` usa `/api/users/delete`.**
- **`DT-M15` [RESUELTO] — Runners usan `printf -v ... '%(%s)T'` sin subprocesos y `backup_logs` lee solo los últimos 256 KiB:** `backup_spec/render.rs`, `helpers/backup/control.rs`.

---

## 5. Severidad BAJA, Monolitos y Código Legado/Muerto

### `DT-B01` — Archivos sobredimensionados (>450 líneas) que requieren modularización
- `crates/nas-web/templates/layout.html` (**3,359** líneas) y `assets/js/app.js` (**818** líneas)
- `crates/nas-engine/src/backup_dedup.rs` (**2,889** líneas; `deduplicate_task_inner` mide 448 líneas)
- `crates/nas-core/src/resources.rs` (**867** líneas)
- `crates/nas-engine/src/backup_spec.rs` (**685**), `deploy/samba.rs` (**514**), `deploy/mod.rs` (**509**), `deploy/artifacts.rs` (**502**), `deploy/storage.rs` (**493**), `tasks/runner.rs` (**481**), `tasks.rs` (**472**), `backup_spec/render.rs` (**465**), `storage_ops.rs` (**461**) *(ya modularizados en submódulos: `security.rs`, `storage.rs`, `helpers/storage.rs`, `update.rs`, `helpers/backup.rs` y `helpers/accounts.rs`)*
- `crates/nas-cli/src/main.rs` (**578**), `helper/dispatch.rs` (**537**), `commands/deploy.rs` (**489**) *(ya modularizado: `commands/update.rs`)*
- `crates/nas-web/src/routes/users.rs` (**686**), `files/zip.rs` (**668**), `routes/files/mutations.rs` (**619**), `db.rs` (**531**), `legacy/storage.rs` (**504**), `files.rs` (**494**), `files/trash.rs` (**471**) *(ya modularizado: `routes/system.rs`)*
- `src/core/updater.sh` (**705**), `src/core/deploy.sh` (**626**), `scripts/qa_live_runner.py` (**619**)

### `DT-B02` a `DT-B08` — Código muerto, restos PHP/Whiptail y deuda de pruebas
- **`DT-B02` (31 aliases triviales en `deploy/`):** `deploy/helpers.rs:30-271` (16), `deploy/samba.rs:192-401` (11), `deploy/samba/wsdd2.rs:17-20,92-105` (4).
- **`DT-B03` (Funciones sin callers / `dead_code`):** `terminal.rs:84-100` (`fn tmux`), `storage_ops.rs:265-280` (`LvmRequest`), `deploy/users.rs:168-210` (`sudoers_content` con `%grp_superadmin ALL=(ALL)`), `deploy/storage.rs:219-280,298-306,405-427`, `deploy/samba.rs:170-176,208-386`, `deploy/mod.rs:265-285` (`deploy_sh_invocation`), `nasctl/src/main.rs:218-222` (`remote_not_available`), `files/zip.rs:281-284` (`FileRoot::zip`), `legacy/storage.rs:433-441` (`calculate_task_size`), `views.rs:16` (`ICONS_SVG`), `routes/common.rs:304` (`ExtQuery`).
- **`DT-B04` (Textos y etiquetas heredadas de PHP/Whiptail/GPG):** `users/crud.js:35-36,260,302-303` protege/nombra `"www-data (sistema)"` en vez de `"nas-web"`; `src/lib/colors.sh:20-40` conserva `NEWT_COLORS`; `install.sh:236` usa fallback `TARGET_TAG="v0.1.4"`; `commands/migrate.rs:27` lista `install.sh` como legado a retirar; `nasctl/src/main.rs:23-24` anuncia transporte SSH futuro (F3).
- **`DT-B05` (Nomenclatura inconsistente):** `ServerRole::Arquivos` / `ArquivosBackup` (ortografía portuguesa en `nas-core/src/config.rs:14,18`); módulo `crates/nas-web/src/legacy.rs` cuyo nombre sugiere código muerto aunque atiende la API productiva.
- **`DT-B06` (Edge cases menores):** `parse_loadavg_1m` / `parse_psi_avg10` sin filtro `is_finite()` (`resources.rs:510-530`); `Inventory::load` mapea `PermissionDenied` a `NotFound` (`nas-inventory/src/lib.rs:117-119`); `nas-dev/src/preflight.rs:303-308` lee `.tar.gz` entero en RAM al calcular SHA-256.
- **`DT-B07` (Sin tests unitarios JavaScript ni E2E en navegador):** `.github/workflows/ci.yml` solo valida sintaxis (`node --check`). Regresiones funcionales (`unidades/<id>`, `onclick=""` en `layout.html`, `res.blob()`) pasan CI sin fallo.
- **`DT-B08` (Cobertura unitaria faltante en UI/CLI):** sin unit tests propios en `nas-tui/src/app.rs`, `nasctl/src/main.rs`, `nas-dev/src/{preflight,remote,sync}.rs` y subcomandos CLI (`audit`, `backup`, `deploy`, `diag`, `migrate`, `network`, `services`, `status`, `storage`, `users`).

---

## 6. Estado de la Auditoría Anterior (`docs/auditoria.md`: A-01 a A-09)

| ID | Estado Actual | Evidencia |
| :--- | :--- | :--- |
| **A-01** (`system_update_run`) | **Resuelto** | `routes/system.rs:180-198` invoca `sudo /usr/bin/systemd-run --unit=nas-system-update /usr/local/bin/nas --json update apply ...`. |
| **A-02** (`packaging/` vs `deploy/`) | **Resuelto** | `deploy/{web,helpers,artifacts,samba}.rs` consumen `packaging/*` vía `include_str!` con tests de paridad. |
| **A-03** (`su root grp_samba` en `logrotate`) | **Resuelto** | `deploy/artifacts.rs:23` incluye `su root grp_samba`. |
| **A-04** (`UPDATE users` en `install.sh`) | **Resuelto** | `packaging/install.sh:144-169` actualiza `web_users` (`password_sha256`). |
| **A-05** (`gpg_verified` vs `Ed25519`) | **Parcial (brecha nueva)** | `routes/system.rs:260-265` ya lee `ed25519_verified`, pero `install.sh:198,299` lo escribe en `true` sin verificar firma (`DT-C02`). |
| **A-06** (`256 MiB` en `zip.rs`) | **Resuelto** | `ZIP_MAX_BYTES = 50 GiB` sin mensajes viejos (falta ZIP64 `DT-C03`). |
| **A-07** (Restos PHP/Git/GPG) | **Parcial** | Limpio en `services.rs` y partes de `layout.html`/`system.rs`, pero persisten `www-data` (`crud.js`), token PHP (`layout.html:3357`), `tests/parity/fixtures/*.php` y `src/core/*.sh`. |
| **A-08** (`80-udisks2-hide-os.rules`) | **Resuelto en Rust / Abierto en Bash** | Eliminado de `crates/nas-engine/src/uninstall.rs`, aún presente en `src/core/uninstall.sh:76`. |
| **A-09** (`version.json` y helpers en `nas update`) | **Resuelto** | `nas-engine/src/update.rs:214-249` y `commands/update.rs:63-113,640-654` sincronizan `bin/`, `sbin/`, `version.json` y `release.pub`. |

---

## 7. Plan de Remediación Priorizado (Olas)

1. **Ola 1 — Seguridad y Integridad de Datos (Inmediata):**
   - `DT-C01` (bloquear rutas protegidas en `FileRoot::resolve_dir`).
   - `DT-C02` (verificación SHA-256 obligatoria y validación Ed25519 real o no mentir en `ed25519_verified` en `install.sh`).
   - `DT-C04`, `DT-C05`, `DT-C06` (`umount -f/-l`, `nohup` sin `timeout` bloqueante, `-prune` de `BACKUPS_*` en `purge_recycle_in`).
   - `DT-A01`, `DT-A02`, `DT-A03`, `DT-A04`, `DT-A09` (claves SSH, `KEY_FILE`, temporales `O_EXCL`, `restore.rs`, `smartctl` con `sudo`; `DT-A08` resuelto).
2. **Ola 2 — Bugs Funcionales Web / Deduplicación (Alta prioridad):**
   - `DT-C03` (ZIP64 + descarga directa sin `res.blob()` en RAM).
   - `DT-A13` (papelera en `trash.js` para `unidades/<id>`; explorador y mover ya resueltos), `DT-A14`, `DT-A15`, `DT-A16`, `DT-A17`, `DT-A18` (tokens de `layout.html`, `files_upload` con `TempFile`, `trash` IDs, SVG, `user_update`).
   - `DT-A10`, `DT-A11` (`backup_dedup.rs` guardado de estado `ro`/`+i`, cota real de RAM para `partial_cache` y cubos, eliminación de `chown -R` innecesarios).
3. **Ola 3 — Consolidación Arquitectónica y Desacople:**
   - Unificar `deploy::storage` / `helpers::storage` / `storage_ops` / `storage` y `tasks/runner.rs` vs `backup_spec/render.rs`.
   - Subir `nas-cli` para que consuma directamente `nas-engine` (`services`, `diag`, `storage`, `deploy`) sin reimplementar.
   - Modularizar `backup_dedup.rs`, `layout.html` y `files/mutations.rs`; corregir o retirar `nas-dev sync` / `onboard` (`DT-C07`).
4. **Ola 4 — Limpieza Legacy y Pruebas:**
   - Retirar o reducir `src/core/{deploy,updater,uninstall}.sh` a shims puros sin heredocs PHP muertos; migrar BATS para probar binarios `nas` reales y eliminar `tests/parity/fixtures/*.php` cuando `qa_ui_audit.py` cubra el contrato desde `layout.html`.
   - Añadir pruebas unitarias/E2E de JS en CI y cobertura unitaria en `nas-tui`, `nasctl` y `nas-cli`.
