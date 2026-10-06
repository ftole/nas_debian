#!/bin/bash
# ==============================================================================
# Motor de Despliegue Automatizado Base (Debian 13)
# ==============================================================================

set -e

if [ "$EUID" -ne 0 ]; then
  echo "[-] Este script debe ejecutarse con privilegios de root (sudo bash $0)"
  exit 1
fi

# -----------------------------------------------------------------------------
# Registro de despliegue y conteo de advertencias para el resumen final
# -----------------------------------------------------------------------------
NAS_WARNINGS=0
NAS_LOG_DIR="/var/log/nas"
install -d -m 0750 -o root -g root "$NAS_LOG_DIR" 2>/dev/null || mkdir -p "$NAS_LOG_DIR"
NAS_LOG="$NAS_LOG_DIR/deploy_$(date +%Y%m%d_%H%M%S).log"
: > "$NAS_LOG"
chmod 600 "$NAS_LOG"

log() {
    printf '%s\n' "$*" >> "$NAS_LOG"
}

advertir() {
    NAS_WARNINGS=$((NAS_WARNINGS + 1))
    echo "  [!] $*" >&2
    log "[ADVERTENCIA] $*"
}

trap 'log "[ERROR] Fallo en la línea $LINENO"' ERR


LIB_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../lib" && pwd)"
# shellcheck source=src/lib/colors.sh
source "$LIB_DIR/colors.sh"
# shellcheck source=src/lib/helpers.sh
source "$LIB_DIR/helpers.sh"

TARGET_DISK="${1:-LOCAL}"
SMB_WORKGROUP="${2:-$(obtener_workgroup_defecto)}"
SMB_NETBIOS="${3:-$(obtener_netbios_defecto)}"
ADMIN_USER="${4:-$(detect_default_user)}"
ADMIN_PASS="${5:-}"
if [ -n "${5:-}" ] && [ "$5" != "-" ]; then
    echo "[-] Por seguridad, la contraseña debe recibirse por stdin usando '-' en lugar del argumento."
    printf '%s\n' '    Ejemplo: printf "%s\n" "<CLAVE>" | bash deploy.sh ... usuario - ARCHIVOS'
    exit 1
fi
if [ "$ADMIN_PASS" == "-" ]; then
    IFS= read -r ADMIN_PASS || true
fi
SERVER_ROLE="${6:-ARCHIVOS}"

# Sanear identificadores de red para evitar expansión/inyección en las configuraciones
SMB_NETBIOS=$(printf '%s' "$SMB_NETBIOS" | tr -cd 'A-Za-z0-9_-' | tr '[:lower:]' '[:upper:]')
SMB_WORKGROUP=$(printf '%s' "$SMB_WORKGROUP" | tr -cd 'A-Za-z0-9_-' | tr '[:lower:]' '[:upper:]')
SERVER_ROLE=$(printf '%s' "$SERVER_ROLE" | tr -cd 'A-Za-z' | tr '[:lower:]' '[:upper:]')
[ -z "$SERVER_ROLE" ] && SERVER_ROLE="ARCHIVOS"

# Validar estrictamente el nombre de usuario administrador antes de usarlo.
if [[ ! "$ADMIN_USER" =~ ^[a-z_][a-z0-9_-]{0,31}$ ]]; then
    echo "[-] ERROR: Nombre de usuario administrador inválido: '$ADMIN_USER'."
    echo "    Debe iniciar con letra minúscula o guion bajo y contener solo [a-z0-9_-]."
    exit 1
fi

# --force confirma sin preguntar, pero NO salta los chequeos de seguridad.
# --confirm indica que el llamador ya obtuvo confirmación explícita del usuario.
# --ignore-in-use permite formatear un disco en uso (peligroso, con confirmación textual).
FORCE=false
CONFIRM=false
IGNORE_IN_USE=false
KEEP_DATA=false
for _arg in "$@"; do
    [ "$_arg" == "--force" ] && FORCE=true
    [ "$_arg" == "--confirm" ] && CONFIRM=true
    [ "$_arg" == "--ignore-in-use" ] && IGNORE_IN_USE=true
    [ "$_arg" == "--keep-data" ] && KEEP_DATA=true
done

SERVER_IP=$(obtener_ip_local)

echo "=============================================================================="
echo " INICIANDO DESPLIEGUE: $SERVER_ROLE (IP: $SERVER_IP)"
echo " Servidor: $SMB_NETBIOS | Workgroup: $SMB_WORKGROUP | Admin: $ADMIN_USER"
echo "=============================================================================="
log "Inicio de despliegue: rol=$SERVER_ROLE disco=$TARGET_DISK netbios=$SMB_NETBIOS"

echo " [1/9] Actualizando repositorios e instalando paquetes base..."
if ! DEBIAN_FRONTEND=noninteractive apt-get update -qq >/dev/null 2>&1; then
    advertir "No se pudieron actualizar los repositorios (apt-get update)."
fi
if ! DEBIAN_FRONTEND=noninteractive apt-get install -y -qq \
    sudo acl samba samba-common-bin wsdd2 smbclient samba-vfs-modules openssl \
    nginx-light php-fpm php-cli php-sqlite3 php-zip sqlite3 rsyslog \
    realmd sssd sssd-tools adcli libpam-sss libnss-sss krb5-user packagekit \
    cifs-utils rsync sshpass cron parted ufw btrfs-progs >/dev/null 2>&1; then
    echo "[-] ERROR CRITICO: no se pudieron instalar los paquetes base."
    log "[ERROR] Fallo en la instalación de paquetes base."
    exit 1
fi

