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
NAS_LOG="/tmp/nas_deploy_$(date +%Y%m%d_%H%M%S).log"

log() {
    printf '%s\n' "$*" >> "$NAS_LOG"
}

advertir() {
    NAS_WARNINGS=$((NAS_WARNINGS + 1))
    echo "  [!] $*" >&2
    log "[ADVERTENCIA] $*"
}

trap 'log "[ERROR] Fallo en la línea $LINENO: $BASH_COMMAND"' ERR

# Restaura los parches de Cockpit desde la copia de seguridad más reciente.
restaurar_parches_cockpit() {
    local orig ultimo
    for orig in /usr/share/cockpit/identities/assets/*.js /usr/share/cockpit/storaged/storaged.js.gz; do
        [ -f "$orig" ] || continue
        case "$orig" in
            *.bak-*) continue ;;
        esac
        ultimo=$(ls -1 "$orig".bak-* 2>/dev/null | sort | tail -n 1)
        if [ -n "$ultimo" ] && [ -f "$ultimo" ]; then
            cp -p "$ultimo" "$orig"
            echo "  [•] Parche restaurado en $orig desde $ultimo"
        else
            echo "  [!] Sin respaldo disponible para $orig"
        fi
    done
}

# Aplica el parche de Identities con respaldo, verificación y escritura atómica.
aplicar_parche_identities() {
    local PATCH_PY PATCH_SALIDA
    PATCH_PY=$(mktemp)
    cat << 'PY' > "$PATCH_PY"
import glob, os, shutil, time

REEMPLAZOS = [
    ("l.value=f.split(\"\\n\").filter(w=>!/^\\s*$/.test(w))",
     "l.value=f.split(\"\\n\").filter(w=>w.startsWith(\"grp_\"))"),
    ("if(u<1e3&&u!==0)return null;",
     "if(u<1e3||u>=6e4)return null;"),
    ("if(u<1e3)return null;",
     "if(u<1e3||u>=6e4)return null;"),
]

for js in glob.glob("/usr/share/cockpit/identities/assets/*.js"):
    try:
        with open(js, "r", encoding="utf-8") as f:
            contenido = f.read()
        aplicados = 0
        for origen, destino in REEMPLAZOS:
            if origen in contenido:
                contenido = contenido.replace(origen, destino)
                aplicados += 1
        if aplicados == 0:
            print("  [!] Parche Identities omitido (patrones no encontrados): %s" % js)
            continue
        respaldo = "%s.bak-%s" % (js, time.strftime("%Y%m%d_%H%M%S"))
        shutil.copy2(js, respaldo)
        for viejo in sorted(glob.glob(js + ".bak-*"))[:-3]:
            try:
                os.remove(viejo)
            except OSError:
                pass
        tmp = "%s.tmp" % js
        with open(tmp, "w", encoding="utf-8") as f:
            f.write(contenido)
        os.replace(tmp, js)
        print("  [OK] Parche Identities aplicado en %s (respaldo: %s)" % (js, respaldo))
    except Exception as e:
        print("  [!] No se pudo parchear %s: %s" % (js, e))
PY
    if ! PATCH_SALIDA=$(python3 "$PATCH_PY" 2>&1); then
        rm -f "$PATCH_PY"
        advertir "Falló el parche de Cockpit Identities."
        return 0
    fi
    rm -f "$PATCH_PY"
    echo "$PATCH_SALIDA"
    log "$PATCH_SALIDA"
    if echo "$PATCH_SALIDA" | grep -q "omitido"; then
        advertir "El parche de Cockpit Identities se omitió (patrones no encontrados)."
    fi
}

# Aplica el parche de Storage con respaldo, verificación y escritura atómica.
aplicar_parche_storage() {
    local PATCH_PY PATCH_SALIDA
    PATCH_PY=$(mktemp)
    cat << 'PY' > "$PATCH_PY"
import gzip, os, shutil, time, glob

path = "/usr/share/cockpit/storaged/storaged.js.gz"
if os.path.exists(path):
    try:
        with gzip.open(path, "rt", encoding="utf-8") as f:
            contenido = f.read()
        target1 = "if(o||(o=b.drives_multipath_blocks[t.path][0]),!o||Zr(b,o.path))return;"
        repl1    = "if(o||(o=b.drives_multipath_blocks[t.path][0]),!o||Zr(b,o.path)||o.HintIgnore)return;"
        target2 = "function yT(e,t){if(Zr(b,t.path))return;"
        repl2    = "function yT(e,t){if(Zr(b,t.path)||t.HintIgnore)return;"
        if target1 not in contenido and target2 not in contenido:
            print("  [!] Parche Storage omitido (patrones no encontrados).")
        else:
            respaldo = "%s.bak-%s" % (path, time.strftime("%Y%m%d_%H%M%S"))
            shutil.copy2(path, respaldo)
            for viejo in sorted(glob.glob(path + ".bak-*"))[:-3]:
                try:
                    os.remove(viejo)
                except OSError:
                    pass
            contenido = contenido.replace(target1, repl1).replace(target2, repl2)
            tmp = "%s.tmp" % path
            with gzip.open(tmp, "wt", encoding="utf-8") as f:
                f.write(contenido)
            os.replace(tmp, path)
            print("  [OK] Parche Storage aplicado (respaldo: %s)." % respaldo)
    except Exception as e:
        print("  [!] No se pudo parchear %s: %s" % (path, e))
PY
    if ! PATCH_SALIDA=$(python3 "$PATCH_PY" 2>&1); then
        rm -f "$PATCH_PY"
        advertir "Falló el parche de Cockpit Storage."
        return 0
    fi
    rm -f "$PATCH_PY"
    echo "$PATCH_SALIDA"
    log "$PATCH_SALIDA"
    if echo "$PATCH_SALIDA" | grep -q "omitido"; then
        advertir "El parche de Cockpit Storage se omitió (patrones no encontrados)."
    fi
}

# Permite restaurar los parches de Cockpit y salir sin ejecutar el despliegue.
for _arg in "$@"; do
    if [ "$_arg" == "--restore-patches" ]; then
        restaurar_parches_cockpit
        exit 0
    fi
done

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
for _arg in "$@"; do
    [ "$_arg" == "--force" ] && FORCE=true
    [ "$_arg" == "--confirm" ] && CONFIRM=true
    [ "$_arg" == "--ignore-in-use" ] && IGNORE_IN_USE=true
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
    sudo acl samba samba-common-bin wsdd2 smbclient samba-vfs-modules \
    cockpit cockpit-storaged cockpit-networkmanager cockpit-packagekit \
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
    local ES_HDD
    ES_HDD=$(cat "/sys/block/$DISCO_BASE/queue/rotational" 2>/dev/null || echo "1")
    local RAM_KB
    RAM_KB=$(awk '/MemTotal/ {print $2}' /proc/meminfo)
    local CORES
    CORES=$(nproc)

    # 1. Ajuste de CPU (Compresión Zstd)
    local BTRFS_COMPRESS="zstd:1"
    if [ "$CORES" -ge 8 ]; then
        BTRFS_COMPRESS="zstd:5"
    elif [ "$CORES" -ge 3 ]; then
        BTRFS_COMPRESS="zstd:3"
    fi

    # 2. Ajuste de Disco (HDD vs SSD)
    BTRFS_OPTS="rw,noatime,compress=$BTRFS_COMPRESS,space_cache=v2"
    if [ "$ES_HDD" -eq 1 ]; then
        BTRFS_OPTS="$BTRFS_OPTS,autodefrag"
        READAHEAD_KB=4096
    else
        BTRFS_OPTS="$BTRFS_OPTS,ssd,discard=async"
        READAHEAD_KB=1024
    fi

    # 3. Ajuste de RAM (Sysctl Dirty Bytes)
    local DIRTY_BYTES=$((256 * 1024 * 1024)) # Default 256MB
    if [ "$RAM_KB" -gt 8388608 ]; then # > 8GB
        DIRTY_BYTES=$((1024 * 1024 * 1024)) # 1GB
    elif [ "$RAM_KB" -gt 4194304 ]; then # > 4GB
        DIRTY_BYTES=$((512 * 1024 * 1024)) # 512MB
    fi
    local DIRTY_BG_BYTES=$((DIRTY_BYTES / 2))

    # Escribir Sysctl Dinámico
    cat <<EOF > /etc/sysctl.d/99-nas-tuning.conf
vm.swappiness = 10
vm.vfs_cache_pressure = 50
vm.dirty_bytes = $DIRTY_BYTES
vm.dirty_background_bytes = $DIRTY_BG_BYTES
EOF
    sysctl -p /etc/sysctl.d/99-nas-tuning.conf >/dev/null 2>&1 || true

    # Aplicar Readahead
    blockdev --setra $((READAHEAD_KB * 2)) "$DISCO" 2>/dev/null || true

    # Exportar variables para usarlas en el formateo
    export BTRFS_OPTS
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
else
    echo "  -> Inicializando y formateando disco dedicado: $TARGET_DISK"
    if [ "$IGNORE_IN_USE" != "true" ] && disco_en_uso "$TARGET_DISK"; then
        echo "[-] ERROR: $TARGET_DISK parece estar en uso (montado, PV de LVM o miembro de RAID)."
        echo "    Abortando por seguridad. Usa --ignore-in-use bajo tu responsabilidad si realmente deseas formatearlo."
        exit 1
    fi

    # Mostrar la información completa del disco antes de formatear.
    echo "  ╔══════════════════════════════════════════════════════════════════╗"
    echo "  ║  DISCO A FORMATEAR: $TARGET_DISK"
    echo "  ╚══════════════════════════════════════════════════════════════════╝"
    if ! lsblk -o NAME,SIZE,MODEL,TYPE,MOUNTPOINT "$TARGET_DISK" 2>/dev/null; then
        echo "[-] ERROR: no se pudo obtener la información del disco $TARGET_DISK."
        echo "    Abortando para no formatear un dispositivo desconocido."
        exit 1
    fi

    # Confirmación explícita antes de una operación destructiva.
    if [ "$IGNORE_IN_USE" == "true" ]; then
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
    log "Formateo confirmado para el disco $TARGET_DISK"

    auto_tune_hardware "$TARGET_DISK"

    while read -r _part; do
        [ -z "$_part" ] && continue
        umount "/dev/$_part" 2>/dev/null || true
    done < <(lsblk -ln -o NAME "$TARGET_DISK" 2>/dev/null | tail -n +2)
    parted -s "$TARGET_DISK" mklabel gpt mkpart primary btrfs 0% 100%
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

    mkfs.btrfs -f -L "NAS_DATA" "$PART_NAS"
    UUID_NAS=$(blkid -s UUID -o value "$PART_NAS")

    sed -i '\|/srv/nas|d' /etc/fstab
    if [ -n "$UUID_NAS" ]; then
        echo "UUID=$UUID_NAS /srv/nas btrfs defaults,$BTRFS_OPTS 0 2" >> /etc/fstab
    else
        echo "$PART_NAS /srv/nas btrfs defaults,$BTRFS_OPTS 0 2" >> /etc/fstab
    fi
    MOUNT_OK=false
    if mount -o "$BTRFS_OPTS" "$PART_NAS" /srv/nas 2>/dev/null; then
        MOUNT_OK=true
    elif mount /srv/nas 2>/dev/null; then
        MOUNT_OK=true
    fi
    if [ "$MOUNT_OK" != "true" ]; then
        echo "[-] ERROR CRITICO: No se pudo montar $PART_NAS en /srv/nas."
        echo "    Abortando para evitar escribir los respaldos en la particion del sistema."
        exit 1
    fi
fi

echo " [3/9] Instalando extensiones de Cockpit (File Sharing, Identities, Navigator)..."
TMP_DIR=$(mktemp -d)
cd "$TMP_DIR"

install_deb_pkg() {
    local url="$1"
    local filename="$2"
    local plugin_dir="$3"
    local expected_sha="$4"
    if wget -q --spider "$url" 2>/dev/null; then
        wget -q "$url" -O "$filename"
        if [ ! -s "$filename" ]; then
            echo "  [!] Aviso: la descarga de $filename quedo vacia."
            return
        fi
        if [ -n "$expected_sha" ]; then
            local actual_sha
            actual_sha=$(sha256sum "$filename" | awk '{print $1}')
            if [ "$actual_sha" != "$expected_sha" ]; then
                echo "  [!] Aviso: checksum SHA256 invalido para $filename. Se omite por seguridad."
                return
            fi
        fi
        if ! DEBIAN_FRONTEND=noninteractive apt-get install -y -qq --no-install-recommends ./"$filename" >/dev/null 2>&1; then
            echo "  [!] Aviso: no se pudo instalar $filename (posibles dependencias incompatibles). Se omitira el modulo."
        elif [ -n "$plugin_dir" ] && [ ! -d "$plugin_dir" ]; then
            echo "  [!] Aviso: $filename se instalo pero no se detecto el directorio $plugin_dir."
        fi
    else
        echo "  [!] Aviso: no se pudo descargar $filename desde GitHub."
    fi
}

install_deb_pkg "https://github.com/45Drives/cockpit-file-sharing/releases/download/v3.3.4/cockpit-file-sharing_3.3.4-1focal_all.deb" "cockpit-file-sharing.deb" "/usr/share/cockpit/file-sharing" "fd75ee1690159642de3663870b46efc5bb25dddf983e1a1726c0089d4b0cf27e"
install_deb_pkg "https://github.com/45Drives/cockpit-identities/releases/download/v0.1.12/cockpit-identities_0.1.12-1focal_all.deb" "cockpit-identities.deb" "/usr/share/cockpit/identities" "85d1412da210c86d0ebad35624fc512d895fd52f09ee0a8629cc1bc3bd0e825a"
install_deb_pkg "https://github.com/45Drives/cockpit-navigator/releases/download/v0.5.10/cockpit-navigator_0.5.10-1focal_all.deb" "cockpit-navigator.deb" "/usr/share/cockpit/navigator" "784b8b1d7e02224594d34e6d60945c72b54a557692a37fefbb0046146b74040e"

cd /
rm -rf "$TMP_DIR"

# 1. Ocultar menú nativo redundante 'Accounts' de Cockpit en favor de 'Identities'
if [ -f /usr/share/cockpit/users/manifest.json ]; then
    cat << 'ACCOUNTS_EOF' > /usr/share/cockpit/users/manifest.json
{
    "version": 1.0
}
ACCOUNTS_EOF
fi

# 2. Parche Cockpit Identities (Filtrar exclusivamente grupos grp_* y usuarios reales 1000 <= UID < 60000)
aplicar_parche_identities

# 3. Instalar Módulo Web Nativo de Backups (EAD) en Cockpit
SRC_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
if [ -d "$SRC_DIR/web/backups" ]; then
    mkdir -p /usr/share/cockpit/backups
    cp -rf "$SRC_DIR/web/backups/"* /usr/share/cockpit/backups/
    # Copiar PatternFly CSS desde Navigator (mismo archivo que usan todos los plugins 45Drives)
    if [ -f /usr/share/cockpit/navigator/cockpit.css.gz ]; then
        cp -f /usr/share/cockpit/navigator/cockpit.css.gz /usr/share/cockpit/backups/
    fi
    chmod -R 755 /usr/share/cockpit/backups
fi

# 4. Ocultar disco del sistema operativo de la interfaz de Almacenamiento (Cockpit Storage / UDisks2)
ROOT_DEV_OS=$(findmnt -n -o SOURCE / 2>/dev/null || df / | tail -1 | awk '{print $1}')
ROOT_DISK_OS=$(lsblk -no PKNAME "$ROOT_DEV_OS" 2>/dev/null || basename "$ROOT_DEV_OS")
if [ -n "$ROOT_DISK_OS" ]; then
    cat << UDEV_EOF > /etc/udev/rules.d/80-udisks2-hide-os.rules
# Ocultar disco del sistema operativo ($ROOT_DISK_OS) de la interfaz de Almacenamiento (UDisks2 / Cockpit Storage)
KERNEL=="${ROOT_DISK_OS}*", ENV{UDISKS_IGNORE}="1"
UDEV_EOF
    udevadm control --reload-rules 2>/dev/null || true
    udevadm trigger 2>/dev/null || true
    systemctl restart udisks2 2>/dev/null || true

    # Parche Cockpit Storage (Ocultar unidades con HintIgnore)
    aplicar_parche_storage
fi

# Parche WSDD2
echo "WSDD2_OPTS=\"-N $SMB_NETBIOS -G $SMB_WORKGROUP -H $SMB_NETBIOS\"" > /etc/default/wsdd2
mkdir -p /etc/systemd/system/wsdd2.service.d
cat << WSDDOVERRIDE > /etc/systemd/system/wsdd2.service.d/override.conf
[Service]
ExecStart=
ExecStart=/usr/sbin/wsdd2 \$WSDD2_OPTS
WSDDOVERRIDE

echo " [4/9] Creando grupo maestro Sistemas y configurando Administrador ($ADMIN_USER)..."
groupadd -f grp_sistemas

if ! id "$ADMIN_USER" &>/dev/null; then
    adduser --disabled-password --gecos "" "$ADMIN_USER"
fi

usermod -aG sudo,adm,grp_sistemas "$ADMIN_USER"
SUDOERS_FILE="/etc/sudoers.d/90-${ADMIN_USER//[^A-Za-z0-9_-]/_}"
echo "$ADMIN_USER ALL=(ALL:ALL) ALL" > "$SUDOERS_FILE"
chmod 0440 "$SUDOERS_FILE"
if command -v visudo &>/dev/null && ! visudo -c -f "$SUDOERS_FILE" >/dev/null 2>&1; then
    rm -f "$SUDOERS_FILE"
fi

if [ -n "$ADMIN_PASS" ]; then
    echo "${ADMIN_USER}:${ADMIN_PASS}" | chpasswd
    if ! printf '%s\n%s\n' "$ADMIN_PASS" "$ADMIN_PASS" | smbpasswd -a -s "$ADMIN_USER" 2>/dev/null; then
        advertir "No se pudo registrar la contraseña Samba de $ADMIN_USER (quizá ya existe una cuenta Samba)."
    fi
fi

echo " [5/9] Preparando almacenamiento base en /srv/nas con permisos para Sistemas..."
mkdir -p /srv/nas /srv/nas/BACKUPS_HISTORICOS /srv/nas/LOGS_BACKUP
chown -R root:grp_sistemas /srv/nas
find /srv/nas -type d -exec chmod 2775 {} +
find /srv/nas -type f -exec chmod 664 {} +

# Rotación de logs de backup para evitar llenar el disco
cat << 'LOGROTATE_EOF' > /etc/logrotate.d/nas-backups
/srv/nas/LOGS_BACKUP/*.log {
    weekly
    rotate 8
    missingok
    notifempty
    compress
    delaycompress
    copytruncate
}
LOGROTATE_EOF

VFS_IOURING_LINE=""
if find /usr/lib -path "*samba/vfs/io_uring.so" -print -quit 2>/dev/null | grep -q .; then
    VFS_IOURING_LINE="   vfs objects = io_uring"
fi

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
   dns proxy = no
   include = registry

   # Optimizaciones de Rendimiento y Red (Auto-Tuning)
   use sendfile = yes
   min receivefile size = 16384
   aio read size = 16384
   aio write size = 16384
$VFS_IOURING_LINE
   socket options = TCP_NODELAY IPTOS_LOWDELAY

   log file = /var/log/samba/log.%m
   max log size = 1000
   logging = file
SMBCONF

echo " [7/9] Aplicando parches de compatibilidad en español y límites de cuentas para Cockpit..."
sed -i 's/^UID_MIN.*/UID_MIN\t\t\t 1000/' /etc/login.defs 2>/dev/null || true
grep -q "^SYS_UID_MAX" /etc/login.defs || echo -e "SYS_UID_MAX\t\t 999" >> /etc/login.defs
grep -q "^SYS_GID_MAX" /etc/login.defs || echo -e "SYS_GID_MAX\t\t 999" >> /etc/login.defs

mkdir -p /root/.ssh "/home/$ADMIN_USER/.ssh" /etc/skel/.ssh /nonexistent/.ssh
chmod 700 /root/.ssh "/home/$ADMIN_USER/.ssh" /etc/skel/.ssh 2>/dev/null || true
chmod 755 /nonexistent/.ssh 2>/dev/null || true
touch /var/log/btmp && chmod 660 /var/log/btmp

mkdir -p /usr/local/sbin /usr/local/bin

cat << 'CHAGE_WRAP' > /usr/local/sbin/chage
#!/bin/bash
exec /usr/bin/env LC_ALL=C LANG=C /usr/bin/chage "$@"
CHAGE_WRAP
chmod 755 /usr/local/sbin/chage

cat << 'PASSWD_WRAP' > /usr/local/sbin/passwd
#!/bin/bash
if [ "$1" = "-S" ]; then
    exec /usr/bin/env LC_ALL=C LANG=C /usr/bin/passwd "$@"
fi
exec /usr/bin/passwd "$@"
PASSWD_WRAP
chmod 755 /usr/local/sbin/passwd

cat << 'LASTB_WRAP' > /usr/local/bin/lastb
#!/bin/bash
if [ -f /var/log/btmp ] && [ -s /var/log/btmp ]; then
    /usr/bin/last -f /var/log/btmp "$@" 2>/dev/null || echo "btmp begins $(date -Iseconds)"
else
    echo "btmp begins $(date -Iseconds)"
fi
LASTB_WRAP
chmod 755 /usr/local/bin/lastb
if command -v dpkg-divert &>/dev/null; then
    dpkg-divert --add --rename --divert /usr/bin/lastb.distrib /usr/bin/lastb 2>/dev/null || true
fi
ln -sf /usr/local/bin/lastb /usr/bin/lastb 2>/dev/null || true

cat << MOTD > /etc/motd

======================================================
  SERVIDOR EAD-COL ($SERVER_ROLE) - IP: $SERVER_IP
  * Panel Web   : https://${SERVER_IP}:9090
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
systemctl daemon-reload
if ! systemctl restart smbd nmbd wsdd2 cockpit.socket cockpit.service 2>/dev/null; then
    if ! systemctl restart smbd nmbd wsdd2 cockpit.socket 2>/dev/null; then
        advertir "No se pudieron reiniciar todos los servicios Samba/Cockpit."
    fi
fi
if ! systemctl enable smbd nmbd wsdd2 cockpit.socket 2>/dev/null; then
    advertir "No se pudieron habilitar todos los servicios Samba/Cockpit."
fi
if ! systemctl enable cron 2>/dev/null; then
    advertir "No se pudo habilitar el servicio cron."
fi
if ! systemctl start cron 2>/dev/null; then
    advertir "No se pudo iniciar el servicio cron."
fi

for _svc in smbd nmbd wsdd2 cockpit.socket cron; do
    if systemctl is-active "$_svc" &>/dev/null; then
        echo "  [OK]  $_svc activo"
    else
        echo "  [!]   $_svc NO está activo"
    fi
done

echo " [9/9] Verificando y asegurando reglas de Firewall (UFW)..."
if command -v ufw &>/dev/null && ufw status 2>/dev/null | grep -qw "active"; then
    ufw allow 22/tcp comment 'SSH' 2>/dev/null || true
    ufw allow 9090/tcp comment 'Cockpit Web Admin' 2>/dev/null || true
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
echo " Panel Web       : https://${SERVER_IP}:9090"
printf " Red Windows     : \\\\\\\\%s (o \\\\\\\\%s)\n" "${SERVER_IP}" "$SMB_NETBIOS"
echo "=============================================================================="
