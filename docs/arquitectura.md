# Arquitectura del Sistema • NAS & Central de Respaldos (Debian 13)

Este documento explica de forma clara, directa y transparente cómo está construido el servidor por dentro, cómo interactúan sus componentes y por qué se tomaron estas decisiones de ingeniería.

---

## 1. Visión General de la Arquitectura

A diferencia de soluciones pesadas basadas en contenedores o paneles web que consumen gigabytes de memoria en reposo, este proyecto adopta una **arquitectura nativa y ligera sobre Debian 13 (Trixie)**. 

El servidor opera directamente sobre el kernel de Linux y los servicios estándar del sistema (`smbd`, `nginx`, `sssd`, `cron`), con toda la plataforma implementada en **Rust** y tres formas de administración complementarias:
1. **CLI (`nas`):** Administración programática con salida ANSI y `--json` para automatización.
2. **TUI (`nas-tui`):** Asistente de terminal con dashboard, servicios, respaldos y diagnóstico.
3. **Panel Web (`nas-web`, axum + Slate UI):** Las mismas vistas y el mismo `app.js` del panel original, con los assets embebidos en el binario, 100% offline y cero dependencias externas.

```
               [ Clientes Windows / Linux / macOS / Navegador Web ]
                                      │
           ┌──────────────────────────┴──────────────────────────┐
           ▼                                                     ▼
     Red SMB/CIFS (Puerto 445)                             Panel Web (Puerto 80/443)
   [Samba 4 + VFS acl/streams]                       [Nginx proxy_pass → axum]
           │                                                     │
           │                                        argv tipado (CommandSpec, sin shell)
           │                                                     ▼
           │                                    [nas-web: vistas + API + assets]
           │                                          [nas-engine: lógica]
           │                                          [Base SQLite en modo WAL]
           │                                                     │
           └──────────────────────────┬──────────────────────────┘
                                      ▼
                        [ Filesystems en /srv/nas ]
                    • ext4 optimizado (Rol ARCHIVOS)
                    • Btrfs con ZSTD (Rol BACKUP)
                                      ▼
                           [ Discos HDD / SSD / NVMe ]
```

A continuación se muestra el diagrama completo de capas y flujo del sistema:

![Arquitectura General del Sistema](assets/arquitectura_general.svg)

---

## 2. Desglose de Capas

### Capa 1: Hardware, Kernel y Ajustes del Sistema
El sistema detecta automáticamente las características físicas del almacenamiento y aplica reglas específicas:
- **Detección de tipo de medio:** Inspecciona `/sys/block/<disco>/queue/rotational`. Si es `1` (disco mecánico HDD), aplica un readahead de `4096 KB` y sincronización de transacciones cada 2 segundos. Si es `0` (unidad de estado sólido SSD o NVMe), fija un readahead de `1024 KB` y activa `fstrim.timer`.
- **Ajustes de memoria sucia (`sysctl`):** Configurados en `/etc/sysctl.d/99-nas-tuning.conf`. Limita la memoria sucia a `256 MB` (`vm.dirty_bytes`) y `64 MB` en segundo plano (`vm.dirty_background_bytes`). Esto evita que transferencias gigantescas colapsen la RAM y provoquen congelamientos de disco.
- **Limpieza de sesiones SMB huérfanas:** Mediante `net.ipv4.tcp_keepalive_time = 120`, el kernel detecta y cierra equipos cliente que se desconectaron de golpe en 3 minutos, en lugar de mantener sesiones abiertas durante 2 horas.
- **Aislamiento de la partición raíz:** La regla udev `/etc/udev/rules.d/80-udisks2-hide-os.rules` añade la marca `UDISKS_IGNORE="1"` al disco del sistema operativo, impidiendo que herramientas de almacenamiento lo formateen por descuido.

---

### Capa 2: Almacenamiento y Filesystems
Todo el almacenamiento gestionado se centraliza en el punto de montaje `/srv/nas`. La preparación admite dos estrategias:

| Estrategia | Cuándo se usa | Qué hace |
| :--- | :--- | :--- |
| **Formateo Limpio** | Discos nuevos o dedicados | Particiona con GPT, crea la etiqueta `NAS_DATA` y formatea con el sistema de archivos óptimo (`ext4` para NAS o `Btrfs` para copias de seguridad). |
| **Conservar Datos (`--keep-data`)** | Discos existentes con información previa | Identifica la partición válida, detecta su sistema de archivos (`ext4`, `btrfs`, etc.) y la monta en `/srv/nas` aplicando los parámetros de rendimiento sin tocar los datos. |