auto_tune_hardware() {
    local DISCO="$1"
    local DISCO_BASE
    DISCO="${DISCO%%[*}"
    DISCO_BASE=$(resolver_disco_base "$DISCO")
    # El valor lo consumen las ramas de montaje y mantenimiento que llaman a
    # esta función; debe permanecer visible en el ámbito del despliegue.
    ES_HDD=$(cat "/sys/block/$DISCO_BASE/queue/rotational" 2>/dev/null || echo "1")

    # 1. Configuración de Filesystem por Rol y Hardware
    if [ "$SERVER_ROLE" == "BACKUP" ]; then
        FS_TYPE="btrfs"
        if [ "$ES_HDD" = "0" ]; then
            FS_OPTS="rw,noatime,compress=zstd:3,space_cache=v2,ssd,discard=async"
            READAHEAD_KB=1024
        else
            FS_OPTS="rw,noatime,compress=zstd:3,space_cache=v2,autodefrag"
            READAHEAD_KB=4096
        fi
    else
        FS_TYPE="ext4"
        if [ "$ES_HDD" = "0" ]; then
            FS_OPTS="rw,noatime,commit=5"
            READAHEAD_KB=1024
        else
            FS_OPTS="rw,noatime,commit=2"
            READAHEAD_KB=4096
        fi
    fi

    # 2. Kernel Tuning Persistente (/etc/sysctl.d/99-nas-tuning.conf)
    mkdir -p /etc/sysctl.d
    cat << 'SYSCTL_EOF' > /etc/sysctl.d/99-nas-tuning.conf
fs.inotify.max_user_instances = 2048
fs.inotify.max_user_watches = 524288
net.ipv4.tcp_keepalive_time = 120
net.ipv4.tcp_keepalive_intvl = 15
net.ipv4.tcp_keepalive_probes = 4
vm.vfs_cache_pressure = 30
vm.dirty_background_bytes = 67108864
vm.dirty_bytes = 268435456
vm.dirty_expire_centisecs = 300
SYSCTL_EOF
    sysctl -p /etc/sysctl.d/99-nas-tuning.conf >/dev/null 2>&1 || true

    # 3. Readahead Tuning Persistente en udev
    if [ -n "$DISCO_BASE" ]; then
        mkdir -p /etc/udev/rules.d
        cat << UDEV_EOF > /etc/udev/rules.d/60-nas-readahead.rules
# Readahead tuning persistente para disco NAS ($DISCO_BASE)
ACTION=="add|change", KERNEL=="$DISCO_BASE", ATTR{bdi/read_ahead_kb}="$READAHEAD_KB"
ACTION=="add|change", KERNEL=="$DISCO_BASE", ATTR{queue/read_ahead_kb}="$READAHEAD_KB"
UDEV_EOF
        udevadm control --reload-rules 2>/dev/null || true
        udevadm trigger 2>/dev/null || true
    fi
    blockdev --setra $((READAHEAD_KB * 2)) "$DISCO" 2>/dev/null || true

    # Exportar variables para usarlas en el formateo y montaje
    export FS_TYPE FS_OPTS ES_HDD
}

echo " [2/9] Configurando almacenamiento (/srv/nas) en $TARGET_DISK..."
mkdir -p /srv/nas
if mountpoint -q /srv/nas 2>/dev/null; then
    echo "  [!] Aviso: /srv/nas ya estaba montado; se reconfigurará el almacenamiento."
fi

ROOT_DEV=$(findmnt -n -o SOURCE / 2>/dev/null || df / | tail -1 | awk '{print $1}')
ROOT_DISK="/dev/$(resolver_disco_base "$ROOT_DEV")"
ROOT_DEVS="$(resolver_discos_raiz "$ROOT_DEV")"

if [ "$TARGET_DISK" == "LOCAL" ] || [ "$TARGET_DISK" == "$ROOT_DEV" ] || [ "$TARGET_DISK" == "$ROOT_DISK" ] || printf '%s\n' "$ROOT_DEVS" | grep -qx "$TARGET_DISK"; then
    echo "  -> Almacenamiento local configurado en la partición raíz."
    auto_tune_hardware "$ROOT_DEV"
    if [ "$ES_HDD" = "0" ]; then
        systemctl enable --now fstrim.timer 2>/dev/null || true
    fi
    if [ "$SERVER_ROLE" == "BACKUP" ]; then
        ROOT_FS=$(findmnt -n -o FSTYPE / 2>/dev/null || echo "")
        if [ "$ROOT_FS" == "btrfs" ]; then
            cat << 'CRON_SCRUB' > /etc/cron.d/nas-btrfs-scrub
0 2 1 * * root btrfs scrub start -B /srv/nas >/dev/null 2>&1
CRON_SCRUB
            chmod 644 /etc/cron.d/nas-btrfs-scrub
        fi
    fi
