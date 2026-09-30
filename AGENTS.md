# AGENTS.md • Servidor NAS & Central de Respaldos Multiplataforma EAD-COL (Debian 13)

> [!IMPORTANT]
> Este documento contiene la **especificación técnica completa, inventario de scripts, arquitectura de seguridad, soluciones a peculiaridades del sistema y estado del proyecto** para que cualquier agente de IA o ingeniero pueda retomar y continuar este trabajo en cualquier equipo.


---

## 1. Identidad y Propósito del Proyecto

* **Sistema Operativo Base:** Debian 13 (Trixie) GNU/Linux x86_64.
* **Organización / Grupo de Trabajo:** `TEAM-JOFRATO`.
* **Identificadores NetBIOS por Defecto:**
  * Servidor NAS de Archivos: `SRV-NAS`
  * Servidor Central de Backup: `SRV-BKP`
* **Dirección IP de Red Local de Prueba:** `10.10.1.2` (Subred `10.10.1.0/24`).
* **Credenciales Administrativas de Entorno:**
  * Usuario: `usuario asignado` / `root`
  * Contraseña predeterminada: `la que se asigne`
* **Convención de Commits de Git:**
  * **REGLA ESTRICTA:** Los commits deben ser atómicos y granulares. **Debe existir un commit independiente por cada archivo modificado.** Si se alteran 3 archivos, se deben registrar 3 commits por separado.
  * Todos los mensajes de commit deben redactarse en **español** siguiendo el estándar convencional:
    * `feat: <descripción en español>` (nuevas características)
    * `fix: <descripción en español>` (corrección de errores)
    * `docs: <descripción en español>` (documentación)
    * `refactor: <descripción en español>` (mejoras de código)
    * `ci: <descripción en español>` (cambios en integración continua)
  * **REGLA DE INTEGRACIÓN CONTINUA (CI):** Cada *push* debe validar el CI de GitHub Actions. En caso de fallo, se debe corregir el código y volver a validar. **Bajo ninguna circunstancia se pueden utilizar directivas para ignorar advertencias en el CI** (`# shellcheck disable` o banderas `-e`). El código debe ajustarse a la regla, no la regla al código.

---

## 2. Inventario de Archivos del Proyecto

Todos los archivos del proyecto son portables y se adaptan dinámicamente al directorio donde se alojen y al hardware del servidor:

| Archivo / Ruta | Tipo | Descripción |
| :--- | :--- | :--- |
| `install.sh` | Script Bash CLI | **Instalador Remoto Oficial y Gestor CLI** para desplegar el comando `nas`, con auto-actualización (`update`) y desinstalación limpia. |
| `src/asistente.sh` | Script Bash (TUI `whiptail`) | **Asistente Visual Interactivo** con colores nativos, detección dinámica de discos/IP/usuario, validación en vivo, ciclo de edición y 9 módulos de gestión. |
| `src/core/deploy.sh` | Script Bash CLI | **Motor de Despliegue Automatizado** con detección inteligente de entorno, protección de partición raíz, soporte de roles (`ARCHIVOS` o `BACKUP`), formateo, Samba, Cockpit y parches. |
| `src/core/uninstall.sh` | Script Bash CLI | **Desinstalador y Limpiador Total** para restablecer el servidor a su estado base limpio. |
| `src/core/updater.sh` | Script Bash CLI | **Motor de actualización remota desde GitHub** para entornos simplificados. |
| `src/lib/{colors,helpers}.sh` | Bash Lib | Paleta ANSI y funciones de detección de entorno (IP, NetBIOS, Workgroup, usuario, disco base). |
| `src/modules/*.sh` | Bash (TUI `whiptail`) | Módulos del asistente: `deploy_wizard`, `groups`, `shares`, `backups`, `users`, `diagnostics`. |
| `src/web/backups/` | Web (Cockpit) | Plugin de respaldos: `index.html`, `main.js`, `style.css`, `backup_api.py` y FontAwesome. |
| `tests/helpers.bats` | BATS | Pruebas unitarias de las funciones auxiliares de entorno. |
| `tests/failure_*.bats` | BATS | Pruebas de inyección de fallos (discos en uso y runners de backup). |
| `tests/test_api.py` | pytest | Pruebas de validación y fallos del backend web. |
| `FAILURE_MODES.md` | Markdown | Modos de fallo y su verificación (FMEA). |
| `.github/workflows/ci.yml` | CI | Pipeline de GitHub Actions: ShellCheck, BATS y Flake8. |
| `.gitattributes` | Config | Normalización de fin de línea (LF) y tratamiento de binarios. |
| `README.md` | Markdown | **Guía de Puesta a Punto Paso a Paso** para preparación y hardening de Debian 13. |
| `SMB_DEBIAN.md` | Markdown | **Manual Técnico y Guía de Replicación** para usuarios y administradores. |
| `SECURITY.md` | Markdown | **Modelo de seguridad**, rollback de actualizaciones, identidades SSH, credenciales y parches de Cockpit. |
| `AGENTS.md` | Markdown | **Este documento maestro de contexto para agentes de IA**. |

