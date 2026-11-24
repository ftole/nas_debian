# Deuda Técnica, Bugs Latentes y Plan de Remediación (`v0.1.12+`)

> [!IMPORTANT]
> **Auditoría código por código del workspace completo** (11 crates Rust, frontend HTML/JS, `packaging/`, `install.sh`, `src/`, `scripts/`, `tests/` y `.github/workflows/`) sobre el estado actual del repositorio (`v0.1.12` + resguardos locales para discos de grado consumidor pendientes de commit).
> Este documento actúa como **registro vivo de deuda técnica**, complementando la línea base histórica [docs/auditoria.md](./auditoria.md) (`0f7a272`).

---

## 1. Resumen Ejecutivo e Inventario Auditado

| Área Inspeccionada | Volumen Aproximado | Estado General |
| :--- | :--- | :--- |
| **Crates Base y Orquestación** (`nas-core`, `nas-exec`, `nas-audit`, `nas-inventory`, `nas-ssh`, `nas-tui`, `nasctl`, `nas-dev`, `nas-cli`) | 39 archivos (~9,989 líneas) | Núcleo sólido (`#![forbid(unsafe_code)]`, `CommandSpec` sin shell, `ResourceBudget`), pero con duplicación CLI↔Engine y fallas críticas en `nas-dev`. |
| **Motor del Sistema** (`crates/nas-engine/src/**`) | 55 archivos (~20,067 líneas) | Funcional y bien probado en parsers/core, pero con monolitos (`backup_dedup.rs`), rutas duplicadas (`deploy` vs `helpers` vs `tasks`) y puntos de bloqueo/desgaste en fallos eléctricos. |
| **Panel Web y Frontend** (`crates/nas-web/src/**`, `templates/`, `assets/js/**`) | Backend `axum` + `layout.html` (3,359 ln) + 28 JS | Seguridad auth/CSRF/PBKDF2 robusta, pero con brechas graves en upload/ZIP64/`unidades/<id>`, tokens PHP residuales en `layout.html` y cero tests JS/E2E en CI. |
| **Empaquetado, Bash Legacy, Scripts y CI** (`install.sh`, `packaging/`, `src/`, `scripts/`, `tests/`, `.github/workflows/`) | 3 shims Bash + scripts Python + 8 suites BATS | `packaging/` está alineado con `deploy/` en `include_str!` y `install.sh` valida SHA-256 + Ed25519 real; `src/core/*.sh` conserva código muerto incompatible y los tests BATS prueban heredocs muertos en lugar de binarios Rust. |

### Conteo de Hallazgos Activos
- **Críticos:** `1` activo (`DT-C01`, `DT-C02`, `DT-C04`, `DT-C06` y `DT-C07` resueltos; `DT-C05` parcialmente resuelto)
- **Altos:** `10` activos (`DT-A01`–`DT-A09`, `DT-A11` y `DT-A12` resueltos; `DT-A10` y `DT-A13` parcialmente resueltos)
- **Medios:** `11` activos (`DT-M02`, `DT-M03`, `DT-M05` y `DT-M12` resueltos; `DT-M08` parcialmente resuelto)
- **Bajos / Deuda Estructural:** `8` (`DT-B01` a `DT-B08`)

---

## 2. Hallazgos de Severidad CRÍTICA