else
    echo "  -> Inicializando y configurando disco dedicado: $TARGET_DISK"
    if [ ! -b "$TARGET_DISK" ]; then
        echo "[-] ERROR: $TARGET_DISK no es un dispositivo de bloque válido."
        exit 1
    fi
    if [ "$(lsblk -dn -o TYPE "$TARGET_DISK" 2>/dev/null)" != "disk" ]; then
        echo "[-] ERROR: $TARGET_DISK no es un disco completo (se requiere TYPE=disk)."
        exit 1
    fi
    if printf '%s\n' "$ROOT_DEVS" | grep -qx "$TARGET_DISK"; then
        echo "[-] ERROR CRITICO: $TARGET_DISK respalda el sistema raíz. Abortando."
        exit 1
    fi
    if blkid "$TARGET_DISK"* 2>/dev/null | grep -q 'TYPE="crypto_LUKS"'; then
        echo "[-] ERROR: $TARGET_DISK contiene volúmenes cifrados LUKS. Abortando."
        exit 1
    fi
    if [ "$KEEP_DATA" != "true" ] && findmnt -n -o SOURCE /srv/nas 2>/dev/null | grep -q .; then
        NAS_SRC=$(findmnt -n -o SOURCE /srv/nas 2>/dev/null)
        if [ "/dev/$(resolver_disco_base "$NAS_SRC")" == "$TARGET_DISK" ]; then
            echo "[-] ERROR CRITICO: $TARGET_DISK es el almacenamiento actual de /srv/nas. Abortando."
            exit 1
        fi
    fi
    if disco_en_uso_critico "$TARGET_DISK"; then
        echo "[-] ERROR CRITICO: $TARGET_DISK es un PV de LVM o un miembro de RAID activo."
        echo "    Formatearlo puede dañar otros volúmenes o arreglos. Abortando (no se omite con --ignore-in-use)."
        exit 1
    fi
    if [ "$IGNORE_IN_USE" != "true" ] && [ "$KEEP_DATA" != "true" ] && disco_en_uso "$TARGET_DISK"; then
        echo "[-] ERROR: $TARGET_DISK parece estar en uso (montado, PV de LVM o miembro de RAID)."
        echo "    Abortando por seguridad. Usa --ignore-in-use bajo tu responsabilidad si realmente deseas formatearlo."
        exit 1
    fi

    # Mostrar la información completa del disco antes de formatear/configurar.
    echo "  ╔══════════════════════════════════════════════════════════════════╗"
    echo "  ║  DISCO DE ALMACENAMIENTO: $TARGET_DISK"
    echo "  ╚══════════════════════════════════════════════════════════════════╝"
    if ! lsblk -o NAME,SIZE,MODEL,TYPE,MOUNTPOINT "$TARGET_DISK" 2>/dev/null; then
        echo "[-] ERROR: no se pudo obtener la información del disco $TARGET_DISK."
        echo "    Abortando para no formatear un dispositivo desconocido."
        exit 1
    fi

    # Confirmación explícita antes de una operación destructiva.
    if [ "$KEEP_DATA" == "true" ]; then
        echo "  [•] Modo --keep-data activado: se preservarán los datos del disco $TARGET_DISK."
    elif [ "$IGNORE_IN_USE" == "true" ]; then
        echo "  [!] ADVERTENCIA EXTREMA: --ignore-in-use permite formatear un disco EN USO."
        echo "      TODOS los datos serán eliminados. Esta acción puede dañar el sistema."
        if [ -t 0 ]; then
            read -r -p "  Escribe 'SI-FORMATEAR' para confirmar: " RESP
            if [ "$RESP" != "SI-FORMATEAR" ]; then
                echo "[-] Formateo cancelado."
                exit 1
            fi
        else
            echo "[-] ERROR: --ignore-in-use requiere confirmación interactiva."
            exit 1
        fi
    elif [ "$FORCE" == "true" ] || [ "$CONFIRM" == "true" ]; then
        echo "  [•] Confirmación recibida. Continuando con el formateo."
    elif [ -t 0 ]; then
        echo "  [!] ADVERTENCIA: se formateará $TARGET_DISK y se borrarán TODOS sus datos."
        read -r -p "  ¿Deseas continuar con el formateo? [s/N]: " RESP
        if [[ ! "$RESP" =~ ^[sSyY]$ ]]; then
            echo "[-] Formateo cancelado por el usuario."
            exit 1
        fi
    else
        echo "[-] ERROR: se requiere confirmación explícita para formatear $TARGET_DISK."
        echo "    Usa --confirm (si ya confirmaste en el asistente) o --force bajo tu responsabilidad."
        exit 1
    fi
    log "Configuración de almacenamiento para el disco $TARGET_DISK (keep_data=$KEEP_DATA)"

    auto_tune_hardware "$TARGET_DISK"

    if [ "$KEEP_DATA" == "true" ]; then
        echo "  -> Conservando datos existentes en $TARGET_DISK..."
        PART_NAS=""
        NAS_CURRENT_SRC=$(findmnt -n -o SOURCE /srv/nas 2>/dev/null || echo "")
        if [ -n "$NAS_CURRENT_SRC" ] && [ "/dev/$(resolver_disco_base "$NAS_CURRENT_SRC")" == "$TARGET_DISK" ]; then
            PART_NAS="$NAS_CURRENT_SRC"
        fi
        if [ -z "$PART_NAS" ]; then
            for _p in $(lsblk -ln -o NAME,TYPE "$TARGET_DISK" 2>/dev/null | awk '$2=="part"{print $1}'); do
                if [ "$(blkid -s LABEL -o value "/dev/$_p" 2>/dev/null)" == "NAS_DATA" ]; then
                    PART_NAS="/dev/$_p"
                    break
                fi
            done
        fi
        if [ -z "$PART_NAS" ]; then
            while read -r _pname _ptype; do
                [ "$_ptype" == "part" ] || continue
                _pdev="/dev/$_pname"
                _ptype_fs=$(blkid -s TYPE -o value "$_pdev" 2>/dev/null || echo "")
                if [ -n "$_ptype_fs" ] && [ "$_ptype_fs" != "vfat" ] && [ "$_ptype_fs" != "swap" ]; then
                    PART_NAS="$_pdev"
                    break
                fi
            done < <(lsblk -ln -o NAME,TYPE "$TARGET_DISK" 2>/dev/null)
        fi
        if [ -z "$PART_NAS" ]; then
            while read -r _pname _ptype; do
                if [ "$_ptype" == "part" ]; then
                    PART_NAS="/dev/$_pname"
                    break
                fi
            done < <(lsblk -ln -o NAME,TYPE "$TARGET_DISK" 2>/dev/null)
        fi
        if [ -z "$PART_NAS" ]; then
            PART_NAS="$TARGET_DISK"
        fi
        FS_DETECTED=$(blkid -s TYPE -o value "$PART_NAS" 2>/dev/null || echo "")
        if [ -z "$FS_DETECTED" ]; then
            echo "[-] ERROR: no se pudo detectar el sistema de archivos en $PART_NAS para conservar datos."
            exit 1
        fi
        FS_TYPE="$FS_DETECTED"
        if [ "$FS_TYPE" == "ext4" ]; then
            if [ "$ES_HDD" = "0" ]; then
                FS_OPTS="rw,noatime,commit=5"
            else
                FS_OPTS="rw,noatime,commit=2"
            fi
        elif [ "$FS_TYPE" == "btrfs" ]; then
            if [ "$ES_HDD" = "0" ]; then
                FS_OPTS="rw,noatime,compress=zstd:3,space_cache=v2,ssd,discard=async"
            else
                FS_OPTS="rw,noatime,compress=zstd:3,space_cache=v2,autodefrag"
            fi
        else
            FS_OPTS="defaults,noatime"
        fi
        UUID_NAS=$(blkid -s UUID -o value "$PART_NAS" 2>/dev/null || echo "")
    else
        while read -r _part; do
            [ -z "$_part" ] && continue
            umount "/dev/$_part" 2>/dev/null || true
        done < <(lsblk -ln -o NAME "$TARGET_DISK" 2>/dev/null | tail -n +2)
        parted -s "$TARGET_DISK" mklabel gpt mkpart primary "$FS_TYPE" 0% 100%
        partprobe "$TARGET_DISK" 2>/dev/null || true
        udevadm settle 2>/dev/null || true

        PART_NAS=""
        for _ in {1..10}; do
            if [ -b "${TARGET_DISK}1" ]; then
                PART_NAS="${TARGET_DISK}1"
                break
            elif [ -b "${TARGET_DISK}p1" ]; then
                PART_NAS="${TARGET_DISK}p1"
                break
            fi
            sleep 1
        done
        if [ -z "$PART_NAS" ]; then
            echo "[-] ERROR CRITICO: No se detecto la particion en $TARGET_DISK tras el particionado."
            echo "    Abortando para no formatear el disco completo por error."
            exit 1
        fi

        if [ "$FS_TYPE" == "ext4" ]; then
            mkfs.ext4 -F -L "NAS_DATA" "$PART_NAS"
            tune2fs -m 1 "$PART_NAS" >/dev/null 2>&1 || true
        else
            mkfs.btrfs -f -L "NAS_DATA" "$PART_NAS"
        fi
        UUID_NAS=$(blkid -s UUID -o value "$PART_NAS" 2>/dev/null || echo "")
    fi

    FSTAB_BAK="/etc/fstab.bak-$(date +%Y%m%d_%H%M%S)"
    cp -a /etc/fstab "$FSTAB_BAK"
    FSTAB_TMP=$(mktemp /etc/fstab.nas.XXXXXX)
    trap 'rm -f "$FSTAB_TMP"' EXIT
    awk '
        /^# BEGIN NAS_DEBIAN \/srv\/nas$/ {skip=1; next}
        /^# END NAS_DEBIAN \/srv\/nas$/ {skip=0; next}
        skip {next}
        $2 != "/srv/nas"
    ' /etc/fstab > "$FSTAB_TMP"
    {
        echo "# BEGIN NAS_DEBIAN /srv/nas"
        if [ -n "$UUID_NAS" ]; then
            echo "UUID=$UUID_NAS /srv/nas $FS_TYPE defaults,$FS_OPTS 0 2"
        else
            echo "$PART_NAS /srv/nas $FS_TYPE defaults,$FS_OPTS 0 2"
        fi
        echo "# END NAS_DEBIAN /srv/nas"
    } >> "$FSTAB_TMP"
    mv "$FSTAB_TMP" /etc/fstab
    if ! findmnt --verify --verbose >/dev/null 2>&1; then
        echo "[-] ERROR CRITICO: /etc/fstab quedó inválido; restaurando el respaldo."
        log "[ERROR] findmnt --verify falló; restaurando $FSTAB_BAK."
        cp -a "$FSTAB_BAK" /etc/fstab
        exit 1
    fi
    if mountpoint -q /srv/nas 2>/dev/null; then
        umount /srv/nas 2>/dev/null || true
    fi
    MOUNT_OK=false
    if mount -o "$FS_OPTS" "$PART_NAS" /srv/nas 2>/dev/null; then
        MOUNT_OK=true
    elif mount /srv/nas 2>/dev/null; then
        MOUNT_OK=true
    fi
    if [ "$MOUNT_OK" != "true" ]; then
        echo "[-] ERROR CRITICO: No se pudo montar $PART_NAS en /srv/nas."
        echo "    Abortando para evitar escribir los respaldos en la particion del sistema."
        exit 1
    fi

    if [ "$ES_HDD" = "0" ]; then
        systemctl enable --now fstrim.timer 2>/dev/null || true
    fi
    if [ "$SERVER_ROLE" == "BACKUP" ] && [ "$FS_TYPE" == "btrfs" ]; then
        cat << 'CRON_SCRUB' > /etc/cron.d/nas-btrfs-scrub
