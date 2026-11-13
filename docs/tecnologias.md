# Stack Tecnológico, Siglas y Fundamentos

> [!NOTE]
> Este documento explica con lenguaje claro qué tecnologías integran la plataforma **NAS & Central de Respaldos** sobre **Debian 13 Trixie (`x86_64`)**, qué problema práctico resuelve cada una y qué significa cada sigla técnica en inglés y en español.

---

## 1. Núcleo de Plataforma y Lenguaje Rust

La plataforma actual (`0.1.10`) se ejecuta de forma nativa en **Rust** (edición 2021, versión mínima `1.80`, política `#![forbid(unsafe_code)]` en todo el workspace). La elección de Rust elimina intérpretes y demonios pesados en tiempo de ejecución: el servidor web, el CLI y el asistente visual consumen memoria mínima en reposo y validan tipos antes de tocar disco o red.

| Tecnología / Crate | Qué Significa | Qué Problema Resuelve y Beneficio Práctico |
| :--- | :--- | :--- |
| **Rust + Workspace de 11 Crates** | Compilación nativa con seguridad de memoria sin recolector de basura | Reemplaza por completo la antigua capa PHP/Bash de gestión con binarios estáticos rápidos (`nas`, `nas-web`, `nas-tui`, `nasctl`, `nas-dev`) y cero riesgo de desbordamiento de memoria. |
| **`axum` 0.8 + `tokio`** | Framework web asíncrono y *runtime* de concurrencia para Rust | Atiende el panel administrativo en `127.0.0.1:8080` con latencia submilisegundo y consumo despreciable en reposo. |
| **SQLite en modo WAL** | *Write-Ahead Logging* (Registro de Escritura Anticipada) vía `rusqlite` | Base de datos embebida (`/var/lib/nas/nas.sqlite`) sin servidor externo. Permite lecturas concurrentes sin bloquear escrituras para sesiones, usuarios web, auditoría y *rate limiting*. |
| **`hmac` (`0.12`) + `sha2` (`0.10`)** | *Hash-based Message Authentication Code* + *Secure Hash Algorithm 2* | Implementa directamente en Rust **`PBKDF2-HMAC-SHA512`** (*Password-Based Key Derivation Function 2*, 210,000 iteraciones) para contraseñas y **`HMAC-SHA256`** para tokens anti-CSRF. |
| **`ed25519-dalek` (`2.x`)** | Firma digital de curva elíptica Edwards 25519 | Verifica criptográficamente el manifiesto de las actualizaciones oficiales contra la clave pública `packaging/release.pub` antes de tocar cualquier binario en producción. |
| **`ratatui` + `crossterm`** | *Rust Terminal User Interface* (Interfaz de Usuario en Terminal) | Impulsa `nas-tui` con 12 secciones interactivas utilizables directamente desde consola local o sesión SSH sin entorno gráfico. |
| **`clap` (`4.x`)** | *Command Line Argument Parser* (Analizador de Argumentos de Línea de Comandos) | Provee el CLI unificado `nas` con validación estricta y salida estructurada `--json` (`{success, message, data}`). |
| **`zip` (`2.x`)** | Empaquetado ZIP por flujo en Rust | Genera descargas comprimidas al vuelo desde el explorador web hasta el límite configurado (`50 GiB` en `ZIP_MAX_BYTES`). |

---

## 2. Interfaz Web Offline (Slate UI)

El panel de administración está diseñado para redes corporativas cerradas sin salida a internet (**100% offline**). Todos los recursos gráficos y lógicos se incrustan byte a byte dentro del binario `nas-web` mediante `include_str!` e `include_bytes!`.

- **Slate UI + 52 iconos SVG embebidos:** definidos como `<symbol>` en `crates/nas-web/templates/layout.html` junto con el logotipo vectorial `/img/logo-iintec.svg`, evitando fuentes de iconos externas o peticiones a CDN (*Content Delivery Network* — Red de Distribución de Contenidos).
- **Arquitectura JavaScript modular (28 archivos propios + 3 *vendor*):**
  - 28 archivos sin transpiladores externos ni `node_modules`: `app.js`, 4 utilidades en `assets/js/lib/` (`api.js`, `modal.js`, `toast.js`, `utils.js`) y 23 módulos por dominio en `assets/js/modules/`.
  - 3 librerías locales (*vendor*) con carga perezosa (*lazy loading*, solo se descargan al previsualizar un documento):
    - **`xlsx.full.min.js` (SheetJS):** lectura en navegador de hojas de cálculo (`.xlsx`, `.xls`, `.ods`, `.csv`), protegida además con un sanitizador DOM (*Document Object Model* — Modelo de Objetos del Documento) por lista blanca.
    - **`docx-preview.min.js`:** renderizado en cliente de documentos Word (`.docx`) sin enviar archivos a nubes externas ni consumir CPU del servidor NAS.
    - **`jszip.min.js`:** descompresión en navegador de paquetes `.docx` / `.pptx` para previsualización local.
