# Manual Operativo, Administración y Diagnóstico

> [!NOTE]
> Este manual consolida la puesta en marcha, administración diaria por perfiles, mantenimiento preventivo, endurecimiento manual de Debian 13, resolución de incidentes y la matriz FMEA (*Failure Mode and Effects Analysis* — Análisis de Modos de Falla y Efectos) para la plataforma Rust `0.1.12`.

---

## 1. Guía Rápida por Perfiles de Uso

### 1.1 Administrador de Sistemas / Infraestructura
- Instala el servidor (`install.sh`) y aprovisiona el rol (`sudo nas deploy apply`).
- Configura el almacenamiento (`/srv/nas`, LVM, Btrfs), las reglas de red/dominio Active Directory, los usuarios/grupos corporativos y el endurecimiento manual de Debian 13 (`UFW`, SSH, `fail2ban`, `AIDE`).
- Audita el estado con `nas diag`, `nas audit verify` y la vista **Diagnóstico / Auditoría** del panel web.

### 1.2 Operador de Respaldos
- Programa tareas CIFS (Windows), SSH (Linux) o Locales desde la vista **Respaldos** del panel o `nas backup`.
- Verifica el uso de disco por debajo del **85%** (`>95%` o `<2 GiB` aborta la tarea), revisa bitácoras en `/srv/nas/LOGS_BACKUP/` y comprueba la conservación de *Golden Snapshots* (`auto_permanent`).

### 1.3 Usuario Final Windows (Red Local)
- Accede mediante Explorador de Windows (`\\SRV-NAS` o `\\<IP>`), descubierto automáticamente por **WSDD2** (sin SMBv1).
- Recupera documentos borrados por accidente desde la carpeta oculta `.recycle` dentro de cada recurso compartido antes de la purga periódica (>15 días).
- Usa el portal web HTTPS (si tiene cuenta asignada) para subir/descargar carpetas ZIP o previsualizar hojas Excel, Word y PowerPoint sin instalar software adicional.

---

## 2. Instalación y Puesta en Marcha del Servidor

### 2.1 Requisitos Mínimos (Debian 13 Trixie `x86_64`)
- Sistema base Debian 13 limpio con acceso `root` o `sudo`.
- Disco separado recomendado para datos (`/dev/sdb`, `/dev/nvme1n1`, etc.), o uso explícito de `LOCAL` en laboratorio.
- Conectividad LAN estable y sincronización horaria (`systemd-timesyncd` / NTP).

### 2.2 Paso 1: Instalar la Plataforma Rust (`install.sh`)

```bash
curl -fsSL https://raw.githubusercontent.com/ftole/nas_debian/main/install.sh | sudo bash
```

O compilando desde el código fuente del repositorio:

```bash
sudo bash install.sh --build-from-source
```

`install.sh` y `packaging/install.sh` instalan los binarios (`nas`, `nas-web`, `nas-tui`, `nasctl`, `nas-dev`, symlink `nas-cli -> nas`), los 7 helpers privilegiados en `/usr/local/sbin/`, crean el usuario de servicio `nas-web`, inicializan `/etc/nas-web.env`, configuran `nas-web.service` + Nginx TLS autofirmado en `:443` y validan `/etc/sudoers.d/nas-web-rust`.

### 2.3 Paso 2: Aprovisionar Disco, Rol y Samba (`nas deploy apply`)

Selecciona uno de los **3 roles oficiales**:
- **`ARCHIVOS`**: NAS departamental (`ext4` por defecto).
- **`BACKUP`**: Central de respaldos con directorios dedicados `BACKUPS_HISTORICOS`, `BACKUPS_PERMANENTES` y `LOGS_BACKUP` en `2770` (`Btrfs` con `zstd:3` por defecto, más soporte para recursos ocultos `$` en Samba).
- **`ARCHIVOS_BACKUP`**: Servidor híbrido unificado (`Btrfs` con `zstd:3` por defecto).