| ID | Título | Ubicación Verificada | Riesgo Real |
| :--- | :--- | :--- | :--- |
| **DT-C01** | **[RESUELTO]** `FileRoot::resolve_dir` bloquea rutas protegidas y `nas-web.service` retira `AmbientCapabilities` | `crates/nas-web/src/files.rs`, `routes/files/mutations.rs`, `packaging/nas-web.service` | `FileRoot::resolve_dir` e `is_protected_path` bloquean subidas/escrituras a `.trash`, `.nas_staging`, `LOGS_BACKUP`, `BACKUPS_*` y `unidades/<id>/...`, y `nas-web.service` opera con `AmbientCapabilities=` vacío. |
| **DT-C02** | **[RESUELTO]** `install.sh` exige manifiesto firmado, valida SHA-256 + Ed25519 con `openssl` y reporta `ed25519_verified` verídico | `install.sh` | Exige descarga del manifiesto `.json`, valida `SHA-256` y firma `Ed25519` vía `openssl pkeyutl -rawin` antes de descomprimir y marca `ed25519_verified: false` en `--build-from-source`. |
| **DT-C03** | ZIP de hasta 50 GiB sin ZIP64 (`large_file`) y descarga web que carga todo el archivo en RAM del navegador (`res.blob()`) | `crates/nas-web/src/files/zip.rs:15,226-227`, `routes/files.rs:30-36,306-308`, `assets/js/modules/files/actions.js:173-237` | Archivos/ZIP > 4 GiB fallan sin `.large_file(true)` en `zip` v2; además el JS llama `await res.blob()` sobre el stream directo (hasta 50 GiB en RAM del cliente) e ignora `prepare=1`/`token`. |
| **DT-C04** | **[RESUELTO]** `cleanup()` en runners de backup ahora usa `umount \|\| umount -f \|\| umount -l` | `crates/nas-engine/src/backup_spec/render.rs:113-115` | Evita bloqueos en `D-state` reteniendo los `flock` (FD 8 y 9) si el servidor remoto Windows cae durante `rsync`. |
| **DT-C05** | **[PARCIAL]** `run_backup_task` aplica límites cgroup `ResourceBudget` y valida errores de `systemd-run` | `crates/nas-engine/src/helpers/backup.rs:519-560` | La rama `systemd-run` ya aplica `Nice=10`, `IOSchedulingClass=best-effort`, `CPUQuota`, `MemoryHigh`/`Max` y propaga errores; solo el fallback sin `systemd` (`nice -n 10 nohup`) conserva timeout de 10s en `Runner`. |
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
- **`DT-A13` [PARCIAL] — Papelera pendiente para unidades adicionales (`unidades/<id>`):** `crates/nas-web/src/routes/files.rs::file_root_for` ya admite `unidades/<id>` (retirando el prefijo `unidades/` y validando `valid_name`), `files.js` muestra el origen en la barra de estado y `files/actions.js::submitMoveFile` ya envía `root`; queda pendiente que `assets/js/modules/files/trash.js` envíe `root` al listar o restaurar la papelera (`/api/files/trash` y `/api/files/trash/restore-all`).
- **`DT-A14` — `layout.html` con tarjetas de rol `superadmin` rotas (`onclick=""`) y ~30 tokens PHP sin poblar:** `crates/nas-web/templates/layout.html:259,768-891,2539,2661,3357` frente a `views.rs:138-152`. No se puede seleccionar el rol `superadmin` desde los modales de usuario/edición.
- **`DT-A15` — Subida multipart (`files_upload`) sensible al orden de campos, con fuga de staging y TOCTOU:** `crates/nas-web/src/routes/files/mutations.rs:250-296,345-359,443-459,480-485` depende de que `root` llegue antes que `file`, no usa guardas `TempFile` con `Drop`, hace `try_exists` + `rename` no atómico y corre `df -Pk` cada 16 MiB.
- **`DT-A16` — Papelera (`trash`) con truncamiento `parseInt`, XSS en `data-id`, race TOCTOU y recorrido síncrono:** `crates/nas-web/assets/js/modules/files/trash.js:156,242` y `files/trash.rs:99-101,124,330-368`.
- **`DT-A17` — Vista previa SVG rota:** `assets/js/modules/files/preview.js:104-108` intenta cargar `/api/files/raw` en `<img>`, pero `routes/preview.rs:42-52,159-164` lo sirve como `application/octet-stream` + `nosniff`.
- **`DT-A18` — `user_update` habilita acceso web sin contraseña (200 OK falso) e ignora `/etc/nas/superadmin`:** `crates/nas-web/src/routes/users.rs:202-203,459-469`.
- **`DT-A19` — `nas-cli deploy` y `nas-cli update` con errores silenciados y detección incompleta de disco en uso:** `crates/nas-cli/src/commands/deploy.rs:163-175,242-291,345-394` solo mira `findmnt` (omite LVM/RAID que sí revisa `nas-engine`) y omite auditoría `FAILED` si `useradd` falla; `commands/update.rs:148-152,368-376,429-431,645-683` compara `update check` contra `CARGO_PKG_VERSION` en lugar de la versión desplegada e ignora `--manifest` si falta `--artifact`.
- **`DT-A20` — `nas-inventory::Inventory::resolve` hace fallbacks silenciosos sin validar roles ni usuarios:** `crates/nas-inventory/src/lib.rs:196-239` convierte roles inválidos en `ArquivosBackup`, entornos desconocidos en `prod` y usuarios vacíos en `"nasadmin"`.
- **`DT-A21` — Scripts `src/core/{updater,deploy,uninstall}.sh` conservan flujos incompatibles con Rust:** `updater.sh:256,393-394,667,690-693` invoca `php`, tablas SQLite antiguas y `git`; `deploy.sh:129-626` conserva bloque muerto PHP/FPM usado por BATS; `uninstall.sh:76` aún referencia `80-udisks2-hide-os.rules`.

