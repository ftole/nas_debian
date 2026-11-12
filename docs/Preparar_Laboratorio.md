# Guía de Puesta a Punto: Preparar Laboratorio de Desarrollo Multi-Agente (Debian 13)

> [!NOTE]
> Este documento registra en tiempo real los pasos exactos y verificados para dejar el servidor Debian 13 limpio "de fábrica" y posteriormente configurarlo para desarrollo colaborativo multi-agente (OpenCode CLI, Antigravity CLI) con *Git Worktrees*.

---

## 1. Información del Entorno
* **Sistema Operativo:** Debian 13 (Trixie) GNU/Linux x86_64.
* **Usuario principal:** `frank`.
* **Host:** `nasdeb`.
* **Estado inicial:** Servidor con pruebas previas que se restablece a estado base limpio.

### Regla Crítica: Instalador Oficial (`install.sh`) vs Despliegue de Desarrollo (`deploy_local.sh`)

> [!WARNING]
> **NUNCA ejecutes `./scripts/deploy_local.sh` en un servidor virgen.**  
> `deploy_local.sh` NO instala infraestructura (no instala paquetes del sistema, ni Nginx, ni Samba, ni el servicio systemd, ni variables de entorno). Su único propósito es **recompilar binarios durante el desarrollo en un servidor que YA TIENE la plataforma instalada**.

| Escenario | Comando Oficial | ¿Qué hace? |
| :--- | :--- | :--- |
| **Servidor limpio (recién instalado Debian)** | `curl -fsSL https://raw.githubusercontent.com/ftole/nas_debian/main/install.sh \| sudo bash` | **Instalador Oficial 1-Paso:** Instala dependencias con apt, descarga release oficial precompilado, crea el servicio systemd, Nginx y arranca la web en 30 segundos. |
| **Servidor limpio (compilando desde código)** | `git clone ... && cd nas_debian && sudo bash install.sh --build-from-source` | Instala dependencias apt, instala toolchain de Rust, compila todo el workspace y aprovisiona el servicio. |
| **Servidor tras reseteo / fábrica** | `cd /opt/dev/nas_debian && ./scripts/deploy_local.sh --all && sudo bash packaging/install.sh` | Compila los binarios y aprovisiona el servicio systemd y proxy Nginx. |
| **Desarrollo diario (código ya modificado)** | `./scripts/deploy_local.sh` | Solo recompila el binario modificado, lo copia a `/usr/local/bin` y reinicia el servicio activo en 2 segundos. |

---

## FASE 0: Restauración del Sistema a Estado Base ("De Fábrica")

### Paso 0.1: Retirar servicios, binarios y configuraciones del NAS anterior
Detiene los servicios en segundo plano, retira las unidades systemd, reglas de sudoers, proxies de Nginx, binarios en `/usr/local` y el directorio `/etc/nas`.

**Comandos ejecutados:**
```bash
sudo systemctl disable --now nas-web.service 2>/dev/null || true
sudo rm -f /etc/systemd/system/nas-web.service /etc/sudoers.d/nas-web-rust /etc/nas-web.env
sudo rm -f /etc/nginx/sites-enabled/nas-web /etc/nginx/sites-available/nas-web
sudo rm -f /usr/local/bin/nas* /usr/local/sbin/nas-*
sudo rm -rf /etc/nas /var/lib/nas /var/log/nas /etc/backup-credentials /opt/nas_debian
sudo systemctl daemon-reload
```

### Paso 0.2: Restauración de Kernel (sysctl), Udev, Auditoría y Almacenamiento
Restaura los parámetros de red y memoria del kernel a los valores por defecto de Debian, retira las reglas de precarga udev, limpia las reglas de auditoría y desmonta `/srv/nas` limpiando `/etc/fstab`.

**Comandos ejecutados:**
```bash
# 1. Desmontar almacenamiento anterior y limpiar /etc/fstab
sudo umount -R /srv/nas 2>/dev/null || true
sudo rm -rf /srv/nas
sudo sed -i '/\/srv\/nas/d' /etc/fstab

# 2. Retirar reglas de kernel, udev, auditoría y cron
sudo rm -f /etc/sysctl.d/99-nas-tuning.conf
sudo rm -f /etc/udev/rules.d/60-nas-readahead.rules
sudo rm -f /etc/udev/rules.d/80-udisks2-hide-os.rules
sudo rm -f /etc/audit/rules.d/nas.rules
sudo rm -f /etc/logrotate.d/nas-backups
sudo rm -f /etc/cron.d/nas-btrfs-scrub

# 3. Recargar configuraciones de fábrica en el sistema
sudo sysctl --system
sudo udevadm control --reload-rules
sudo augenrules --load 2>/dev/null || true
sudo systemctl restart auditd 2>/dev/null || true
```

