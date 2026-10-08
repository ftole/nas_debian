# Seguridad y Endurecimiento • NAS Debian 13

Este documento constituye la **especificación técnica completa, exhaustiva y operativa** de todas las medidas de ciberseguridad, endurecimiento (*hardening*), auditoría forense, control de identidades y resiliencia con las que cuenta la plataforma NAS y Central de Respaldos (Debian 13 Trixie).

---

## 1. Principio de Defensa en Profundidad y Arquitectura de 4 Niveles

La plataforma no confía en un único perímetro defensivo. Implementa un modelo estricto de **defensa en profundidad (*Defense in Depth*)** estructurado en **4 niveles complementarios e independientes**, donde la vulneración teórica de una capa es contenida de forma inmediata por las salvaguardas de los niveles adyacentes:

![Modelo de Seguridad en Profundidad](assets/modelo_seguridad.svg)

### Matriz Resumen de los 4 Niveles de Seguridad

| Nivel | Ámbito / Capa | Tecnologías Clave | Propósito Principal |
| :--- | :--- | :--- | :--- |
| **Nivel 1** | **Núcleo y Kernel** | `sysctl`, udev, ext4 `commit=2`, Btrfs `scrub`, LVM/RAID lock | Endurecimiento del stack de red IP, aislamiento de memoria/punteros, protección física de disco y mitigación de DoS. |
| **Nivel 2** | **Integridad y Auditoría** | `auditd`, AIDE (FIM), Samba `full_audit`, rsyslog, logrotate | Registro de eventos del kernel en tiempo real, verificación criptográfica offline de archivos y trazabilidad de operaciones SMB. |
| **Nivel 3** | **Perímetro y Red** | UFW Firewall, OpenSSH TOFU (`accept-new`), fail2ban, WSDD2 | Denegación por defecto de puertos, autenticación SSH sin root, bloqueo de fuerza bruta y descubrimiento seguro sin NetBIOS broadcast. |
| **Nivel 4** | **Aplicación y Datos** | PHP-FPM ondemand, `proc_open`, CSRF tokens, Rate Limiting, RBAC tripartito, Staging Atómico, Hardlinks | Privilegios mínimos en entorno web, prevención total de inyección de comandos, control de acceso granular y respaldos inmunes a apagones. |

---

## 2. Nivel 1: Endurecimiento del Kernel y Núcleo del Sistema (Kernel Sysctl & Storage Hardening)

El nivel más bajo de la arquitectura blinda los parámetros del kernel Linux mediante `/etc/sysctl.d/99-nas-tuning.conf` y reglas udev persistentes.

### 2.1 Mitigación de Ataques de Red a Nivel de Kernel (Anti-Spoofing y SYN Flood)
- **Anti-IP Spoofing (RFC 3704):**
  ```ini
  net.ipv4.conf.all.rp_filter = 1
  net.ipv4.conf.default.rp_filter = 1
  ```
  Activa el filtrado de ruta inversa estricto (*Strict Reverse Path Forwarding*). El kernel descarta automáticamente cualquier paquete entrante cuya dirección IP de origen no coincida con la ruta de retorno correspondiente en la tabla de enrutamiento local, mitigando ataques de falsificación de IP y amplificación DDoS.
- **Mitigación de SYN Flood (Agotamiento de Conexiones TCP):**
  ```ini
  net.ipv4.tcp_syncookies = 1
  ```
  Habilita los *SYN Cookies* criptográficos. Cuando la cola de conexiones semi-abiertas (*backlog*) se satura, el kernel deja de reservar memoria en tablas de estado y responde codificando la secuencia en el número SYN-ACK, frustrando ataques DoS por inundación SYN.
- **Reciclaje Rápido de Sesiones TCP Huérfanas:**
  ```ini
  net.ipv4.tcp_keepalive_time = 120
  net.ipv4.tcp_keepalive_intvl = 15
  net.ipv4.tcp_keepalive_probes = 4
  ```
  Sondea conexiones inactivas a partir de los 2 minutos (en lugar de las 2 horas estándar de Linux) y las cierra tras 60 segundos sin respuesta. Esto libera descriptores de archivo y memoria ante caídas abruptas de clientes Windows.

