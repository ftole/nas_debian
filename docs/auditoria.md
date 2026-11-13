# Auditoría Técnica: Código Real vs. Documentación (`main` `0f7a272`)

> [!IMPORTANT]
> **Alcance estrictamente diagnóstico:** Esta auditoría refleja la inspección directa del código fuente en `main` (`0f7a272`) frente a la documentación histórica del proyecto. **No se modificó código Rust, Bash ni JavaScript** para corregir ni maquillar los hallazgos aquí registrados.

---

## 1. Resumen Ejecutivo de la Auditoría

Tras completar la migración de PHP/Bash hacia Rust (`0.1.10`), la documentación anterior describía una fotografía obsoleta (panel PHP, 36 iconos SVG, 2 roles, `SMB2_02`, regla `udisks2`) y ocultaba divergencias reales introducidas durante la transición. Al verificar directamente en `0f7a272` los **6 hallazgos previos** reportados y revisar commits recientes (como `9802002` y `e258d56`), se confirma que **todos los hallazgos centrales siguen presentes en `main`**, además de residuos de textos PHP/Git y reglas no instaladas.

| ID | Hallazgo | Estado en `0f7a272` | Severidad | Archivo / Líneas Verificadas |
| :--- | :--- | :--- | :--- | :--- |
| **A-01** | `system_update_run` ejecuta `nas update apply` sin `sudo` y bajo `ProtectSystem=yes` | **Persiste** | **Crítica** | `crates/nas-web/src/routes/system.rs:387-424` |
| **A-02** | Deriva severa entre `packaging/` y `crates/nas-engine/src/deploy/` | **Persiste** | **Crítica** | `deploy/web.rs:9-15`, `deploy/helpers.rs:20-27`, `deploy/artifacts.rs:54-60`, `deploy/samba.rs:50-84` |
| **A-03** | `LOGROTATE_BACKUPS` omite `su root grp_samba` sobre directorio `2770` (`H1`) | **Persiste** | **Alta** | `crates/nas-engine/src/deploy/artifacts.rs:44-52`, `packaging/install.sh:83` |
| **A-04** | SQL `UPDATE users SET password_hash=...` falla silenciosamente en BD SQLite real | **Persiste** | **Alta** | `packaging/install.sh:141-143` vs `crates/nas-web/src/db.rs:127-132` |
| **A-05** | Lectura muerta de `gpg_verified` vs verificación real `Ed25519` | **Persiste** | **Media** | `crates/nas-web/src/routes/system.rs:179-220` vs `scripts/package.sh:58-70` |
| **A-06** | Doc-comment y mensaje de error obsoletos de `256 MiB` en descarga ZIP | **Persiste** | **Baja** | `crates/nas-web/src/files/zip.rs:15, 89, 277` |
| **A-07** | Textos y campos residuales PHP / Git / GPG en UI, API, CLI y lista blanca | **Persiste** | **Baja / Media** | `layout.html:1655,3279,3285`, `system.js:18-23,172`, `system.rs:334`, `services.rs:31`, `update.rs:48` |
| **A-08** | Regla inexistente `80-udisks2-hide-os.rules` aún referenciada en `uninstall.rs` | **Nuevo / Confirmado** | **Baja** | `crates/nas-engine/src/uninstall.rs:124` |
| **A-09** | `nas update apply` no actualiza `version.json` activo ni helpers `/usr/local/sbin` | **Nuevo / Confirmado** | **Media** | `crates/nas-engine/src/update.rs:152-216`, `routes/system.rs:162-167`, `commands/update.rs:24` |

---

## 2. Detalle Técnico de Hallazgos Verificados en `main` (`0f7a272`)

### A-01 • `system_update_run` falla en producción por falta de privilegios y ausencia de unidad `nas-system-update`

