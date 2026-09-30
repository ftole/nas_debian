# Manual Técnico: Servidor NAS y Central de Respaldos (Debian 13)

Este documento describe la arquitectura, el motor de copias de seguridad, el modelo de seguridad y los procedimientos de operación del proyecto. Está dirigido a administradores que necesiten entender el funcionamiento interno o replicar el despliegue. Para la instalación y el uso general, consulta el `README.md`.

## 1. Introducción y alcance

El sistema cubre dos funciones excluyentes:

1. **Servidor de archivos (NAS departamental):** almacenamiento en red para clientes Windows mediante Samba, con descubrimiento WSDD2 y panel Cockpit.
2. **Central de copias de seguridad:** repositorio dedicado a respaldar servidores Windows, servidores Linux, estaciones de trabajo y carpetas locales, con snapshots deduplicados y retención configurable.

> [!IMPORTANT]
> El despliegue base es idéntico para ambos roles: crea el grupo `grp_sistemas` y el directorio `/srv/nas`, sin recursos compartidos. Los grupos y recursos se añaden después desde el asistente según las necesidades del entorno.

## 2. Arquitectura

```text
                         +---------------------------+
                         |     Debian 13 (Trixie)    |
                         +-------------+-------------+
                                       |
          +----------------------------+----------------------------+
          |                            |                            |
  +-------+-------+          +---------+---------+        +---------+---------+
  |  Samba/WSDD2  |          |  Cockpit + web    |        |  Motor de backups |
  |  (SMB / WSD)  |          |  (panel Backups)  |        |  (CIFS/SSH/local) |
  +---------------+          +-------------------+        +-------------------+
          |                            |                            |
  +-------+-------+          +---------+---------+        +---------+---------+
  |  VFS acl/ads  |          |  Python 3 API     |        |  Staging atómico  |
  |  Office tuning|          |  JSON backend     |        |  .inprogress_*    |
  +---------------+          +-------------------+        +-------------------+
```

Componentes principales:

- **Instalador y CLI `nas`** (`install.sh`): despliega el proyecto en `/opt/nas_debian` y crea el comando global `/usr/local/bin/nas`.
- **Asistente de terminal** (`src/asistente.sh` y `src/modules/`): menú interactivo TUI de 9 módulos basado en `whiptail`.
- **Motor de despliegue** (`src/core/deploy.sh`): particionado inteligente, formateo o conservación de datos (`--keep-data`), configuración optimizada de filesystem (`ext4` o `btrfs`), Samba, Cockpit, reglas `udev` y tuning de kernel `sysctl`.
- **Motor de backups** (`src/modules/backups.sh` y `src/web/backups/backup_api.py`): runners autónomos de copia con staging atómico, chequeo preventivo de espacio y deduplicación.
- **Panel web Cockpit** (`src/web/backups/`): interfaz web nativa PatternFly 4 para gestión gráfica de tareas, historial y bitácoras.

### 2.1 Cuadro Maestro de Tecnologías, Subsistemas y Librerías

La siguiente matriz documenta exhaustivamente todos los componentes, librerías, subsistemas del kernel y herramientas que sustentan la infraestructura, detallando su función técnica y los beneficios que aportan al entorno de producción:

| Tecnología / Subsistema / Librería | Componente / Ámbito | Para qué sirve | Beneficio que aporta |
| :--- | :--- | :--- | :--- |
| **Debian 13 (Trixie) x86_64** | Sistema Operativo Base | Plataforma del sistema operativo Linux con kernel 6.12+ y glibc moderna. | Estabilidad empresarial, soporte extendido y compatibilidad directa con hardware de almacenamiento contemporáneo. |
| **Samba 4 (`smbd` / `nmbd`)** | Servicio de Red SMB/CIFS | Demonio de compartición de archivos y resolución de nombres NetBIOS. | Compatibilidad nativa con clientes Windows 10/11 y servidores corporativos bajo protocolos seguros. |
| **Módulos VFS Samba (`acl_xattr`, `streams_xattr`)** | Capa VFS de Samba | Emulación de flujos de datos alternativos (ADS) de NTFS y almacenamiento de ACLs de Windows en atributos extendidos POSIX (`xattr`). | **Prevención de cuellos de botella con +100 equipos en Excel/Office**: permite concurrencia sin bloqueos de archivos temporales (`~$`), guardado atómico y preservación de marcas de seguridad Windows (`Zone.Identifier`). |
| **Directivas Samba para Alto Rendimiento** | Configuración `smb.conf` | `store dos attributes = yes`, `inherit permissions = yes`, `strict sync = yes`, `use sendfile = yes`, `aio read/write size = 16384`, `max open files = 65535`. | Transferencia de archivos directo de red a disco sin saltos de memoria de usuario; latencia ultrabaja en apertura de libros contables masivos y eliminación de bloqueos por agotamiento de descriptores. |
| **ext4 con optimizaciones (`tune2fs -m 1`, `commit=2`/`commit=5`)** | Sistema de Archivos (`ARCHIVOS`) | Filesystem con transacciones de *journal* para el servidor NAS departamental. | Recuperación del 4% de espacio reservado para superusuario (`-m 1`); sincronización de transacciones cada 2 segundos (`commit=2` en HDD) que minimiza la ventana de pérdida ante **cortes de energía o apagones**; reducción del 30% en I/O con `noatime`. |
| **BTRFS con Zstandard (`compress=zstd:3`, `space_cache=v2`)** | Sistema de Archivos (`BACKUP`) | Filesystem avanzado con compresión transparente y sumas de comprobación integradas. | **Ahorro de espacio físico entre 20% y 40%** sin sobrecarga apreciable de CPU; asignación inmediata de bloques libres (`space_cache=v2`); integridad verificable bloque a bloque mediante hashes criptográficos nativos. |
| **BTRFS Scrub Programado (`nas-btrfs-scrub`)** | Mantenimiento Mensual | Tarea programada en `/etc/cron.d/nas-btrfs-scrub` que ejecuta `btrfs scrub start -B /srv/nas` el día 1 de cada mes a las 02:00. | **Prevención y detección de corrupción silenciosa (*Bit Rot*)**: valida cada bloque contra su suma de comprobación y repara sectores inconsistentes automáticamente antes de que afecten a las copias de seguridad. |
| **rsync (`-aAXH --numeric-ids --link-dest`)** | Motor de Replicación | Sincronización diferencial y creación de snapshots mediante enlaces duros. | **Deduplicación superior al 85%**: los archivos no modificados comparten el inodo en disco consumiendo 0 bytes extras; preservación exacta de ACLs (`-A`), xattrs (`-X`), hardlinks (`-H`) y números de UID/GID exactos sin alteración. |
| **Staging Atómico (`.inprogress_*`)** | Pipeline de Backups | Escritura en directorio temporal oculto y promoción atómica (`mv`) al finalizar. | **Resiliencia crítica contra apagones y fallos de red**: las rutinas de limpieza (`trap cleanup EXIT TERM INT`) eliminan copias incompletas; el repositorio histórico únicamente expone snapshots 100% íntegros. |
| **cifs-utils (`mount.cifs`) SMB 3.1.1** | Conector Windows | Montaje temporal en solo lectura (`ro,vers=3.1.1,noserverino,cache=none,soft,timeo=30`). | Protocolo de cifrado y firmas modernas SMB 3.1.1; `noserverino` previene errores de inodos remotos; `cache=none` asegura lectura de datos frescos; `soft,timeo=30` evita cuelgues del kernel si el host remoto de Windows se reinicia o desconecta. |
| **Desglose de Dominios Active Directory** | Autenticación de Red | Parser en el asistente y en la API que detecta sintaxis `DOMINIO\usuario` y `DOMINIO/usuario`. | Permite conectar a carpetas compartidas corporativas protegidas por Directorio Activo sin exponer contraseñas en memoria de procesos (`ps`) mediante archivos de credenciales `0600 root:root`. |
| **OpenSSH / sshpass (`StrictHostKeyChecking=accept-new`)** | Conector Linux | Replicación remota cifrada por SSH con almacenamiento de firmas en `/root/.ssh/known_hosts_backup`. | Previene ataques de intermediario (*Man-in-the-Middle*) al registrar hosts nuevos automáticamente sin intervención manual y sin deshabilitar la comprobación de claves. |
| **Tuning de Kernel sysctl (`99-nas-tuning.conf`)** | Parámetros del Kernel | Configuración de límites del VFS, monitoreo inotify y reciclaje de memoria sucia. | `fs.inotify` masivo (524,288 watches); `tcp_keepalive` (120s/15s/4) limpia sesiones SMB inactivas en 3 minutos en lugar de 2 horas; `vm.dirty_bytes=256MB` fuerza ráfagas breves de escritura a disco, previniendo congelamientos de I/O por saturación de RAM. |
| **Readahead Tuning udev (`60-nas-readahead.rules`)** | Subsistema de Bloques | Reglas udev persistentes para precarga de disco (`1024 KB` en SSD / `4096 KB` en HDD). | Aumenta el rendimiento sostenido en lecturas secuenciales pesadas a través de la red y agiliza las comparaciones diferenciales de `rsync`. |
| **Protección udev del Disco del SO (`80-udisks2-hide-os.rules`)** | Aislamiento de Almacenamiento | Asignación de la bandera `UDISKS_IGNORE="1"` al disco base del sistema operativo. | Oculta el disco del sistema en la interfaz de Cockpit Storage y UDisks2, impidiendo su borrado o modificación inadvertida. |
| **Reutilización de Almacenamiento (`--keep-data`)** | Motor de Despliegue | Detección de particiones preexistentes y montaje sin formateo en `/srv/nas`. | Facilita reinstalaciones y migraciones de servidor sin requerir volcado externo ni poner en riesgo datos ya almacenados. |
| **Cockpit + PatternFly 4 + 45Drives Plugins** | Interfaz Web | Panel administrativo modular sin servicios residentes pesados (activación por socket `systemd`). | Consumo despreciable de memoria en reposo, diseño responsivo estandarizado y gestión gráfica intuitiva. |
| **Python 3 (`backup_api.py`)** | Backend API para Cockpit | Puente de comandos estructurado en JSON con saneamiento de parámetros. | Elimina vectores de inyección de comandos, ejecuta comprobaciones con privilegios acotados y ofrece lectura retrospectiva de bitácoras sin saturar la UI. |
| **whiptail + Bash 5** | Interfaz Visual TUI | Asistente de terminal interactivo con detección automática de recursos y validaciones en vivo. | Gestión integral del servidor desde la consola local o sesiones SSH sin necesidad de interfaz gráfica X11. |
| **Control de Concurrencia con `flock`** | Programación de Tareas | Bloqueo por descriptor de archivo en `/var/lock/backup_<tarea>.lock`. | Garantiza la exclusión mutua de procesos impidiendo sobrecargas o escrituras simultáneas sobre una misma tarea. |
| **Monitoreo de Umbrales de Espacio (`df -Pk`)** | Seguridad Operativa | Comprobación de capacidad previa al inicio de cada respaldo. | Emite alertas tempranas al superar el 85% de uso y cancela la copia si restan menos de 2 GB o se supera el 95%, evitando la corrupción por desbordamiento del filesystem. |
| **`fstrim.timer`** | Mantenimiento para SSD | Tarea periódica de descarte de bloques no referenciados en almacenamiento flash. | Mantiene velocidades de escritura constantes y optimiza la vida útil de los dispositivos de estado sólido. |
| **`logrotate` (`nas-backups`, `nas-deploy`)** | Mantenimiento de Bitácoras | Rotación semanal/mensual con directiva `copytruncate` y compresión `gzip`. | Mantiene controlados los registros de actividad evitando que saturen el espacio de almacenamiento. |
| **WSDD2 con Override Systemd** | Descubrimiento de Red | Implementación ligera del protocolo Web Services Discovery y LLMNR. | Visibilidad instantánea en el explorador de red de Windows 10/11 sin activar protocolos obsoletos ni inseguros como NetBIOS broadcast o SMBv1. |
| **Aislamiento de Idioma (`LC_ALL=C LANG=C`)** | Compatibilidad de Sistema | Envoltorios de ejecución en `/usr/local/sbin/chage`, `passwd` y `lastb`. | Normaliza las salidas de utilidades administrativas al estándar en inglés, evitando fallos de parseo en Cockpit en servidores instalados en español. |
| **ACLs POSIX (`setfacl`) con Herencia por Defecto** | Control de Acceso Granular | Configuración de permisos multi-grupo y reglas por defecto (`default ACL`) en carpetas compartidas. | Permite esquemas mixtos donde coexisten grupos con solo lectura y grupos con permisos de escritura exclusiva, garantizando que todo nuevo archivo herede los permisos correctos. |