### 2.2 Aislamiento de Memoria, Punteros de Kernel y Control de Procesos
- **Ocultamiento de Direcciones de Memoria del Kernel (`kptr_restrict`):**
  ```ini
  kernel.kptr_restrict = 2
  ```
  Oculta todas las referencias y punteros de memoria del kernel (`%pK`) a todos los usuarios del sistema, **incluyendo al usuario root**, mostrándolos como ceros. Esto previene que adversarios locales calculen desplazamientos de memoria para construir cadenas ROP (*Return-Oriented Programming*) o evadir KASLR (*Kernel Address Space Layout Randomization*).
- **Restricción del Buffer de Registro del Kernel (`dmesg_restrict`):**
  ```ini
  kernel.dmesg_restrict = 1
  ```
  Restringe la lectura de `dmesg` y del buffer de logs del kernel únicamente a procesos que posean la capacidad `CAP_SYSLOG`. Impide que atacantes locales sin privilegios inspeccionen fallos de segmentación, módulos cargados o trazas de memoria.
- **Bloqueo de Inyección de Código entre Procesos (`ptrace_scope` de Yama LSM):**
  ```ini
  kernel.yama.ptrace_scope = 2
  ```
  Bloquea la invocación de `ptrace()` por parte de procesos no administradores. Solo un proceso con `CAP_SYS_PTRACE` puede inspeccionar o depurar otros procesos, neutralizando herramientas de inyección en tiempo de ejecución, volcado de credenciales en memoria y hooks maliciosos.

### 2.3 Resiliencia de Entrada/Salida (I/O) y Control de Memoria Sucia
- **Límites de Escritura a Disco en Ráfaga:**
  ```ini
  vm.dirty_background_bytes = 67108864   # 64 MB
  vm.dirty_bytes = 268435456            # 256 MB
  vm.dirty_expire_centisecs = 300       # 3 segundos
  ```
  Fuerza al subsistema VFS a vaciar datos a disco en bloques pequeños y continuos. Evita que transferencias masivas de 50+ GB saturen la memoria RAM provocando congelamientos completos del sistema (*I/O stalls*).
- **Protección de Caché de Inodos y Monitoreo Inotify:**
  ```ini
  vm.vfs_cache_pressure = 30
  fs.inotify.max_user_instances = 2048
  fs.inotify.max_user_watches = 524288
  ```
  Mantiene en RAM los metadatos de archivos de uso frecuente para alto rendimiento y permite a Samba supervisar hasta 524,288 archivos concurrentes sin agotar recursos.

### 2.4 Protección del Almacenamiento Físico y Prevención de Corrupción
- **Aislamiento de la Unidad del SO:** Regla udev `/etc/udev/rules.d/80-udisks2-hide-os.rules` asigna `UDISKS_IGNORE="1"` al disco donde reside la raíz `/`, ocultándolo en UDisks2 e interfaces gráficas.
- **Bloqueo Incondicional de Volúmenes en Uso:** El motor de despliegue (`deploy.sh`) y la API web (`StorageService.php`) verifican firmas LVM (`PV/VG`) y arreglos RAID (`mdadm`). Si un disco contiene metadatos de volumen activo, **el sistema prohíbe formatearlo categóricamente**.
- **Confirmación Textual Estricta:** Cualquier formateo manual exige el paso interactivo de escribir exactamente `SI-FORMATEAR`. Las banderas `--force` nunca sobrepasan discos en uso.
- **Reutilización Segura de Datos (`--keep-data`):** Reconoce particiones preexistentes (`NAS_DATA`) y las monta en `/srv/nas` sin formateo, permitiendo migraciones sin riesgo de pérdida.
- **Sincronización contra Apagones y Bit Rot:**
  - En discos mecánicos ext4 (`rotational=1`): directiva `commit=2` que vacía transacciones del *journal* cada 2 segundos.
  - En Central de Backup con Btrfs: compresión `zstd:3` y auditoría mensual programada en `/etc/cron.d/nas-btrfs-scrub` (`btrfs scrub start -B /srv/nas`) para verificar y autoreparar sumas de comprobación frente a corrupción silenciosa (*Bit Rot*).