0 2 1 * * root btrfs scrub start -B /srv/nas >/dev/null 2>&1
CRON_SCRUB
        chmod 644 /etc/cron.d/nas-btrfs-scrub
    fi
fi

echo " [3/9] Configurando entorno web nativo (Nginx-light + PHP-FPM ondemand + MVC)..."
REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
WEB_SRC=""
if [ -d "$REPO_ROOT/web" ]; then
    WEB_SRC="$REPO_ROOT/web"
elif [ -d "$(dirname "${BASH_SOURCE[0]}")/../web" ]; then
    WEB_SRC="$(dirname "${BASH_SOURCE[0]}")/../web"
fi

# 1. Configurar Pool de PHP-FPM bajo demanda (pm = ondemand, ~0 MB RAM en reposo)
PHP_POOL_DIR=$(find /etc/php -maxdepth 3 -type d -name "pool.d" 2>/dev/null | tail -1)
if [ -z "$PHP_POOL_DIR" ]; then
    PHP_VER=$(php -r 'echo PHP_MAJOR_VERSION.".".PHP_MINOR_VERSION;' 2>/dev/null || echo "8.4")
    PHP_POOL_DIR="/etc/php/$PHP_VER/fpm/pool.d"
fi
mkdir -p "$PHP_POOL_DIR" /run/php

cat << 'PHP_POOL_EOF' > "$PHP_POOL_DIR/nas-web.conf"
[nas-web]
user = www-data
group = www-data
listen = /run/php/php-fpm-nas.sock
listen.owner = www-data
listen.group = www-data
listen.mode = 0660
pm = ondemand
pm.max_children = 10
pm.process_idle_timeout = 10s
pm.max_requests = 500
php_admin_value[upload_max_filesize] = 512M
php_admin_value[post_max_size] = 512M
php_admin_value[max_execution_time] = 300
php_admin_value[max_input_time] = 300
php_admin_value[memory_limit] = 512M
PHP_POOL_EOF

# 2. Directorio persistente para base de datos SQLite nativa
mkdir -p /var/lib/nas
chown -R www-data:www-data /var/lib/nas 2>/dev/null || true
chmod 0770 /var/lib/nas 2>/dev/null || true