- **Evidencia en código:** `crates/nas-web/src/routes/system.rs:387-424`.
- **Contexto histórico:** El commit `9802002` (*"fix: limpia variables de entorno antes de fijar PATH al actualizar"*) limpió el entorno de `Command::new(nas_executable)`, pero no corrigió el modelo de ejecución.
- **Diagnóstico actual:**
  1. `nas-web` corre como usuario sin privilegios `nas-web` dentro de `packaging/nas-web.service` con `ProtectSystem=yes`.
  2. La línea `407` ejecuta directamente `/usr/local/bin/nas --json update apply --repo ftole/nas_debian --root /opt/nas_debian --yes` **sin `sudo`**, mientras `/etc/sudoers.d/nas-web-rust` (`packaging/install.sh:223,234`) solo autoriza `systemd-run ... --unit=nas-system-update /bin/bash /opt/nas_debian/src/core/updater.sh --yes` o dicho script directamente.
  3. Además, `system_update_run` comprueba `systemctl is-active nas-system-update` (línea `389`), unidad que nunca se crea al invocar directamente el binario `nas`, y prioriza `paths.repo.join("target/release/nas")` (líneas `400-405`), pudiendo ejecutar un binario de compilación distinto al instalado.
- **Impacto:** El botón **"Actualizar ahora"** del panel web no puede actualizar `/opt/nas_debian` ni `/usr/local/bin/` en producción.
- **Remediación recomendada:** Añadir una regla `sudoers` acotada para `/usr/local/bin/nas --json update apply --repo ftole/nas_debian --root /opt/nas_debian --yes` (o lanzar vía `systemd-run --unit=nas-system-update`) e invocarlo con `sudo -n`.

---

### A-02 • Deriva severa entre `packaging/` y `crates/nas-engine/src/deploy/`

- **Evidencia en código:** `crates/nas-engine/src/deploy/web.rs:9-15`, `deploy/helpers.rs:20-27`, `deploy/artifacts.rs:54-60`, `deploy/samba.rs:50-84` frente a `packaging/nas-web.service`, `packaging/install.sh`, `packaging/nginx-nas-web.conf` y `packaging/nas.rules`.
- **Contexto histórico:** El commit `e258d56` (*"fix: elimina /usr/local/sbin de ReadWritePaths en la unidad systemd embebida"*) tocó `deploy/web.rs`, pero dejó intacta la deriva de fondo.
- **Diferencias verificadas en `0f7a272`:**
  1. **`SYSTEMD_UNIT` (`deploy/web.rs:9`) vs `packaging/nas-web.service`:** la plantilla embebida carece de `NoNewPrivileges=no`, `ProtectKernelTunables=yes`, `ProtectKernelModules=yes`, `ProtectControlGroups=yes`, `RestrictAddressFamilies`, `RestrictNamespaces`, `LockPersonality`, `MemoryDenyWriteExecute`, `SystemCallFilter=@system-service`, `CapabilityBoundingSet`, `AmbientCapabilities` y omite `-/etc/cron.d -/etc/backup-credentials -/etc/samba` de `ReadWritePaths`.
  2. **`SUDOERS` (`deploy/web.rs:15`) vs `packaging/install.sh:209-235`:** la constante embebida solo incluye **5 reglas** (`systemctl restart nas-web`, `nas helper *`, `nas-samba-apply`, `nas-storage-apply`, `/usr/bin/true`). Omite todas las reglas de `smbd`, `nmbd`, `wsdd2`, `nginx`, `sssd`, `cron`, `reboot`, `smbstatus`, `testparm`, `realm`, `btrfs`, `fstrim`, `nas-share-dir`, `nas-share-acl`, `nas-accounts`, `pdbedit`, `passwd`, `nas-backup-manage`, `nas-terminal`.
  3. **`ROOT_HELPERS` (`deploy/helpers.rs:20-27`):** lista solo **6 helpers**, omitiendo **`nas-share-dir`** que sí instala `packaging/install.sh:154`.
  4. **`NGINX_CONF` (`deploy/web.rs:12`):** carece de las directivas `gzip` presentes en `packaging/nginx-nas-web.conf:25-28`.
  5. **`AUDIT_RULES` (`deploy/artifacts.rs:54-60`) vs `packaging/nas.rules`:** omite `-D`, `-b 8192`, `-f 1`, usa clave `samba_config` en lugar de `samba_conf_changes` y añade `/usr/local/bin/nas -p x -k nas_cli`.
  6. **`global_smb_conf` (`deploy/samba.rs:50-84`):** omite `recycle:exclude` y `recycle:excludedir` presentes en `deploy/artifacts.rs:85-86`.