---

## 4. Hallazgos de Severidad MEDIA

- **`DT-M01` [PARCIAL] — Duplicación crítica entre módulos de sistema:**
  - *Resuelto:* validadores de dispositivo, nombres de usuario/grupo/share/unidad, cálculo `parent_disk`/`first_partition_path` y `RESERVED_SYSTEM_USERS` centralizados en `crates/nas-engine/src/validation.rs` (con `OnceLock<Regex>`); `nas-cli` (`services`, `diag`, `storage`) delega directamente en `nas_engine`.
  - *Pendiente:* `is_os_disk` (4 variantes), `update_fstab` (3 variantes), parsers de `df -Pk`/IP primaria, dos motores de ejecución de backups divergentes (`tasks/runner.rs` vs `backup_spec/render.rs`) y 3 parsers distintos de `backup_<id>.sh`.
- **`DT-M02` [RESUELTO] — `tasks/runner.rs` limpia `.inprogress_*` y resuelve pool sin `[subvol]`:** `crates/nas-engine/src/tasks/runner.rs` purga `.inprogress_*` tanto al inicio (`clean_orphan_inprogress_dirs`) como al fallar (`remove_dir_all(task.staging_path(now))`), añade `umount -f`/`-l` ante caída CIFS y recorta `[subvol]` en `destination_pool_key`.
- **`DT-M03` [RESUELTO] — `nas-exec::command.rs` combina `SudoMode::WithPassword` + `stdin_data` y evita bloqueos con nietos en timeout:** `crates/nas-exec/src/command.rs` y `redact.rs` ordenan secretos por longitud descendente, redactan `CommandOutput::command` y acotan la espera de pipes tras timeout.
- **`DT-M04` — SQLite en `db.rs` corre `ALTER TABLE web_audit` antes de `CREATE TABLE web_audit` y serializa toda la concurrencia bajo un `Mutex` único:** `crates/nas-web/src/db.rs:37,111-124,140`.
- **`DT-M05` [RESUELTO] — Endpoint `/api/terminal/history` exige `SuperAdminUser`:** `crates/nas-web/src/routes/terminal.rs:295`.
- **`DT-M06` — Campos ignorados y fallbacks rotos entre JS y API en AD, Shares y Permisos de Usuarios:** `routes/domain.rs:18-22,136-140` ignora `ou` y `user`/`password` enviados por `domain.js`; `users/permissions.js:151,180` pasa HTML a `.textContent`; `routes/shares/access.rs:227-233,283,309-319` no hace rollback de `smb.conf` si falla `apply_acl` e ignora `body.force`.
- **`DT-M07` — `nas-web` usa rutas hardcodeadas y `du` fuera de semáforo:** `views.rs:64,74,86,273` usa `NasPaths::default()`/`"/srv/nas"`; `legacy.rs:157,181` hardcodea `"/usr/local/bin"` y `"/etc/cron.d"`; `legacy/storage.rs:153-186` lanza `du` de 180s sin adquirir `heavy_ops`; `routes/backups.rs:126-161` retiene `system_ops` durante múltiples `du` de 15s; `template.rs:50-62,110-122` pierde tokens restantes ante un `<!--IF:...` mal cerrado.
- **`DT-M08` [PARCIAL] — Escritura atómica y rollback de `smb.conf` cubiertos; quedan mutaciones menores en `deploy`:** `helpers/samba.rs` usa `atomic_install_bytes` (`O_EXCL` + `sync_all` + `rename`) y revierte `/etc/samba/smb.conf` si falla `systemctl reload smbd`; quedan pendientes silencios de salida en `deploy/artifacts.rs:163-216`, `daemon-reload` (`deploy/web.rs:203`, `wsdd2.rs:83`) y `accounts.rs:175-187`.
- **`DT-M09` — `helpers/terminal.rs` oculta fallos de `tmux` en `capture_terminal` y abre socket como `root` en `start_terminal`:** `helpers/terminal.rs:21-33,78-85,253-294`.
- **`DT-M10` — `nas-tui` con rutas hardcodeadas (`/srv/nas`, `/dev/null`), sin scroll en `Paragraph` y sin guard `Drop` para restaurar TTY en panic:** `crates/nas-tui/src/{app.rs:263,307,334,357, ui.rs:47-55,284, main.rs:37-55}`.
- **`DT-M11` — Inconsistencias `--json` y códigos de salida en `nas-cli` / `nasctl` / `nas-dev`:** errores siempre imprimen JSON en `stderr` sin `--json`; `audit verify`, `diag`, `update verify` y `nasctl check` emiten `"success": true` al fallar; `nasctl` omite hosts si un hilo entra en pánico; `nas-dev sync` deja temporales `nas-dev-<pid>` sin limpiar.
- **`DT-M12` [RESUELTO] — `btrfs scrub` y `fstrim` para volúmenes adicionales en `/srv/nas/unidades/*`:** `helpers/storage.rs::create_pool` y `helpers/storage/units.rs::mount_unit` registran `/etc/cron.d/nas-btrfs-scrub-unit-<id>` mediante `register_btrfs_scrub_for_target` (y `umount_unit` lo retira); además `/api/storage/scrub?unit=<id>` y `/api/storage/trim?unit=<id>` (`nas helper storage-apply scrub-unit-status|scrub-unit-start|trim-unit`) operan por unidad.
- **`DT-M13` — `update.rs` healthcheck `curl` sin `-f`:** `crates/nas-engine/src/update.rs:253-262` no falla ante HTTP `500`/`502`.
- **`DT-M14` — Brechas en CI y tests BATS:** `.github/workflows/ci.yml:58` deshabilita `tests/deploy_shim.bats`; `failure_runners.bats` y `rotation.bats` prueban Bash inline con `chattr -R +i` / `chmod -R u+w` en lugar de runners Rust reales; `scripts/parity_extract.py` depende de fixtures PHP (`layout.php` de 3,154 ln); `scripts/qa_live_runner.py:157` apunta a `/api/users/update` en vez de `/api/users/delete`.
- **`DT-M15` — Subprocesos innecesarios y lectura completa en RAM en runners/logs:** `backup_spec/render.rs:168` llama `$(date +%s)` en cada línea de progreso `rsync`; `AUTO_PERMANENT` hace doble recorrido (`du -sb` + `find`); `helpers/backup.rs:643` lee todo el `.log` con `read_to_string`; `helpers/share.rs:54-60` corre `chown -R` en shares existentes.

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