# 3. Desplegar aplicación web MVC en /var/www/nas-web
mkdir -p /var/www/nas-web
rm -rf /var/www/nas-web/*
if [ -n "$WEB_SRC" ] && [ -d "$WEB_SRC" ]; then
    cp -rf "$WEB_SRC/"* /var/www/nas-web/
fi

GIT_COMMIT="release"
GIT_DATE="$(date +%Y-%m-%d)"
if git -C "$REPO_ROOT" rev-parse --is-inside-work-tree >/dev/null 2>&1; then
    GIT_COMMIT=$(git -C "$REPO_ROOT" rev-parse --short HEAD 2>/dev/null || echo "release")
    GIT_DATE=$(git -C "$REPO_ROOT" log -1 --format=%cd --date=short 2>/dev/null || date +%Y-%m-%d)
fi

cat << VERSION_EOF > /var/www/nas-web/version.json
{
  "version": "1.0.0",
  "commit": "$GIT_COMMIT",
  "date": "$GIT_DATE",
  "channel": "GitHub main"
}
VERSION_EOF

chown -R www-data:www-data /var/www/nas-web 2>/dev/null || true
chmod -R 755 /var/www/nas-web 2>/dev/null || true
usermod -aG systemd-journal,adm,grp_sistemas www-data 2>/dev/null || true

# 4. Generar Certificado SSL/TLS autofirmado para acceso HTTPS
if [ ! -f /etc/ssl/certs/nas-web.crt ] || [ ! -f /etc/ssl/private/nas-web.key ]; then
    mkdir -p /etc/ssl/certs /etc/ssl/private
    openssl req -x509 -nodes -days 3650 -newkey rsa:2048 \
        -subj "/C=ES/ST=Admin/L=Server/O=TEAM-JOFRATO/CN=${SMB_NETBIOS:-SRV-NAS}" \
        -keyout /etc/ssl/private/nas-web.key \
        -out /etc/ssl/certs/nas-web.crt 2>/dev/null || advertir "No se pudo generar el certificado SSL autofirmado."
    chmod 600 /etc/ssl/private/nas-web.key 2>/dev/null || true
    chmod 644 /etc/ssl/certs/nas-web.crt 2>/dev/null || true
fi

# 4. Configurar Host Virtual de Nginx (HTTP + HTTPS)
mkdir -p /etc/nginx/sites-available /etc/nginx/sites-enabled
cat << 'NGINX_EOF' > /etc/nginx/sites-available/nas-web
# 1. Servidor HTTP (Puerto 80: Redirección forzada a HTTPS)
server {
    listen 80 default_server;
    listen [::]:80 default_server;
    server_name _;
    return 301 https://$host$request_uri;
}

# 2. Servidor HTTPS (Puerto 443)
server {
    listen 443 ssl default_server;
    listen [::]:443 ssl default_server;
    server_name _;
    root /var/www/nas-web/public;
    index index.php index.html;

    ssl_certificate /etc/ssl/certs/nas-web.crt;
    ssl_certificate_key /etc/ssl/private/nas-web.key;
    ssl_protocols TLSv1.2 TLSv1.3;
    ssl_ciphers HIGH:!aNULL:!MD5;

    # Cabeceras de seguridad
    add_header Strict-Transport-Security "max-age=31536000; includeSubDomains" always;
    add_header X-Content-Type-Options "nosniff" always;
    add_header X-Frame-Options "SAMEORIGIN" always;
    add_header X-XSS-Protection "1; mode=block" always;

    client_max_body_size 512M;

    location / {
        try_files $uri $uri/ /index.php?$query_string;
    }

    location ~ \.php$ {
        include snippets/fastcgi-php.conf;
        fastcgi_pass unix:/run/php/php-fpm-nas.sock;
        fastcgi_param SCRIPT_FILENAME $document_root$fastcgi_script_name;
        fastcgi_param HTTPS on;
        include fastcgi_params;
    }

    location ~ /\. {
        deny all;
    }
}
NGINX_EOF
rm -f /etc/nginx/sites-enabled/default
ln -sf /etc/nginx/sites-available/nas-web /etc/nginx/sites-enabled/nas-web

# 5. Configurar sudoers para www-data con permisos acotados y seguros
cat << SUDOERS_EOF > /etc/sudoers.d/nas-web
Cmnd_Alias NAS_SERVICES = /bin/systemctl reload smbd, /usr/bin/systemctl reload smbd, \\
    /bin/systemctl restart smbd, /usr/bin/systemctl restart smbd, \\
    /bin/systemctl restart nmbd, /usr/bin/systemctl restart nmbd, \\
    /bin/systemctl restart wsdd2, /usr/bin/systemctl restart wsdd2, \\
    /bin/systemctl restart nginx, /usr/bin/systemctl restart nginx, \\
    /bin/systemctl reload nginx, /usr/bin/systemctl reload nginx, \\
    /bin/systemctl start smbd, /usr/bin/systemctl start smbd, \\
    /bin/systemctl start nmbd, /usr/bin/systemctl start nmbd, \\
    /bin/systemctl start wsdd2, /usr/bin/systemctl start wsdd2, \\
    /bin/systemctl start nginx, /usr/bin/systemctl start nginx, \\
    /bin/systemctl stop smbd, /usr/bin/systemctl stop smbd, \\
    /bin/systemctl stop nmbd, /usr/bin/systemctl stop nmbd, \\
    /bin/systemctl stop wsdd2, /usr/bin/systemctl stop wsdd2, \\
    /bin/systemctl stop nginx, /usr/bin/systemctl stop nginx, \\
    /bin/systemctl restart php${PHP_VER}-fpm, /usr/bin/systemctl restart php${PHP_VER}-fpm, \\
    /bin/systemctl reload php${PHP_VER}-fpm, /usr/bin/systemctl reload php${PHP_VER}-fpm, \\
    /bin/systemctl start php${PHP_VER}-fpm, /usr/bin/systemctl start php${PHP_VER}-fpm, \\
    /bin/systemctl stop php${PHP_VER}-fpm, /usr/bin/systemctl stop php${PHP_VER}-fpm, \\
    /bin/systemctl status php${PHP_VER}-fpm, /usr/bin/systemctl status php${PHP_VER}-fpm, \\
    /bin/systemctl restart cron, /usr/bin/systemctl restart cron, \\
    /bin/systemctl start cron, /usr/bin/systemctl start cron, \\
    /bin/systemctl stop cron, /usr/bin/systemctl stop cron, \\
    /bin/systemctl status smbd, /usr/bin/systemctl status smbd, \\
    /bin/systemctl status nmbd, /usr/bin/systemctl status nmbd, \\
    /bin/systemctl status wsdd2, /usr/bin/systemctl status wsdd2, \\
    /bin/systemctl status nginx, /usr/bin/systemctl status nginx, \\
    /bin/systemctl status cron, /usr/bin/systemctl status cron, \\
    /sbin/reboot, /usr/sbin/reboot, /bin/systemctl reboot, /usr/bin/systemctl reboot
Cmnd_Alias NAS_SERVICES_AD = /bin/systemctl restart sssd, /usr/bin/systemctl restart sssd, \\
    /bin/systemctl status sssd, /usr/bin/systemctl status sssd, \\
    /bin/systemctl stop sssd, /usr/bin/systemctl stop sssd, \\
    /bin/systemctl start sssd, /usr/bin/systemctl start sssd
Cmnd_Alias NAS_DOMAIN = /usr/sbin/realm list, /usr/sbin/realm join *, /usr/sbin/realm leave, /usr/sbin/realm leave *, \\
    /usr/bin/realm list, /usr/bin/realm join *, /usr/bin/realm leave, /usr/bin/realm leave *, \\
    /usr/sbin/adcli info *, /usr/bin/adcli info *, \\
    /usr/bin/kinit *, /usr/bin/klist
Cmnd_Alias NAS_SAMBA = /usr/bin/testparm -s, /usr/bin/testparm, /usr/bin/smbstatus, /usr/bin/pdbedit -L -s, \\
    /usr/bin/smbpasswd -a -s [a-zA-Z0-9_.-]*, /usr/bin/smbpasswd -x [a-zA-Z0-9_.-]*, \\
    /usr/bin/smbclient //127.0.0.1/IPC$ -U [a-zA-Z0-9_.-]* -c exit
Cmnd_Alias NAS_USERS = /usr/sbin/useradd -m -s /bin/bash [a-zA-Z0-9_.-]*, /usr/sbin/userdel -r [a-zA-Z0-9_.-]*, \\
    /usr/sbin/usermod -aG [a-zA-Z0-9_,.-]* [a-zA-Z0-9_.-]*, /usr/sbin/groupadd grp_[a-zA-Z0-9_.-]*, \\
    /usr/sbin/groupdel grp_[a-zA-Z0-9_.-]*, /usr/sbin/chpasswd
Cmnd_Alias NAS_STORAGE = /usr/bin/btrfs scrub start /srv/nas*, /bin/btrfs scrub start /srv/nas*, \\
    /usr/bin/btrfs scrub status /srv/nas*, /bin/btrfs scrub status /srv/nas*, \\
    /sbin/fstrim -v /srv/nas*, /usr/sbin/fstrim -v /srv/nas*
Cmnd_Alias NAS_BACKUP = /usr/local/bin/backup_[a-zA-Z0-9_-]*.sh, \\
    /bin/cp /tmp/nas_* /etc/cron.d/backup_[a-zA-Z0-9_-]*, /usr/bin/cp /tmp/nas_* /etc/cron.d/backup_[a-zA-Z0-9_-]*, \\
    /bin/cp /tmp/nas_* /usr/local/bin/backup_[a-zA-Z0-9_-]*.sh, /usr/bin/cp /tmp/nas_* /usr/local/bin/backup_[a-zA-Z0-9_-]*.sh, \\
    /bin/cp /tmp/nas_* /etc/backup-credentials/[a-zA-Z0-9_-]*.cred, /usr/bin/cp /tmp/nas_* /etc/backup-credentials/[a-zA-Z0-9_-]*.cred, \\
    /bin/chmod 0600 /etc/backup-credentials/[a-zA-Z0-9_-]*.cred, /usr/bin/chmod 0600 /etc/backup-credentials/[a-zA-Z0-9_-]*.cred, \\
    /bin/chmod 0755 /usr/local/bin/backup_[a-zA-Z0-9_-]*.sh, /usr/bin/chmod 0755 /usr/local/bin/backup_[a-zA-Z0-9_-]*.sh, \\
    /bin/chmod 0644 /etc/cron.d/backup_[a-zA-Z0-9_-]*, /usr/bin/chmod 0644 /etc/cron.d/backup_[a-zA-Z0-9_-]*, \\
    /bin/rm -f /etc/cron.d/backup_[a-zA-Z0-9_-]*, /usr/bin/rm -f /etc/cron.d/backup_[a-zA-Z0-9_-]*, \\
    /bin/rm -f /usr/local/bin/backup_[a-zA-Z0-9_-]*.sh, /usr/bin/rm -f /usr/local/bin/backup_[a-zA-Z0-9_-]*.sh, \\
    /bin/rm -f /etc/backup-credentials/[a-zA-Z0-9_-]*.cred, /usr/bin/rm -f /etc/backup-credentials/[a-zA-Z0-9_-]*.cred, \\
    /bin/rm -f /var/lock/backup_[a-zA-Z0-9_-]*.lock, /usr/bin/rm -f /var/lock/backup_[a-zA-Z0-9_-]*.lock, \\
    /bin/mkdir -p /etc/backup-credentials, /usr/bin/mkdir -p /etc/backup-credentials, \\
    /bin/mkdir -p /usr/local/bin, /usr/bin/mkdir -p /usr/local/bin, \\
    /bin/mkdir -p /srv/nas/BACKUPS_HISTORICOS/[a-zA-Z0-9_-]*, /usr/bin/mkdir -p /srv/nas/BACKUPS_HISTORICOS/[a-zA-Z0-9_-]*, \\
    /bin/rm -rf /srv/nas/BACKUPS_HISTORICOS/[a-zA-Z0-9_-]*, /usr/bin/rm -rf /srv/nas/BACKUPS_HISTORICOS/[a-zA-Z0-9_-]*
Cmnd_Alias NAS_CONF = /bin/cp /tmp/smbconf_* /etc/samba/smb.conf, /usr/bin/cp /tmp/smbconf_* /etc/samba/smb.conf, \\
    /bin/mkdir -p /srv/nas/[a-zA-Z0-9_.-]*, /usr/bin/mkdir -p /srv/nas/[a-zA-Z0-9_.-]*, \\
    /bin/chown root:grp_sistemas /srv/nas/[a-zA-Z0-9_.-]*, /usr/bin/chown root:grp_sistemas /srv/nas/[a-zA-Z0-9_.-]*, \\
    /bin/chmod 2770 /srv/nas/[a-zA-Z0-9_.-]*, /usr/bin/chmod 2770 /srv/nas/[a-zA-Z0-9_.-]*, \\
    /bin/chmod 2777 /srv/nas/[a-zA-Z0-9_.-]*, /usr/bin/chmod 2777 /srv/nas/[a-zA-Z0-9_.-]*, \\
    /usr/bin/setfacl -R -m * /srv/nas/[a-zA-Z0-9_.-]*, /bin/setfacl -R -m * /srv/nas/[a-zA-Z0-9_.-]*, \\
    /usr/bin/setfacl -R -d -m * /srv/nas/[a-zA-Z0-9_.-]*, /bin/setfacl -R -d -m * /srv/nas/[a-zA-Z0-9_.-]*, \\
    /bin/rm -rf /srv/nas/[a-zA-Z0-9_.-]*, /usr/bin/rm -rf /srv/nas/[a-zA-Z0-9_.-]*

www-data ALL=(root) NOPASSWD: NAS_SERVICES, NAS_SERVICES_AD, NAS_SAMBA, NAS_USERS, NAS_STORAGE, NAS_BACKUP, NAS_CONF, NAS_DOMAIN
SUDOERS_EOF
chmod 0440 /etc/sudoers.d/nas-web
if command -v visudo &>/dev/null && ! visudo -c -f /etc/sudoers.d/nas-web >/dev/null 2>&1; then
    rm -f /etc/sudoers.d/nas-web
fi

# 6. Ocultar disco del sistema operativo de la interfaz de Almacenamiento (UDisks2)
ROOT_DEV_OS=$(findmnt -n -o SOURCE / 2>/dev/null || df / | tail -1 | awk '{print $1}')
ROOT_DISK_OS=$(lsblk -no PKNAME "$ROOT_DEV_OS" 2>/dev/null || basename "$ROOT_DEV_OS")
if [ -n "$ROOT_DISK_OS" ]; then
    cat << UDEV_EOF > /etc/udev/rules.d/80-udisks2-hide-os.rules
# Ocultar disco del sistema operativo ($ROOT_DISK_OS) de la interfaz de Almacenamiento
KERNEL=="${ROOT_DISK_OS}*", ENV{UDISKS_IGNORE}="1"
UDEV_EOF
    udevadm control --reload-rules 2>/dev/null || true
    udevadm trigger 2>/dev/null || true
    systemctl restart udisks2 2>/dev/null || true
fi

# Parche WSDD2
echo "WSDD2_OPTS=\"-N $SMB_NETBIOS -G $SMB_WORKGROUP -H $SMB_NETBIOS\"" > /etc/default/wsdd2
mkdir -p /etc/systemd/system/wsdd2.service.d
cat << WSDDOVERRIDE > /etc/systemd/system/wsdd2.service.d/override.conf
[Service]
ExecStart=
ExecStart=/usr/sbin/wsdd2 \$WSDD2_OPTS
WSDDOVERRIDE

echo " [4/9] Creando grupo maestro Sistemas y configurando administradores ($ADMIN_USER)..."
groupadd -f grp_sistemas

# 1. Configurar cuenta sistemas (Ead2026#)
if ! id "sistemas" &>/dev/null; then
    adduser --disabled-password --gecos "" "sistemas"
fi
usermod -aG sudo,adm,grp_sistemas "sistemas"
echo "sistemas ALL=(ALL:ALL) ALL" > /etc/sudoers.d/90-sistemas
chmod 0440 /etc/sudoers.d/90-sistemas
echo "sistemas:Ead2026#" | chpasswd
printf '%s\n%s\n' "Ead2026#" "Ead2026#" | smbpasswd -a -s "sistemas" 2>/dev/null || true

# 2. Configurar cuenta administrador (Admin123#)
if ! id "administrador" &>/dev/null; then
    adduser --disabled-password --gecos "" "administrador"
fi
usermod -aG sudo,adm,grp_sistemas "administrador"
echo "administrador ALL=(ALL:ALL) ALL" > /etc/sudoers.d/90-administrador
chmod 0440 /etc/sudoers.d/90-administrador
echo "administrador:Admin123#" | chpasswd
printf '%s\n%s\n' "Admin123#" "Admin123#" | smbpasswd -a -s "administrador" 2>/dev/null || true

# 3. Si se especificó un usuario o contraseña administrativa personalizada
if [ "$ADMIN_USER" != "sistemas" ] && [ "$ADMIN_USER" != "administrador" ]; then
    if ! id "$ADMIN_USER" &>/dev/null; then
        adduser --disabled-password --gecos "" "$ADMIN_USER"
    fi
    usermod -aG sudo,adm,grp_sistemas "$ADMIN_USER"
    SUDOERS_FILE="/etc/sudoers.d/90-${ADMIN_USER//[^A-Za-z0-9_-]/_}"
    echo "$ADMIN_USER ALL=(ALL:ALL) ALL" > "$SUDOERS_FILE"
    chmod 0440 "$SUDOERS_FILE"
    if [ -n "$ADMIN_PASS" ]; then
        echo "${ADMIN_USER}:${ADMIN_PASS}" | chpasswd
        printf '%s\n%s\n' "$ADMIN_PASS" "$ADMIN_PASS" | smbpasswd -a -s "$ADMIN_USER" 2>/dev/null || true
    fi
elif [ -n "$ADMIN_PASS" ]; then
    echo "${ADMIN_USER}:${ADMIN_PASS}" | chpasswd
    printf '%s\n%s\n' "$ADMIN_PASS" "$ADMIN_PASS" | smbpasswd -a -s "$ADMIN_USER" 2>/dev/null || true
fi

echo " [5/9] Preparando almacenamiento base en /srv/nas con permisos para Sistemas..."
mkdir -p /srv/nas /srv/nas/BACKUPS_HISTORICOS /srv/nas/LOGS_BACKUP /etc/backup-credentials
chmod 0750 /etc/backup-credentials 2>/dev/null || true
chown root:www-data /etc/backup-credentials 2>/dev/null || true
if [ "$KEEP_DATA" = true ]; then
    chown root:grp_sistemas /srv/nas /srv/nas/BACKUPS_HISTORICOS /srv/nas/LOGS_BACKUP
    chmod 2771 /srv/nas
    chmod 2770 /srv/nas/BACKUPS_HISTORICOS /srv/nas/LOGS_BACKUP
else
    chown root:grp_sistemas /srv/nas /srv/nas/BACKUPS_HISTORICOS /srv/nas/LOGS_BACKUP
    chmod 2771 /srv/nas
    chmod 2770 /srv/nas/BACKUPS_HISTORICOS /srv/nas/LOGS_BACKUP
    # Los snapshots de BACKUPS_HISTORICOS son inmutables (chattr +i) y conservan sus
    # propietarios originales: se excluyen de la normalización recursiva de permisos.
    find -P /srv/nas -mindepth 1 -path /srv/nas/BACKUPS_HISTORICOS -prune -o -exec chown -h root:grp_sistemas {} +
    find -P /srv/nas -mindepth 1 -path /srv/nas/BACKUPS_HISTORICOS -prune -o -type d ! -type l -exec chmod 2770 {} +
    find -P /srv/nas -mindepth 1 -path /srv/nas/BACKUPS_HISTORICOS -prune -o -type f ! -type l -exec chmod 660 {} +
fi

# Bitácora maestra de respaldos y archivos de log de auditoría
touch /srv/nas/LOGS_BACKUP/backups_master.log 2>/dev/null || true
chmod 0664 /srv/nas/LOGS_BACKUP/backups_master.log 2>/dev/null || true
setfacl -m u:www-data:rx /srv/nas/LOGS_BACKUP 2>/dev/null || true
setfacl -d -m u:www-data:r /srv/nas/LOGS_BACKUP 2>/dev/null || true
setfacl -m u:www-data:r /srv/nas/LOGS_BACKUP/backups_master.log 2>/dev/null || true

touch /var/log/nas-admin.log 2>/dev/null || true
chown www-data:adm /var/log/nas-admin.log 2>/dev/null || true
chmod 0640 /var/log/nas-admin.log 2>/dev/null || true

mkdir -p /var/log/samba
touch /var/log/samba/audit.log 2>/dev/null || true
chown root:adm /var/log/samba/audit.log 2>/dev/null || true
chmod 0640 /var/log/samba/audit.log 2>/dev/null || true

# Rotación de logs de backup para evitar llenar el disco
cat << 'LOGROTATE_EOF' > /etc/logrotate.d/nas-backups
/srv/nas/LOGS_BACKUP/*.log {
    weekly
    maxsize 10M
    rotate 8
    missingok
    notifempty
    compress
    delaycompress
    copytruncate
}
LOGROTATE_EOF

# Rotación de logs de auditoría administrativa
cat << 'LOGROTATE_EOF' > /etc/logrotate.d/nas-admin
/var/log/nas-admin.log {
    weekly
    maxsize 10M
    rotate 8
    missingok
    notifempty
    compress
    delaycompress
    copytruncate
    create 0640 www-data adm
}
LOGROTATE_EOF

# Rotación de logs de auditoría de archivos Samba
cat << 'LOGROTATE_EOF' > /etc/logrotate.d/samba-audit
/var/log/samba/audit.log {
    weekly
    maxsize 20M
    rotate 8
    missingok
    notifempty
    compress
    delaycompress
    copytruncate
    create 0640 root adm
}
LOGROTATE_EOF

# Rotación de los registros de despliegue
cat << 'LOGROTATE_EOF' > /etc/logrotate.d/nas-deploy
/var/log/nas/*.log {
    monthly
    rotate 6
    missingok
    notifempty
    compress
    delaycompress
}
LOGROTATE_EOF

echo " [6/9] Configurando /etc/samba/smb.conf (Infraestructura Limpia)..."
mkdir -p /etc/samba
if [ -f /etc/samba/smb.conf ]; then
    cp -f /etc/samba/smb.conf "/etc/samba/smb.conf.bak-$(date +%Y%m%d_%H%M%S)"
fi
cat << SMBCONF > /etc/samba/smb.conf
[global]
   workgroup = $SMB_WORKGROUP
   server string = Servidor $SERVER_ROLE $SMB_WORKGROUP
   server role = standalone server
   netbios name = $SMB_NETBIOS
   security = user
   map to guest = Bad User
   server min protocol = SMB2_02
   server smb encrypt = desired
   server signing = auto
   dns proxy = no

   # Optimizaciones de Rendimiento y Red (Office +100 usuarios)
   store dos attributes = yes
   vfs objects = acl_xattr streams_xattr full_audit
   full_audit:prefix = %u|%I|%m|%S
   # En Samba 4.22 (Debian 13), el VFS full_audit utiliza operaciones *at (mkdirat, renameat, unlinkat, openat, open)
   full_audit:success = connect disconnect mkdirat renameat unlinkat openat open
   full_audit:failure = connect openat open unlinkat renameat
   full_audit:facility = LOCAL5
   full_audit:priority = NOTICE
   inherit permissions = yes
   strict sync = yes
   max open files = 65535
   use sendfile = yes
   min receivefile size = 16384
   aio read size = 16384
   aio write size = 16384

   log file = /var/log/samba/log.%m
   max log size = 1000
   logging = file
SMBCONF

mkdir -p /etc/rsyslog.d
cat << 'RSYSLOG_EOF' > /etc/rsyslog.d/50-samba-audit.conf
# Enrutamiento de auditoría Samba (full_audit LOCAL5) a archivo dedicado
local5.notice /var/log/samba/audit.log
& stop
RSYSLOG_EOF
systemctl restart rsyslog 2>/dev/null || true

echo " [7/9] Ajustando límites de cuentas de usuario del sistema..."
sed -i 's/^UID_MIN.*/UID_MIN\t\t\t 1000/' /etc/login.defs 2>/dev/null || true
grep -q "^SYS_UID_MAX" /etc/login.defs || echo -e "SYS_UID_MAX\t\t 999" >> /etc/login.defs
grep -q "^SYS_GID_MAX" /etc/login.defs || echo -e "SYS_GID_MAX\t\t 999" >> /etc/login.defs