## 3. Motor de copias de seguridad

### 3.1 Snapshots, deduplicación y staging atómico

- **Staging atómico (`.inprogress_*`):** Toda copia se escribe inicialmente en el directorio temporal oculto `/srv/nas/BACKUPS_HISTORICOS/<tarea>/.inprogress_YYYY-MM-DD_HHMMSS`. Solo cuando el proceso de sincronización concluye con éxito total (código de salida 0), se promueve de forma atómica mediante `mv` a `snapshot_YYYY-MM-DD_HHMMSS`. Esto previene que se expongan copias incompletas en el histórico.
- **Deduplicación por enlaces duros (*hardlinks*):** El comando `rsync --link-dest` localiza el snapshot cronológico más reciente (`find ... -name 'snapshot_*' | sort | tail -n 1`). Los archivos cuyos metadatos y contenido no hayan variado comparten el mismo inodo físico en disco, alcanzando una tasa de deduplicación superior al 85% sin penalización de CPU ni licencias propietarias.
- **Monitoreo preventivo de espacio en disco:** Antes de iniciar la transferencia, el runner evalúa el almacenamiento mediante `df -Pk "$BKP_DIR"`. Si el porcentaje de uso supera el 85%, se genera una advertencia en la bitácora. Si el espacio libre es inferior a 2 GB (2,097,152 KB) o la ocupación supera el 95%, el script aborta la operación de inmediato para impedir la saturación del sistema de archivos.
- **Política de retención cronológica:** Al culminar cada copia exitosa, el sistema contabiliza los snapshots disponibles. Si superan la cuota configurada $N$ (por defecto 30 para Windows/Local y 15 para Linux), se eliminan los más antiguos por orden de nombre cronológico (`sort | head -n -"$RETENTION"`). Los datos de archivos compartidos no se eliminan físicamente gracias al contador de enlaces del sistema de archivos Linux.

