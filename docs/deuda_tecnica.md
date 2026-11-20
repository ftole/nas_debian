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
| **Empaquetado, Bash Legacy, Scripts y CI** (`install.sh`, `packaging/`, `src/`, `scripts/`, `tests/`, `.github/workflows/`) | 3 shims Bash + scripts Python + 8 suites BATS | `packaging/` está alineado con `deploy/` en `include_str!`, pero `install.sh` omite firma Ed25519 real, `src/core/*.sh` conserva código muerto incompatible y los tests BATS prueban heredocs muertos en lugar de binarios Rust. |

### Conteo de Hallazgos Activos
- **Críticos:** `5` activos (`DT-C04` resuelto; `DT-C05` parcialmente resuelto)
- **Altos:** `19` activos (`DT-A08` y `DT-A12` resueltos; `DT-A09`, `DT-A10` y `DT-A13` parcialmente resueltos)
- **Medios:** `13` activos (`DT-M02` y `DT-M12` resueltos)
- **Bajos / Deuda Estructural:** `8` (`DT-B01` a `DT-B08`)

---

## 2. Hallazgos de Severidad CRÍTICA

| ID | Título | Ubicación Verificada | Riesgo Real |
| :--- | :--- | :--- | :--- |
| **DT-C01** | Subida web a rutas protegidas (`.trash`, `LOGS_BACKUP`, `BACKUPS_*`, `.nas_staging`) potenciada por `CAP_DAC_OVERRIDE` | `crates/nas-web/src/files.rs:334-342`, `routes/files/mutations.rs:397,409-426`, `packaging/nas-web.service:11,50-51`, `packaging/install.sh:83-85` | `FileRoot::resolve_dir` no llama a `is_protected_path()`; cualquier usuario `operator` autenticado puede subir archivos dentro de `.trash/.meta`, `LOGS_BACKUP` o `BACKUPS_HISTORICOS`. |
| **DT-C02** | `install.sh` no verifica firma Ed25519, permite omitir SHA-256 en silencio y falsifica `"ed25519_verified": true` | `install.sh:198,266-284,299` | Si fallan los `curl` de manifiesto/`.sha256` (`\|\| true`), instala el tarball sin comprobar integridad, nunca valida firma Ed25519 y escribe `ed25519_verified: true` en `/etc/nas/version.json`. |
| **DT-C03** | ZIP de hasta 50 GiB sin ZIP64 (`large_file`) y descarga web que carga todo el archivo en RAM del navegador (`res.blob()`) | `crates/nas-web/src/files/zip.rs:15,226-227`, `routes/files.rs:30-36,306-308`, `assets/js/modules/files/actions.js:173-237` | Archivos/ZIP > 4 GiB fallan sin `.large_file(true)` en `zip` v2; además el JS llama `await res.blob()` sobre el stream directo (hasta 50 GiB en RAM del cliente) e ignora `prepare=1`/`token`. |
| **DT-C04** | **[RESUELTO]** `cleanup()` en runners de backup ahora usa `umount \|\| umount -f \|\| umount -l` | `crates/nas-engine/src/backup_spec/render.rs:113-115` | Evita bloqueos en `D-state` reteniendo los `flock` (FD 8 y 9) si el servidor remoto Windows cae durante `rsync`. |
| **DT-C05** | **[PARCIAL]** `run_backup_task` aplica límites cgroup `ResourceBudget` y valida errores de `systemd-run` | `crates/nas-engine/src/helpers/backup.rs:519-560` | La rama `systemd-run` ya aplica `Nice=10`, `IOSchedulingClass=best-effort`, `CPUQuota`, `MemoryHigh`/`Max` y propaga errores; solo el fallback sin `systemd` (`nice -n 10 nohup`) conserva timeout de 10s en `Runner`. |
| **DT-C06** | `purge_recycle_in` escanea diariamente todo `/srv/nas` incluidos backups históricos | `crates/nas-engine/src/helpers/share.rs:169-216`, `deploy/artifacts.rs:115-126` | Ni el Rust helper ni `/etc/cron.daily/nas-recycle-purge` podan (`-prune`) `BACKUPS_HISTORICOS`, `BACKUPS_PERMANENTES` ni `LOGS_BACKUP`: recorre millones de inodos diarios y puede borrar `.recycle` dentro de snapshots históricos. |
| **DT-C07** | `nas-dev onboard` genera `sudoers` con escalada trivial a `root` y `nas-dev sync` despliega a docroot PHP inexistente | `crates/nas-dev/src/remote.rs:244-270`, `crates/nas-dev/src/sync.rs:177,289-298,338-357,374-383` | `sudoers` da `NOPASSWD` completo a `rm`, `mv`, `tar`, `rsync` sin acotar; además `sync` sube archivo antes de `mkdir` y sincroniza hacia `/var/www/nas-web-dev/` (inactivo tras F7). |

---

## 3. Hallazgos de Severidad ALTA