#### Optimización de Filesystems según Rol:
1. **Rol `ARCHIVOS` (ext4):**
   - Se ejecuta `tune2fs -m 1`, reduciendo la reserva de superusuario del 5% habitual al 1%, recuperando gigabytes de espacio útil.
   - En HDD se monta con `commit=2` (sincroniza el journal cada 2 segundos), reduciendo drásticamente la ventana de pérdida ante apagones repentinos.
   - Se aplica `noatime` para evitar escrituras constantes cada vez que alguien lee un archivo.
2. **Rol `BACKUP` (Btrfs):**
   - Se activa compresión transparente `compress=zstd:3`, logrando entre 20% y 40% de ahorro de espacio físico sin impacto perceptible en la CPU.
   - Se incluye `space_cache=v2` y `autodefrag` en discos mecánicos.
   - En SSD se configuran `discard=async` y `fstrim.timer`.
   - Se programa una tarea mensual en `/etc/cron.d/nas-btrfs-scrub` que valida cada bloque contra sumas de verificación SHA256 para prevenir y reparar corrupción silenciosa (*Bit Rot*).

---

### Capa 3: Servicios de Red y Dominio

#### Samba 4 (SMB/CIFS) y Módulos VFS
Para soportar más de 100 usuarios simultáneos en Microsoft Office y Excel sin bloqueos de red:
- **`vfs objects = acl_xattr streams_xattr full_audit`:** Guarda los permisos de Windows en atributos extendidos de Linux (`xattr`), emula los flujos de datos alternativos de NTFS (*Alternate Data Streams*) y audita forensemente las operaciones de archivos (`openat`, `renameat`, `unlinkat`, `mkdirat`) hacia `/var/log/samba/audit.log`. Esto permite que archivos temporales como `~$Libro1.xlsx` o marcas `Zone.Identifier` se gestionen de forma nativa sin corromper el archivo principal.
- **Rendimiento de red:** Directivas como `use sendfile = yes`, `aio read/write size = 16384` y `max open files = 65535` reducen las transferencias de contexto en el procesador y garantizan estabilidad bajo alta concurrencia.
- **Descubrimiento Windows sin SMBv1:** Se usa `wsdd2` con un override de systemd para que los equipos Windows 10/11 vean el NAS de inmediato en su explorador de red sin recurrir a protocolos obsoletos ni a difusiones NetBIOS inseguras.

#### Integración con Directorio Activo (Active Directory)
El sistema incluye la pila nativa corporativa (`realmd`, `sssd`, `adcli`, `krb5-user`). Esto permite:
- Unir el NAS al dominio Windows de la empresa con un clic desde el panel web o por consola.
- Autenticar usuarios del dominio corporativo con Kerberos.
- Desglosar credenciales en formato `DOMINIO\usuario` de forma segura.

---

### Capa 4: Capa de Aplicación Web (Rust + axum + Slate UI)

La aplicación web es un binario único (`/usr/local/bin/nas-web`) servido por `nas-web.service`
y publicado por Nginx mediante `proxy_pass http://127.0.0.1:8080`. El workspace se organiza así:

```
crates/
├── nas-core/         -> Configuración, rutas, roles (`ServerRole`), errores y contratos
├── nas-exec/         -> Ejecutor de comandos con argv tipado, timeouts y redacción
├── nas-audit/        -> Bitácora append-only con encadenamiento criptográfico SHA-256
├── nas-inventory/    -> Inventarios YAML multi-servidor y separación de perfiles
├── nas-ssh/          -> Transporte OpenSSH con ejecución remota y host keys
├── nas-engine/       -> Lógica de sistema (almacenamiento, Samba, respaldos, dominio…)
├── nas-cli/          -> CLI `nas` modularizada en subcomandos (`src/commands/`)
├── nas-web/          -> Panel web axum modularizado por dominios (`src/routes/`)
│   ├── templates/    -> layout.html y login.html generados desde layout.php/login.php
│   └── assets/       -> app.js, CSS, vendor de ofimática y SVG (include_bytes!)
├── nas-tui/          -> Asistente visual interactivo `nas-tui` (ratatui)
└── nasctl/, nas-dev/ -> Orquestación multi-host e inspección de entorno
```

