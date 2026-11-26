# Deuda Técnica, Bugs Latentes y Plan de Remediación (`v0.1.14`)

> [!IMPORTANT]
> **Auditoría código por código del workspace completo** (11 crates Rust, frontend HTML/JS, `packaging/`, `install.sh`, `src/`, `scripts/`, `tests/` y `.github/workflows/`) actualizada sobre `v0.1.14`.
> Este documento actúa como **registro vivo de deuda técnica**, complementando la línea base histórica [docs/auditoria.md](./auditoria.md) (`0f7a272`).

---

## 1. Resumen Ejecutivo e Inventario Auditado

| Área Inspeccionada | Volumen Aproximado | Estado General |
| :--- | :--- | :--- |
| **Crates Base y Orquestación** (`nas-core`, `nas-exec`, `nas-audit`, `nas-inventory`, `nas-ssh`, `nas-tui`, `nasctl`, `nas-dev`, `nas-cli`) | 45 archivos (~11,600 líneas) | Sólido (`#![forbid(unsafe_code)]`, `CommandSpec` sin shell, `ResourceBudget`, auditoría O(1) en cola, contratos JSON/exit codes consistentes, TTY `Drop` en `nas-tui`, `nas-dev sync` con `--strip-components=1`). |
| **Motor del Sistema** (`crates/nas-engine/src/**`) | 89 archivos (~29,700 líneas) | Endurecido en seguridad (`backup-manage`, `restore`, `share`, `privileged_delete`, `domain`, `is_os_disk()` Btrfs/LVM/RAID), retención Btrfs RO y timeouts `mkfs`/permanentes 600–900s; `backup_dedup.rs` pendiente de modularización futura. |
| **Panel Web y Frontend** (`crates/nas-web/src/**`, `templates/`, `assets/js/**`) | Backend `axum` + `layout.html` + 29 JS propios (+ 4 vendor + 1 WASM) | Auth/CSRF/PBKDF2 robusto; ZIP64 + descarga streaming, XSS papelera, borrado/papelera privilegiada y `unidades/<id>` cerrados, `files_upload` sin orden/TOCTOU, preview SVG/`.docx` aislado y panel de seguridad en vivo (`GET /api/security/status`). |
| **Empaquetado, Bash Legacy, Scripts y CI** (`install.sh`, `packaging/`, `src/`, `scripts/`, `tests/`, `.github/workflows/`) | 3 shims Bash + scripts Python + 8 suites BATS | `packaging/` alineado vía `include_str!`, dependencias Debian 13 mínimo completas, exclusiones AIDE `!/...` verificadas, `ci.yml` con `deploy_shim.bats` y `shellcheck` en wrappers, y `release.yml` con tests + `release.pub` previo al tarball. |

### Conteo de Hallazgos Activos
- **Críticos:** `0` activos (`DT-C01`–`DT-C07` y `C-01`–`C-03` resueltos)
- **Altos:** `0` activos (`DT-A01`–`DT-A20` resueltos; `DT-A10` acota RAM en `backup_dedup.rs`; `DT-A21` reducido a bloques BATS retrocompatibles)
- **Medios:** `0` activos (`DT-M01`–`DT-M15` resueltos; `tasks/runner.rs` y `backup_spec/render.rs` coexisten intencionalmente por roles distintos)
- **Bajos / Deuda Estructural:** `4` parciales (`DT-B01` restante en HTML/JS y `backup_dedup.rs`; `DT-B05` nomenclatura `Arquivos` / `legacy.rs`; `DT-B07` cobertura JS/E2E; `DT-B08` ampliado en `nas-tui`, `nasctl`, `nas-dev` y `nas-cli`)

---

## 2. Hallazgos de Severidad CRÍTICA

