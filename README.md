# Servidor NAS & Central de Respaldos Multiplataforma (Debian 13)

[![Debian 13](https://img.shields.io/badge/OS-Debian%2013%20(Trixie)-A81D33?style=for-the-badge&logo=debian&logoColor=white)](https://github.com/ftole/nas_debian) [![Bash Shell](https://img.shields.io/badge/Scripting-Bash-4EAA25?style=for-the-badge&logo=gnu-bash&logoColor=white)](https://github.com/ftole/nas_debian) [![Samba](https://img.shields.io/badge/Service-Samba%20SMB-0066CC?style=for-the-badge)](https://github.com/ftole/nas_debian) [![Cockpit](https://img.shields.io/badge/Web%20UI-Cockpit-FF6600?style=for-the-badge)](https://github.com/ftole/nas_debian)

Este repositorio contiene la suite de scripts interactivos y automatizados para desplegar y administrar servidores de almacenamiento en red (**NAS Departamental**) y **Centrales de Copias de Seguridad** inmunes a ransomware bajo **Debian 13 (Trixie)**.

## ⚡ Instalación Rápida en 1 Línea (One-Liner Remoto)

> [!TIP]
> Este instalador verifica dependencias, descarga el entorno y te presenta el menú interactivo sin tocar tus particiones hasta que lo autorices.
> Requiere `curl` (o `wget`) y `ca-certificates` para la descarga por HTTPS.

Puedes instalar y desplegar todo el entorno en cualquier servidor Debian 13 ejecutando una sola línea en tu terminal:

```bash
curl -fsSL https://raw.githubusercontent.com/ftole/nas_debian/main/install.sh | sudo bash
```
*(o si dispones de `wget`: `wget -qO- https://raw.githubusercontent.com/ftole/nas_debian/main/install.sh | sudo bash`)*

### 🎮 Comandos Globales del CLI `nas`:

Una vez instalado, el comando **`nas`** queda registrado en el sistema para uso local y offline:

| Comando | Acción |
| :--- | :--- |
| `sudo nas` | Abre el **Asistente Visual Interactivo** con todos sus módulos. |
| `sudo nas update` | Sincroniza el proyecto con la última versión de GitHub. |
| `sudo nas status` | Diagnóstico en tiempo real de servicios (Samba/Cockpit), almacenamiento, recursos compartidos y tareas de backup. |
| `sudo nas version` | Muestra la versión actual y el último commit instalado. |
| `sudo nas uninstall` | Desinstala el comando `nas` y limpia el servidor por completo. |

> [!NOTE]
> El instalador despliega el proyecto en `/opt/nas_debian` y crea el comando global `/usr/local/bin/nas`. El comando `nas help` muestra la ayuda.

---

## 📋 Guía de Puesta a Punto Manual del Servidor (Paso a Paso)

Si prefieres preparar el servidor manualmente paso a paso antes de lanzar el asistente:

---

### Paso 1: Cambiar a usuario `root`

Para realizar configuraciones administrativas a nivel de sistema:

```bash
su -
```
> [!IMPORTANT]
> Es fundamental incluir el espacio y el guion (`su -`). Esto asegura que Debian cargue el entorno completo de `root`, incluyendo el directorio de utilidades del sistema (`/usr/sbin/`) en tu `$PATH`.

---

### Paso 2: Configurar los Repositorios APT (Debian 13 Trixie)

Debian 12+ usa por defecto el formato **deb822** en `/etc/apt/sources.list.d/debian.sources`. Configúralo ahí (y vacía `sources.list`) para evitar fuentes duplicadas:

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

---

### Paso 3: Actualizar el Sistema

Actualiza la lista de paquetes e instala las últimas actualizaciones disponibles:

```bash
apt update && apt upgrade -y
```

---

### Paso 4: Instalar Paquetes Base Esenciales

Instala utilidades clave de diagnóstico, descarga, certificados TLS y firewall (necesarios para el one-liner por HTTPS):

```bash
apt install -y curl wget ca-certificates htop ufw
```

---

### Paso 5: Verificar el Estado de Red

Comprueba las interfaces y direcciones IP asignadas:

```bash
ip a
```

Para verificar la puerta de enlace predeterminada:
```bash
ip r
```

---

### Paso 6: Comprobar Conectividad a Internet y DNS

Prueba de conexión directa por IP:
```bash
ping -c 4 8.8.8.8
```

Prueba de resolución DNS:
```bash
ping -c 4 google.com
```

---

### Paso 7: Listar Usuarios y Otorgar Permisos de Administrador (`sudo`)

1. **Listar usuarios con shell interactivo:**
   ```bash
   grep -E '/bin/bash|/bin/sh' /etc/passwd
   ```

2. **Instalar `sudo` y conceder permisos directos e inmediatos:**
   ```bash
   apt install -y sudo
   usermod -aG sudo <nombre_usuario>
   echo "<nombre_usuario> ALL=(ALL:ALL) ALL" > /etc/sudoers.d/90-<nombre_usuario>
   chmod 0440 /etc/sudoers.d/90-<nombre_usuario>
   ```
   *Ejemplo para el usuario `jose`:*
   ```bash
   usermod -aG sudo jose
   echo "jose ALL=(ALL:ALL) ALL" > /etc/sudoers.d/90-jose
   chmod 0440 /etc/sudoers.d/90-jose
   ```

3. **Salir de root:**
   ```bash
   exit
   ```

> [!NOTE]
> La creación del archivo en `/etc/sudoers.d/` garantiza que los permisos de `sudo` surtan efecto **inmediatamente** en todas las terminales activas sin necesidad de cerrar sesión o reiniciar.

> [!TIP]
> `sudo` **ignora** los archivos de `/etc/sudoers.d/` cuyo nombre contenga un punto. Si el usuario tiene punto (p. ej. `jose.perez`), reemplázalo por guion bajo en el nombre del archivo (`90-jose_perez`) y valida con `visudo -c`.

---

### Paso 8: Desactivar el Acceso de `root` por SSH

Por seguridad, restringe el acceso directo de root vía SSH para obligar a usar un usuario estándar con escalado de privilegios:

```bash
# Aplicar en el archivo principal y en los drop-ins (tienen mayor precedencia)
sed -i 's/^#*PermitRootLogin.*/PermitRootLogin no/' /etc/ssh/sshd_config
grep -rl "PermitRootLogin" /etc/ssh/sshd_config.d/ 2>/dev/null | xargs -r sed -i 's/^#*PermitRootLogin.*/PermitRootLogin no/'
systemctl restart ssh || systemctl restart sshd

# Verificar el valor efectivo
sshd -T 2>/dev/null | grep -i permitrootlogin
```

---

### Paso 9: Activar Actualizaciones de Seguridad Automáticas

Mantén el servidor protegido contra vulnerabilidades instalando `unattended-upgrades`:

```bash
apt install -y unattended-upgrades
dpkg-reconfigure -plow unattended-upgrades
```

---

### Paso 10: Configurar el Firewall (`ufw`)

Configura la política base restrictiva y habilita los puertos necesarios para administración (SSH, Cockpit) y servicios de red (Samba, WSDD2):

```bash
# Políticas base: bloquear entrante, permitir saliente
ufw default deny incoming
ufw default allow outgoing

# SSH (Acceso remoto)
ufw allow 22/tcp comment 'SSH'

# Panel Web Cockpit
ufw allow 9090/tcp comment 'Cockpit Web Admin'

# Samba (Compartición de Archivos)
ufw allow 137,138/udp comment 'Samba NetBIOS'
ufw allow 139,445/tcp comment 'Samba SMB'

# WSDD2 y LLMNR (Detección de equipos en red Windows WSD/LLMNR)
ufw allow 3702/udp comment 'WSDD2 WSD Discovery UDP'
ufw allow 3702/tcp comment 'WSDD2 WSD Discovery TCP'
ufw allow 5355/udp comment 'WSDD2 LLMNR UDP'
ufw allow 5355/tcp comment 'WSDD2 LLMNR TCP'
ufw allow 5357/tcp comment 'WSDD2 WSD HTTP'

# Activar firewall
ufw --force enable

# Verificar estado
ufw status verbose
```

---

### Paso 11: Instalar y Configurar `fail2ban`

Protege el servidor contra ataques de fuerza bruta en SSH:

```bash
apt install -y fail2ban
cp /etc/fail2ban/jail.conf /etc/fail2ban/jail.local
systemctl enable --now fail2ban
systemctl status fail2ban
```

---

### Paso 12: Ejecución del Asistente NAS

Una vez preparado el servidor, simplemente ejecuta el comando global del sistema:

```bash
sudo nas
```
*(O si estás dentro de la carpeta del proyecto: `sudo bash src/asistente.sh`)*

#### Módulos de Gestión del Asistente:
* **[1] Desplegar Servidor:** Asistente en 5 pasos para **Servidor de Archivos (NAS)** o **Central de Backup (Inmune a Ransomware)**.
* **[2] Gestión de Grupos:** Creación y asignación de grupos departamentales (`grp_*`).
* **[3] Gestión de Recursos Compartidos:** Creación de recursos visibles u ocultos (`$`) con 4 esquemas de permisos granulares.
* **[4] Gestión de Tareas de Backup:** Programación de copias incrementales deduplicadas (con *hardlinks*) para servidores Windows (CIFS), Linux (SSH) o carpetas locales.
* **[5] Gestión de Usuarios:** Creación y asignación de grupos mediante checklist dinámico y contraseñas de red Samba.
* **[6] Diagnóstico, Discos y Recursos:** Monitoreo en tiempo real de servicios, almacenamiento, recursos compartidos y tareas cron.
* **[7] Reiniciar Servicios:** Recarga limpia de Samba, WSDD2 y Cockpit.
* **[8] Buscar Actualizaciones:** Sincronización automática con las últimas mejoras de GitHub.
* **[9] Desinstalar y Limpiar:** Restablecimiento total del sistema a su estado base.