#### Características del Backend:
- **Cero inyección de comandos:** Las utilidades del sistema (`systemctl`, `journalctl`, `smbpasswd`, `realm`, `adcli`, `rsync`) se ejecutan con `nas-exec`, que recibe un arreglo de argumentos tipado, aplica timeout y redacción de secretos; la contención fina se materializa en sudoers acotados y nunca se usa `/bin/sh -c`.
- **Helpers privilegiados acotados:** Las operaciones que requieren root se delegan en `nas-samba-apply`, `nas-share-dir`, `nas-share-acl`, `nas-accounts` y `nas-backup-manage`, autorizados individualmente en `/etc/sudoers.d/nas-web-rust`.
- **Base de datos SQLite en modo WAL:** El archivo `/var/lib/nas/nas.sqlite` almacena sesiones, auditoría (`web_audit`), historial de terminal, límites de peticiones (`api_rate_limits`) y usuarios del panel (`web_users`), con 0 MB en reposo.
- **Assets intocables:** `app.js`, `app.css`, `login.css`, el favicon y las librerías de ofimática se sirven byte a byte desde el binario; sus hashes están fijados en `tests/parity/assets.sha256`.
- **Paridad verificada:** Las plantillas se generaron desde `layout.php`/`login.php` y se validan en CI (`tests/parity.rs`), además de una comparación en vivo PHP vs Rust previa al retiro del panel anterior.

---

### Capa 5: Motor de Copias de Seguridad y Deduplicación

El motor de respaldos trabaja de forma autónoma mediante tareas de `cron` o ejecuciones bajo demanda:
- **Aislamiento con `flock`:** Cada tarea adquiere un cerrojo en `/var/lock/backup_<tarea>.lock`. Si una copia previa todavía sigue transfiriendo datos, la nueva ejecución se omite limpiamente sin solaparse.
- **Staging atómico (`.inprogress_*`):** Toda copia escribe en una carpeta temporal oculta. Si se apaga la luz, se corta la red o el usuario cancela la tarea, las rutinas de salida (`trap cleanup EXIT TERM INT`) purgan los datos incompletos. Únicamente las copias que finalizan con éxito total (código 0) se promueven mediante un renombrado atómico `mv` a `snapshot_YYYY-MM-DD_HHMMSS`.
- **Deduplicación por enlaces duros (*hardlinks*):** Con `rsync --link-dest`, los archivos que no cambiaron entre copias comparten el mismo inodo físico en disco. Esto ahorra más de un 85% de espacio respecto a copias completas repetitivas.

---

## 3. Rutas Clave del Sistema

| Ruta | Propósito | Permisos |
| :--- | :--- | :--- |
| `/srv/nas` | Raíz de almacenamiento de datos compartidos y backups | `2770 root:grp_samba` |
| `/srv/nas/BACKUPS_HISTORICOS/` | Repositorio de snapshots inmutables deduplicados | `0750 root:grp_samba` |
| `/srv/nas/LOGS_BACKUP/` | Bitácoras de cada tarea de respaldo | `0750 root:grp_samba` |
| `/var/lib/nas/nas.sqlite` | Base de datos SQLite (sesiones, auditoría, límites, usuarios del panel) | `0660 nas-web:nas-web` |
| `/etc/backup-credentials/` | Archivos de credenciales CIFS de origen | `0600 root:root` |
| `/root/.ssh/known_hosts_backup` | Almacén de huellas SSH para tareas de respaldo | `0600 root:root` |
| `/var/lock/backup_*.lock` | Descriptores de bloqueo de concurrencia | `0644 root:root` |
| `/etc/sudoers.d/nas-web-rust` | Concesión mínima de privilegios del panel (helpers y systemctl acotado) | `0440 root:root` |
| `/opt/nas_debian/` | Código fuente del proyecto y scripts de mantenimiento | `0755 root:root` |
| `/usr/local/bin/nas-web` | Binario del panel web (servicio `nas-web.service`) | `0755 root:root` |
| `/usr/local/bin/nas` | CLI de la plataforma Rust | `0755 root:root` |