- **Impacto:** Ejecutar `nas deploy apply` después de `install.sh` sobrescribe `/etc/sudoers.d/nas-web-rust` y `/etc/systemd/system/nas-web.service`, degradando el endurecimiento y rompiendo la gestión de servicios, AD, usuarios, terminal PTY y backups desde la web.
- **Remediación recomendada:** Unificar una única fuente de verdad (preferiblemente embebiendo o compartiendo las plantillas de `packaging/`) y añadir un test de paridad en CI entre `packaging/` y `deploy/`.

---

### A-03 • Hallazgo `H1`: `logrotate` de respaldos sin directiva `su root grp_samba`

- **Evidencia en código:** `crates/nas-engine/src/deploy/artifacts.rs:44-52` frente a `packaging/install.sh:83`.
- **Diagnóstico:** `/srv/nas/LOGS_BACKUP` se crea con permisos `2770 root:grp_samba` (escribible por el grupo `grp_samba`). Sin embargo, `LOGROTATE_BACKUPS` en `deploy/artifacts.rs` define la rotación sobre `/srv/nas/LOGS_BACKUP/*.log` sin especificar `su root grp_samba`.
- **Impacto:** `logrotate` en Debian rechaza rotar logs alojados en directorios escribibles por grupo cuando falta la directiva `su`, dejando crecer los registros indefinidamente.
- **Remediación recomendada:** Añadir `su root grp_samba` dentro de `LOGROTATE_BACKUPS` en `crates/nas-engine/src/deploy/artifacts.rs`.

---

### A-04 • SQL `UPDATE users` silenciosamente roto en `packaging/install.sh`

- **Evidencia en código:** `packaging/install.sh:141-143` frente a `crates/nas-web/src/db.rs:127-132`.
- **Diagnóstico:** La línea `142` de `packaging/install.sh` ejecuta:
  ```bash
  sqlite3 "$STATE_DIR/nas.sqlite" "UPDATE users SET password_hash='$hash' WHERE username='admin';" 2>/dev/null || true
  ```
  Sin embargo, la tabla real en `db.rs` se llama **`web_users`** y la columna **`password_sha256`**. Al ocultar stderr con `2>/dev/null || true`, la consulta falla en silencio cada vez que existe una base SQLite previa.
- **Impacto:** Reinstalar o reconfigurar con `NAS_WEB_PASSWORD=...` sobre un servidor ya inicializado no actualiza la contraseña de `admin` y deja un mensaje engañoso de éxito.
- **Remediación recomendada:** Corregir la sentencia a `UPDATE web_users SET password_sha256=... WHERE username='admin';` (o derivar el hash PBKDF2 vía CLI) y eliminar la supresión silenciosa de error SQL.

---

### A-05 • Lectura muerta de `gpg_verified` frente a firma real `Ed25519`

- **Evidencia en código:** `crates/nas-web/src/routes/system.rs:179-220` y `crates/nas-web/assets/js/modules/system.js:18-23` frente a `scripts/package.sh:58-70` y `install.sh`.
- **Diagnóstico:** El endpoint `/api/system/updates` lee la clave `"gpg_verified"` de `version.json` con valor por defecto `false`. Solo `install.sh` escribe `"gpg_verified": true` en `version.json`; `scripts/package.sh` (usado para los tarballs oficiales firmados con `Ed25519`) **no escribe `gpg_verified`**. En consecuencia, un servidor actualizado o instalado desde tarball reporta `"Sin firma GPG"` en la UI aunque haya sido verificado con `Ed25519` (`packaging/release.pub`).
- **Remediación recomendada:** Reemplazar el campo y badge `gpg_verified` por metadatos explícitos de verificación `Ed25519` (`ed25519_verified` / presencia de firma validada en el manifiesto).

---

### A-06 • Comentario y mensaje de error obsoletos de `256 MiB` en descarga ZIP

- **Evidencia en código:** `crates/nas-web/src/files/zip.rs:15, 89, 277`.
- **Diagnóstico:** La constante real `ZIP_MAX_BYTES` en la línea `15` fija el límite en `50 * 1024 * 1024 * 1024` (**50 GiB**). Sin embargo, el comentario doc de la línea `89` y el mensaje de error en tiempo de ejecución de la línea `277` aún indican `"256 MiB"`.
- **Remediación recomendada:** Actualizar ambos textos en `files/zip.rs` para usar `ZIP_MAX_BYTES` dinámicamente (tal como ya hace la línea `261`).