### Paso 0.3: Restauración de Samba a Configuración de Fábrica y Limpieza de Grupos
Restaura `smb.conf` a la plantilla estándar limpia de Debian, reinicia demonios Samba, elimina credenciales de prueba (`passdb.tdb`) y borra los grupos residuales del NAS (`grp_samba`, `grp_web`, `grp_superadmin`).

**Comandos ejecutados:**
```bash
# 1. Restaurar smb.conf limpio
cat << 'EOF' | sudo tee /etc/samba/smb.conf
[global]
   workgroup = WORKGROUP
   server string = %h server
   server role = standalone server
   log file = /var/log/samba/log.%m
   max log size = 1000
   panic action = /usr/share/samba/panic-action %d
   obey pam restrictions = yes
   unix password sync = yes
   pam password change = yes
   map to guest = bad user
EOF

# 2. Reiniciar demonios Samba y limpiar bases de contraseñas de prueba
sudo rm -f /var/lib/samba/private/passdb.tdb /var/lib/samba/passdb.tdb 2>/dev/null || true
sudo systemctl restart smbd nmbd 2>/dev/null || true

# 3. Eliminar grupos residuales del NAS anterior
sudo groupdel grp_samba 2>/dev/null || true
sudo groupdel grp_web 2>/dev/null || true
sudo groupdel grp_superadmin 2>/dev/null || true
```

---

## FASE 1: Configuración del Directorio Colaborativo y Permisos Multi-Usuario

### Paso 1.1: Creación del Grupo `devs`, Usuarios Desarrolladores (`beto`, `chris`) y Directorio `/opt/dev`
Crea el grupo común `devs`, da de alta a los desarrolladores `beto` y `chris` (con home `/home/<usuario>`, shell `/bin/bash` y privilegios `sudo`), vincula a `frank`, `beto` y `chris` al grupo `devs`, y crea la estructura en `/opt/dev/workspaces` con permiso especial `setgid` (`2775`). Esto garantiza que cualquier archivo o commit creado por cualquier usuario o agente herede automáticamente los permisos de lectura y escritura para todo el equipo.

**Comandos ejecutados:**
```bash
# 1. Crear grupo devs y asociar al usuario frank
sudo groupadd -f devs
sudo usermod -aG devs frank

# 2. Crear los usuarios desarrolladores beto y chris (con sudo y grupo devs)
id -u beto >/dev/null 2>&1 || sudo useradd -m -s /bin/bash -g devs -G sudo beto
id -u chris >/dev/null 2>&1 || sudo useradd -m -s /bin/bash -g devs -G sudo chris

# 3. Asignar contraseña temporal a beto y chris (pueden cambiarla con passwd)
echo "beto:<contraseña_asignada>" | sudo chpasswd
echo "chris:<contraseña_asignada>" | sudo chpasswd

# 4. Asegurar pertenencia al grupo común devs y sudo
sudo usermod -aG sudo,devs frank
sudo usermod -aG sudo,devs beto
sudo usermod -aG sudo,devs chris

# 5. Crear directorios para el repositorio base y los worktrees
sudo mkdir -p /opt/dev/workspaces

# 6. Asignar propiedad a frank:devs y activar el bit setgid (2775)
sudo chown -R frank:devs /opt/dev
sudo chmod -R 2775 /opt/dev

# 7. Verificación
getent group devs
ls -ld /opt/dev
```

### Paso 1.2: Instalación de Paquetes del Sistema y Toolchain de Rust
Instala las dependencias nativas de Debian 13 requeridas para compilar el workspace (compilador C, bibliotecas de desarrollo SSL, SQLite, herramientas de almacenamiento y servicios auxiliares) e instala Rust vía `rustup`.