### 2.1 Cuadro Maestro de Tecnologías, Subsistemas y Librerías

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

---

## 3. Roles del Servidor y Matriz de Seguridad

El sistema está diseñado para operar bajo dos roles mutuamente excluyentes:

```text
                               ┌─────────────────────────┐
                               │    Servidor Debian 13   │
                               └────────────┬────────────┘
                     ┌──────────────────────┴──────────────────────┐
                     ▼                                             ▼
        ┌─────────────────────────┐                   ┌─────────────────────────┐
        │      Rol: ARCHIVOS      │                   │       Rol: BACKUP       │
        │    (NAS Departamental)  │                   │  (Central de Respaldos) │
        └────────────┬────────────┘                   └────────────┬────────────┘
                     │                                             │
      ┌──────────────┴──────────────┐               ┌──────────────┴──────────────┐
      ▼                             ▼               ▼                             ▼
Carpetas Visibles:            Grupos:         Carpetas Ocultas ($):         Grupos:
[SISTEMAS]                    grp_sistemas    [BACKUPS_WINDOWS$]            SOLO grp_sistemas
[CAMPANA_UNO_*]               grp_empleados   [BACKUPS_LINUX$]              SOLO grp_backups
[CAMPANA_DOS_*]               grp_c1_*, c2_*  [BACKUPS_SERVIDORES$]         (Cero empleados)
```

### A. Despliegue Base Limpio (Servidor NAS o Central de Backup):
* **0 Redes Compartidas Automáticas:** El archivo `smb.conf` se inicializa únicamente con la sección `[global]` optimizada, sin recursos de prueba ni carpetas innecesarias.
* **Grupo Maestro:** Únicamente se crea `grp_sistemas` (con permisos totales `2770` sobre `/srv/nas`). El administrador del servidor queda asignado a `sudo,adm,grp_sistemas`.
* **Gestión 100% Modular desde el Asistente:**
  * **Creación de Grupos (Menú [2]):** Grupos departamentales o técnicos según las necesidades del entorno.
  * **Creación de Recursos (Menú [3]):** Configuración guiada con elección de visibilidad (Oculto `$` por defecto o Visible) y 4 esquemas de permisos granulares:
    1. *Lectura y Escritura por Grupo:* Todos los grupos autorizados (`read only = no`, `mask 0770`).
    2. *Solo Lectura General + Escritura Exclusiva:* Acceso general de lectura con escritura restringida (`read only = yes`, `write list = @grupo_escritura`, `mask 0775`).
    3. *Solo Lectura Estricta:* Consulta histórica sin modificación (`read only = yes`, `mask 0755`).
    4. *Acceso Público / Invitados:* Libre acceso con o sin clave (`guest ok = yes`).

### B. Selección Condicional de Filesystem según Rol y Hardware:
* El script `deploy.sh` y el asistente inspeccionan `/sys/block/<disco>/queue/rotational`:
  * **Rol ARCHIVOS en HDD (`rotational=1`):** Formateo en `ext4`, reducción de reserva con `tune2fs -m 1`, montaje con `rw,noatime,commit=2` (sincronización cada 2s para resiliencia ante apagones) y readahead de `4096 KB`.
  * **Rol ARCHIVOS en SSD (`rotational=0`):** `ext4` con `rw,noatime,commit=5`, readahead de `1024 KB` y activación de `fstrim.timer`.
  * **Rol BACKUP en HDD (`rotational=1`):** Formateo en `Btrfs`, montaje con `rw,noatime,compress=zstd:3,space_cache=v2,autodefrag`, readahead de `4096 KB` y auditoría mensual contra *Bit Rot*.
  * **Rol BACKUP en SSD (`rotational=0`):** `Btrfs` con `rw,noatime,compress=zstd:3,space_cache=v2,ssd,discard=async`, readahead de `1024 KB`, `fstrim.timer` y auditoría mensual.