| ID | Título | Ubicación Verificada | Riesgo Real |
| :--- | :--- | :--- | :--- |
| **DT-C01** | **[RESUELTO]** `FileRoot::resolve_dir` bloquea rutas protegidas y `nas-web.service` retira `AmbientCapabilities` | `crates/nas-web/src/files.rs`, `routes/files/mutations.rs`, `packaging/nas-web.service` | `FileRoot::resolve_dir` e `is_protected_path` bloquean subidas/escrituras a `.trash`, `.nas_staging`, `LOGS_BACKUP`, `BACKUPS_*` y `unidades/<id>/...`, y `nas-web.service` opera con `AmbientCapabilities=` vacío. |
| **DT-C02** | **[RESUELTO]** `install.sh` exige manifiesto firmado, valida SHA-256 + Ed25519 con `openssl` y reporta `ed25519_verified` verídico | `install.sh` | Exige descarga del manifiesto `.json`, valida `SHA-256` y firma `Ed25519` vía `openssl pkeyutl -rawin` antes de descomprimir y marca `ed25519_verified: false` en `--build-from-source`. |
| **DT-C03** | **[RESUELTO]** ZIP64 (`large_file(true)`) y descarga directa por token en streaming sin `res.blob()` en RAM | `crates/nas-web/src/files/zip.rs`, `routes/files.rs`, `assets/js/modules/files/actions.js` | `SimpleFileOptions` habilita `.large_file(true)` para >4 GiB y `actions.js` descarga el ZIP preparado mediante `/api/files/zip?token=...` sin almacenar el archivo completo en memoria del navegador. |
| **DT-C04** | **[RESUELTO]** `cleanup()` en runners de backup ahora usa `umount \|\| umount -f \|\| umount -l` | `crates/nas-engine/src/backup_spec/render.rs:117-119` | Evita bloqueos en `D-state` reteniendo los `flock` (FD 8 y 9) si el servidor remoto Windows cae durante `rsync`. |
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
- **`DT-A10` [RESUELTO] — `backup_dedup.rs` ya protege `ro=true` / `+i` con guarda RAII y recuperación tras apagón:** `ContainerProtectionGuard` y `.nas_dedup_guard.json` (`recover_interrupted_dedup_guard`) restauran `ro=true` y `chattr +i` ante fallo, `Drop` o corte eléctrico previo; `collect_files_in_container_partitioned` (`969-991`) ya acota estrictamente `out.len() < budget.max_records_in_ram` incluso al alcanzar `divisor >= 4096`.
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
- **`DT-A21` [PARCIAL — Retrocompatibilidad BATS] — `src/core/uninstall.sh` retiró `80-udisks2-hide-os.rules`;** se conservan únicamente bloques de referencia de `deploy.sh`/`updater.sh` requeridos por las suites BATS vigentes (sin riesgo operativo activo).

---

## 4. Hallazgos de Severidad MEDIA

- **`DT-M01` [RESUELTO] — Unificación de validadores, detección de disco SO y parsers:**
  - *Resuelto:* validadores de dispositivo, nombres de usuario/grupo/share/unidad, cálculo `parent_disk`/`first_partition_path` y `RESERVED_SYSTEM_USERS` centralizados en `crates/nas-engine/src/validation.rs`; `nas-cli` (`services`, `diag`, `storage`) delega en `nas_engine`; `is_os_disk`/`os_disks` unificados en `helpers/storage/disks.rs` (consumido por `storage_ops.rs`, `deploy/mod.rs` y `storage/pools.rs`); `read_task_info` delega en `BackupTask::load`.
  - *Nota de diseño:* `tasks/runner.rs` (planificación tipada/dry-run para CLI, TUI y vista previa) y `backup_spec/render.rs` (emisión de scripts Bash para `cron`/`systemd-run` desde el panel web) coexisten intencionalmente por responsabilidades distintas.
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

### `DT-B01` — Archivos sobredimensionados (>450 líneas)
- *Ya modularizados en submódulos:* `nas-core/src/resources.rs` (`parsers.rs`, `cgroup.rs`, `tests.rs`), `security/acls.rs` (`probe.rs`, `discover.rs`), `helpers/backup/install.rs` (`install/tests.rs`), `storage_ops.rs` (`maintenance.rs`, `tests.rs`), `routes/files/mutations.rs` (`upload.rs`), además de `security.rs`, `storage.rs`, `helpers/storage.rs`, `update.rs`, `helpers/backup.rs`, `helpers/accounts.rs`, `commands/update.rs` y `routes/system.rs`.
- *Pendientes de modularización futura:* `crates/nas-web/templates/layout.html` (**3,675** líneas), `assets/js/app.js` (**823** líneas), `crates/nas-engine/src/backup_dedup.rs` (**3,028** líneas), `src/core/{updater,deploy}.sh` y `scripts/qa_live_runner.py`.

### `DT-B02` a `DT-B08` — Código muerto, restos PHP/Whiptail y deuda de pruebas
- **`DT-B02` [RESUELTO]:** retirados los aliases triviales sin uso en `deploy/{helpers.rs, samba.rs, samba/wsdd2.rs}` y migrados los llamadores en `commands/deploy.rs`.
- **`DT-B03` [RESUELTO]:** retiradas las funciones/estructuras sin llamadores (`terminal.rs::tmux`, `storage_ops::LvmRequest`, `deploy/users::{sudoers_content, sudoers_artifact, install_sudoers}`, `deploy/storage::{lvm_*, setup_storage}`, `deploy/samba::{generate_smb_conf, posix_acl_plan, apply_posix_acls}`, `nasctl::remote_not_available`, `legacy/storage::calculate_task_size*`, `views::ICONS_SVG`) y confinado `FileRoot::zip` a `#[cfg(test)]`. *(Nota: `deploy_sh_invocation` y `ExtQuery` sí tienen llamadores activos y se conservan).*
- **`DT-B04` [RESUELTO]:** retirado `install.sh` de `migrate.rs` (al ser el instalador activo), actualizado el texto `"fase F3"` en `nasctl` y eliminado `NEWT_COLORS` de `src/lib/colors.sh`.
- **`DT-B05` (Nomenclatura inconsistente):** `ServerRole::Arquivos` / `ArquivosBackup` (ortografía portuguesa en `nas-core/src/config.rs:14,18`); módulo `crates/nas-web/src/legacy.rs` cuyo nombre sugiere código muerto aunque atiende la API productiva.
- **`DT-B06` [RESUELTO]:** `parse_loadavg_1m` / `parse_psi_avg10` validan `is_finite() && >= 0.0`, `Inventory::load` distingue `NotFound` de `PermissionDenied` y `nas-dev::sha256_file` calcula el hash en streaming de 64 KiB.
- **`DT-B07` (Sin tests unitarios JavaScript ni E2E en navegador):** `.github/workflows/ci.yml` valida sintaxis (`node --check`) y contrato estático (`qa_ui_audit.py --static`).
- **`DT-B08` [PARCIAL — Ampliado]:** añadidas pruebas unitarias en `nas-tui/src/app.rs`, `nasctl/src/main.rs`, `nas-dev/src/{preflight,remote,sync}.rs` y `nas-cli/src/commands/migrate.rs`.