### 3.2 Orígenes soportados

- **Windows (CIFS / SMB 3.1.1):** 
  - Montaje temporal en solo lectura: `mount -t cifs "//$SRC_IP/$SRC_SHARE" "$MOUNT_POINT" -o credentials="$CRED_FILE",ro,iocharset=utf8,vers=3.1.1,noserverino,cache=none,soft,timeo=30`.
  - Las credenciales se resguardan en `/etc/backup-credentials/<tarea>.cred` con permisos `0600 root:root`.
  - **Soporte de dominios corporativos:** El sistema desglosa automáticamente cuentas locales y cuentas de Directorio Activo (`DOMINIO\usuario` y `DOMINIO/usuario`), extrayendo los parámetros `username`, `password` y `domain` en el archivo de credenciales.
  - Parámetros de resiliencia: `vers=3.1.1` garantiza protocolos criptográficos actuales; `noserverino` previene fallos por IDs de inodo generados por servidores remotos; `cache=none` evita lecturas desactualizadas; `soft,timeo=30` impide bloqueos indefinidos del kernel si el host Windows se apaga o reinicia durante el proceso.
- **Linux (SSH):** 
  - Sincronización diferencial directa mediante `rsync -aAXH --numeric-ids -v -z --timeout=60` a través de un túnel SSH autenticado mediante `sshpass`.
  - Las firmas digitales de los hosts remotos se validan y almacenan automáticamente en el primer contacto mediante `StrictHostKeyChecking=accept-new` en un archivo aislado `/root/.ssh/known_hosts_backup`.
  - Preserva íntegramente permisos POSIX, propietarios y grupos numéricos (`--numeric-ids`), enlaces duros (`-H`), atributos extendidos (`-X`) y ACLs (`-A`).