- **Terminal PTY (*Pseudo-Terminal* — Seudoterminal) real:** integra sesiones `tmux` por usuario mediante el helper `nas-terminal`, permitiendo ejecutar `sudo`, comandos interactivos y programas de consola desde el navegador.

---

## 3. Almacenamiento, Sistemas de Archivos y Protección de Disco

Cada servidor se optimiza según el **rol** asignado (`ARCHIVOS`, `BACKUP`, `ARCHIVOS_BACKUP`) y el tipo de unidad física detectada en `/sys/block/*/queue/rotational` (disco mecánico HDD *Hard Disk Drive* vs unidad de estado sólido SSD *Solid State Drive*).

| Tecnología | Qué Significa y Qué Hace | Beneficio Práctico |
| :--- | :--- | :--- |
| **`ext4` optimizado** | *Fourth Extended Filesystem* (Sistema de Archivos Extendido 4) | Usado por defecto en el rol **`ARCHIVOS`**. Configura `noatime` (evita escrituras al leer), `commit=2` en HDD (`commit=5` en SSD) para vaciar transacciones cada 2 segundos ante apagones, y `tune2fs -m 1` para recuperar el 4% de espacio reservado al superusuario. |
| **`Btrfs` + `zstd:3`** | *B-tree File System* (Sistema de Archivos Árbol-B) + compresión *Zstandard* | Usado por defecto en **`BACKUP`** y **`ARCHIVOS_BACKUP`**. Activa `compress=zstd:3` y `space_cache=v2` (ahorro físico típico del 20% al 40%), `autodefrag` en HDD o `ssd,discard=async` en SSD, y sumas de comprobación bloque a bloque. |
| **`btrfs scrub` mensual** | Verificación de integridad física en Btrfs | Tarea programada en `/etc/cron.d/nas-btrfs-scrub` (día 1 de cada mes a las 02:00) para detectar y corregir corrupción silenciosa (*Bit Rot* — degradación silenciosa de bits en disco). |
| **LVM (*Logical Volume Manager*)** | Administrador de Volúmenes Lógicos (`pvcreate`, `vgcreate`, `lvcreate`) | Permite crear y ampliar volúmenes lógicos y subvolúmenes Btrfs directamente desde la vista de Almacenamiento del panel web. |
| **`fstrim.timer`** | Descarte periódico de bloques libres (*TRIM*) | Mantiene la velocidad sostenida de escritura y prolonga la vida útil en SSD y NVMe (*Non-Volatile Memory Express*). |
| **Guardián `is_os_disk()` en Rust** | Verificación en código de dispositivos raíz y `/boot` | Reemplaza el antiguo uso de reglas `udisks2` (`80-udisks2-hide-os.rules` ya no se despliega ni instala). `nas-engine` impide formatear o reparticionar el disco del sistema operativo desde CLI y Web. |
| **Modo `--keep-data`** | Reutilización segura de particiones existentes | Monta particiones `ext4` o `Btrfs` previas en `/srv/nas` sin formatear al reinstalar Debian o migrar el servidor. |

---

## 4. Red Windows/Linux, Concurrencia Office y Descubrimiento

| Tecnología | Significado y Configuración Real | Problema que Resuelve |
| :--- | :--- | :--- |
| **Samba 4 (`smbd` / `nmbd`)** | Implementación libre del protocolo **SMB** (*Server Message Block*) / **CIFS** (*Common Internet File System*) | Comparte carpetas nativamente con Windows 10/11 y servidores corporativos. Configura `server min protocol = SMB2_10`, `server max protocol = SMB3_11`, `server signing = mandatory` y `smb encrypt = desired`. |
| **Módulos VFS Samba** | *Virtual File System* (Sistema de Archivos Virtual): `acl_xattr`, `streams_xattr`, `recycle` | Almacena ACLs (*Access Control Lists*) de Windows y flujos alternativos NTFS (*ADS — Alternate Data Streams*) en atributos extendidos POSIX. Junto con `smb3 directory leases = auto`, `strict sync = yes`, `aio read/write size = 16384` y `max open files = 65535`, evita bloqueos con +100 usuarios editando libros de Excel simultáneamente. |
| **Papelera de Red (`.recycle`)** | Retención temporal de archivos borrados por SMB | Recupera archivos eliminados por usuarios Windows sin restaurar un backup completo; un trabajo diario (`cron.daily/nas-recycle-purge`) purga elementos antiguos. |
| **ACLs POSIX (`setfacl`)** | *Access Control Lists* bajo estándar *POSIX* (*Portable Operating System Interface*) | Permisos base `2770` + `default ACL` heredables para combinar grupos de lectura/escritura (`RW`) y solo lectura (`RO`) en una misma carpeta compartida. |
| **`WSDD2`** | *Web Services Discovery Daemon* (Demonio de Descubrimiento por Servicios Web) + LLMNR | Hace visible el servidor NAS en el Explorador de Red de Windows 10/11 sin habilitar protocolos obsoletos e inseguros como SMBv1 o NetBIOS *broadcast*. |
| **`realmd` + `sssd` + `adcli`** | Integración nativa con **AD** (*Active Directory* — Directorio Activo) y **Kerberos** | Permite unir el servidor NAS a un dominio Windows corporativo para autenticar usuarios de dominio sin duplicar contraseñas locales. |