---

### A-07 • Textos y campos residuales de la etapa PHP / Git / GPG

- **Evidencia verificada en `0f7a272`:**
  1. `crates/nas-web/templates/layout.html:1655`: botón con `id="btn-reload-php" onclick="reloadPhpService(this)"`.
  2. `crates/nas-web/templates/layout.html:3279, 3285`: el modal de actualización sigue describiendo *"Avance rápido (Fast-Forward) y verificación de firma GPG del tag de release"* y *"sintaxis de todos los scripts y módulos"* en lugar de la descarga de artefacto tarball, hash `SHA-256`, firma `Ed25519` y slots A/B.
  3. `crates/nas-web/assets/js/modules/system.js:18-23, 172`: badges `"Sin firma GPG"` / `"Tag criptográficamente firmado y verificado con GPG"` y fase *"Avance rápido y verificación de firmas criptográficas GPG"*.
  4. `crates/nas-web/src/routes/system.rs:334`: mantiene la clave JSON `"php_version"` junto a `"rust_version"`.
  5. `crates/nas-engine/src/services.rs:31`: `is_allowed_service()` sigue aceptando `php*-fpm` en su lista blanca pese a haberse retirado el stack PHP.
  6. `crates/nas-cli/src/commands/update.rs:48`: imprime `"Requisito de firma: Ed25519 / GPG obligatorio"`.

---

### A-08 • Referencia huérfana a `80-udisks2-hide-os.rules`

- **Evidencia en código:** `crates/nas-engine/src/uninstall.rs:124`.
- **Diagnóstico:** Ni `packaging/` ni `crates/nas-engine/src/deploy/` crean `/etc/udev/rules.d/80-udisks2-hide-os.rules` (la protección real del disco del sistema operativo se implementa en `is_os_disk()` en Rust). Sin embargo, `uninstall.rs` aún incluye esa ruta en su paso de limpieza y la documentación antigua la describía como mecanismo activo.

---

### A-09 • `nas update apply` no sincroniza `version.json` activo ni helpers `/usr/local/sbin`

- **Evidencia en código:** `crates/nas-engine/src/update.rs:152-216`, `crates/nas-web/src/routes/system.rs:162-167`, `crates/nas-cli/src/commands/update.rs:24`.
- **Diagnóstico:** `build_plan()` extrae el tarball en `/opt/nas_debian/releases/<version>/`, enlaza `/opt/nas_debian/current`, copia únicamente los 5 binarios a `/usr/local/bin/`, valida Nginx y reinicia `nginx` + `nas-web`. Sin embargo, no copia `version.json` hacia `/etc/nas/version.json` ni `/opt/nas_debian/version.json`, ni reinstala los 7 helpers en `/usr/local/sbin/`. Además, ni `system_updates` (`system.rs:162-167`) ni `nas update status` (`commands/update.rs:24`) leen `/opt/nas_debian/current/version.json`.
- **Impacto:** Tras actualizar vía `nas update apply`, el panel puede seguir mostrando el `version.json` antiguo (commit y notas previos) y las actualizaciones que añadan cambios en `packaging/nas-*` no actualizarán los helpers root.
- **Remediación recomendada:** Añadir `/opt/nas_debian/current/version.json` como primera ruta candidata en `system.rs` y `commands/update.rs`, e incluir la instalación segura de los 7 helpers y `version.json` dentro de `build_plan()` (y su reversión en `rollback`).

---

## 3. Verificación de Documentación Consolidada

Como parte de este trabajo de auditoría y reorganización documental sobre `0f7a272`:
- Se retiraron los 13 documentos redundantes u obsoletos que arrastraban datos de la era PHP/Bash o fragmentaban guías operativas y de desarrollo.
- Se consolidó la documentación técnica en **7 documentos en `docs/`** (`arquitectura.md`, `tecnologias.md`, `seguridad.md`, `manual.md`, `lab_y_desarrollo.md`, `auditoria.md` y `propuesta_ux_usuarios.md`) junto con `README.md` y 7 diagramas SVG actualizados en `docs/assets/`.