- **Local:** 
  - Copia directa sobre el propio servidor mediante `rsync -aAXH --numeric-ids --timeout=60`, aprovechando la deduplicación por inodos entre directorios del almacenamiento.

### 3.3 Programación, concurrencia y resiliencia ante cortes

- **Lanzador de tareas:** Cada tarea programada en `cron` se despacha mediante `systemd-run --collect` asignando prioridad acotada (*nice/ionice*) para no impactar las operaciones interactivas de red.
- **Exclusión mutua con `flock`:** Se aplica un cerrojo exclusivo sobre `/var/lock/backup_<tarea>.lock` (`flock -n 9`). Si una tarea previa continúa en curso, la nueva ejecución se omite limpiamente dejando constancia en el log.
- **Protección contra apagones y manejadores de señales:** Los runners configuran interceptores de salida (`trap cleanup EXIT`, `trap 'exit 143' TERM`, `trap 'exit 130' INT`). Si la tarea es abortada manualmente (desde el asistente o Cockpit), se pierde la conexión de red o el servidor experimenta un apagón, el recurso remoto se desmonta de inmediato y el directorio temporal `.inprogress_*` se destruye, garantizando que el almacenamiento nunca aloje copias a medio escribir.

---

## 4. Modelo de seguridad y tuning del sistema

### 4.1 Selección condicional de sistema de archivos (Filesystem por Rol y Medio)

Durante el despliegue, el sistema detecta si la unidad de almacenamiento es un disco mecánico rotacional (`rotational=1`) o un dispositivo de estado sólido flash (`rotational=0`) leyendo `/sys/block/<disco>/queue/rotational`:

- **Rol ARCHIVOS (NAS Departamental):**
  - **En HDD:** Se inicializa `ext4` y se ejecuta `tune2fs -m 1` (liberando un 4% de almacenamiento reservado para superusuario). Se monta con `rw,noatime,commit=2` y readahead en `4096 KB`. La directiva `commit=2` reduce el intervalo de vaciado del journal a 2 segundos (en lugar de 5), ofreciendo **máxima resiliencia contra pérdidas en cortes eléctricos repentinos**.
  - **En SSD:** Se inicializa `ext4` montado con `rw,noatime,commit=5`, readahead en `1024 KB` y se habilita el servicio de recorte continuo `systemctl enable --now fstrim.timer`.
- **Rol BACKUP (Central de Respaldos):**
  - **En HDD:** Se inicializa `Btrfs` montado con `rw,noatime,compress=zstd:3,space_cache=v2,autodefrag`, readahead en `4096 KB` y se programa el mantenimiento mensual contra corrupción silenciosa.
  - **En SSD:** Se inicializa `Btrfs` montado con `rw,noatime,compress=zstd:3,space_cache=v2,ssd,discard=async`, readahead en `1024 KB`, `fstrim.timer` y mantenimiento de integridad.