---

## 6. Estado de la Auditoría Anterior (`docs/auditoria.md`: A-01 a A-09)

| ID | Estado Actual | Evidencia |
| :--- | :--- | :--- |
| **A-01** (`system_update_run`) | **Resuelto** | `crates/nas-web/src/routes/system/updates.rs:69,296-365` invoca `sudo /usr/bin/systemd-run --unit=nas-system-update /usr/local/bin/nas --json update apply ...`. |
| **A-02** (`packaging/` vs `deploy/`) | **Resuelto** | `deploy/{web,helpers,artifacts,samba}.rs` consumen `packaging/*` vía `include_str!` con tests de paridad. |
| **A-03** (`su root grp_samba` en `logrotate`) | **Resuelto** | `deploy/artifacts.rs:23` incluye `su root grp_samba`. |
| **A-04** (`UPDATE users` en `install.sh`) | **Resuelto** | `packaging/install.sh:174-202` actualiza `web_users` (`password_sha256`). |
| **A-05** (`gpg_verified` vs `Ed25519`) | **Resuelto (`DT-C02`)** | `install.sh` valida SHA-256 + Ed25519 con `openssl` antes de descomprimir y reporta `ed25519_verified` verídico. |
| **A-06** (`256 MiB` en `zip.rs`) | **Resuelto (`DT-C03`)** | `ZIP_MAX_BYTES = 50 GiB`, ZIP64 (`large_file(true)`) y descarga por token en streaming. |
| **A-07** (Restos PHP/Git/GPG) | **Resuelto en Rust / Mínimo en BATS** | Limpio en código Rust y `layout.html`; `tests/parity/fixtures/*.php` y `src/core/*.sh` se mantienen únicamente por retrocompatibilidad BATS. |
| **A-08** (`80-udisks2-hide-os.rules`) | **Resuelto** | Retirado tanto de `crates/nas-engine/src/uninstall.rs` como de `src/core/uninstall.sh`. |
| **A-09** (`version.json` y helpers en `nas update`) | **Resuelto** | `crates/nas-engine/src/update/{plan,rollback}.rs` y `commands/update.rs:69-113,652-656` sincronizan `bin/`, `sbin/`, `version.json` y `release.pub`. |

---

## 7. Estado de las Olas de Remediación

1. **Ola 1 — Seguridad e Integridad de Datos (`[COMPLETADA]`):** cerrados `DT-C01`–`DT-C07` y `DT-A01`–`DT-A09`, `DT-A12` (`FileRoot::resolve_dir`, `install.sh` Ed25519, `umount -f/-l`, `ResourceBudget`, `-prune` en papelera SMB, `nas-dev`, temporales `O_EXCL`, `restore.rs`, `sanitize_fstab`, `nas-audit` `sync_data()`).
2. **Ola 2 — Bugs Funcionales Web / Deduplicación (`[COMPLETADA]`):** cerrados `DT-C03`, `DT-A10`, `DT-A11`, `DT-A13`–`DT-A20` (ZIP64 streaming, RAII `ContainerProtectionGuard` y cota de RAM en `backup_dedup.rs`, papelera/explorador en `unidades/<id>`, subida multipart sin TOCTOU, visor SVG/`iframe sandbox`, validación SemVer en `update`).
3. **Ola 3 — Consolidación Arquitectónica y Desacople (`[COMPLETADA]`):** cerrados `DT-M01`–`DT-M15` y `DT-B02`–`DT-B04`, `DT-B06` (validadores centralizados en `nas-engine/src/validation.rs`, modularización de `files/mutations/upload.rs`, `storage_ops`, `resources.rs`, `nas-tui` `Drop`, `nasctl` y `ci.yml`).
4. **Ola 4 — Limpieza Estructural y Cobertura (`[PENDIENTE OPCIONAL — Severidad Baja]`):**
   - `DT-B01`: modularizar progresivamente `layout.html` (3,675 líneas), `app.js` (823 líneas) y `backup_dedup.rs` (3,028 líneas).
   - `DT-B05`: normalizar nomenclatura `Arquivos` / `legacy.rs` cuando no rompa contratos.
   - `DT-B07` y `DT-B08`: ampliar tests unitarios/E2E de JavaScript y retirar shims legacy (`src/core/*.sh`, `tests/parity/fixtures/*.php`) cuando BATS pruebe exclusivamente binarios `nas` reales.