---

## 3. Nivel 2: Integridad del Sistema, Auditoría Forense y Detección de Intrusiones (FIM & Auditing)

El segundo nivel garantiza la trazabilidad forense inmutable de todas las acciones críticas del sistema operativo y de los recursos compartidos.

### 3.1 Auditoría del Kernel en Tiempo Real con `auditd`
El subsistema de auditoría del kernel (`auditd`) se despliega con reglas específicas en `/etc/audit/rules.d/nas.rules`, cargadas dinámicamente mediante `augenrules --load`:

```text
# Auditoría de modificaciones en permisos y configuración administrativa
-w /etc/sudoers -p wa -k sudoers_changes
-w /etc/sudoers.d/ -p wa -k sudoers_changes

# Auditoría de modificaciones en la configuración de recursos Samba
-w /etc/samba/smb.conf -p wa -k samba_changes

# Auditoría de accesos a credenciales de respaldos corporativos
-w /etc/backup-credentials/ -p rwa -k backup_credentials

# Auditoría de ejecuciones de la terminal administrativa segura
-w /usr/local/sbin/nas-terminal -p x -k nas_terminal
```

- **Vigilancia de Privilegios Administrativos:** Cualquier intento de modificar `/etc/sudoers` o los archivos drop-in en `/etc/sudoers.d/` genera un evento de kernel inmediato con la etiqueta `sudoers_changes`, registrando UID, PID, comando y archivo alterado.
- **Integridad de Samba:** Modificaciones no autorizadas en `/etc/samba/smb.conf` quedan etiquetadas como `samba_changes`.
- **Protección de Credenciales de Red:** Todo acceso de lectura, escritura o cambio de atributos sobre `/etc/backup-credentials/` genera eventos `backup_credentials` (`-p rwa`).
- **Trazabilidad de Terminal:** Toda ejecución del helper de terminal `/usr/local/sbin/nas-terminal` queda registrada bajo la clave `nas_terminal` (`-p x`).

### 3.2 Monitoreo de Integridad de Archivos Offline con AIDE (File Integrity Monitoring)
La plataforma incluye `aide` preconfigurado e inicializado de manera no bloqueante durante el despliegue (`aideinit -y -f` / `aide --init`).
- **Línea Base Criptográfica:** Genera la base de datos `/var/lib/aide/aide.db` con hashes criptográficos (SHA-256, SHA-512) de todos los binarios esenciales (`/bin`, `/sbin`, `/usr/bin`, `/usr/sbin`) y librerías del sistema.
- **Detección de Rootkits y Modificaciones Silenciosas:** Permite auditar en frío si algún binario del sistema operativo fue reemplazado o adulterado mediante `aide --check`.

### 3.3 Auditoría Forense de Archivos Samba con VFS `full_audit`
Cada recurso compartido de Samba incorpora en su definición el módulo VFS `full_audit`:
```ini
vfs objects = acl_xattr streams_xattr full_audit
full_audit:prefix = %u|%I|%m|%S
full_audit:success = connect disconnect mkdirat renameat unlinkat openat open
full_audit:failure = connect openat open unlinkat renameat
full_audit:facility = LOCAL5
full_audit:priority = NOTICE
```
- **Captura Granular de Eventos:** En Samba 4.22 (Debian 13), registra aperturas (`openat`, `open`), creaciones (`mkdirat`), renombrados (`renameat`) y eliminaciones (`unlinkat`).
- **Enrutamiento Dedicado en Rsyslog:** La directiva `local5.notice` se canaliza mediante `/etc/rsyslog.d/50-samba-audit.conf` hacia `/var/log/samba/audit.log`, aislando la auditoría de los registros generales del sistema.
- **Visualización en Tiempo Real:** El panel web (módulo Registros) parsea este archivo proporcionando auditoría interactiva con filtros por usuario, equipo cliente, recurso y operación.