### 4.2 Prevención de corrupción silenciosa (*Bit Rot*)

En los despliegues de rol `BACKUP` sobre Btrfs, se instala una tarea periódica en `/etc/cron.d/nas-btrfs-scrub`:
```cron
0 2 1 * * root btrfs scrub start -B /srv/nas >/dev/null 2>&1
```
Este proceso audita los bloques de datos y metadatos comparándolos contra sus sumas de verificación (*checksums* SHA256 integrados en Btrfs), identificando y mitigando la degradación magnética o silenciosa antes de que afecte a la recuperación de desastres.

### 4.3 Optimización del Kernel Linux (`sysctl`)

En `/etc/sysctl.d/99-nas-tuning.conf` se consolidan ajustes de alto rendimiento y resiliencia:
- `fs.inotify.max_user_instances = 2048` y `fs.inotify.max_user_watches = 524288`: Soporta indexación masiva de directorios concurrentes.
- `net.ipv4.tcp_keepalive_time = 120`, `net.ipv4.tcp_keepalive_intvl = 15`, `net.ipv4.tcp_keepalive_probes = 4`: Detecta y depura conexiones SMB huérfanas en solo 180 segundos (frente a las 2 horas estándar de Linux).
- `vm.vfs_cache_pressure = 30`: Prioriza la retención de inodos y directorios en la memoria RAM, acelerando la navegación en carpetas con decenas de miles de archivos.
- `vm.dirty_background_bytes = 67108864` (64 MB), `vm.dirty_bytes = 268435456` (256 MB) y `vm.dirty_expire_centisecs = 300`: Obliga al kernel a volcar datos sucios a disco en ráfagas pequeñas continuas, evitando que la memoria se sature y congelamientos del sistema durante transferencias pesadas.

### 4.4 Reglas persistentes de almacenamiento (`udev`)

- **Readahead dinámico (`/etc/udev/rules.d/60-nas-readahead.rules`):** Configura los parámetros de precarga secuencial `bdi/read_ahead_kb` y `queue/read_ahead_kb` según el tipo de almacenamiento asignado al NAS.
- **Protección del disco del sistema operativo (`/etc/udev/rules.d/80-udisks2-hide-os.rules`):** Inyecta `UDISKS_IGNORE="1"` sobre la unidad de disco raíz, impidiendo que herramientas web como Cockpit Storage la expongan para operaciones de formateo accidental.

### 4.5 Samba Hardening y compatibilidad ofimática (+100 usuarios en Microsoft Office / Excel)

La sección `[global]` de `/etc/samba/smb.conf` incorpora directivas críticas para entornos corporativos:
- `vfs objects = acl_xattr streams_xattr`: Almacena descriptores de seguridad NT en atributos extendidos de Linux y habilita flujos alternativos NTFS. Esto elimina bloqueos y corrupciones en aperturas simultáneas de hojas de cálculo de Excel (`.xlsx`), archivos temporales `~$` y plantillas compartidas.
- `store dos attributes = yes` y `inherit permissions = yes`: Garantiza que los atributos de solo lectura, oculto y archivo de Windows se sincronicen fielmente con el sistema de archivos Linux.
- `strict sync = yes`: Previene la pérdida de transacciones de archivos ofimáticos en aperturas de red.
- `use sendfile = yes`, `aio read size = 16384` y `aio write size = 16384`: Maximiza el rendimiento del bus de red reduciendo el uso de ciclos de procesador.
- `max open files = 65535`: Evita la denegación de servicio por agotamiento de descriptores de archivo en cargas de más de un centenar de puestos de trabajo.

### 4.6 Control de acceso y herencia de ACLs POSIX