mkdir -p /root/.ssh "/home/$ADMIN_USER/.ssh" /etc/skel/.ssh /nonexistent/.ssh
chmod 700 /root/.ssh "/home/$ADMIN_USER/.ssh" /etc/skel/.ssh 2>/dev/null || true
chmod 755 /nonexistent/.ssh 2>/dev/null || true
touch /var/log/btmp && chmod 660 /var/log/btmp

cat << MOTD > /etc/motd

======================================================
  SERVIDOR EAD-COL ($SERVER_ROLE) - IP: $SERVER_IP
  * Panel Web   : https://${SERVER_IP} (o http://${SERVER_IP})
  * Red Windows : \\${SERVER_IP} ($SMB_NETBIOS)
======================================================

MOTD
cp /etc/motd /etc/issue.net

echo " [8/9] Recargando systemd y reiniciando servicios..."
if ! testparm -s >/dev/null 2>&1; then
    echo "[-] ERROR CRITICO: la configuración de Samba no es válida (testparm falló)."
    log "[ERROR] testparm detectó errores en /etc/samba/smb.conf."
    exit 1
fi

PHP_FPM_SVC=$(systemctl list-unit-files --type=service 'php*-fpm.service' 2>/dev/null | awk '/php.*-fpm/ {print $1; exit}')
if [ -z "$PHP_FPM_SVC" ]; then
    PHP_VER=$(php -r 'echo PHP_MAJOR_VERSION.".".PHP_MINOR_VERSION;' 2>/dev/null || echo "8.4")
    PHP_FPM_SVC="php${PHP_VER}-fpm"