### 3.4 Rotación Segura de Bitácoras (`logrotate`)
Las bitácoras críticas (`/var/log/nas-admin.log`, `/var/log/samba/audit.log`, `/srv/nas/LOGS_BACKUP/backups_master.log`) cuentan con configuración de rotación en `/etc/logrotate.d/` con `copytruncate` y compresión gzip semanal/mensual, evitando el agotamiento de espacio en disco.

---

## 4. Nivel 3: Perímetro de Red, Control de Tráfico y Conectividad Blindada

El tercer nivel blinda la exposición de servicios del NAS hacia la red local y externa.

### 4.1 Cortafuegos UFW Restrictivo
El firewall aplica una política estricta de **denegación por defecto para todo tráfico entrante** (`default deny incoming`) y habilita exclusivamente los servicios esenciales:

| Puerto / Protocolo | Servicio | Justificación de Seguridad |
| :--- | :--- | :--- |
| `22/tcp` | SSH | Acceso de administración remota cifrado con claves |
| `80/tcp` | Nginx HTTP | Redirección obligatoria a HTTPS y administración (acotable a subred local) |
| `443/tcp` | Nginx HTTPS | Panel web seguro cifrado con TLS 1.2 / 1.3 |
| `137,138/udp` | Samba NetBIOS | Resolución de nombres de red local |
| `139,445/tcp` | Samba SMB/CIFS | Compartición de archivos SMB 3.1.1 con firmas y cifrado |
| `3702/udp, tcp` | WSDD2 WSD | Descubrimiento dinámico de Windows 10/11 sin broadcast obsoleto |
| `5355/udp, tcp` | WSDD2 LLMNR | Resolución local de nombres de enlace |
| `5357/tcp` | WSDD2 HTTP | Notificación de eventos de descubrimiento |

Todos los demás puertos (incluyendo bases de datos o servicios auxiliares) quedan terminantemente cerrados.

### 4.2 Blindaje de SSH y Gestión de Huellas TOFU
- **Root Deshabilitado:** `PermitRootLogin no` en `/etc/ssh/sshd_config` y archivos drop-in. Exige autenticación con usuario regular y escalada mediante `sudo`.
- **Almacén Dedicado para Respaldos Remotos:** Las conexiones rsync/SSH hacia servidores Linux remotos no usan el archivo compartido del sistema ni degradan la comprobación. Utilizan `/root/.ssh/known_hosts_backup` (permisos `0600 root:root`) con la directiva `StrictHostKeyChecking=accept-new`.
- **Protección contra *Man-in-the-Middle*:** La primera conexión registra la clave pública de forma confiable (*Trust On First Use*). Si la firma remota se altera en conexiones posteriores, el proceso aborta de inmediato y lo reporta en la bitácora.

### 4.3 Detección Activa contra Fuerza Bruta y Parches Automáticos
- **fail2ban:** Supervisa autenticaciones SSH y bloquea automáticamente direcciones IP tras múltiples intentos fallidos.
- **Actualizaciones Desatendidas (`unattended-upgrades`):** Aplica de forma automática los parches críticos de seguridad procedentes del repositorio oficial Debian Security (`trixie-security`).
- **Bloqueo de Suspensión Física:** Reglas en `systemd-logind` que inhiben el apagado o suspensión por cierre de tapa o pulsación accidental de botón físico en servidores compactos o portátiles de prueba.

---

## 5. Nivel 4: Capa de Aplicación Web, Control de Acceso (RBAC), Cifrado y Respaldos Resilientes

El nivel superior protege la interfaz de administración web, el motor de ejecución de comandos, el almacenamiento estructurado y el ciclo de vida de los respaldos.