---

## 5. Motor de Respaldos, Kernel y Auditoría

- **`rsync -aAXH --numeric-ids --link-dest`:** motor diferencial que preserva ACLs (`-A`), atributos extendidos (`-X`), enlaces duros (`-H`) y UIDs/GIDs numéricos exactos (`--numeric-ids`). Cuando un archivo no cambia respecto al snapshot previo, crea un *hardlink* que comparte inodo y ocupa **0 bytes adicionales** (deduplicación típica >85%).
- **Conector Windows (`mount.cifs`):** monta en solo lectura con `ro,vers=3.1.1,noserverino,cache=none,soft,nobrl`. Destaca **`nobrl`** (*no byte-range locks* — sin bloqueos de rango de bytes), que evita errores `EAGAIN` al copiar archivos abiertos en Windows, y `soft`, que impide bloqueos indefinidos del kernel si el origen se apaga.
- **Conector Linux (`OpenSSH` + `sshpass`):** usa clave privada Ed25519 por tarea o contraseña en archivo `0600` (`sshpass -e`), registrando huellas en `/root/.ssh/known_hosts_backup` bajo política **TOFU** (*Trust On First Use* — Confianza en el Primer Uso, `StrictHostKeyChecking=accept-new`).
- **Control `flock` y Staging `.inprogress_*`:** exclusión mutua por archivo de cerrojo en `/var/lock/backup_<id>.lock`, monitoreo previo de espacio (`df -Pk`: alerta al 85%, aborta si supera 95% o queda < 2 GiB libres), directorio temporal oculto con limpieza automática ante corte (`trap cleanup EXIT TERM INT`) y soporte de *Golden Snapshots* (`auto_permanent`).
- **Tuning de Kernel (`sysctl` + `udev`):**
  - `99-nas-tuning.conf`: `fs.inotify.max_user_watches=524288`, `tcp_keepalive` (limpia sesiones SMB muertas), `vm.dirty_bytes` (evita saturación de caché sucia), `rp_filter=1`, `tcp_syncookies=1`, `kptr_restrict=2`, `dmesg_restrict=1`, `yama.ptrace_scope=2` y desactivación de IPv6 (`disable_ipv6=1`).
  - `60-nas-readahead.rules`: precarga persistente de bloques (`1024 KB` SSD / `4096 KB` HDD).
- **Trazabilidad doble (`nas-audit` + `auditd`):** bitácora propia en Rust con encadenamiento SHA-256 (*tamper-evident*) complementada por `auditd` (*Linux Audit Daemon* — Demonio de Auditoría del Kernel) vigilando `/etc/sudoers*`, `/etc/samba/smb.conf`, `/etc/backup-credentials/` y `/usr/local/sbin/nas-terminal`.

---

## 6. Glosario Rápido de Siglas

| Sigla | Significado en Inglés | Traducción / Sentido en Español |
| :--- | :--- | :--- |
| **ACL** | *Access Control List* | Lista de Control de Acceso (permisos granulares por usuario o grupo). |
| **AD** | *Active Directory* | Directorio Activo de Microsoft para identidades y políticas de dominio. |
| **AIDE** | *Advanced Intrusion Detection Environment* | Entorno Avanzado de Detección de Intrusiones (verificación de integridad offline). |
| **CI** | *Continuous Integration* | Integración Continua (puertas automáticas de pruebas y calidad antes de integrar código). |
| **CLI** | *Command-Line Interface* | Interfaz de Línea de Comandos (`nas`, `nasctl`). |
| **CSP** | *Content Security Policy* | Política de Seguridad de Contenidos (cabecera HTTP anti-XSS). |
| **CSRF** | *Cross-Site Request Forgery* | Falsificación de Petición en Sitios Cruzados (ataque bloqueado con tokens HMAC). |
| **FIM** | *File Integrity Monitoring* | Monitoreo de Integridad de Archivos. |
| **FMEA** | *Failure Mode and Effects Analysis* | Análisis de Modos de Falla y Efectos (matriz preventiva de fallos y recuperación). |
| **HSTS** | *HTTP Strict Transport Security* | Seguridad de Transporte Estricta HTTP (fuerza HTTPS). |
| **NAS** | *Network Attached Storage* | Almacenamiento Conectado en Red. |
| **PBKDF2** | *Password-Based Key Derivation Function 2* | Función de Derivación de Claves Basada en Contraseña (210k rondas SHA-512). |
| **PTY** | *Pseudo-Terminal* | Seudoterminal interactivo para consola web (`tmux`). |
| **TUI** | *Terminal User Interface* | Interfaz Visual de Terminal (`nas-tui`). |
| **UFW** | *Uncomplicated Firewall* | Cortafuegos Simplificado para `iptables`/`nftables` en Debian. |