```bash
# Ejemplo rol ARCHIVOS sobre disco dedicado /dev/sdb
sudo nas deploy apply \
  --disk /dev/sdb \
  --role ARCHIVOS \
  --workgroup TEAM-JOFRATO \
  --netbios SRV-NAS \
  --admin admin

# Ejemplo rol BACKUP en disco SSD reutilizando datos previos (--keep-data)
sudo nas deploy apply \
  --disk /dev/sdb \
  --role BACKUP \
  --ssd \
  --keep-data \
  --workgroup TEAM-JOFRATO \
  --netbios SRV-BKP \
  --admin admin
```

> [!NOTE]
> **Actualización desde versiones `<= 0.1.10`:** Tras ejecutar `sudo nas update apply`, ejecuta `sudo bash /opt/nas_debian/current/packaging/install.sh` una sola vez para refrescar `/etc/sudoers.d/nas-web-rust`, helpers y la unidad `nas-web.service`; a partir de `v0.1.11`, las actualizaciones posteriores pueden lanzarse directamente desde el panel web.

---

## 3. Interfaces de Administración

### 3.1 CLI Unificado `nas` (`--json` disponible en todos los subcomandos)

| Comando | Propósito |
| :--- | :--- |
| `nas status` | Métricas de CPU, RAM, tiempo activo e IP principal. |
| `nas network` | Inventario de interfaces, puerta de enlace, DNS, Samba y puertos. |
| `nas services` | Estado de `smbd`, `nmbd`, `wsdd2`, `nginx` y `nas-web`. |
| `nas diag` | Diagnóstico integral con código de salida significativo para scripts. |
| `nas storage <subcomando>` | Inventario de discos (mostrando `protected: true` en el disco del SO), particiones y uso de `/srv/nas`. |
| `nas helper storage-apply <acción>` | Acciones privilegiadas de almacenamiento: `create-pool` (1 o N discos: `btrfs_single`, `btrfs_raid1`, `ext4`, `lvm_ext4`, `lvm_btrfs`), `extend-pool-target`, `remove-pool-device`, `umount-unit`, `wipe-disk`, `sanitize-fstab`, `scrub-unit-status`, `scrub-unit-start` y `trim-unit`. |
| `nas users <subcomando>` | Listado e inventario de usuarios/grupos del sistema y Samba. |
| `nas backup <subcomando>` | Listado de tareas de respaldo, ejecución, bitácoras, Golden Snapshots, `repair-runners` y `deduplicate`. |
| `nas audit <subcomando>` | Consulta y verificación criptográfica SHA-256 de la bitácora `nas-audit`. |
| `nas update <status \| check \| verify \| plan \| apply>` | Consulta de estado, verificación `Ed25519` y actualización atómica A/B desde consola (acepta `--previous <ver>` para rollback automático ante fallo). |
| `nas uninstall [--yes --purge --scope os]` | Desinstalación controlada (conserva siempre `/srv/nas`). |

### 3.2 Asistente Visual de Terminal (`nas-tui`)

Ejecuta `nas-tui` (o `sudo nas-tui` para operaciones de sistema) sobre cualquier sesión TTY/SSH. Incluye **12 secciones** accesibles con `Tab` / `Shift+Tab` / flechas (`q` o `Esc` para salir, `F5` o `r` para refrescar):
`Dashboard`, `Network`, `Shares`, `Users`, `Backups`, `Services`, `Diagnostics`, `Storage`, `Files`, `Domain`, `Terminal` y `Quit`.

### 3.3 Panel Web Slate UI (14 Vistas de Navegación + Auditoría)

Accesible en `https://<IP-del-servidor>` (usuario inicial configurado durante la instalación; la contraseña autogenerada inicial se guarda temporalmente en `/etc/nas/.initial_admin_password`):