En el **Esquema 2 (Solo lectura general + escritura exclusiva)**, el sistema aplica tanto permisos octales `2770` como ACLs POSIX por defecto (`default ACL`):
```bash
setfacl -R -m "g:$GRUPO_RO:r-x" "$RUTA_SHARE"
find -P "$RUTA_SHARE" -type d ! -type l -exec setfacl -d -m "g:$GRUPO_RO:r-x" {} +
setfacl -R -m "g:$GRUPO_RW:rwx" "$RUTA_SHARE"
find -P "$RUTA_SHARE" -type d ! -type l -exec setfacl -d -m "g:$GRUPO_RW:rwx" {} +
```
Esto garantiza que cualquier documento o subcarpeta creada por un usuario del grupo de escritura herede automáticamente la regla de lectura para el resto de los grupos departamentales sin requerir reajustes periódicos.

---

## 5. Métodos de despliegue

### 5.1 Despliegue interactivo con el asistente

El asistente se ejecuta mediante:
```bash
sudo nas
```
En el menú principal, la opción `[1] Desplegar servidor` guía al administrador en 5 pasos interactivos. Permite elegir entre formatear la unidad dedicada o **conservar los datos preexistentes (`--keep-data`)**, analizando el tipo de hardware para seleccionar automáticamente el sistema de archivos óptimo (`ext4` para NAS o `Btrfs` para copias de seguridad).

### 5.2 Despliegue automatizado por línea de comandos

Sintaxis oficial:
```bash
printf '%s\n' '<CLAVE_ADMIN>' | sudo bash src/core/deploy.sh [DISCO/LOCAL] [WORKGROUP] [NETBIOS] [ADMIN_USER] - [ROL] [OPCIONES]
```

**Parámetros:**
- `DISCO/LOCAL`: Ruta del disco dedicado (ej. `/dev/sdb`) o la palabra clave `LOCAL` para utilizar la partición del sistema operativo.
- `WORKGROUP`: Nombre del grupo de trabajo de red (ej. `EAD-COL`).
- `NETBIOS`: Nombre NetBIOS asignado al servidor (ej. `SRV-EAD-NAS` o `SRV-EAD-BKP`).
- `ADMIN_USER`: Nombre del usuario administrador del sistema (ej. `admin`).
- `-`: Indica que la contraseña administrativa se recibirá de forma segura a través de `stdin` (sin exponerla en `ps`).
- `ROL`: Rol del servidor, `ARCHIVOS` o `BACKUP`.

**Banderas opcionales:**
- `--keep-data`: Reutiliza una partición o disco preexistente con datos en `/srv/nas` sin formatear ni perder información. Identifica automáticamente particiones válidas (etiqueta `NAS_DATA` o partición previa) y detecta su sistema de archivos (`ext4`, `btrfs`, etc.) para montar con opciones optimizadas.
- `--force`: Acepta confirmaciones no destructivas automáticamente. **No** omite las validaciones de seguridad ante discos en uso.
- `--ignore-in-use`: Permite reutilizar o formatear discos detectados como montados tras confirmación explícita con la cadena `SI-FORMATEAR`. Las particiones que sean miembros activos de un arreglo RAID o volúmenes físicos LVM están bloqueadas permanentemente por seguridad.

### 5.3 Desinstalación y limpieza total

Para restablecer el equipo a su configuración limpia inicial:
```bash
sudo nas uninstall
```

---

## 6. Gestión modular

- **Gestión de grupos (`grp_*`):** Módulo [2]. El grupo maestro `grp_sistemas` cuenta con privilegios administrativos sobre `/srv/nas`.
- **Recursos compartidos:** Módulo [3]. Permite visibilidad abierta o recurso oculto (con sufijo `$`), asignando uno de los 4 esquemas de acceso:
  1. *Lectura y Escritura por Grupo:* Las ACL POSIX conceden permisos totales a los grupos seleccionados (`mask 0770`).
  2. *Solo Lectura General + Escritura Exclusiva:* Las ACL POSIX conceden lectura a los grupos seleccionados y escritura al grupo designado (`read only = no`, `mask 0770`).
  3. *Solo Lectura Estricta:* Contenido histórico de solo lectura mediante ACL POSIX (`read only = yes`, `mask 0770`).
  4. *Acceso Público / Invitados:* Acceso sin credenciales (`guest ok = yes`).