### 3.1 Seguridad, Credenciales y Validación de Entradas
- **`DT-A01` — Lectura arbitraria de claves SSH locales en `test_ssh_conn`:** `crates/nas-engine/src/helpers/backup/conn.rs:193-215,236` pasa cualquier ruta local existente en disco (`Path::new(trimmed_auth).is_file()`) como `-i <ruta>` bajo `root` sin redacción de secreto en `Runner`.
- **`DT-A02` — Inyección de opciones SSH en `KEY_FILE` de runners:** `crates/nas-engine/src/backup_spec/render.rs:65` interpola `ssh -i $KEY_FILE` sin comillas dentro de `-e` de `rsync`, y `backup_spec.rs:155-160` permite espacios o `-oProxyCommand=...` al no restringir a `/etc/backup-credentials/`.
- **`DT-A03` — Creación predecible de temporales en `/tmp` con `std::process::id()`:** `deploy/web.rs:146-147`, `deploy/artifacts.rs:284-285`, `deploy/storage.rs:344-345`, `helpers/storage/units.rs:111-112`, `deploy/samba/wsdd2.rs:46-51` y `crates/nas-cli/src/commands/deploy/credentials.rs:133-149,191-223` escriben en `/tmp` sin `O_EXCL` (riesgo TOCTOU/symlink y exposición temporal de secretos antes de `install -m 0600`).
- **`DT-A04` — `restore_backup` no canonicaliza origen ni protege objetivos `/tmp`:** `crates/nas-engine/src/helpers/backup/restore.rs:47-48,74-82` acepta cualquier carpeta bajo `/srv/nas/` (no solo `BACKUPS_*`, ni valida symlinks tras `canonicalize()`), carece de `TaskLock` y usa timeouts fijos de `300s`/`600s` (además **rompe restores SSH por clave Ed25519** al usar solo `sshpass` en `líneas 188-210`).
- **`DT-A05` — `update/remote.rs` no valida `repo` (`owner/repo`) ni `tag_name` antes de construir URL y ruta JSON:** `crates/nas-engine/src/update/remote.rs:69-70,154-156`.
- **`DT-A06` — `nas-ssh::validate_private_key` falla con claves OpenSSH modernas cifradas:** `crates/nas-ssh/src/lib.rs:231-235` busca `"ENCRYPTED"`, que no aparece en formato `OPENSSH PRIVATE KEY` con `bcrypt` (falso negativo) pero sí en comentarios (falso positivo); tampoco pasa `--` en `ssh`/`scp` (`lib.rs:68-70,168`).
- **`DT-A07` — `nas-cli helper` con lectura ilimitada en `stdin`, fallbacks silenciosos y contraseñas en `argv`:** `crates/nas-cli/src/helper/dispatch.rs:12-34,71-95,144-187,476-479` lee `stdin` sin cota máxima (OOM), trunca en 8 KiB con `read_stdin_single_line`, convierte códigos >255 a `u8`, oculta errores de `systemd-run` de deduplicación y mantiene contraseñas en `argv` para `CreateUser`/`UpdateUser`.

### 3.2 Almacenamiento, Resiliencia ante Apagones y Migración Deduplicada
- **`DT-A08` [RESUELTO] — `deploy/storage.rs::update_fstab` y `helpers/storage/units.rs` ahora delegan en `deploy/artifacts.rs::update_fstab`:** ambos aplican `fstab_options_for_target` (`nofail,x-systemd.device-timeout=15s`, retirando `autodefrag`/`discard=async`) con temporales `O_EXCL` (`write_unique_temp_file`) y respaldo `.bak.<epoch>`, además de saneamiento bajo demanda vía `sanitize_fstab` (`/api/storage/fstab-sanitize`).
- **`DT-A09` [PARCIAL] — S.M.A.R.T. bajo `nas-web` sin `sudo`:** `crates/nas-engine/src/storage.rs::check_block_health` ya limpia `[/@subvol]` mediante `clean_block_source(device)` antes de validar el dispositivo, pero sigue invocando `smartctl -H` sin `SudoMode::Passwordless` cuando se ejecuta desde el servicio sin privilegios `nas-web`.
- **`DT-A10` [PARCIAL] — `backup_dedup.rs` ya protege `ro=true` / `+i` con guarda RAII y recuperación tras apagón:** `ContainerProtectionGuard` y `.nas_dedup_guard.json` (`recover_interrupted_dedup_guard`) restauran `ro=true` y `chattr +i` ante fallo, `Drop` o corte eléctrico previo; queda pendiente la cota de cubos cuando `divisor >= 4096` en `collect_files_in_container_partitioned` (`969-990`).
- **`DT-A11` — `chown -R` y `chattr -R -i` remanentes sobre árboles históricos de backups:** `crates/nas-engine/src/helpers/backup.rs:135-139,360` y `helpers/backup/permanent.rs:183-187,426-444` siguen lanzando `chown -R` / `chattr -R -i` completos; `permanent.rs:30-66` ejecuta `dir_stats` síncrono con `HashSet` ilimitado; `backup_spec/render.rs:331-368` deja `FULL_*` huérfano bloqueando futuras congelaciones si falla `cp -al`/`--reflink`.
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