1. **Dashboard:** resumen de carga, disco `/srv/nas`, demonios activos y estado de respaldos.
2. **Archivos (`files`):** explorador Drag & Drop sobre `/srv/nas`, descarga ZIP (hasta 50 GiB) y visores 100% offline para `.xlsx`/`.xls`/`.ods`/`.csv`, `.docx`, `.pptx`, imágenes y texto.
3. **Bitácoras (`logs`):** consulta de logs de Samba, sistema y tareas de respaldo.
4. **Carpetas Compartidas (`shares`):** alta/edición de recursos SMB, visibilidad (`$` oculto) y esquemas de ACL POSIX `2770` + herencia por defecto.
5. **Respaldos (`backups`):** gestión de tareas CIFS/SSH/Locales, credenciales vía `stdin`, ejecución bajo demanda y Golden Snapshots.
6. **Almacenamiento (`storage`):** discos físicos (bloqueo estricto en disco del SO y miembros de pools Btrfs activos), asistente de creación de pools uni/multi-disco (`btrfs_single`, `btrfs_raid1`, `ext4`, `lvm_ext4`, `lvm_btrfs`) en `/srv/nas` o `/srv/nas/unidades/<id>`, ampliación en línea (LVM y Btrfs con conversión opcional de metadatos a RAID1), retiro seguro de miembros Btrfs (`btrfs device remove`), desmontaje LIFO de unidades, borrado de firmas (`SI-DESTRUIR-DATOS`), auditoría y saneamiento de `/etc/fstab` (`nofail`), subvolúmenes Btrfs y `scrub`/`fstrim` por unidad.
7. **Terminal (`terminal`):** PTY interactiva real respaldada por sesiones `tmux` aisladas por usuario (`nas-terminal`).
8. **Usuarios y Grupos (`users`):** gestión de cuentas web, usuarios Samba/Linux y membresía de grupos.
9. **Servicios (`services`):** control (`start`, `stop`, `restart`, `reload`) de `smbd`, `nmbd`, `wsdd2`, `nginx`, `cron` y `nas-web`.
10. **Diagnóstico (`diagnostics`):** comprobaciones de red, disco, servicios y permisos.
11. **Red (`networking`):** configuración IP, DNS y puertos activos.
12. **Dominio AD (`domain`):** descubrimiento, unión y salida de Active Directory (`realmd` / `sssd` / `adcli`).
13. **Aplicaciones (`applications`):** resumen de módulos integrados (explorador, Btrfs, motores de copia).
14. **Actualizaciones (`updates`) y Auditoría (`/audit`):** versión instalada vs GitHub Releases y consulta de eventos auditados.

---

## 4. Mantenimiento Periódico y Copias de Configuración

| Frecuencia | Tarea Automática / Manual | Archivo / Comando | Qué Verifica o Mantiene |
| :--- | :--- | :--- | :--- |
| **Diaria** | Purga de papelera SMB | `/etc/cron.daily/nas-recycle-purge` | Elimina archivos en `.recycle` con antigüedad mayor a 15 días. |
| **Semanal** | TRIM en unidades flash | `systemctl status fstrim.timer` | Libera bloques no referenciados en SSD/NVMe sin degradar velocidad de escritura. |
| **Semanal / Mensual** | Rotación de bitácoras | `/etc/logrotate.d/nas-backups` | Comprime logs de respaldo (`copytruncate`). |
| **Mensual (día 1, 02:00 / 03:00)** | Verificación Btrfs | `/etc/cron.d/nas-btrfs-scrub` y `/etc/cron.d/nas-btrfs-scrub-unit-<id>` | Ejecuta `btrfs scrub start -B` sobre `/srv/nas` y cada unidad Btrfs adicional en `/srv/nas/unidades/<id>` contra *Bit Rot* (también ejecutable bajo demanda desde `/dashboard#storage`). |
| **Semanal (Manual)** | Respaldo de estado del panel | Copia segura de `/var/lib/nas/nas.sqlite`, `/etc/nas/`, `/etc/samba/smb.conf` y `/etc/cron.d/backup_*` | Permite restaurar configuración, usuarios web y tareas cron ante fallo de disco raíz. |
| **Mensual (Manual)** | Verificación de auditoría y actualización | `nas audit verify` y `sudo nas update apply` | Comprueba integridad SHA-256 de la bitácora y aplica parches firmados `Ed25519`. |

