# Tecnologías Usadas y Cuadro Maestro • NAS Debian 13

Este documento recopila de forma detallada y ordenada todas las tecnologías, librerías, demonios y subsistemas del kernel utilizados en el proyecto. Explica con claridad para qué sirve cada componente y qué beneficio concreto aporta al servidor.

---

## 1. Filosofía Tecnológica

En lugar de crear un sistema dependiente de Docker, máquinas virtuales intermedias o servicios pesados en Java o Electron, este servidor apuesta por la **potencia nativa de Debian 13 (Trixie)**:

1. **Cero consumo superfluo:** En reposo, los servicios administrativos consumen ~0 MB de RAM.
2. **Herramientas estándar de la industria:** Todo se apoya en utilidades UNIX comprobadas a lo largo de décadas (`samba`, `rsync`, `btrfs`, `ext4`, `nginx`, `sqlite3`, `whiptail`).
3. **Resiliencia ante fallos reales:** Diseñado específicamente para no corromper datos ante cortes de energía, desconexiones de red repentinas o saturación de disco.
4. **100% Offline:** Sin dependencias externas ni rastreadores, funcionando perfectamente en redes locales aisladas.

---

## 2. Cuadro Maestro de Componentes por Subsistema

### 2.1 Base del Sistema Operativo y Kernel

| Tecnología / Subsistema | Componente | ¿Para qué sirve? | Beneficio técnico directo |
| :--- | :--- | :--- | :--- |
| **Debian 13 (Trixie) x86_64** | Sistema Operativo Base | Plataforma Linux con kernel 6.12+ y glibc moderna. | Soporte a largo plazo, estabilidad probada y máxima compatibilidad con hardware moderno y heredado. |
| **Tuning de Kernel `sysctl`** | `/etc/sysctl.d/99-nas-tuning.conf` | Ajusta parámetros de memoria sucia, inotify y TCP. | `vm.dirty_bytes=256MB` evita congelamientos de I/O en transferencias masivas; `tcp_keepalive` (120s) cierra clientes colgados en 3 min; `fs.inotify` soporta más de 500k archivos vigilados. |
| **Readahead Dinámico `udev`** | `/etc/udev/rules.d/60-nas-readahead.rules` | Búfer de precarga de lectura secuencial. | Fija `4096 KB` en HDD para acelerar lecturas continuas y `1024 KB` en SSD para priorizar latencia baja sin saturar la RAM. |
| **Aislamiento de Disco del SO** | `/etc/udev/rules.d/80-udisks2-hide-os.rules` | Inyecta `UDISKS_IGNORE="1"` en la unidad raíz. | Impide que herramientas de disco o descuidados humanos formateen por accidente la unidad donde corre el sistema operativo. |
| **Bloqueo de Suspensión** | `systemd-logind` + mask targets | Desactiva suspensión, hibernación y cierre de tapa. | Asegura que el servidor permanezca siempre encendido y respondiendo en red 24/7. |

---

### 2.2 Almacenamiento y Filesystems

| Tecnología / Subsistema | Componente | ¿Para qué sirve? | Beneficio técnico directo |
| :--- | :--- | :--- | :--- |
| **ext4 Optimizado** | Filesystem (Rol `ARCHIVOS`) | Sistema de archivos transaccional para almacenamiento NAS. | `tune2fs -m 1` recupera el 4% de espacio reservado para root; `commit=2` en HDD vacía el journal cada 2s contra apagones; `noatime` reduce escrituras innecesarias en un 30%. |
| **Btrfs con Zstandard** | Filesystem (Rol `BACKUP`) | Sistema de archivos avanzado con compresión en tiempo real. | `compress=zstd:3` ahorra entre 20% y 40% de espacio físico; `space_cache=v2` agiliza asignación de bloques; sumas SHA256 integradas validan cada bloque de datos. |
| **Btrfs Scrub Programado** | `/etc/cron.d/nas-btrfs-scrub` | Tarea mensual automática (día 1 a las 02:00). | Audita los datos contra sus sumas criptográficas, detectando y corrigiendo corrupción silenciosa (*Bit Rot*) antes de que afecte restauraciones. |
| **`fstrim.timer`** | Servicio de Sistema | Recorte periódico de bloques descartados en SSD/NVMe. | Mantiene tasas óptimas de rendimiento de escritura sostenida y previene el desgaste prematuro de memorias flash. |
| **Modo Reutilización (`--keep-data`)** | Motor de Despliegue | Monta particiones preexistentes en `/srv/nas`. | Permite reinstalar o migrar el sistema operativo conservando terabytes de archivos intactos sin necesidad de formatear. |