- **`DT-M01` — Duplicación crítica entre módulos de sistema:**
  - `valid_device` / `is_valid_device` (4 variantes), cálculo `parent_disk` / `partition_path` (5 variantes), `is_os_disk` (4 variantes), `update_fstab` (3 variantes), listas `RESERVED` de usuarios (3 variantes, `samba.rs` omite `"nas"`), validadores de share/ID/usuario (algunos compilan `Regex::new` en cada llamada), parsers de `df -Pk` e IP primaria.
  - `nas-cli` reimplementa `services`, `diag` (omitiendo `sqlite`, `red` y `montaje:rw` de `nas-engine`) y `storage` (usando `lsblk` propio en vez de `nas_engine::storage::disks`).
  - Dos motores de ejecución de backups divergentes (`tasks/runner.rs` vs `backup_spec/render.rs`) y 3 parsers distintos de `backup_<id>.sh`.
- **`DT-M02` [RESUELTO] — `tasks/runner.rs` limpia `.inprogress_*` y resuelve pool sin `[subvol]`:** `crates/nas-engine/src/tasks/runner.rs` purga `.inprogress_*` tanto al inicio (`clean_orphan_inprogress_dirs`) como al fallar (`remove_dir_all(task.staging_path(now))`), añade `umount -f`/`-l` ante caída CIFS y recorta `[subvol]` en `destination_pool_key`.
- **`DT-M03` — `nas-exec::command.rs` descarta `stdin_data` bajo `SudoMode::WithPassword` y puede bloquearse con nietos abiertos:** `crates/nas-exec/src/command.rs:311-315,350-376`; además `Redactor` (`redact.rs:31-48`) no ordena secretos por longitud descendente ni redacta `CommandSpec::describe()`.
- **`DT-M04` — SQLite en `db.rs` corre `ALTER TABLE web_audit` antes de `CREATE TABLE web_audit` y serializa toda la concurrencia bajo un `Mutex` único:** `crates/nas-web/src/db.rs:37,111-124,140`.
- **`DT-M05` — Endpoint `/api/terminal/history` usa `AuthenticatedUser` en vez de `SuperAdminUser`:** `crates/nas-web/src/routes/terminal.rs:295`.
- **`DT-M06` — Campos ignorados y fallbacks rotos entre JS y API en AD, Shares y Permisos de Usuarios:** `routes/domain.rs:18-22,136-140` ignora `ou` y `user`/`password` enviados por `domain.js`; `users/permissions.js:151,180` pasa HTML a `.textContent`; `routes/shares/access.rs:227-233,283,309-319` no hace rollback de `smb.conf` si falla `apply_acl` e ignora `body.force`.
- **`DT-M07` — `nas-web` usa rutas hardcodeadas y `du` fuera de semáforo:** `views.rs:64,74,86,273` usa `NasPaths::default()`/`"/srv/nas"`; `legacy.rs:157,181` hardcodea `"/usr/local/bin"` y `"/etc/cron.d"`; `legacy/storage.rs:153-186` lanza `du` de 180s sin adquirir `heavy_ops`; `routes/backups.rs:126-161` retiene `system_ops` durante múltiples `du` de 15s; `template.rs:50-62,110-122` pierde tokens restantes ante un `<!--IF:...` mal cerrado.
- **`DT-M08` — `deploy` mutaciones silenciosas y copia no atómica de `smb.conf`:** `helpers/samba.rs:55-75` usa `fs::copy` in-place (sin renombrado atómico ni rollback si falla `reload smbd`); `deploy/artifacts.rs:163-216` silencia fallos en `apply_hardening`; `deploy/web.rs:203` y `wsdd2.rs:83` ignoran código de salida de `daemon-reload`; `helpers/accounts.rs:175-187` puede vaciar grupos críticos en `sync_groups` si falla `id -nG`.
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
- `crates/nas-engine/src/storage.rs` (**857** líneas), `helpers/backup.rs` (**850**), `helpers/accounts.rs` (**707**), `backup_spec.rs` (**685**), `update.rs` (**553**), `deploy/samba.rs` (**514**), `deploy/mod.rs` (**509**), `deploy/artifacts.rs` (**502**), `deploy/storage.rs` (**493**), `tasks/runner.rs` (**481**), `tasks.rs` (**472**), `backup_spec/render.rs` (**465**), `storage_ops.rs` (**461**)
- `crates/nas-cli/src/commands/update.rs` (**828**), `main.rs` (**578**), `helper/dispatch.rs` (**537**), `commands/deploy.rs` (**489**)
- `crates/nas-web/src/routes/users.rs` (**686**), `files/zip.rs` (**668**), `routes/system.rs` (**625**), `routes/files/mutations.rs` (**619**), `db.rs` (**531**), `legacy/storage.rs` (**504**), `files.rs` (**494**), `files/trash.rs` (**471**)
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