### 4.1 Parámetros de Montaje y Verificación Rápida en Consola

* **Opciones de montaje aplicadas por rol y hardware:**
  * **`ext4` (`ARCHIVOS`):** `rw,noatime,commit=5,errors=remount-ro` + `tune2fs -m 1` (recupera 4% de espacio reservado); `readahead` `4096 KB` en HDD y `1024 KB` en SSD (`fstrim.timer`).
  * **`Btrfs` (`BACKUP` / `ARCHIVOS_BACKUP`):** `rw,noatime,compress=zstd:3,space_cache=v2` (`ssd` en flash, sin `autodefrag` ni `discard=async` continuo); *scrub* mensual con `ionice` idle (`-c 3`), monitoreo de errores en `btrfs device stats` y deduplicación `rsync --link-dest` (`--no-o --no-g` en CIFS, `stdbuf -oL` y progreso `to-chk`/`ir-chk`).
  * **Resiliencia eléctrica en hardware low-cost:** SQLite en `WAL` + `synchronous=FULL` (`wal_autocheckpoint=200`); entradas secundarias de `/etc/fstab` con `nofail,x-systemd.device-timeout=15s`; toggle opcional por disco para desactivar caché DRAM volátil (`hdparm -W0` + regla `udev` persistente); módulo Plug & Play opcional para UPS USB (`nut`); límites dinámicos `ResourceBudget` en backups (`systemd-run`), `journald` (`SystemMaxUse=256M`) y auto-aprovisionamiento `nas deploy ensure-deps` desde Debian limpio.

```bash
# 1. Salud general del servidor, servicios y diagnósticos
sudo nas status
sudo nas diag
systemctl is-active nas-web smbd nmbd wsdd2 nginx

# 2. Almacenamiento: discos, disco protegido del SO, montaje y S.M.A.R.T.
lsblk -o NAME,SIZE,TYPE,FSTYPE,MOUNTPOINT
findmnt /srv/nas
sudo smartctl -H /dev/sdb

# 3. Auditoría criptográfica y cuotas dinámicas cgroups de nas-web
sudo nas audit verify
systemctl cat nas-web | grep -E "MemoryHigh|MemoryMax|CPUQuota|TasksMax"
```

---

## 5. Checklist de Endurecimiento Manual en Debian 13