### 5.1 Aislamiento del Servidor Web y Mínimos Privilegios
- **Usuario no Privilegiado:** Nginx y PHP-FPM operan bajo la cuenta de sistema `www-data`, sin permisos de escritura fuera de su directorio de trabajo.
- **Pool Ondemand Ultraligero:** La directiva `pm = ondemand` detiene los procesos de PHP-FPM cuando no hay peticiones activas. El consumo de memoria en reposo es nulo (~0 MB) y la ventana de exposición de código PHP en RAM se minimiza.
- **Cifrado TLS y Cabeceras HTTP:** Certificado autofirmado con longitud de clave segura (o certificado corporativo) en `/etc/ssl/certs/nas-web.crt`, forzando cabeceras:
  - `X-Content-Type-Options: nosniff`
  - `X-Frame-Options: SAMEORIGIN`
  - `X-XSS-Protection: 1; mode=block`

### 5.2 Prevención Absoluta de Inyección de Comandos (`proc_open`)
Las clases controladoras y de servicio PHP (`SystemService`, `BackupService`, `UserService`, `StorageService`) interactúan con el sistema exclusivamente mediante `proc_open` pasando arreglos de cadenas individuales:
```php
// Ejecución segura: NO se invoca /bin/sh -c y los argumentos no se concatenan como texto plano
$process = proc_open(['systemctl', 'restart', 'smbd'], $descriptors, $pipes);
```
Esto erradica de raíz cualquier vector de **Command Injection** por caracteres especiales (`;`, `&&`, `|`, `` ` ``, `$()`).

### 5.3 Escalada Acotada con Sudoers (`nas-web`)
El archivo `/etc/sudoers.d/nas-web` contiene una lista blanca cerrada con los comandos y rutas absolutas que `www-data` puede invocar con `sudo`:
- Los alias `NAS_SERVICES`, `NAS_SAMBA`, `NAS_USERS`, `NAS_STORAGE`, `NAS_TERMINAL` y `NAS_BACKUP_EXEC` especifican argumentos y rutas canónicas.
- Cada modificación se valida antes con `visudo -c -f` para evitar corrupciones en el parser de sudo.

### 5.4 Confinamiento contra *Path Traversal* en el Explorador de Archivos
En el gestor de archivos web, cada ruta solicitada es normalizada mediante `realpath()` y validada contra `/srv/nas`:
- Intentos de escape con `..`, caracteres nulos o accesos simbólicos que apunten fuera de `/srv/nas` son rechazados con HTTP 400/403.
- La papelera oculta `.trash` queda estrictamente confinada y protegida contra navegación directa.

### 5.5 Seguridad Web: Tokens Anti-CSRF y Rate Limiting
- **Protección contra Falsificación de Peticiones en Sitios Cruzados (CSRF):** Cada sesión genera un token criptográfico de 64 caracteres hexadecimales (`hash_hmac`). Todas las peticiones mutantes (POST, PUT, DELETE) deben incluir el token coincidente en el cuerpo o en la cabecera `X-CSRF-Token`, bloqueando peticiones no autorizadas desde navegadores externos.
- **Rate Limiting contra Fuerza Bruta Web:** El middleware de autenticación (`AuthService`) rastrea intentos fallidos por IP. Al acumular 5 fallos, bloquea la dirección respondiendo de inmediato con código HTTP 429 (*Too Many Requests*).
- **100% Offline (Inmunidad a la Cadena de Suministro):** No se realizan llamadas a CDNs, Google Fonts ni scripts externos. La interfaz Slate UI incluye sus 36 iconos SVG y estilos de forma local.

### 5.6 Terminal Administrativa Segura (PTY Real)
- La terminal web opera mediante un helper root (`/usr/local/sbin/nas-terminal`) que gestiona sesiones PTY bajo `tmux` ejecutadas en el contexto del usuario autenticado.
- Cuenta con lista blanca estricta de acciones (`start`, `keys`, `capture`, `resize`, `kill`), directorio confinado y compatibilidad transparente con `sudo` interactivo.

### 5.7 Control de Acceso Basado en Roles (RBAC) Tripartito
El sistema clasifica las cuentas en tres niveles estrictos:
1. **`grp_superadmin` (Superadministrador Inmutable):**
   - Cuenta principal designada en el despliegue y registrada en `/etc/nas/superadmin`.
   - Control total sobre el sistema y única autorizada para otorgar o remover permisos de superadministrador.
   - **Inmutable:** No puede ser eliminada, degradada ni suspendida desde la interfaz web ni mediante scripts estándar.
2. **`grp_samba` (Administrador de Infraestructura):**
   - Acceso a consola/SSH, permisos de elevación con `sudo` y acceso completo al panel web.
3. **`grp_web` (Operador de Servicios):**
   - Acceso web para supervisar estado, explorar archivos, consultar bitácoras, gestionar redes compartidas y respaldos.
   - Sin acceso a almacenamiento físico, terminal interactiva, unión a dominio ni reinicio del host.
4. **Usuarios de Almacenamiento Estándar:**
   - Cuentas restringidas a Samba. Su shell de sistema se fija en `/usr/sbin/nologin`, impidiendo cualquier sesión interactiva por SSH o terminal local.

### 5.8 Sincronización y Herencia de Permisos con ACLs POSIX
- Las redes compartidas se configuran en `smb.conf` y se complementan con listas de control de acceso POSIX (`setfacl`).
- La inyección de ACLs por defecto (`setfacl -d`) en subdirectorios garantiza que cualquier nuevo archivo o carpeta creada por usuarios de un grupo herede automáticamente los permisos de lectura o escritura requeridos por los demás grupos autorizados.

### 5.9 Motor de Copias de Seguridad: Staging Atómico y Deduplicación
- **Credenciales Protegidas:** Contraseñas de red en `/etc/backup-credentials/<tarea>.cred` con permisos `0600 root:root`. Se rechaza cualquier credencial con permisos laxos.
- **Montaje en Solo Lectura:** Las fuentes CIFS/SMB se montan con el flag `ro`, imposibilitando cualquier alteración en el servidor de origen.
- **Staging Atómico y Trampas de Limpieza:** Cada copia escribe en un directorio oculto temporal `.inprogress_<timestamp>`. Interceptores de señal (`trap cleanup EXIT TERM INT`) garantizan que ante apagones o caídas de red, los datos parciales se eliminen y los recursos se desmonten. Solo al completar con éxito se ejecuta la promoción atómica mediante `mv`.
- **Deduplicación por Hardlinks Superior al 85%:** El motor utiliza `rsync --link-dest=<ultimo_snapshot>`, enlazando inodos idénticos sin duplicar bloques físicos en disco.

---

## 6. Actualización Segura y Rollback Automático

El gestor de actualizaciones (`sudo nas update` o menú [8] del asistente) incorpora salvaguardas para garantizar la continuidad del servicio:

1. **Validación de Origen:** Comprueba que la rama activa sea `main` y el repositorio remoto sea el oficial (`ftole/nas_debian`).
2. **Protección de Cambios Locales:** Si existen modificaciones sin confirmar en `/opt/nas_debian`, la actualización se suspende para no sobreescribir configuraciones locales.
3. **Respaldo Previo Atómico:** Antes de aplicar cambios, crea una copia completa del directorio en `/opt/nas_debian.update_backup_<timestamp>`.
4. **Fusión Estricta Fast-Forward:** Aplica `git merge --ff-only`. Si la historia diverge, la operación se cancela de forma segura.
5. **Validación Integral Posterior:**
   - Valida la integridad del repositorio con `git fsck`.
   - Verifica la sintaxis de todos los scripts Bash mediante `bash -n`.
   - Si alguna prueba falla, **restaura automáticamente el estado anterior desde la copia de respaldo**.
6. **Verificación Criptográfica Opcional (Firmas GPG):** En entornos de alta seguridad, se puede exigir la huella GPG del firmante (`NAS_REQUIRE_SIGNED_TAGS=true` y `NAS_UPDATE_SIGNER="<huella-40-hex>"`), verificando tags oficiales `vMAJOR.MINOR.PATCH`.

---

## 7. Matriz de Trazabilidad de Amenazas vs. Medidas de Seguridad

La siguiente tabla resume cómo interactúan los 4 niveles defensivos ante vectores de ataque y fallos habituales:

| Amenaza / Vector de Riesgo | Nivel Defensivo | Medida Aplicada | Resultado Operativo |
| :--- | :---: | :--- | :--- |
| **IP Spoofing / DoS Amplificado** | Nivel 1 | `rp_filter = 1` en sysctl | El kernel descarta paquetes con IP origen incoherente. |
| **TCP SYN Flood (Agotamiento DoS)** | Nivel 1 | `tcp_syncookies = 1` en sysctl | El host responde mediante cookies criptográficas sin agotar RAM. |
| **Explotación Local de Kernel (ROP/KASLR)** | Nivel 1 | `kptr_restrict = 2` y `dmesg_restrict = 1` | Direcciones de memoria y buffer de depuración ocultos incluso a root. |
| **Inyección de Código entre Procesos** | Nivel 1 | `kernel.yama.ptrace_scope = 2` | Procesos no privilegiados no pueden inspeccionar memoria ajena. |
| **Formateo Inadvertido del Disco del SO** | Nivel 1 | Regla udev `80-udisks2-hide-os.rules` | Disco del sistema ignorado y protegido en UDisks2. |
| **Modificación Silenciosa de Sudoers/SMB** | Nivel 2 | Reglas `nas.rules` en `auditd` | Generación instantánea de eventos de auditoría forense en el kernel. |
| **Alteración de Binarios del SO (Rootkits)** | Nivel 2 | Línea base criptográfica en `aide.db` | Detección de modificaciones no autorizadas mediante `aide --check`. |
| **Manipulación Indebida de Archivos SMB** | Nivel 2 | VFS `full_audit` + rsyslog dedicado | Trazabilidad completa de accesos, modificaciones y borrados en red. |
| **Acceso no Autorizado a Puertos de Red** | Nivel 3 | Firewall UFW (política de denegación por defecto) | Solo puertos autorizados responden; el resto queda en silencio. |
| **Fuerza Bruta en Acceso SSH** | Nivel 3 | `fail2ban` + `PermitRootLogin no` | Bloqueo automático de IP y prohibición de inicio directo como root. |
| **Ataque Man-in-the-Middle en Respaldos** | Nivel 3 | OpenSSH con `StrictHostKeyChecking=accept-new` | Cancelación inmediata de la copia si la firma remota fue alterada. |
| **Inyección de Comandos en Panel Web** | Nivel 4 | Invocación `proc_open` con listas de argumentos | No se invoca shell; parámetros nunca se concatenan como texto. |
| **Falsificación de Peticiones Web (CSRF)** | Nivel 4 | Tokens criptográficos de 64 caracteres hex | Peticiones POST/PUT/DELETE rechazadas si el token no coincide. |
| **Fuerza Bruta contra el Login Web** | Nivel 4 | Rate Limiting (HTTP 429 tras 5 fallos) | Bloqueo temporal de la IP atacante. |
| **Escape de Directorio (*Path Traversal*)** | Nivel 4 | Normalización `realpath()` contra `/srv/nas` | Rutas fuera de la raíz autorizada devuelven error 400/403. |
| **Escalada o Supresión de Cuentas Maestras** | Nivel 4 | Modelo RBAC y protección de `grp_superadmin` | Cuenta superadministradora inmutable; roles estrictamente divididos. |
| **Apagón Eléctrico durante Respaldo** | Nivel 4 | Staging atómico en `.inprogress_*` y `trap` | Datos parciales eliminados; repositorio histórico siempre íntegro. |
| **Corrupción Silenciosa de Datos (*Bit Rot*)** | Nivel 1 | Tarea mensual de `btrfs scrub` programada | Detección y reparación de bloques dañados antes de que afecten datos. |