**Comandos ejecutados:**
```bash
# 1. Actualizar repositorios e instalar paquetes esenciales
sudo apt update && sudo apt install -y \
  git curl build-essential pkg-config libssl-dev libsqlite3-dev \
  samba nginx ufw wsdd2 tmux rsync btrfs-progs lvm2 parted acl

# 2. Instalar toolchain de Rust oficial (rustup) para frank
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
source "$HOME/.cargo/env"

# 3. Enlazar cargo, rustc y rustup globalmente a /usr/local/bin
sudo ln -sf /home/frank/.cargo/bin/cargo /usr/local/bin/cargo
sudo ln -sf /home/frank/.cargo/bin/rustc /usr/local/bin/rustc
sudo ln -sf /home/frank/.cargo/bin/rustup /usr/local/bin/rustup

# 4. Asignar la toolchain stable por defecto para root (para ejecuciones con sudo)
sudo rustup default stable

# 5. Comprobación de versión
rustc --version && cargo --version
sudo cargo --version
```

### Paso 1.3: Clonar Repositorio Principal y Habilitar Modo Colaborativo Git
Descarga el repositorio oficial en `/opt/dev/nas_debian` y activa la directiva `core.sharedRepository = group` para que todos los commits, ramas y objetos de Git creados por cualquier usuario o agente hereden los permisos del grupo `devs`.

**Comandos ejecutados:**
```bash
cd /opt/dev
git clone https://github.com/ftole/nas_debian.git nas_debian
cd /opt/dev/nas_debian
git config core.sharedRepository group
ls -la /opt/dev/nas_debian
```

### Paso 1.4: Validación Inicial de Compilación en Debian
Comprueba que el compilador, las cabeceras del kernel y las bibliotecas C/Rust (`libssl-dev`, `libsqlite3-dev`) funcionen correctamente compilando el workspace con todas las banderas de verificación.

**Comandos ejecutados:**
```bash
cd /opt/dev/nas_debian
cargo check --workspace --all-targets
```

### Paso 1.4.1: Instalación Oficial y Puesta en Marcha del Servidor (`install.sh`)
Una vez clonado el repositorio y configurada la toolchain de Rust tanto para el usuario `frank` como para `root`, se ejecuta el instalador oficial con compilación desde código fuente. Este comando compila todo el workspace, instala los binarios en `/usr/local/bin`, crea el servicio `nas-web.service` en systemd, aprovisiona Nginx con proxy inverso SSL en los puertos 80/443 y arranca el panel web.

**Comandos ejecutados:**
```bash
cd /opt/dev/nas_debian

# 1. Configurar safe.directory para permitir operaciones Git de root sobre el repositorio
sudo git config --system --add safe.directory "*"

# 2. Ejecutar la instalación oficial desde fuentes
sudo bash install.sh --build-from-source
```

**Resultado verificado:**
* Servicio activo: `systemctl status nas-web` en estado `active (running)`.
* Proxy web: Nginx escuchando en puertos 80 y 443 con proxy a `127.0.0.1:8080`.
* Panel web accesible inmediatamente en el navegador: `https://<IP_DEL_SERVIDOR>/`.

> **Nota para desarrollo posterior:** Una vez que el servidor ya tiene la infraestructura instalada con este paso, para actualizar cambios de código futuros durante el desarrollo solo se ejecuta `./scripts/deploy_local.sh`.

### Paso 1.5: Creación de los Árboles de Trabajo Aislados (Git Worktrees)
Crea las carpetas de trabajo físicas independientes en `/opt/dev/workspaces/` asociadas a sus respectivas ramas temáticas. Esto permite que múltiples agentes (OpenCode CLI, Antigravity CLI) o desarrolladores trabajen en paralelo sin colisiones ni bloqueos de Git.

**Comandos ejecutados:**
```bash
cd /opt/dev/nas_debian

# 1. Espacio para Almacenamiento (Gestión de discos y pools)
git worktree add -b feature/storage /opt/dev/workspaces/storage main

# 2. Espacio para Respaldos (Rsync, Btrfs y tareas)
git worktree add -b feature/backups /opt/dev/workspaces/backups main

# 3. Espacio para Frontend / UI (Modularización de vistas y JS)
git worktree add -b feature/frontend /opt/dev/workspaces/frontend main

# 4. Listar worktrees activos
git worktree list
```

---

## FASE 2: Migración de Clave GPG para Firma de Commits y Tags

### Paso 2.1: Exportación desde Windows e Importación en Debian
Transfiere la clave de firma digital GPG (`B10B8F775202C0EA` de Francisco Toledo) desde el entorno de Windows hacia Debian para mantener la firma verificada en GitHub.