---

### 2.3 Red, Compartición y Dominio

| Tecnología / Subsistema | Componente | ¿Para qué sirve? | Beneficio técnico directo |
| :--- | :--- | :--- | :--- |
| **Samba 4 (`smbd` / `nmbd`)** | Servicio de Red SMB/CIFS | Compartición de archivos para Windows, Linux y macOS. | Protocolo estándar nativo, cifrado negociado (`desired`), protocolo mínimo seguro SMB2_02 y aislamiento de usuarios. |
| **Módulos VFS Samba** | `vfs objects = acl_xattr streams_xattr` | Compatibilidad con ADS de NTFS y ACLs en atributos extendidos. | **Previene cuellos de botella con +100 equipos en Excel/Office**: edición simultánea sin bloqueos de temporales `~$` y soporte de marcas Windows (`Zone.Identifier`). |
| **Directivas Samba de Rendimiento** | Configuración `smb.conf` | `store dos attributes`, `strict sync`, `use sendfile`, `aio read/write size = 16384`, `max open files = 65535`. | Transferencia directa kernel-red sin saltos a memoria de usuario; latencia ultrabaja en libros contables masivos y eliminación de límites de descriptores. |
| **WSDD2** | Descubrimiento de Red | Implementación ligera de Web Services Discovery y LLMNR. | Visibilidad instantánea en el Explorador de Windows 10/11 sin activar protocolos inseguros como NetBIOS broadcast o SMBv1. |
| **Active Directory Nativo** | `realmd`, `sssd`, `adcli`, Kerberos | Integración corporativa con dominio Windows. | Autentica usuarios de dominio directamente en el NAS y Samba, con soporte para sintaxis `DOMINIO\usuario`. |
| **ACLs POSIX Granulares** | `acl` / `setfacl` | Control de acceso multi-grupo con herencia por defecto. | Permite esquemas con múltiples grupos en lectura y uno exclusivo en escritura (`default ACL` en subdirectorios). |

---

### 2.4 Motor de Copias de Seguridad y Replicación

| Tecnología / Subsistema | Componente | ¿Para qué sirve? | Beneficio técnico directo |
| :--- | :--- | :--- | :--- |
| **rsync con Hardlinks** | Replicación y Deduplicación | `rsync -aAXH --numeric-ids --link-dest`. | **Deduplicación superior al 85%**: los archivos no modificados comparten el inodo en disco consumiendo 0 bytes extras; preservación exacta de ACLs, UIDs y atributos. |
| **Staging Atómico** | Pipeline de Backups | Escritura en `.inprogress_*` y promoción con `mv`. | **Resistencia crítica ante apagones y fallas de red**: trampas `trap cleanup` borran copias incompletas; el repositorio histórico únicamente contiene snapshots 100% íntegros. |
| **cifs-utils (SMB 3.1.1)** | Conector de Respaldo Windows | Montaje temporal de solo lectura (`ro,vers=3.1.1,noserverino,cache=none,soft,timeo=30`). | Protocolo moderno SMB 3.1.1; `noserverino` evita errores de inodos remotos; `soft,timeo=30` impide cuelgues del kernel si Windows se apaga. |
| **OpenSSH / sshpass** | Conector de Respaldo Linux | Túnel SSH con `known_hosts_backup` dedicado y `accept-new`. | Cifrado punto a punto contra ataques *Man-in-the-Middle*; registra claves nuevas automáticamente sin rebajar la seguridad a `StrictHostKeyChecking=no`. |
| **Control de Concurrencia con `flock`** | Programación de Tareas | Bloqueo por archivo en `/var/lock/backup_<tarea>.lock`. | Garantiza la exclusión mutua de procesos impidiendo que dos copias de una misma tarea se ejecuten a la vez. |
| **Monitoreo de Espacio (`df -Pk`)** | Seguridad Operativa | Comprobación de capacidad previa al inicio de cada copia. | Emite alerta preventiva al superar el 85% de uso y cancela la copia si restan menos de 2 GB o se supera el 95%, evitando caídas por disco lleno. |