### C. Reutilización de Datos Existentes (`--keep-data`):
* Soporte en el Asistente visual y en CLI (`--keep-data`) para montar discos preexistentes en `/srv/nas` sin formatear ni perder información. Detección automática de la partición válida (`NAS_DATA` o partición previa) y de su sistema de archivos (`ext4`, `btrfs`, etc.) con opciones optimizadas.

### D. Optimización Samba para Office / Excel (+100 puestos concurrentes):
* Inclusión de módulos VFS `acl_xattr` y `streams_xattr` para emulación nativa de flujos alternativos NTFS y almacenamiento de ACLs.
* Directivas de red de alto rendimiento: `store dos attributes = yes`, `inherit permissions = yes`, `strict sync = yes`, `use sendfile = yes`, `aio read/write size = 16384` y `max open files = 65535`.

### E. Herencia de Permisos y ACLs por Defecto (Esquema 2):
* Inyección de `default ACL` (`setfacl -d`) en subdirectorios para que cualquier archivo creado por el grupo de escritura otorgue lectura a los demás grupos autorizados sin requerir tareas cron de corrección.

---

## 4. Motor de Copias de Seguridad Multiplataforma

### A. Respaldo de Servidores Windows:
* **Protocolo:** CIFS / SMB 3.1.1 con parámetros de alta resiliencia (`vers=3.1.1,noserverino,cache=none,soft,timeo=30`).
* **Soporte de Dominios Active Directory:** Desglose automático de credenciales en formato `DOMINIO\usuario` y `DOMINIO/usuario` en `/etc/backup-credentials/<tarea>.cred` con permisos `0600 root:root`.
* **Mecanismo:** Montaje temporal en `/mnt/backup_sources/<tarea>` con flag `ro` (Solo Lectura) ➜ Ejecución de Snapshot ➜ Desmontaje inmediato.

### B. Respaldo de Servidores Linux Remotos:
* **Protocolo:** SSH túnel con `rsync -aAXH --numeric-ids -v -z --timeout=60` y `sshpass`.
* **Seguridad de Llaves:** Verificación automática de firmas en `/root/.ssh/known_hosts_backup` con `StrictHostKeyChecking=accept-new` (sin degradar a `no`).
* **Mecanismo:** Preserva propietarios, fechas, ACLs, atributos extendidos y permisos POSIX exactos.

### C. Deduplicación por Enlaces Duros (*Hardlinks*):
* **Estructura en disco:** `/srv/nas/BACKUPS_HISTORICOS/<tarea>/snapshot_YYYY-MM-DD_HHMMSS/`.
* **Cómo funciona:** `rsync --link-dest=<ultimo_snapshot>`. Los archivos que no cambiaron apuntan al mismo inodo físico en disco (0% espacio extra duplicado). Ahorro superior al 85% de disco.
* **Política de Retención:** Al crearse un snapshot, el script cuenta los existentes; si superan $N$, elimina el más antiguo cronológicamente (`head -n -"$RETENTION"`). Los archivos vigentes **nunca se borran** gracias al contador de referencias de inodos de Linux.

### D. Test de Conexión en Vivo y Ciclo de Edición:
* Antes de crear una tarea, el asistente prueba la IP, recurso compartido y contraseña en 1 segundo.
* Si falla (ej. error tipográfico o nombre NetBIOS sin DNS), muestra el error detallado y permite **corregir los datos sin perder lo escrito**.

### E. Staging Atómico y Resiliencia ante Apagones:
* Cada backup escribe inicialmente en `$BKP_DIR/.inprogress_$DATE_STR`.
* Trampas de salida (`trap cleanup EXIT TERM INT`) garantizan que ante apagones, cortes de red o aborto manual, el recurso remoto se desmonte y los datos parciales se eliminen.
* Solo al concluir con éxito (código 0), se realiza el renombrado atómico `mv` a `snapshot_$DATE_STR`.

### F. Monitoreo Preventivo de Espacio Libre:
* Evaluación previa con `df -Pk`: advertencia en la bitácora si el uso supera el 85%; cancelación segura inmediata si el espacio libre es inferior a 2 GB o la ocupación supera el 95%.

---

## 5. Parches Críticos y Soluciones de Debian 13 Integradas

Si se reinstala el servidor desde cero o en otra máquina, estos parches están incluidos en `src/core/deploy.sh`:

1. **Visibilidad en Red Windows (WSDD2):**
   * *Problema:* `wsdd2` en Debian 13 usa `DynamicUser=true` y falla al ejecutar `testparm` para leer `smb.conf`.
   * *Solución:* Override en `/etc/default/wsdd2` y `/etc/systemd/system/wsdd2.service.d/override.conf` con `WSDD2_OPTS="-N <NETBIOS> -G <WORKGROUP> -H <NETBIOS>"`.
2. **Compatibilidad de Cockpit en Servidores en Español:**
   * *Problema:* Cockpit espera salida en inglés de herramientas del sistema (`chage`, `passwd -S`, `lastb`).
   * *Solución:* Wrappers en `/usr/local/sbin/chage`, `/usr/local/sbin/passwd` y `/usr/local/bin/lastb` que fuerzan `LC_ALL=C LANG=C`.
3. **Interfaz Web (Cockpit Backups):**
   * *Arquitectura:* Aplicación web ES5 nativa sin frameworks pesados, inyectada en `/usr/share/cockpit/backups`.
   * *Estilos:* Utiliza PatternFly 4 (`cockpit.css.gz`) importado desde Navigator para mantener coherencia de diseño oscuro/claro nativo de 45Drives.
   * *Backend:* `backup_api.py` actúa como puente JSON. `cockpit.spawn` invoca la API localmente usando escalada de privilegios segura.
4. **Optimización del Kernel sysctl (`/etc/sysctl.d/99-nas-tuning.conf`):**
   * Ampliación de descriptores inotify (`max_user_watches = 524288`), keepalive TCP SMB (`tcp_keepalive_time = 120`), retención de caché VFS (`vfs_cache_pressure = 30`) y control estricto de memoria sucia (`vm.dirty_bytes = 268435456`, `dirty_background_bytes = 67108864`).
5. **Readahead Tuning por udev (`/etc/udev/rules.d/60-nas-readahead.rules`):**
   * Reglas udev persistentes que asignan `1024 KB` en unidades SSD y `4096 KB` en discos mecánicos HDD.
6. **Protección udev del Disco del SO (`/etc/udev/rules.d/80-udisks2-hide-os.rules`):**
   * Inyección de `UDISKS_IGNORE="1"` para ocultar la unidad del sistema operativo en Cockpit Storage y UDisks2.
7. **Prevención de Corrupción Silenciosa (*Bit Rot*):**
   * Tarea cron mensual en `/etc/cron.d/nas-btrfs-scrub` (`0 2 1 * *`) para auditar la integridad criptográfica de los datos en Btrfs.
8. **Mantenimiento SSD y Rotación de Logs:**
   * Habilitación de `fstrim.timer` para optimización de bloques flash y rotación programada con compresión mediante `logrotate` en `/etc/logrotate.d/nas-backups` y `nas-deploy`.

> [!NOTE]
> Las directivas globales `set -e` fueron removidas de los módulos importables (`source`) como `updater.sh` para prevenir crashes abruptos del TUI al cancelar diálogos de `whiptail`.

---

## 6. Comandos de Operación Rápida

### Lanzar el Asistente Interactivo:
```bash
sudo nas
```

### Despliegue Manual por Consola:
```bash
# Servidor NAS (usando partición local o disco dedicado):
printf '%s\n' '<CLAVE_ADMIN>' | sudo bash src/core/deploy.sh LOCAL EAD-COL SRV-EAD-NAS admin - ARCHIVOS

# Servidor NAS conservando datos preexistentes en un disco dedicado (/dev/sdb):
printf '%s\n' '<CLAVE_ADMIN>' | sudo bash src/core/deploy.sh /dev/sdb EAD-COL SRV-EAD-NAS admin - ARCHIVOS --keep-data

# Servidor de Backup con disco secundario formateándolo desde cero (/dev/sdb):
printf '%s\n' '<CLAVE_ADMIN>' | sudo bash src/core/deploy.sh /dev/sdb EAD-COL SRV-EAD-BKP admin - BACKUP
```

> [!IMPORTANT]
> Por seguridad, `deploy.sh` **aborta** si el disco dedicado está en uso (montado, PV de LVM o miembro de RAID); `--force` solo confirma sin preguntar (sin saltar chequeos). Para formatear un disco en uso se exige `--ignore-in-use` con confirmación textual explícita (`SI-FORMATEAR`). La clave puede enviarse por `stdin` usando `-` en su lugar (evita exponerla en `ps`).

### Limpieza y Desinstalación Total:
```bash
sudo nas uninstall
```

### Verificar Tareas y Logs de Backup:
```bash
ls -la /etc/cron.d/backup_*
tail -f /srv/nas/LOGS_BACKUP/backup_*.log
```