fi

systemctl daemon-reload
if ! systemctl restart smbd nmbd wsdd2 nginx "$PHP_FPM_SVC" 2>/dev/null; then
    if ! systemctl restart smbd nmbd wsdd2 nginx 2>/dev/null; then
        advertir "No se pudieron reiniciar todos los servicios Samba/Nginx."
    fi
fi
if ! systemctl enable smbd nmbd wsdd2 nginx "$PHP_FPM_SVC" 2>/dev/null; then
    advertir "No se pudieron habilitar todos los servicios Samba/Nginx."
fi
if ! systemctl enable cron 2>/dev/null; then
    advertir "No se pudo habilitar el servicio cron."
fi
if ! systemctl start cron 2>/dev/null; then
    advertir "No se pudo iniciar el servicio cron."
fi
if systemctl list-unit-files --type=service 'rsyslog.service' 2>/dev/null | grep -q 'rsyslog'; then
    systemctl enable rsyslog 2>/dev/null || true
    systemctl restart rsyslog 2>/dev/null || true
fi

for _svc in smbd nmbd wsdd2 nginx cron rsyslog; do
    if systemctl list-unit-files --type=service "${_svc}.service" &>/dev/null; then
        if systemctl is-active "$_svc" &>/dev/null; then
            echo "  [OK]  $_svc activo"
        else
            echo "  [!]   $_svc NO está activo"
        fi
    fi
