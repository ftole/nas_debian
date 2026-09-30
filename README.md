# Servidor NAS y Central de Respaldos Multiplataforma (Debian 13)

[![Debian 13](https://img.shields.io/badge/OS-Debian%2013%20(Trixie)-A81D33?style=for-the-badge&logo=debian&logoColor=white)](https://github.com/ftole/nas_debian) [![Bash Shell](https://img.shields.io/badge/Scripting-Bash-4EAA25?style=for-the-badge&logo=gnu-bash&logoColor=white)](https://github.com/ftole/nas_debian) [![Samba](https://img.shields.io/badge/Service-Samba%20SMB-0066CC?style=for-the-badge)](https://github.com/ftole/nas_debian) [![Cockpit](https://img.shields.io/badge/Web%20UI-Cockpit-FF6600?style=for-the-badge)](https://github.com/ftole/nas_debian)

Este repositorio contiene los scripts para desplegar y administrar, sobre Debian 13 (Trixie), un servidor de archivos en red (NAS departamental) y una central de copias de seguridad pensada para resistir ransomware. El despliegue se realiza desde la terminal, mediante un asistente interactivo o por línea de comandos, y la operación diaria puede gestionarse también desde un panel web basado en Cockpit.

## Características principales

- Asistente de despliegue con detección automática de discos, dirección IP y usuario administrador.
- Dos roles excluyentes: `ARCHIVOS` (recursos compartidos visibles) y `BACKUP` (repositorios ocultos).
- Selección condicional de sistema de archivos según rol y medio físico: `ext4` optimizado para NAS en HDD/SSD y `Btrfs` con compresión Zstandard para central de copias de seguridad.
- Soporte para reutilización de almacenamiento con datos existentes (`--keep-data`) sin formatear.
- Copias de seguridad atómicas (`.inprogress_*`) y deduplicadas mediante enlaces duros para Windows (CIFS SMB 3.1.1 con soporte de dominios corporativos), Linux (SSH) y carpetas locales.
- Alta concurrencia y compatibilidad ofimática: optimizado para más de 100 usuarios en Microsoft Office/Excel mediante módulos VFS `acl_xattr` y `streams_xattr`.
- Protección del sistema contra apagones y cortes eléctricos (`commit=2` en HDD, umbrales de memoria sucia acotados `vm.dirty_bytes` y descarte atómico de transferencias interrumpidas).
- Mantenimiento proactivo integrado: verificación de integridad silenciosa (`btrfs scrub`), optimización SSD (`fstrim.timer`) y rotación de registros (`logrotate`).
- Gestión de grupos, recursos compartidos, usuarios y tareas desde el asistente o desde el panel web.
- Base limpia: el despliegue no crea recursos de prueba; se añaden según las necesidades del entorno.
- Desinstalación total que devuelve el servidor a su estado base.

## Cuadro Maestro de Tecnologías, Subsistemas y Librerías

El siguiente cuadro detalla todos los componentes, librerías, subsistemas del kernel y tecnologías integradas en el proyecto, explicando su función exacta y el beneficio directo que aportan a la seguridad, estabilidad y rendimiento del servidor:

| Tecnología / Subsistema / Librería | Componente / Ámbito | Para qué sirve | Beneficio que aporta |
| :--- | :--- | :--- | :--- |
| **Debian 13 (Trixie) x86_64** | Sistema Operativo Base | Plataforma Linux de nivel empresarial con soporte a largo plazo y paquetes modernos. | Máxima estabilidad, seguridad probada y consumo mínimo de recursos en hardware moderno y heredado. |
| **Samba 4 (`smbd` / `nmbd`)** | Servicio de Red SMB/CIFS | Compartición de carpetas y archivos en red para clientes Windows, Linux y macOS. | Protocolo estándar nativo en Windows, cifrado negociado (`desired`), protocolo mínimo seguro SMB2_02 y aislamiento de usuarios. |
| **Módulos VFS Samba (`acl_xattr`, `streams_xattr`)** | Capa VFS de Samba | Mapeo de listas de control de acceso (ACLs) de Windows en atributos extendidos (`xattr`) y compatibilidad con flujos de datos alternativos NTFS (*Alternate Data Streams*). | Compatibilidad total con suites ofimáticas (Microsoft Office / Excel con +100 puestos simultáneos sin bloqueos ni errores de guardado temporal `~$`) y preservación de marcas de seguridad Windows (`Zone.Identifier`). |
| **Directivas Samba para Alto Rendimiento y Ofimática** | Configuración `smb.conf` | `store dos attributes = yes`, `inherit permissions = yes`, `strict sync = yes`, `use sendfile = yes`, `aio read/write size = 16384` y `max open files = 65535`. | Transferencia directa kernel-red sin saltos a memoria de usuario; latencia ultrabaja en apertura de libros contables masivos (Excel), prevención de corrupción de datos y eliminación de cuellos de botella por agotamiento de descriptores con +100 equipos. |
| **ext4 optimizado (`tune2fs -m 1`, `commit=2`/`commit=5`, `noatime`)** | Sistema de Archivos (Rol `ARCHIVOS`) | Filesystem transaccional con *journaling* para el almacenamiento departamental en `/srv/nas`. | Recupera hasta un 4% de capacidad reservada para root (`-m 1`); minimiza la ventana de pérdida ante apagones repentinos sincronizando el journal cada 2 s (`commit=2` en HDD); reduce un 30% las operaciones de I/O (`noatime`). |
| **BTRFS con Zstandard (`compress=zstd:3`, `space_cache=v2`)** | Sistema de Archivos (Rol `BACKUP`) | Filesystem avanzado con compresión en tiempo real y sumas de verificación (*checksums*) por bloque. | Ahorro del 20% al 40% de espacio físico sin impacto en CPU; asignación ultrarrápida de bloques libres (`space_cache=v2`); detección intrínseca de errores físicos mediante hashes SHA256 integrados. |
| **BTRFS Scrub Programado (`nas-btrfs-scrub`)** | Mantenimiento Mensual (`cron`) | Tarea automática mensual (`0 2 1 * *`) que audita la totalidad de los datos y metadatos en `/srv/nas`. | Detección proactiva y reporte/reparación de corrupción silenciosa de datos (*Bit Rot* o degradación electromagnética) antes de que afecte restauraciones críticas. |
| **rsync con Hardlinks (`-aAXH --numeric-ids --link-dest`)** | Motor de Copias de Seguridad | Replicación incremental a nivel de archivo con enlace físico a snapshots previos sin cambio. | Deduplicación eficiente (>85% de ahorro de disco); preservación exacta de atributos extendidos (`-X`), ACLs POSIX (`-A`), enlaces duros (`-H`) y UIDs/GIDs numéricos idénticos sin alteración. |
| **Staging Atómico (`.inprogress_*`)** | Pipeline de Runners de Backup | Escritura de snapshots en carpeta temporal oculta y posterior renombrado atómico (`mv`) al finalizar con éxito. | Resiliencia total ante apagones o desconexiones: si el proceso se interrumpe, el manejador de señales (`trap cleanup`) descarta los datos parciales; el histórico solo contiene copias 100% íntegras. |
| **cifs-utils (`mount.cifs`) SMB 3.1.1 (`vers=3.1.1,noserverino,cache=none,soft,timeo=30`)** | Conector de Respaldo Windows | Montaje temporal en solo lectura (`ro`) de carpetas compartidas SMB/CIFS remotas. | Negociación con cifrado y firmas SMB 3.1.1; `noserverino` previene errores de inodos remotos; `cache=none` evita lectura de datos obsoletos; `soft,timeo=30` previene cuelgues del kernel del NAS si el host Windows se reinicia. |
| **Soporte de Dominios Active Directory (`DOMINIO\user`, `DOMINIO/user`)** | Autenticación CIFS en Backups | Análisis y desglose automático de credenciales con dominio corporativo en `/etc/backup-credentials/<tarea>.cred`. | Integración nativa con infraestructuras Active Directory sin exponer contraseñas en memoria de comandos (`ps`) y con permisos estrictos `0600 root:root`. |
| **OpenSSH / sshpass (`StrictHostKeyChecking=accept-new`)** | Conector de Respaldo Linux | Túnel SSH cifrado no interactivo con almacenamiento de huellas en archivo aislado (`known_hosts_backup`). | Protección estricta contra ataques *Man-in-the-Middle*; registra claves nuevas en el primer contacto automáticamente sin requerir interacción manual y sin degradar la seguridad a `no`. |
| **Ajustes de Kernel sysctl (`99-nas-tuning.conf`)** | Parámetros del Kernel Linux | Ajuste fino de descriptores de inotify, reciclaje de conexiones TCP y sincronización de memoria sucia. | `fs.inotify` ampliado para indexar árboles de archivos masivos; `tcp_keepalive` (120s/15s/4) purga conexiones SMB huérfanas en minutos; `vm.dirty_bytes=256MB` fuerza vaciado continuo evitando parálisis de I/O. |
| **Readahead Tuning por udev (`60-nas-readahead.rules`)** | Subsistema de Bloques Linux | Configuración del búfer de lectura anticipada del disco NAS (`1024 KB` en SSD / `4096 KB` en HDD). | Acelera transferencias secuenciales de red masivas y agiliza las comparaciones diferenciales de `rsync` sin sobrecargar la RAM en unidades flash. |
| **Protección udev del Disco del SO (`80-udisks2-hide-os.rules`)** | Aislamiento de Almacenamiento | Inyección de la propiedad `UDISKS_IGNORE=1` en el disco que aloja la partición raíz del sistema operativo. | Impide que operadores o interfaces gráficas (Cockpit Storage) formateen o destruyan accidentalmente el disco donde se ejecuta Debian. |
| **Reutilización de Discos Existentes (`--keep-data`)** | Motor de Despliegue | Detección automática de particiones previas y montaje en `/srv/nas` respetando los datos preexistentes. | Permite migrar o reinstalar el servidor conservando terabytes de información intacta sin requerir formateo ni volcados externos. |
| **Cockpit + PatternFly 4 + Extensiones 45Drives + EAD Backups** | Panel de Administración Web | Interfaz web responsiva activada por socket (`systemd`) sin demonios en segundo plano residentes. | Consumo nulo de memoria RAM en reposo; administración visual coherente de usuarios, carpetas compartidas Samba y bitácoras de respaldo desde cualquier navegador. |
| **Python 3 (`backup_api.py`)** | Backend API para Cockpit | Interfaz JSON segura entre la interfaz gráfica web y los binarios y scripts del sistema. | Validación estricta con expresiones regulares que impide inyecciones de comandos, ejecución con privilegios acotados y lectura de bitácoras retrospectivas sin bloqueos. |
| **whiptail + Bash 5** | Interfaz Visual de Terminal (TUI) | Menús interactivos con colores nativos para administración completa desde consola o SSH. | Operación inmediata sin dependencias gráficas pesadas, validación interactiva y ciclo de corrección de datos sin pérdida de texto escrito. |
| **Control de Concurrencia con `flock`** | Programación de Tareas (`cron`) | Mecanismo de bloqueo exclusivo por descriptor de archivo en `/var/lock/backup_<tarea>.lock`. | Garantiza que nunca se solapen dos ejecuciones de una misma tarea si un respaldo toma más tiempo que su frecuencia programada. |
| **Monitoreo de Espacio Libre (`df -Pk`) en Runners** | Prevención de Saturación de Disco | Evaluación del porcentaje y megabytes disponibles en `/srv/nas` previa a la ejecución de cada respaldo. | Emite advertencia si el uso supera el 85% y cancela de inmediato la tarea si restan menos de 2 GB o se supera el 95%, evitando caídas críticas del sistema de archivos. |
| **`fstrim.timer`** | Mantenimiento para Medios SSD | Recorte periódico de bloques descartados en unidades de estado sólido y arreglos NVMe. | Mantiene tasas óptimas de rendimiento de escritura sostenido y previene la degradación prematura de unidades flash. |
| **`logrotate` (`nas-backups`, `nas-deploy`)** | Mantenimiento de Registros | Rotación programada con compresión y directiva `copytruncate` para registros de despliegue y copias. | Impide el crecimiento indefinido de archivos de bitácora y la consiguiente pérdida silenciosa de espacio en disco. |
| **WSDD2 con Systemd Override** | Descubrimiento de Red Windows | Emisión de mensajes WSD y LLMNR con parámetros explícitos de NetBIOS y Workgroup. | El servidor es detectado al instante en "Red" por equipos Windows 10/11 sin depender de protocolos obsoletos e inseguros como SMBv1 o NetBIOS broadcast. |
| **Aislamiento de Idioma (`LC_ALL=C LANG=C`)** | Compatibilidad de Sistema | Wrappers de ejecución en `/usr/local/sbin/chage`, `passwd` y `lastb`. | Elimina errores de interpretación de fechas o cadenas en módulos de Cockpit cuando Debian se encuentra configurado en español. |
| **ACLs POSIX (`acl` / `setfacl`) con Herencia por Defecto** | Seguridad de Archivos Local | Reglas de acceso multi-grupo sobre carpetas compartidas y herencia automática (`default ACL`). | Permite esquemas mixtos (múltiples grupos en lectura y uno exclusivo en escritura) asegurando que los nuevos archivos mantengan la política definida. |


## Requisitos

- Debian 13 (Trixie) x86_64, con acceso `root` o `sudo`.
- Conexión a Internet durante la instalación (paquetes y extensiones).
- `curl` (o `wget`) y `ca-certificates` para el instalador remoto.
- Disco dedicado opcional para `/srv/nas` (puede formatearse desde cero con el sistema de archivos óptimo o reutilizarse conservando sus datos intactos mediante `--keep-data`).

## Instalación

### Instalador remoto

```bash
curl -fsSL https://raw.githubusercontent.com/ftole/nas_debian/main/install.sh | sudo bash
```

*(o con `wget`: `wget -qO- https://raw.githubusercontent.com/ftole/nas_debian/main/install.sh | sudo bash`)*

> [!TIP]
> El instalador verifica dependencias, descarga el proyecto en `/opt/nas_debian` y presenta el asistente. No modifica particiones hasta que lo autorices.

### Preparación manual (opcional)

Si prefieres preparar el servidor antes de ejecutar el asistente, realiza estos pasos **en orden**.

> [!IMPORTANT]
> Los **Pasos 1–7** se ejecutan como `root` (bootstrap: repositorios, paquetes y creación del usuario administrador). Tras el Paso 7 **sal de `root`** y ejecuta los **Pasos 8–13 con `sudo`** desde tu usuario administrador. `sudo` usa `secure_path` (que incluye `/usr/sbin`), por lo que `ufw` y `dpkg-reconfigure` se encuentran sin problema.

#### Paso 1: Acceso como `root`

```bash
su -
```

> [!IMPORTANT]
> Incluye el espacio y el guion (`su -`). Así Debian carga el entorno completo de `root`, incluido `/usr/sbin/` en el `$PATH`.

#### Paso 2: Repositorios APT (formato deb822)

Debian 12 y posteriores usan el formato deb822 en `/etc/apt/sources.list.d/debian.sources`. Configúralo ahí (y vacía `sources.list`) para evitar fuentes duplicadas:

```bash
cat << 'SOURCES' > /etc/apt/sources.list.d/debian.sources
Types: deb deb-src
URIs: http://deb.debian.org/debian
Suites: trixie trixie-updates
Components: main contrib non-free non-free-firmware

Types: deb deb-src
URIs: http://security.debian.org/debian-security
Suites: trixie-security
Components: main contrib non-free non-free-firmware
SOURCES

: > /etc/apt/sources.list
```

#### Paso 3: Actualizar el sistema

```bash
apt update && apt upgrade -y
```

#### Paso 4: Paquetes base

```bash
apt install -y curl wget ca-certificates htop ufw
```

#### Paso 5: Estado de red

```bash
ip a     # interfaces y direcciones
ip r     # puerta de enlace predeterminada
```

#### Paso 6: Conectividad y DNS

```bash
ping -c 4 8.8.8.8      # conexión directa por IP
ping -c 4 google.com   # resolución DNS
```

#### Paso 7: Usuarios y permisos de administrador

1. Listar usuarios con shell interactivo:

   ```bash
   grep -E '/bin/bash|/bin/sh' /etc/passwd
   ```

2. Instalar `sudo` y conceder permisos (el usuario administrador se **detecta automáticamente**; si no existe, se crea):

   ```bash
   apt install -y sudo
   ADMIN_USER="$(awk -F: '$3 >= 1000 && $3 < 60000 && $1 != "nobody" {print $1; exit}' /etc/passwd)"
   ADMIN_USER="${ADMIN_USER:-nas}"
   id "$ADMIN_USER" &>/dev/null || adduser --disabled-password --gecos "" "$ADMIN_USER"
   SUDO_NAME="$(printf '%s' "$ADMIN_USER" | tr -c 'A-Za-z0-9_-' '_')"
   usermod -aG sudo "$ADMIN_USER"
   echo "$ADMIN_USER ALL=(ALL:ALL) ALL" > "/etc/sudoers.d/90-$SUDO_NAME"
   chmod 0440 "/etc/sudoers.d/90-$SUDO_NAME"
   echo "Administrador configurado: $ADMIN_USER"
   ```

3. **Salir de `root`** (los pasos siguientes se ejecutan con `sudo`):

   ```bash
   exit
   ```

> [!NOTE]
> La creación del archivo en `/etc/sudoers.d/` hace que los permisos de `sudo` surtan efecto de inmediato, sin cerrar sesión.

> [!TIP]
> `sudo` ignora los archivos de `/etc/sudoers.d/` cuyo nombre contenga un punto; el bloque anterior reemplaza esos caracteres por guion bajo de forma automática. Para comprobar la sintaxis, ejecuta `visudo -c`.

#### Paso 8: Desactivar el acceso de `root` por SSH

```bash
# Archivo principal y drop-ins (tienen mayor precedencia)
sudo sed -i 's/^#*PermitRootLogin.*/PermitRootLogin no/' /etc/ssh/sshd_config
grep -rl "PermitRootLogin" /etc/ssh/sshd_config.d/ 2>/dev/null | xargs -r sudo sed -i 's/^#*PermitRootLogin.*/PermitRootLogin no/'
sudo systemctl restart ssh || sudo systemctl restart sshd

# Verificar el valor efectivo
sudo sshd -T 2>/dev/null | grep -i permitrootlogin
```

#### Paso 9: Actualizaciones de seguridad automáticas

```bash
sudo apt install -y unattended-upgrades
sudo dpkg-reconfigure -plow unattended-upgrades
```

#### Paso 10: Firewall (UFW)

```bash
sudo ufw default deny incoming
sudo ufw default allow outgoing

sudo ufw allow 22/tcp comment 'SSH'
sudo ufw allow 9090/tcp comment 'Cockpit Web Admin'
sudo ufw allow 137,138/udp comment 'Samba NetBIOS'
sudo ufw allow 139,445/tcp comment 'Samba SMB'
sudo ufw allow 3702/udp comment 'WSDD2 WSD Discovery UDP'
sudo ufw allow 3702/tcp comment 'WSDD2 WSD Discovery TCP'
sudo ufw allow 5355/udp comment 'WSDD2 LLMNR UDP'
sudo ufw allow 5355/tcp comment 'WSDD2 LLMNR TCP'
sudo ufw allow 5357/tcp comment 'WSDD2 WSD HTTP'

sudo ufw --force enable
sudo ufw status verbose
```

#### Paso 11: Evitar suspensión del servidor

El servidor NAS debe permanecer siempre encendido. Bloquea la suspensión e hibernación y configura `logind` para ignorar la tecla de suspender y el cierre de tapa:

```bash
sudo systemctl mask sleep.target suspend.target hibernate.target hybrid-sleep.target
sudo mkdir -p /etc/systemd/logind.conf.d
printf '[Login]\nHandleSuspendKey=ignore\nHandleHibernateKey=ignore\nHandleLidSwitch=ignore\n' | sudo tee /etc/systemd/logind.conf.d/99-nas.conf >/dev/null
sudo systemctl restart systemd-logind
```

Para comprobar que quedó bloqueado:

```bash
systemctl is-enabled sleep.target     # debe responder: masked
```

> [!NOTE]
> Para revertir: `sudo systemctl unmask sleep.target suspend.target hibernate.target hybrid-sleep.target` y `sudo rm /etc/systemd/logind.conf.d/99-nas.conf`.

#### Paso 12: fail2ban

```bash
sudo apt install -y fail2ban
sudo cp /etc/fail2ban/jail.conf /etc/fail2ban/jail.local
sudo systemctl enable --now fail2ban
sudo systemctl status fail2ban
```

#### Paso 13: Ejecutar el asistente

Ya puedes abrir el asistente como tu usuario administrador:

```bash
sudo nas
```

## Comandos del CLI `nas`

Una vez instalado, el comando `nas` queda registrado en el sistema:

| Comando | Acción |
| :--- | :--- |
| `sudo nas` | Abre el asistente interactivo. |
| `sudo nas update` | Sincroniza el proyecto con la última versión de GitHub. |
| `sudo nas status` | Diagnóstico de servicios, almacenamiento, recursos y tareas de backup. |
| `sudo nas version` | Muestra la versión y el commit instalado. |
| `sudo nas uninstall` | Desinstala el comando y limpia el servidor. |

> [!NOTE]
> El instalador despliega el proyecto en `/opt/nas_debian` y crea el comando `/usr/local/bin/nas`. El comando `nas help` muestra la ayuda.

## Asistente interactivo

| Opción | Módulo | Descripción |
| :--- | :--- | :--- |
| 1 | Desplegar servidor | Asistente en 5 pasos para el rol `ARCHIVOS` o `BACKUP`, con selección condicional de filesystem y opción de conservar datos (`--keep-data`) o formatear. |
| 2 | Gestión de grupos | Crear, listar y eliminar grupos de seguridad (`grp_*`). |
| 3 | Recursos compartidos | Crear, listar, habilitar/deshabilitar y eliminar recursos, con 4 esquemas de permisos y ACLs por defecto. |
| 4 | Tareas de backup | Programar y **abortar** copias para Windows (CIFS), Linux (SSH) o carpetas locales con staging atómico. |
| 5 | Usuarios | Crear usuarios, asignar grupos y gestionar contraseñas de red. |
| 6 | Diagnóstico | Estado de servicios, almacenamiento, recursos y tareas programadas. |
| 7 | Reiniciar servicios | Recarga de Samba, WSDD2 y Cockpit. |
| 8 | Buscar actualizaciones | Sincronización con GitHub. |
| 9 | Desinstalar | Restablecimiento total del sistema. |

## Selección de sistema de archivos según rol y medio

El instalador y el asistente seleccionan y configuran automáticamente el sistema de archivos óptimo según el rol elegido y el tipo de medio físico detectado (`rotational` en `/sys/block/<disco>/queue/rotational`):

| Rol del servidor | Tipo de medio | Sistema de archivos | Opciones de montaje y ajustes | Beneficio técnico |
| :--- | :--- | :--- | :--- | :--- |
| **`ARCHIVOS` (NAS)** | HDD mecánico (`rotational=1`) | `ext4` | `rw,noatime,commit=2`, `tune2fs -m 1`, readahead 4096 KB | Recuperación del 4% de espacio reservado; transacciones volcadas cada 2 s para resiliencia ante cortes eléctricos repentinos. |
| **`ARCHIVOS` (NAS)** | SSD flash (`rotational=0`) | `ext4` | `rw,noatime,commit=5`, readahead 1024 KB, `fstrim.timer` | Minimiza escrituras innecesarias en celdas NAND; optimización continua de bloques no referenciados. |
| **`BACKUP` (Central)** | HDD mecánico (`rotational=1`) | `Btrfs` | `rw,noatime,compress=zstd:3,space_cache=v2,autodefrag`, readahead 4096 KB, `btrfs scrub` mensual | Compresión transparente con ahorro del 20% al 40% de disco; desfragmentación en segundo plano y verificación periódica contra *Bit Rot*. |
| **`BACKUP` (Central)** | SSD flash (`rotational=0`) | `Btrfs` | `rw,noatime,compress=zstd:3,space_cache=v2,ssd,discard=async`, readahead 1024 KB, `fstrim.timer`, `btrfs scrub` mensual | Descarte asíncrono para almacenamiento flash; compresión ZSTD rápida sin latencia de I/O y comprobación criptográfica mensual. |

### Reutilización de almacenamiento con datos existentes (`--keep-data`)

Si el servidor ya cuenta con un disco que contiene información previa, el sistema permite integrarlo sin formatear:
- **En el asistente visual:** En el Paso 2, seleccione la opción `Conservar datos existentes (montar sin formatear, --keep-data)`.
- **Por línea de comandos:** Añadiendo el argumento `--keep-data` a la invocación de `deploy.sh`.
El sistema inspecciona las particiones, identifica la partición de datos (por etiqueta `NAS_DATA` o partición previa válida), verifica el sistema de archivos (`ext4`, `btrfs`, etc.) y la monta en `/srv/nas` aplicando los parámetros de rendimiento correspondientes sin riesgo de pérdida de datos.

### Despliegue automatizado por línea de comandos

Para entornos desatendidos o automatizaciones, `deploy.sh` puede ejecutarse directamente sin interfaz gráfica:

```bash
# Servidor de Archivos en partición local:
printf '%s\n' '<CLAVE_ADMIN>' | sudo bash src/core/deploy.sh LOCAL EAD-COL SRV-EAD-NAS admin - ARCHIVOS

# Servidor de Archivos conservando datos en disco secundario (/dev/sdb):
printf '%s\n' '<CLAVE_ADMIN>' | sudo bash src/core/deploy.sh /dev/sdb EAD-COL SRV-EAD-NAS admin - ARCHIVOS --keep-data

# Servidor de Backup formateando disco secundario desde cero (/dev/sdb):
printf '%s\n' '<CLAVE_ADMIN>' | sudo bash src/core/deploy.sh /dev/sdb EAD-COL SRV-EAD-BKP admin - BACKUP
```

## Copias de seguridad

El motor de copias de seguridad combina rendimiento, resiliencia ante cortes imprevistos y máxima eficiencia de almacenamiento:

- **Staging atómico y protección contra apagones:** Cada respaldo se procesa inicialmente en un directorio temporal oculto (`.inprogress_YYYY-MM-DD_HHMMSS`). Si ocurre un corte de energía, pérdida de conectividad o cancelación manual, los controladores de eventos (`trap cleanup EXIT TERM INT`) desmontan el recurso y eliminan cualquier snapshot parcial corrupto. Solo cuando la copia finaliza con éxito total (código de salida 0), se promueve atómicamente mediante `mv` a su nombre definitivo `snapshot_YYYY-MM-DD_HHMMSS`.
- **Deduplicación por enlaces duros (*hardlinks*):** Mediante `rsync --link-dest`, los archivos no modificados comparten el mismo inodo físico en disco que el snapshot anterior. Esto reduce el consumo de almacenamiento en más de un 85% frente a copias completas repetitivas.
- **Orígenes soportados:**
  - **Windows (CIFS / SMB 3.1.1):** Montaje en solo lectura con parámetros de alta resiliencia (`vers=3.1.1,noserverino,cache=none,soft,timeo=30`). Soporta cuentas locales y cuentas de dominio corporativo de Active Directory (`DOMINIO\usuario` y `DOMINIO/usuario`), almacenando credenciales protegidas en `/etc/backup-credentials/<tarea>.cred` con permisos `0600 root:root`.
  - **Linux (SSH):** Replicación con atributos extendidos completos (`rsync -aAXH --numeric-ids -v -z`) sobre un túnel SSH con verificación estricta de claves (`StrictHostKeyChecking=accept-new`) guardadas en un almacén aislado `/root/.ssh/known_hosts_backup`.
  - **Carpetas locales:** Sincronización directa entre directorios locales del servidor preservando metadatos POSIX exactos.
- **Monitoreo preventivo de espacio:** Los runners validan los umbrales de almacenamiento antes de transferir datos; emiten advertencia si la ocupación supera el 85% y abortan preventivamente si quedan menos de 2 GB libres o la ocupación excede el 95%.
- **Concurrencia y aborto seguro:** Bloqueo exclusivo con `flock` por tarea para evitar solapamientos. Las tareas en ejecución pueden abortarse limpiamente desde el asistente o desde el panel web de Cockpit. El detalle técnico completo se encuentra en `SMB_DEBIAN.md`.

## Seguridad

El proyecto aplica una arquitectura de defensa en profundidad para garantizar la resiliencia operativa y la protección de datos:

- **Validación estricta de entradas:** Tanto en el asistente TUI como en la API web Python se sanitizan rutas, expresiones cron, puertos y nombres para mitigar inyecciones de comandos y ataques de salto de directorio (*path traversal*).
- **Protección de secretos y credenciales:** Las contraseñas administrativas nunca viajan en la línea de comandos (se procesan vía `stdin` o variables de entorno), y las credenciales CIFS se almacenan con permisos `0600 root:root`.
- **Samba hardening y compatibilidad ofimática:** Configuración de `map to guest = Bad User`, cifrado negociado (`desired`), protocolo mínimo SMB2_02, módulos VFS `acl_xattr` y `streams_xattr` para compatibilidad completa con Microsoft Excel / Office multiusuario (+100 puestos concurrentes sin bloqueos de archivos temporales), y herencia de permisos POSIX con ACLs por defecto (`default ACL`).
- **Ajustes de Kernel y Resiliencia Eléctrica:** Parámetros `sysctl` optimizados (`vm.dirty_bytes=256MB`, `vm.dirty_background_bytes=64MB`, `net.ipv4.tcp_keepalive_time=120`, `vm.vfs_cache_pressure=30`) para evitar congelamientos de I/O y asegurar el volcado periódico a disco ante fallos eléctricos. En discos mecánicos se fuerza `commit=2` en ext4.
- **Protección contra Bit Rot y Degradación Flash:** Para el rol de Backup se programa una auditoría periódica mensual de sumas de verificación mediante `btrfs scrub` (`0 2 1 * *`), y para medios SSD se activa el temporizador `fstrim.timer`.
- **Protección del disco del sistema operativo:** Regla udev persistente (`80-udisks2-hide-os.rules`) con `UDISKS_IGNORE=1` para ocultar la unidad raíz en Cockpit Storage, junto con protecciones contra formateo de particiones en uso (LVM, RAID, LUKS).
- **Rotación de logs e integridad de cadena de suministro:** Políticas automáticas en `logrotate` para bitácoras de sincronización y despliegue, junto con verificación de firmas SHA256 de todas las extensiones web descargadas.

El análisis de modos de fallo y las pruebas de resiliencia están documentados en `FAILURE_MODES.md`.

## Estructura del proyecto

```text
install.sh                 Instalador remoto y CLI `nas`
src/asistente.sh           Asistente interactivo (menú principal)
src/lib/                   Funciones auxiliares (colores, entorno, discos)
src/core/deploy.sh         Motor de despliegue
src/core/uninstall.sh      Desinstalación total
src/core/updater.sh        Actualización desde GitHub
src/modules/               Módulos del asistente (grupos, recursos, backups, usuarios, diagnóstico)
src/web/backups/           Panel web de backups (Cockpit)
tests/                     Pruebas unitarias y de fallo
SMB_DEBIAN.md              Manual técnico de arquitectura y referencia
SECURITY.md                Modelo de seguridad, identidades y mitigaciones
FAILURE_MODES.md           Modos de fallo y su verificación (FMEA)
AGENTS.md                  Especificación técnica maestra para agentes e ingenieros
.github/workflows/ci.yml   Integración continua
```

## Solución de problemas

- **`curl: (60) certificate problem`**: instala `ca-certificates` (`apt install -y ca-certificates`).
- **`Permiso denegado` u `orden no encontrada`** en la preparación manual: te faltó anteponer `sudo` (Pasos 8–13) o no ejecutaste como `root` (Pasos 1–7).
- **El asistente no abre**: ejecútalo con `sudo` y en una terminal de al menos 72x20 caracteres.
- **Un disco aparece como "EN USO"**: está montado, es un volumen LVM o un miembro de RAID. Un PV de LVM o un miembro de RAID activo **nunca** se puede formatear. Para un disco simplemente montado, el despliegue por consola permite continuar con `--ignore-in-use` y la confirmación textual `SI-FORMATEAR`.
- **Una tarea de backup no se ejecuta**: comprueba que `cron` esté activo (`systemctl status cron`) y revisa el log en `/srv/nas/LOGS_BACKUP/`.
- **Windows no ve el servidor**: verifica `smbd`, `wsdd2` y las reglas de UFW.

## Licencia

Este proyecto es propiedad exclusiva de su autor. Todos los derechos reservados. Consulta el archivo [LICENSE](LICENSE).