**1. En Windows (Git Bash):**
```bash
gpg --armor --export B10B8F775202C0EA > <ruta_usuario>/clave_publica.asc
gpg --armor --export-secret-keys B10B8F775202C0EA > <ruta_usuario>/clave_privada.asc
scp <ruta_usuario>/clave_*.asc <usuario>@<host>:~
```

**2. En Debian (`<usuario>@nasdeb`):**
```bash
# Importar llaves pública y privada
gpg --import ~/clave_publica.asc
gpg --import ~/clave_privada.asc

# Asignar confianza definitiva (Trust Ultimate)
echo -e "5\ny\n" | gpg --command-fd 0 --expert --edit-key B10B8F775202C0EA trust

# Configurar Git para firmar commits y tags por defecto
git config --global user.signingkey B10B8F775202C0EA
git config --global commit.gpgsign true
git config --global tag.gpgsign true

# Eliminar archivos planos de clave por seguridad
rm -f ~/clave_publica.asc ~/clave_privada.asc

# Comprobar firma
gpg --list-secret-keys --keyid-format=long
```

---

## FASE 3: Instalación de Agentes de IA en Terminal (Antigravity, OpenCode, Kimi)

Matriz de herramientas configuradas por perfil:

| Usuario | Directorio Home | Herramientas Instaladas | Rol / Propósito |
| :--- | :--- | :--- | :--- |
| **`frank`** | `/home/frank` | `agy` (Antigravity CLI), `opencode`, `cargo/rustc` | Arquitectura, Lead Core y Almacenamiento |
| **`beto`** | `/home/beto` | `opencode`, `kimi`, `cargo/rustc` | Módulo de Respaldos y Btrfs/Rsync |
| **`chris`** | `/home/chris` | `opencode`, `kimi`, `cargo/rustc` | Módulo de Frontend y UI Modular |

### Comandos ejecutados por perfil:

**1. Perfil de Frank (`<usuario>@nasdeb`):**
```bash
curl -fsSL https://antigravity.google/cli/install.sh | bash
curl -fsSL https://opencode.ai/install | bash
export PATH="$HOME/.local/bin:$HOME/.cargo/bin:$PATH"
source ~/.bashrc 2>/dev/null || true
agy --version && opencode --version && cargo --version
```

**2. Perfil de Beto (`beto@nasdeb`):**
```bash
curl -fsSL https://opencode.ai/install | bash
curl -fsSL https://code.kimi.com/kimi-code/install.sh | bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
export PATH="$HOME/.local/bin:$HOME/.cargo/bin:$PATH"
source "$HOME/.cargo/env" 2>/dev/null || true
source ~/.bashrc 2>/dev/null || true
opencode --version && kimi --version && cargo --version
```

**3. Perfil de Chris (`chris@nasdeb`):**
```bash
curl -fsSL https://opencode.ai/install | bash
curl -fsSL https://code.kimi.com/kimi-code/install.sh | bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
export PATH="$HOME/.local/bin:$HOME/.cargo/bin:$PATH"
source "$HOME/.cargo/env" 2>/dev/null || true
source ~/.bashrc 2>/dev/null || true
opencode --version && kimi --version && cargo --version
```

---

## 3. Comprobación y Operación del Servidor en Vivo

Una vez completada la instalación, el servidor se opera y verifica con los siguientes comandos estándar:

| Acción | Comando en la Terminal | Resultado esperado |
| :--- | :--- | :--- |
| **Comprobar servicio web** | `sudo systemctl status nas-web --no-pager` | `Active: active (running)` en puerto `127.0.0.1:8080` |
| **Comprobar Nginx** | `sudo systemctl status nginx --no-pager` | `Active: active (running)` en puertos `80` y `443` |
| **Ver puertos de red** | `sudo ss -tulpn \| grep -E '80\|443\|8080'` | Nginx y nas-web en estado `LISTEN` |
| **Ver IP del servidor** | `hostname -I \| awk '{print $1}'` | Muestra la dirección IP para ingresar desde el navegador |
| **Acceso al Panel Web** | Abrir navegador en `https://<IP>/` | Slate UI 100% interactiva y offline |
| **Despliegue tras cambios** | `./scripts/deploy_local.sh` | Recompila e instala la nueva versión en 2 segundos |
