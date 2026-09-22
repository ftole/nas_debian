# Servidor NAS y Central de Respaldos Multiplataforma (Debian 13)

[![Debian 13](https://img.shields.io/badge/OS-Debian%2013%20(Trixie)-A81D33?style=for-the-badge&logo=debian&logoColor=white)](https://github.com/ftole/nas_debian) [![Bash Shell](https://img.shields.io/badge/Scripting-Bash-4EAA25?style=for-the-badge&logo=gnu-bash&logoColor=white)](https://github.com/ftole/nas_debian) [![Samba](https://img.shields.io/badge/Service-Samba%20SMB-0066CC?style=for-the-badge)](https://github.com/ftole/nas_debian) [![Cockpit](https://img.shields.io/badge/Web%20UI-Cockpit-FF6600?style=for-the-badge)](https://github.com/ftole/nas_debian)

Este repositorio contiene los scripts para desplegar y administrar, sobre Debian 13 (Trixie), un servidor de archivos en red (NAS departamental) y una central de copias de seguridad pensada para resistir ransomware. El despliegue se realiza desde la terminal, mediante un asistente interactivo o por línea de comandos, y la operación diaria puede gestionarse también desde un panel web basado en Cockpit.

## Características principales

- Asistente de despliegue con detección automática de discos, dirección IP y usuario administrador.
- Dos roles excluyentes: `ARCHIVOS` (recursos compartidos visibles) y `BACKUP` (repositorios ocultos).
- Copias de seguridad deduplicadas mediante enlaces duros para Windows (CIFS), Linux (SSH) y carpetas locales.
- Gestión de grupos, recursos compartidos, usuarios y tareas desde el asistente o desde el panel web.
- Base limpia: el despliegue no crea recursos de prueba; se añaden según las necesidades del entorno.
- Desinstalación total que devuelve el servidor a su estado base.

## Tecnologías utilizadas

| Área | Tecnología |
| :--- | :--- |
| Sistema operativo | Debian 13 (Trixie) |
| Automatización | Bash 5 y `whiptail` (interfaz de terminal) |
| Compartición de archivos | Samba (`smbd`/`nmbd`) y WSDD2 |
| Panel web | Cockpit con PatternFly 4 |
| Extensiones web | Plugins de 45Drives (File Sharing, Identities, Navigator) |
| Backend web | Python 3 (`backup_api.py`) |
| Frontend web | JavaScript ES5, HTML y CSS |
| Almacenamiento | Btrfs (`parted`, `mkfs.btrfs`) |
| Copias de seguridad | `rsync` con enlaces duros, CIFS (`cifs-utils`) y SSH (`sshpass`) |
| Programación de tareas | `cron`, `systemd-run` y `flock` |
| Seguridad de red | UFW, fail2ban y `unattended-upgrades` |
| Calidad | ShellCheck, BATS y Flake8 sobre GitHub Actions |

## Requisitos

- Debian 13 (Trixie) x86_64, con acceso `root` o `sudo`.
- Conexión a Internet durante la instalación (paquetes y extensiones).
- `curl` (o `wget`) y `ca-certificates` para el instalador remoto.
- Disco dedicado opcional para `/srv/nas`. Si se elige uno, se formatea por completo.

## Instalación

### Instalador remoto

```bash
curl -fsSL https://raw.githubusercontent.com/ftole/nas_debian/main/install.sh | sudo bash
```

*(o con `wget`: `wget -qO- https://raw.githubusercontent.com/ftole/nas_debian/main/install.sh | sudo bash`)*

> [!TIP]
> El instalador verifica dependencias, descarga el proyecto en `/opt/nas_debian` y presenta el asistente. No modifica particiones hasta que lo autorices.

### Preparación manual (opcional)

Si prefieres preparar el servidor antes de ejecutar el asistente, realiza estos pasos **en orden y como `root`**.

> [!IMPORTANT]
> Ejecuta todos los pasos dentro de la **misma sesión de `root`** (Paso 1). **No ejecutes `exit` hasta el final.** Si vuelves a tu usuario normal, los pasos siguientes fallarán con `Permiso denegado` u `orden no encontrada`, porque requieren privilegios de administrador y el `PATH` de `root` (que incluye `/usr/sbin`).

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

2. Instalar `sudo` y conceder permisos:

   ```bash
   apt install -y sudo
   usermod -aG sudo <nombre_usuario>
   echo "<nombre_usuario> ALL=(ALL:ALL) ALL" > /etc/sudoers.d/90-<nombre_usuario>
   chmod 0440 /etc/sudoers.d/90-<nombre_usuario>
   ```

> [!NOTE]
> La creación del archivo en `/etc/sudoers.d/` hace que los permisos de `sudo` surtan efecto de inmediato, sin cerrar sesión. **Continúa en esta misma sesión de `root`** para los pasos siguientes (no ejecutes `exit` todavía).

> [!TIP]
> `sudo` ignora los archivos de `/etc/sudoers.d/` cuyo nombre contenga un punto. Si el usuario tiene punto (por ejemplo `jose.perez`), reemplázalo por guion bajo en el nombre del archivo (`90-jose_perez`) y valida con `visudo -c`.

#### Paso 8: Desactivar el acceso de `root` por SSH

```bash
# Archivo principal y drop-ins (tienen mayor precedencia)
sed -i 's/^#*PermitRootLogin.*/PermitRootLogin no/' /etc/ssh/sshd_config
grep -rl "PermitRootLogin" /etc/ssh/sshd_config.d/ 2>/dev/null | xargs -r sed -i 's/^#*PermitRootLogin.*/PermitRootLogin no/'
systemctl restart ssh || systemctl restart sshd

# Verificar el valor efectivo
sshd -T 2>/dev/null | grep -i permitrootlogin
```

#### Paso 9: Actualizaciones de seguridad automáticas

```bash
apt install -y unattended-upgrades
dpkg-reconfigure -plow unattended-upgrades
```

#### Paso 10: Firewall (UFW)

```bash
ufw default deny incoming
ufw default allow outgoing

ufw allow 22/tcp comment 'SSH'
ufw allow 9090/tcp comment 'Cockpit Web Admin'
ufw allow 137,138/udp comment 'Samba NetBIOS'
ufw allow 139,445/tcp comment 'Samba SMB'
ufw allow 3702/udp comment 'WSDD2 WSD Discovery UDP'
ufw allow 3702/tcp comment 'WSDD2 WSD Discovery TCP'
ufw allow 5355/udp comment 'WSDD2 LLMNR UDP'
ufw allow 5355/tcp comment 'WSDD2 LLMNR TCP'
ufw allow 5357/tcp comment 'WSDD2 WSD HTTP'

ufw --force enable
ufw status verbose
```

#### Paso 11: fail2ban

```bash
apt install -y fail2ban
cp /etc/fail2ban/jail.conf /etc/fail2ban/jail.local
systemctl enable --now fail2ban
systemctl status fail2ban
```

#### Paso 12: Ejecutar el asistente

Ya puedes abrir el asistente (sigue como `root`):

```bash
sudo nas
```

Si deseas volver a tu usuario normal, ejecuta `exit` **solo al terminar**.

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
| 1 | Desplegar servidor | Asistente en 5 pasos para el rol `ARCHIVOS` o `BACKUP`. |
| 2 | Gestión de grupos | Crear, listar y eliminar grupos de seguridad (`grp_*`). |
| 3 | Recursos compartidos | Crear, listar, habilitar/deshabilitar y eliminar recursos, con 4 esquemas de permisos. |
| 4 | Tareas de backup | Programar copias para Windows (CIFS), Linux (SSH) o carpetas locales. |
| 5 | Usuarios | Crear usuarios, asignar grupos y gestionar contraseñas de red. |
| 6 | Diagnóstico | Estado de servicios, almacenamiento, recursos y tareas programadas. |
| 7 | Reiniciar servicios | Recarga de Samba, WSDD2 y Cockpit. |
| 8 | Buscar actualizaciones | Sincronización con GitHub. |
| 9 | Desinstalar | Restablecimiento total del sistema. |

## Copias de seguridad

Cada ejecución genera una carpeta con fecha y hora (`snapshot_YYYY-MM-DD_HHMMSS`). Los archivos que no cambiaron se comparten mediante enlaces duros, de modo que el consumo de disco es muy inferior al de copias completas repetidas. La retención conserva los últimos N snapshots y elimina los más antiguos.

Se admiten tres orígenes: Windows (CIFS, con montaje en solo lectura), Linux (SSH con `rsync`) y carpetas locales. Las tareas se programan con `cron` y se lanzan con `systemd-run`; un bloqueo `flock` evita ejecuciones simultáneas. El detalle técnico está en `SMB_DEBIAN.md`.

## Seguridad

El proyecto aplica varias medidas para reducir el riesgo de errores y de accesos no autorizados:

- Validación estricta de los datos introducidos en el asistente y en la API web.
- Las contraseñas no viajan en la línea de comandos: se envían por `stdin` o mediante variables de entorno, y las credenciales de red se guardan en archivos con permisos `0600`.
- Samba con cifrado negociado, protocolo mínimo SMB2 y mapeo de invitados controlado; los recursos de respaldo son ocultos.
- Montajes CIFS en modo solo lectura y permisos gestionados con ACL de POSIX.
- Protección del disco del sistema (incluye LVM, RAID, LUKS y subvolúmenes Btrfs) y aviso ante discos en uso.
- Rotación de logs, comprobación de espacio libre y verificación de integridad (SHA256) de las extensiones descargadas.

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
FAILURE_MODES.md           Modos de fallo y su verificación
.github/workflows/ci.yml   Integración continua
```

## Solución de problemas

- **`curl: (60) certificate problem`**: instala `ca-certificates` (`apt install -y ca-certificates`).
- **`Permiso denegado` u `orden no encontrada`** al seguir la preparación manual: **no estás como `root`**. Ejecuta `su -` y repite los pasos desde donde falló (no uses `exit` hasta el final).
- **El asistente no abre**: ejecútalo con `sudo` y en una terminal de al menos 72x20 caracteres.
- **Un disco aparece como "EN USO"**: está montado, es un volumen LVM o un miembro de RAID. Un PV de LVM o un miembro de RAID activo **nunca** se puede formatear. Para un disco simplemente montado, el despliegue por consola permite continuar con `--ignore-in-use` y la confirmación textual `SI-FORMATEAR`.
- **Una tarea de backup no se ejecuta**: comprueba que `cron` esté activo (`systemctl status cron`) y revisa el log en `/srv/nas/LOGS_BACKUP/`.
- **Windows no ve el servidor**: verifica `smbd`, `wsdd2` y las reglas de UFW.

## Licencia

Este proyecto es propiedad exclusiva de su autor. Todos los derechos reservados. Consulta el archivo [LICENSE](LICENSE).