> [!NOTE]
> **Automatización pendiente:** La ejecución automática de estos controles vía `nas security harden` durante `install.sh` / actualizaciones está registrada como mejora pendiente en [docs/seguridad.md](./seguridad.md#4-plan-pendiente-automatización-del-endurecimiento-en-rust-nas-security). Mientras tanto, aplícalos con este checklist.

Ejecuta estos pasos después de completar `install.sh` y `nas deploy apply` en un servidor de producción:

```bash
# 1. Cortafuegos UFW restrictivo (ajusta 192.168.1.0/24 a tu subred LAN)
sudo apt-get update && sudo apt-get install -y ufw fail2ban unattended-upgrades aide
sudo sed -i 's/^IPV6=.*/IPV6=no/' /etc/default/ufw
sudo ufw default deny incoming
sudo ufw default allow outgoing
sudo ufw allow from 192.168.1.0/24 to any port 22 proto tcp comment 'SSH LAN'
sudo ufw allow from 192.168.1.0/24 to any port 443 proto tcp comment 'Panel NAS HTTPS'
sudo ufw allow from 192.168.1.0/24 to any port 445 proto tcp comment 'Samba SMB'
sudo ufw allow from 192.168.1.0/24 to any port 139 proto tcp comment 'NetBIOS SMB'
sudo ufw --force enable

# 2. Endurecimiento SSH (solo tras validar tu llave pública en ~/.ssh/authorized_keys)
sudo tee /etc/ssh/sshd_config.d/99-nas-hardening.conf >/dev/null <<'EOF'
PermitRootLogin no
PasswordAuthentication no
PubkeyAuthentication yes
X11Forwarding no
EOF
sudo sshd -t && sudo systemctl reload ssh

# 3. Fail2ban y actualizaciones de seguridad automáticas
sudo systemctl enable --now fail2ban
sudo dpkg-reconfigure -plow unattended-upgrades

# 4. Verificar que la suspensión accidental esté enmascarada (ya automatizado en install.sh / nas deploy)
sudo systemctl mask sleep.target suspend.target hibernate.target hybrid-sleep.target

# 5. Inicializar base criptográfica de integridad offline (AIDE) al cerrar cambios
sudo aideinit
sudo mv /var/lib/aide/aide.db.new /var/lib/aide/aide.db
```

---

## 6. Resolución de Problemas Frecuentes

| Síntoma | Causa Habitual | Solución Verificada |
| :--- | :--- | :--- |
| **Lentitud o bloqueos en Excel/Office con +100 puestos** | Caché de cliente o falta de flujos NTFS/leases en Samba. | Verifica que `/etc/samba/smb.conf` mantenga `vfs objects = acl_xattr streams_xattr recycle`, `smb3 directory leases = auto`, `strict sync = yes` y `max open files` acorde al `LimitNOFILE` de `smbd` (p. ej. `16320`–`65535`). Prueba con `testparm -s`. |
| **El servidor no aparece en Explorador de Red (Windows 10/11)** | `wsdd2` detenido o puertos bloqueados. | Ejecuta `sudo systemctl status wsdd2 smbd nmbd`; en clientes Windows usa `\\<IP>` o `\\SRV-NAS` y verifica que el perfil de red Windows sea **Privado**. |
| **Error `EAGAIN (11)` al respaldar archivos abiertos en Windows** | Bloqueos de rango de bytes activos en CIFS. | Los runners generados por `nas-backup-manage` ya montan con `ro,vers=3.1.1,noserverino,cache=none,soft,nobrl`. Si editaste un runner manualmente, restáuralo desde el panel. |
| **Credenciales de dominio Windows fallan en backup CIFS** | Sintaxis incorrecta de dominio o contraseña con caracteres especiales. | Usa formato `DOMINIO\usuario` o `DOMINIO/usuario` al guardar la tarea en el panel; las credenciales se escriben por `stdin` en `/etc/backup-credentials/<id>.cred` (`0600`). |
| **Respaldo abortado o directorio `.inprogress_*` residual** | Corte eléctrico repentino o volumen >95% lleno (`<2 GiB`). | Libera espacio (`df -h /srv/nas`), revisa `/srv/nas/LOGS_BACKUP/<id>.log`. La próxima ejecución limpia el `.inprogress_*` previo y adquiere `/var/lock/backup_<id>.lock`. |
| **Error `Permission denied (os error 13)` en `/var/lock/nas-update.lock` al usar `sudo nas update apply`** | Un intento previo ejecutado como `nas-web` dejó `/var/lock/nas-update.lock` con propietario `nas-web:nas-web`. | Elimina el bloqueo huérfano (`sudo rm -f /var/lock/nas-update.lock`), ejecuta `sudo nas update apply --repo ftole/nas_debian --yes` y luego `sudo bash /opt/nas_debian/current/packaging/install.sh` para habilitar actualizaciones desde la web (`v0.1.11+`). |
| **`nginx -t` falla con `bind() to 0.0.0.0:80 failed (13: Permission denied)`** | `nginx -t` se invocó sin privilegios `root`. | Ejecuta `sudo nginx -t` (requiere privilegios de superusuario para probar el socket `:80`/`:443`). |
| **Cambios locales de desarrollo no se reflejan en el navegador** | Activos embebidos en el binario y caché HTTP de `max-age=300`. | Ejecuta `sudo ./scripts/deploy_local.sh` y recarga con `Ctrl + Shift + R`. |
| **Snapshots y `FULL_*` consumen espacio múltiple sin deduplicar** | Runners antiguos aplicaban `chmod -R a-w` o diferían en permisos CIFS (`0755` vs `0777`), rompiendo `--link-dest` y dejando archivos duplicados. | Al actualizar/desplegar se reparan los runners y se lanza una única migración en segundo plano (`nas-backup-dedup-migration`, marcador `/var/lib/nas/migrations/backup_dedup_v2.done`) que normaliza permisos CIFS a `0777` y deduplica por tamaño decreciente. Para inspeccionar o ejecutar manualmente: `sudo nas backup deduplicate --dry-run` y `sudo nas backup deduplicate --task <id>`. |

---

## 7. Matriz FMEA (Análisis de Modos de Falla y Efectos)

| Componente | Modo de Falla | Efecto Operativo | Control Preventivo en Diseño | Acción de Recuperación |
| :--- | :--- | :--- | :--- | :--- |
| **Copia de Seguridad (`rsync`)** | Apagón o caída de red a mitad del snapshot | Directorio incompleto si se promoviera prematuramente | Staging aislado `.inprogress_*` + `trap cleanup EXIT TERM INT`; promoción `mv` solo con código `0` | Ejecutar de nuevo la tarea desde panel/CLI; el histórico previo permanece intacto. |
| **Disco de Respaldos (`Btrfs`)** | Llenado del 100% del volumen | Riesgo de bloqueo de metadatos Btrfs y fallo de snapshots | Umbral híbrido (`2%` entre `2 GiB–32 GiB`, corte `95%–98%`) verificado al inicio y cada 15s durante `rsync` | Ajustar retención, purgar snapshots no permanentes y verificar con `btrfs filesystem df /srv/nas`. |
| **Disco de Datos (`ext4`/`Btrfs`)** | Degradación silenciosa de sectores (*Bit Rot*) o corte eléctrico en disco consumidor | Corrupción inadvertida o bloqueo `ro` del controlador | Sumas de comprobación Btrfs + `btrfs scrub` mensual; `commit=5,errors=remount-ro` + `nofail,x-systemd.device-timeout=15s` en `fstab`; pre-chequeo S.M.A.R.T. y **UPS/No-Break obligatorio en producción** | Ejecutar `sudo btrfs device scan` y probar montaje en **solo lectura** primero (`mount -o ro` o `ro,rescue=usebackuproot`); revisar `smartctl -H` y `btrfs scrub status /srv/nas` sin ejecutar `--repair` ni formatear. |
| **Actualización del Sistema** | Paquete corrupto, firma falsa o fallo tras reinicio | Servicio web caído o binario incompatible | Validación `SHA-256` + firma `Ed25519` previa + despliegue en slots A/B `/opt/nas_debian/releases/` + *healthcheck* `/login` | Si se indicó `--previous <ver>`, `nas update apply` revierte el slot automáticamente; en recuperación manual, reenlazar `/opt/nas_debian/current` al slot previo, reinstalar `/opt/nas_debian/current/bin/*` en `/usr/local/bin/` y reiniciar `nas-web` + `nginx`. |
| **Operación de Almacenamiento** | Selección errónea del disco del sistema operativo | Destrucción del SO Debian y pérdida de arranque | Guardián `is_os_disk()` bloquea formateo/repartición en `/` y `/boot`; confirmación textual `SI-FORMATEAR` | No aplica pérdida si actúa el bloqueo; ante fallo físico de disco SO, reinstalar Debian y usar `--keep-data` sobre el disco de datos. |
| **Sesión / Autenticación Web** | Ataque de fuerza bruta o intento XSS en archivos | Compromiso de sesión administrativa | `api_rate_limits` (HTTP 429), `PBKDF2-HMAC-SHA512`, `HMAC-SHA256` CSRF, `/api/files/raw` en `sandbox`, sanitizador DOM `.xlsx` | Inspeccionar `/audit` y `nas audit verify`; rotar contraseñas y revocar sesiones en `/var/lib/nas/nas.sqlite`. |