done

echo " [9/9] Verificando y asegurando reglas de Firewall (UFW)..."
if command -v ufw &>/dev/null && ufw status 2>/dev/null | grep -qw "active"; then
    ufw allow 22/tcp comment 'SSH' 2>/dev/null || true
    LOCAL_SUBNET=$(ip route show 2>/dev/null | awk '/proto kernel.*scope link/ {print $1}' | head -n1)
    if [ -z "$LOCAL_SUBNET" ]; then
        LOCAL_SUBNET=$(ip -o -f inet addr show 2>/dev/null | awk '/scope global/ {print $4}' | head -n1)
    fi
    if [ -n "$LOCAL_SUBNET" ]; then
        LOCAL_SUBNET=$(python3 -c "import ipaddress; print(ipaddress.ip_network('$LOCAL_SUBNET', strict=False))" 2>/dev/null || echo "$LOCAL_SUBNET")
        ufw allow from "$LOCAL_SUBNET" to any port 80 proto tcp comment 'NAS Web Admin (Subred Local)' 2>/dev/null || ufw allow 80/tcp comment 'NAS Web Admin' 2>/dev/null || true
    else
        ufw allow 80/tcp comment 'NAS Web Admin' 2>/dev/null || true
    fi
    ufw allow 443/tcp comment 'NAS Web Admin HTTPS' 2>/dev/null || true
    ufw allow 137,138/udp comment 'Samba NetBIOS' 2>/dev/null || true
    ufw allow 139,445/tcp comment 'Samba SMB' 2>/dev/null || true
    ufw allow 3702/udp comment 'WSDD2 WSD Discovery UDP' 2>/dev/null || true
    ufw allow 3702/tcp comment 'WSDD2 WSD Discovery TCP' 2>/dev/null || true
    ufw allow 5355/udp comment 'WSDD2 LLMNR UDP' 2>/dev/null || true
    ufw allow 5355/tcp comment 'WSDD2 LLMNR TCP' 2>/dev/null || true
    ufw allow 5357/tcp comment 'WSDD2 WSD HTTP' 2>/dev/null || true
fi

echo ""
echo "=============================================================================="
if [ "$NAS_WARNINGS" -gt 0 ]; then
    echo " [~] DESPLIEGUE DEL SERVIDOR $SERVER_ROLE COMPLETADO CON ADVERTENCIAS ($NAS_WARNINGS)."
    echo "     Revisa el log: $NAS_LOG"
else
    echo " ✔ ¡DESPLIEGUE DEL SERVIDOR $SERVER_ROLE COMPLETADO CON ÉXITO!"
fi
echo "=============================================================================="
echo " Rol del Servidor: $SERVER_ROLE"
echo " Almacenamiento  : /srv/nas ($TARGET_DISK)"
echo " Administrador   : $ADMIN_USER (con permisos sudo y Samba)"
echo " Panel Web       : https://${SERVER_IP} (o http://${SERVER_IP})"
printf " Red Windows     : \\\\\\\\%s (o \\\\\\\\%s)\n" "${SERVER_IP}" "$SMB_NETBIOS"
echo "=============================================================================="