---

### 2.5 Panel Web de Administración y Base de Datos

| Tecnología / Subsistema | Componente | ¿Para qué sirve? | Beneficio técnico directo |
| :--- | :--- | :--- | :--- |
| **Nginx-light** | Servidor Web HTTP/HTTPS | Servidor web ultraligero y seguro. | Mínimo consumo de recursos, terminación SSL/TLS y proxy rápido hacia PHP-FPM. |
| **PHP 8 + PHP-FPM ondemand** | Backend de Aplicación | Arquitectura MVC pura servida con `pm = ondemand`. | Consumo nulo en reposo (~0 MB de RAM cuando no se navega); apagado automático de procesos ociosos. |
| **Invocación Segura `proc_open`** | Backend PHP | Ejecución de comandos del sistema pasando arrays de argumentos. | **Elimina por diseño la inyección de comandos**: los argumentos no son evaluados por `/bin/sh`, garantizando ejecución estricta. |
| **SQLite en modo WAL** | Base de Datos Estructurada | Archivo `/var/lib/nas/nas.sqlite` con PDO. | 0 MB de consumo en memoria en reposo; indexación ultrarrápida de auditoría, tareas, configuraciones e historial sin requerir demonios pesados (MySQL/PostgreSQL). |
| **Slate UI 100% Offline** | Frontend Web | CSS unificado con variables de tema (Claro/Oscuro) y JS nativo en ES6+. | Cero CDNs y cero fuentes remotas; 36 iconos SVG nativos; carga instantánea y funcionamiento autónomo sin Internet. |

---

### 2.6 Interfaz de Consola y Mantenimiento

| Tecnología / Subsistema | Componente | ¿Para qué sirve? | Beneficio técnico directo |
| :--- | :--- | :--- | :--- |
| **whiptail + Bash 5** | Interfaz Visual TUI | Menú interactivo con paleta ANSI de 9 módulos. | Administración completa desde terminal local o sesiones remotas SSH sin requerir entorno gráfico X11. |
| **Gestor CLI `nas`** | `/usr/local/bin/nas` | Comando global del sistema con subcomandos (`update`, `status`, `version`, `uninstall`). | Punto de entrada unificado y cómodo para el administrador en cualquier terminal. |
| **`logrotate`** | Mantenimiento de Registros | Rotación con `copytruncate` y compresión `gzip`. | Mantiene controladas las bitácoras evitando que crezcan indefinidamente y saturen el disco. |
| **UFW + fail2ban** | Seguridad de Red | Cortafuegos de paquetes y bloqueo de fuerza bruta. | Política estricta de denegación por defecto; autorización exclusiva de los puertos esenciales del servicio. |

---

## 3. Justificación de Decisiones Técnicas Clave

### ¿Por qué SQLite WAL en vez de MySQL o PostgreSQL?
Un servidor NAS no debe desperdiciar cientos de megabytes de RAM manteniendo demonios de bases de datos residentes solo para guardar configuraciones y registros de auditoría. Con SQLite en modo WAL (*Write-Ahead Logging*), las consultas son inmediatas, las lecturas y escrituras no se bloquean, el archivo se respalda con un simple `cp` y el consumo en memoria en reposo es exactamente **0 MB**.

### ¿Por qué PHP-FPM en modo `pm = ondemand`?
Los paneles de administración tradicionales mantienen procesos web ocupando memoria las 24 horas del día, incluso de noche cuando nadie está conectado. Con `ondemand`, Nginx levanta los procesos de PHP en milisegundos cuando un administrador abre el navegador y los cierra cuando termina, dejando toda la memoria libre para la caché del sistema de archivos de Samba.

### ¿Por qué rsync y hardlinks en vez de soluciones complejas de snapshots?
Las soluciones propietarias o basadas en bases de datos de deduplicación añaden capas de complejidad que pueden volverse irreparables si la base de datos se corrompe. Con `rsync` y enlaces duros (*hardlinks*), cada snapshot es simplemente un árbol de directorios estándar de Linux. Si el servidor fallara, los archivos se pueden leer y copiar directamente conectando el disco a cualquier equipo Linux, sin requerir software especial.