- **Gestión de usuarios:** Módulo [5]. Los integrantes de `grp_sistemas` reciben acceso administrativo web y shell interactivo; los demás usuarios disponen exclusivamente de acceso a carpetas compartidas de red.

---

## 7. Procedimiento de restauración de archivos

1. Acceder a la ruta de almacenamiento de la tarea deseada:
   ```bash
   cd /srv/nas/BACKUPS_HISTORICOS/<nombre_tarea>/
   ls -la
   ```
2. Identificar el snapshot requerido (ejemplo: `snapshot_2026-09-30_230000`).
3. Copiar el archivo o directorio a la carpeta compartida de producción:
   ```bash
   cp -a snapshot_2026-09-30_230000/Contabilidad/Balance_2026.xlsx /srv/nas/CONTABILIDAD/
   ```

---

## 8. Operación y mantenimiento preventivo

- **Bitácoras de copias de seguridad:** Disponibles en `/srv/nas/LOGS_BACKUP/backup_<tarea>.log`, rotadas semanalmente mediante `/etc/logrotate.d/nas-backups`.
- **Diagnóstico del servidor:** `sudo nas status` o mediante la opción [6] del asistente.
- **Auditoría de servicios del sistema:**
  ```bash
  systemctl status smbd nmbd wsdd2 cockpit.socket cron fstrim.timer
  ```
- **Verificación de BTRFS Scrub:**
  ```bash
  btrfs scrub status /srv/nas
  ```
- **Sincronización y actualizaciones:** `sudo nas update`.

---

## 9. Solución de problemas y glosario

**Problemas frecuentes:**

- **El montaje CIFS falla en la tarea de backup:** El host remoto de Windows puede exigir una versión específica de SMB o credenciales de dominio. Verifique `/etc/backup-credentials/<tarea>.cred` y confirme la sintaxis `DOMINIO\usuario`. Si el servidor remoto utiliza un dialecto anterior, ajuste `vers=2.1` o `vers=3.0` en `/usr/local/bin/backup_<tarea>.sh`.
- **La tarea de backup finaliza indicando espacio insuficiente:** El runner requiere un mínimo de 2 GB libres y menos del 95% de ocupación en `/srv/nas`. Libere espacio o purgue snapshots obsoletos con la opción de retención.
- **Windows no detecta el servidor en el Explorador ("Red"):** Compruebe el servicio `systemctl status wsdd2` y asegúrese de que los puertos UDP 3702, 5355 y TCP 5357 estén autorizados en el cortafuegos UFW.
- **Un disco se detecta como "EN USO":** El disco está montado o forma parte de LVM/RAID. Si solo está montado en otra ruta, use `--ignore-in-use` y confirme con `SI-FORMATEAR` o elija `--keep-data` para conservarlo.

**Glosario técnico:**

- **Staging atómico:** Técnica donde los datos se escriben en una ruta provisional (`.inprogress_*`) y solo se exponen con su nombre definitivo mediante una operación atómica (`mv`) una vez comprobada su integridad.
- **Enlace duro (*Hardlink*):** Puntero directo al inodo físico de un archivo en disco. Permite que múltiples snapshots compartan el mismo archivo sin duplicar el consumo de almacenamiento.
- **`--link-dest`:** Parámetro de `rsync` que vincula por enlaces duros los archivos idénticos a los del snapshot precedente.
- **Bit Rot:** Fenómeno de corrupción silenciosa de datos debido a degradación electromagnética en medios de almacenamiento. Mitigado mediante `btrfs scrub`.
- **WSDD2:** Servicio de descubrimiento dinámico para equipos Windows (Web Services Discovery y LLMNR), eliminando la dependencia de NetBIOS broadcast.
