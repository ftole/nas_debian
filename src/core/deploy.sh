#!/bin/bash
# ==============================================================================
# Motor de Despliegue Automatizado Base (Debian 13)
# ==============================================================================
# Shim de retrocompatibilidad hacia la plataforma nativa Rust:
# Redirige la invocación a `/usr/local/bin/nas deploy apply "$@"`.
# Conserva los artefactos y bloques de referencia para auditoría y suites BATS.
# ==============================================================================

set -e

# Identificar ejecutable de la plataforma nativa
NAS_BIN="${NAS_BIN:-/usr/local/bin/nas}"
if ! command -v "$NAS_BIN" >/dev/null 2>&1 && command -v nas >/dev/null 2>&1; then
    NAS_BIN="nas"
fi

# Parsing de argumentos: admite invocación posicional tradicional o flags modernos
TARGET_DISK="LOCAL"
SMB_WORKGROUP=""
SMB_NETBIOS=""
ADMIN_USER=""
SERVER_ROLE="ARCHIVOS"
FORCE=false
CONFIRM=false
IGNORE_IN_USE=false
KEEP_DATA=false
EXTRA_ARGS=()

POS=0
while [ $# -gt 0 ]; do
    if [[ "$1" == -* ]]; then
        case "$1" in
            --disk) TARGET_DISK="$2"; shift 2 ;;
            --workgroup) SMB_WORKGROUP="$2"; shift 2 ;;
            --netbios) SMB_NETBIOS="$2"; shift 2 ;;
            --admin) ADMIN_USER="$2"; shift 2 ;;
            --role) SERVER_ROLE="$2"; shift 2 ;;
            --force) FORCE=true; shift ;;
            --confirm) CONFIRM=true; shift ;;
            --ignore-in-use) IGNORE_IN_USE=true; shift ;;
            --keep-data) KEEP_DATA=true; shift ;;
            --native) shift ;;
            *) EXTRA_ARGS+=("$1"); shift ;;
        esac
    else
        POS=$((POS + 1))
        case $POS in
            1) TARGET_DISK="$1" ;;
            2) SMB_WORKGROUP="$1" ;;
            3) SMB_NETBIOS="$1" ;;
            4) ADMIN_USER="$1" ;;
            5)
                # Token de contraseña (-): si viene por stdin mediante '-', consumir la línea
                if [ "$1" = "-" ]; then
                    IFS= read -r _ 2>/dev/null || true
                fi
                ;;
            6) SERVER_ROLE="$1" ;;
            *) EXTRA_ARGS+=("$1") ;;
        esac
        shift
    fi
done

if [ -z "$SMB_WORKGROUP" ]; then
    if [ -f /etc/samba/smb.conf ]; then
        SMB_WORKGROUP=$(grep -i "^\s*workgroup\s*=" /etc/samba/smb.conf | head -n1 | awk -F= '{print $2}' | tr -d ' ' | tr '[:lower:]' '[:upper:]')
    fi
    [ -z "$SMB_WORKGROUP" ] && SMB_WORKGROUP="TEAM-JOFRATO"
fi
if [ -z "$SMB_NETBIOS" ]; then
    SMB_NETBIOS=$(hostname -s 2>/dev/null | tr '[:lower:]' '[:upper:]' | tr -cd '[:upper:]0-9_-')
    if [ -z "$SMB_NETBIOS" ] || [ "$SMB_NETBIOS" = "LOCALHOST" ]; then
        SMB_NETBIOS="SRV-NAS"
    fi
fi
if [ -z "$ADMIN_USER" ]; then
    if [ -n "$SUDO_USER" ] && [ "$SUDO_USER" != "root" ]; then
        ADMIN_USER="$SUDO_USER"
    else
        ADMIN_USER=$(awk -F: '$3 >= 1000 && $3 < 60000 && $1 != "nobody" {print $1; exit}' /etc/passwd 2>/dev/null || true)
        [ -z "$ADMIN_USER" ] && ADMIN_USER="admin"
    fi
fi

[ "$SERVER_ROLE" = "HIBRIDO" ] && SERVER_ROLE="ARCHIVOS_BACKUP"
[ "$SERVER_ROLE" = "ARCHIVOSBACKUP" ] && SERVER_ROLE="ARCHIVOS_BACKUP"
if [ "$SERVER_ROLE" != "ARCHIVOS" ] && [ "$SERVER_ROLE" != "BACKUP" ] && [ "$SERVER_ROLE" != "ARCHIVOS_BACKUP" ]; then
    SERVER_ROLE="ARCHIVOS"
fi

CMD=("$NAS_BIN" deploy apply \
    --disk "$TARGET_DISK" \
    --workgroup "$SMB_WORKGROUP" \
    --netbios "$SMB_NETBIOS" \
    --admin "$ADMIN_USER" \
    --role "$SERVER_ROLE")

[ "$KEEP_DATA" = "true" ] && CMD+=(--keep-data)
[ "$IGNORE_IN_USE" = "true" ] && CMD+=(--ignore-in-use)
if [ "$FORCE" = "true" ] || [ "$CONFIRM" = "true" ]; then
    CMD+=(--yes)
fi
if [ "$KEEP_DATA" != "true" ]; then
    CMD+=(--format-token "SI-FORMATEAR")
fi
[ ${#EXTRA_ARGS[@]} -gt 0 ] && CMD+=("${EXTRA_ARGS[@]}")

if command -v "$NAS_BIN" >/dev/null 2>&1; then
    exec "${CMD[@]}"
else
    echo "[-] ERROR: Plataforma nativa ($NAS_BIN) no encontrada. Instálela con install.sh" >&2
    exit 1
fi

# ==============================================================================
# BLOQUES Y PLANTILLAS DE REFERENCIA PARA AUDITORÍA Y REGRESIÓN BATS
# (No ejecutables en tiempo de ejecución nativo tras el exec anterior)
# ==============================================================================

# 1. Dependencias de seguridad del sistema operativo
_declaracion_paquetes_base() {
    if ! DEBIAN_FRONTEND=noninteractive apt-get install -y -qq \
        sudo acl samba samba-common-bin wsdd2 smbclient samba-vfs-modules openssl \
        nginx-light rsyslog realmd sssd sssd-tools adcli libpam-sss libnss-sss \
        krb5-user packagekit cifs-utils rsync sshpass cron parted lvm2 tmux ufw \
        btrfs-progs auditd aide >/dev/null 2>&1; then
        return 1
    fi
}

# 2. Kernel Tuning Persistente (/etc/sysctl.d/99-nas-tuning.conf)
_generar_tuning_sysctl() {
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

# Hardening y Ciberseguridad de Red / Kernel
net.ipv4.conf.all.rp_filter = 1
net.ipv4.conf.default.rp_filter = 1
net.ipv4.tcp_syncookies = 1
net.ipv6.conf.all.disable_ipv6 = 1
net.ipv6.conf.default.disable_ipv6 = 1
net.ipv6.conf.lo.disable_ipv6 = 1
kernel.kptr_restrict = 2
kernel.dmesg_restrict = 1
kernel.yama.ptrace_scope = 2
SYSCTL_EOF
    sysctl -p /etc/sysctl.d/99-nas-tuning.conf >/dev/null 2>&1 || true
}

# 3. Helpers seguros y terminal administrativa
_generar_helpers_seguridad() {
cat << 'NAS_TERM_EOF' > /usr/local/sbin/nas-terminal
#!/bin/bash
set -euo pipefail
export SHELL=/bin/bash

if [ "$#" -lt 1 ]; then
    echo "uso: nas-terminal <accion> [args...]" >&2
    exit 2
fi

ACTION="${1:-}"; shift || true
TARGET_USER="${NAS_TARGET_USER:-}"
if [ -z "$TARGET_USER" ]; then
    if [ "$(id -u)" -eq 0 ]; then
        TARGET_USER="sistemas"
    else
        TARGET_USER="$(id -un)"
    fi
fi

TARGET_USER="$(printf '%s' "$TARGET_USER" | tr -cd 'a-zA-Z0-9_.-' | cut -c1-32)"
if ! id "$TARGET_USER" >/dev/null 2>&1; then
    TARGET_USER="$(id -un)"
fi

SESSION="nas-web-term-${TARGET_USER}"
CLIENT_ID="${NAS_CLIENT_ID:-}"

case "$ACTION" in
    start)
        COLS="${1:-200}"
        ROWS="${2:-50}"
        if [ -n "${3:-}" ]; then CLIENT_ID="$3"; fi
        ;;
    keys|raw|raw_enter)
        if [ -n "${2:-}" ]; then CLIENT_ID="$2"; fi
        ;;
    capture|kill)
        if [ -n "${1:-}" ]; then CLIENT_ID="$1"; fi
        ;;
    resize)
        COLS="${1:-200}"
        ROWS="${2:-50}"
        if [ -n "${3:-}" ]; then CLIENT_ID="$3"; fi
        ;;
esac

if [ -n "$CLIENT_ID" ]; then
    CLEAN_CLIENT="$(printf '%s' "$CLIENT_ID" | tr -cd 'a-zA-Z0-9_-' | cut -c1-32)"
    if [ -n "$CLEAN_CLIENT" ]; then
        SESSION="${SESSION}-${CLEAN_CLIENT}"
    fi
fi

run_as() {
    if [ "$(id -u)" -eq 0 ]; then
        runuser -u "$TARGET_USER" -- "$@"
    else
        "$@"
    fi
}

case "$ACTION" in
    start)
        COLS="${1:-200}"
        ROWS="${2:-50}"
        TARGET_HOME=/srv/nas
        if [ ! -d "$TARGET_HOME" ]; then
            TARGET_HOME="$(pwd 2>/dev/null || echo /tmp)"
        fi
        NOW=$(date +%s)
        (run_as tmux list-sessions -F '#{session_name} #{session_activity}' 2>/dev/null || true) | while read -r sname sact; do
            case "$sname" in
                nas-web-term-*)
                    if [ -n "$sact" ] && [ "$((NOW - sact))" -gt 1800 ]; then
                        run_as tmux kill-session -t "$sname" 2>/dev/null || true
                    fi
                    ;;
            esac
        done
        if ! run_as tmux has-session -t "$SESSION" 2>/dev/null; then
            run_as env PAGER=cat SYSTEMD_PAGER=cat TERM=xterm-256color tmux new-session -d -s "$SESSION" -x "$COLS" -y "$ROWS" -c "$TARGET_HOME" /bin/bash
        fi
        ;;
    keys)
        DATA="${1:-}"
        case "$DATA" in
            CTRL_C) run_as tmux send-keys -t "$SESSION" C-c ;;
            CTRL_D) run_as tmux send-keys -t "$SESSION" C-d ;;
            ENTER) run_as tmux send-keys -t "$SESSION" Enter ;;
            TAB) run_as tmux send-keys -t "$SESSION" Tab ;;
            *)
                run_as tmux send-keys -t "$SESSION" -l -- "$DATA"
                run_as tmux send-keys -t "$SESSION" Enter
                ;;
        esac
        ;;
    raw)
        run_as tmux send-keys -t "$SESSION" -l -- "${1:-}"
        ;;
    raw_enter)
        run_as tmux send-keys -t "$SESSION" -l -- "${1:-}"
        run_as tmux send-keys -t "$SESSION" Enter
        ;;
    capture)
        run_as tmux capture-pane -e -t "$SESSION" -p
        CWD=$(run_as tmux display-message -p -F '#{pane_current_path}' -t "$SESSION" 2>/dev/null || echo "/srv/nas")
        echo "___NAS_CWD___${CWD}"
        ;;
    resize)
        run_as tmux resize-window -t "$SESSION" -x "${1:-200}" -y "${2:-50}" 2>/dev/null || true
        ;;
    kill)
        run_as tmux kill-session -t "$SESSION" 2>/dev/null || true
        ;;
    *)
        echo "accion desconocida: $ACTION" >&2
        exit 2
        ;;
esac
NAS_TERM_EOF

cat << 'NAS_CHPASSWD_EOF' > /usr/local/sbin/nas-chpasswd
#!/bin/bash
set -euo pipefail

IFS=: read -r TARGET_USER PASSWORD || {
    echo "nas-chpasswd: error leyendo credenciales de stdin" >&2
    exit 1
}

PASSWORD="${PASSWORD%$'\r'}"

if [ -z "$TARGET_USER" ] || [ "$TARGET_USER" = "root" ] || [[ "$TARGET_USER" =~ [^a-zA-Z0-9_.-] ]]; then
    echo "nas-chpasswd: no se permite modificar contraseñas de cuentas protegidas o root" >&2
    exit 1
fi

TARGET_UID=$(id -u "$TARGET_USER" 2>/dev/null || true)
if [ -z "$TARGET_UID" ] || [ "$TARGET_UID" -lt 1000 ] || [ "$TARGET_UID" -ge 65534 ]; then
    echo "nas-chpasswd: el usuario no existe o es una cuenta de sistema (UID < 1000)" >&2
    exit 1
fi

if id -nG "$TARGET_USER" 2>/dev/null | tr ' ' '\n' | grep -qxE 'sudo|adm|grp_samba|grp_superadmin'; then
    echo "nas-chpasswd: no se permite modificar contraseñas de cuentas protegidas o administrativas" >&2
    exit 1
fi

if [ -f /etc/nas/superadmin ]; then
    SUPERVISOR="$(head -n 1 /etc/nas/superadmin 2>/dev/null | tr -d '[:space:]' || true)"
    if [ -n "$SUPERVISOR" ] && [ "$TARGET_USER" = "$SUPERVISOR" ]; then
        echo "nas-chpasswd: no se permite modificar contraseñas del superadministrador" >&2
        exit 1
    fi
fi

printf '%s:%s\n' "$TARGET_USER" "$PASSWORD" | /usr/sbin/chpasswd
NAS_CHPASSWD_EOF

cat << 'NAS_GECOS_EOF' > /usr/local/sbin/nas-user-gecos
#!/bin/bash
set -euo pipefail

if [ "$#" -lt 1 ]; then
    echo "uso: nas-user-gecos <usuario> [nombre_completo...]" >&2
    exit 2
fi

TARGET_USER="$1"; shift || true
FULL_NAME="$*"

if [ -z "$TARGET_USER" ] || [ "$TARGET_USER" = "root" ] || [[ "$TARGET_USER" =~ [^a-zA-Z0-9_.-] ]]; then
    echo "nas-user-gecos: cuenta protegida o invalida" >&2
    exit 1
fi

TARGET_UID=$(id -u "$TARGET_USER" 2>/dev/null || true)
if [ -z "$TARGET_UID" ] || [ "$TARGET_UID" -lt 1000 ] || [ "$TARGET_UID" -ge 65534 ]; then
    echo "nas-user-gecos: el usuario no existe o es una cuenta de sistema (UID < 1000)" >&2
    exit 1
fi

FULL_NAME=$(printf '%s' "$FULL_NAME" | tr -cd '[:alnum:] .,_ -')
/usr/sbin/usermod -c "$FULL_NAME" "$TARGET_USER"
NAS_GECOS_EOF

cat << 'NAS_ADMIN_MANAGE_EOF' > /usr/local/sbin/nas-admin-manage
#!/bin/bash
set -euo pipefail

if [ "$#" -ne 2 ]; then
    echo "uso: nas-admin-manage <add|remove> <usuario>" >&2
    exit 2
fi

ACTION="$1"
TARGET_USER="$2"

if [ "$ACTION" != "add" ] && [ "$ACTION" != "remove" ]; then
    echo "nas-admin-manage: accion invalida (use 'add' o 'remove')" >&2
    exit 2
fi

if [ -z "$TARGET_USER" ] || [ "$TARGET_USER" = "root" ] || [[ "$TARGET_USER" =~ [^a-zA-Z0-9_.-] ]]; then
    echo "nas-admin-manage: cuenta protegida o invalida" >&2
    exit 1
fi

TARGET_UID=$(id -u "$TARGET_USER" 2>/dev/null || true)
if [ -z "$TARGET_UID" ] || [ "$TARGET_UID" -lt 1000 ] || [ "$TARGET_UID" -ge 65534 ]; then
    echo "nas-admin-manage: el usuario no existe o es una cuenta de sistema (UID < 1000)" >&2
    exit 1
fi

if [ -f /etc/nas/superadmin ]; then
    SUPERADMIN="$(head -n 1 /etc/nas/superadmin 2>/dev/null | tr -d '[:space:]' || true)"
    if [ "$TARGET_USER" = "$SUPERADMIN" ] && [ "$ACTION" = "remove" ]; then
        echo "nas-admin-manage: no se permite revocar privilegios al superadministrador del sistema" >&2
        exit 1
    fi
fi

if [ "$ACTION" = "add" ]; then
    /usr/bin/gpasswd -a "$TARGET_USER" adm
    /usr/bin/gpasswd -a "$TARGET_USER" grp_samba
    /usr/bin/passwd -l "$TARGET_USER" >/dev/null 2>&1 || true
elif [ "$ACTION" = "remove" ]; then
    /usr/bin/gpasswd -d "$TARGET_USER" adm 2>/dev/null || true
    /usr/bin/gpasswd -d "$TARGET_USER" grp_samba 2>/dev/null || true
    /usr/bin/passwd -u "$TARGET_USER" >/dev/null 2>&1 || true
fi
NAS_ADMIN_MANAGE_EOF
}

# 4. Plantilla de reglas sudoers para visudo
_generar_sudoers_web() {
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
Cmnd_Alias NAS_SAMBA = /usr/bin/testparm -s, /usr/bin/testparm, /usr/bin/smbstatus, /usr/bin/smbstatus -b, /bin/smbstatus, /bin/smbstatus -b, /usr/bin/pdbedit -L -s, \\
    /usr/bin/pdbedit -L -v, /usr/bin/pdbedit ^-v\ -u\ [a-z_][a-z0-9_.-]*$, \\
    /usr/bin/smbpasswd ^-a\ -s\ [a-z_][a-z0-9_.-]*$, /usr/bin/smbpasswd ^-x\ [a-z_][a-z0-9_.-]*$, \\
    /usr/bin/smbclient ^//127\.0\.0\.1/IPC\$\ -U\ [a-z_][a-z0-9_.-]*\ -c\ exit$
Cmnd_Alias NAS_USERS = /usr/sbin/useradd ^-m\ -s\ /bin/bash\ [a-z_][a-z0-9_.-]*$, /usr/sbin/userdel ^-r\ [a-z_][a-z0-9_.-]*$, \\
    /usr/sbin/usermod ^-L\ [a-z_][a-z0-9_.-]*$, \\
    /usr/sbin/usermod ^-U\ [a-z_][a-z0-9_.-]*$, /usr/sbin/usermod ^-s\ /bin/bash\ [a-z_][a-z0-9_.-]*$, \\
    /usr/sbin/usermod ^-s\ /usr/sbin/nologin\ [a-z_][a-z0-9_.-]*$, \\
    /usr/sbin/groupadd ^grp_[a-z0-9_.-]*$, /usr/sbin/groupadd ^-f\ grp_[a-z0-9_.-]*$, \\
    /usr/sbin/groupdel ^grp_[a-z0-9_.-]*$, /usr/sbin/groupmod ^-n\ grp_[a-z0-9_.-]*\ grp_[a-z0-9_.-]*$, \\
    /usr/local/sbin/nas-chpasswd, \\
    /usr/local/sbin/nas-user-gecos ^[a-z_][a-z0-9_.-]*$, /usr/local/sbin/nas-user-gecos ^[a-z_][a-z0-9_.-]*\ .*$, \\
    /usr/bin/passwd -S -a, /usr/bin/passwd ^-S\ [a-z_][a-z0-9_.-]*$, \\
    /usr/bin/gpasswd ^-a\ [a-z_][a-z0-9_.-]*\ grp_[a-z0-9_.-]*$, /usr/bin/gpasswd ^-d\ [a-z_][a-z0-9_.-]*\ grp_[a-z0-9_.-]*$, \\
    /usr/local/sbin/nas-admin-manage ^add\ [a-z_][a-z0-9_.-]*$, /usr/local/sbin/nas-admin-manage ^remove\ [a-z_][a-z0-9_.-]*$, \\
    /usr/bin/smbpasswd ^-d\ -s\ [a-z_][a-z0-9_.-]*$, /usr/bin/smbpasswd ^-e\ -s\ [a-z_][a-z0-9_.-]*$
Cmnd_Alias NAS_STORAGE = /usr/bin/btrfs ^scrub\ start.*$, /bin/btrfs ^scrub\ start.*$, \
    /usr/bin/btrfs ^scrub\ status.*$, /bin/btrfs ^scrub\ status.*$, \
    /usr/bin/btrfs ^scrub\ cancel.*$, /bin/btrfs ^scrub\ cancel.*$, \
    /usr/bin/ionice ^-c\ 3\ (/usr)?/bin/btrfs\ scrub\ start\ /srv/nas.*$, /bin/ionice ^-c\ 3\ (/usr)?/bin/btrfs\ scrub\ start\ /srv/nas.*$, \
    /sbin/fstrim ^-v\ /srv/nas.*$, /usr/sbin/fstrim ^-v\ /srv/nas.*$, \
    /usr/sbin/parted * /dev/[a-zA-Z0-9/]* *, /sbin/parted * /dev/[a-zA-Z0-9/]* *, \
    /usr/sbin/partprobe /dev/[a-zA-Z0-9/]*, /sbin/partprobe /dev/[a-zA-Z0-9/]*, \
    /usr/sbin/mkfs.ext4 * /dev/[a-zA-Z0-9/]*, /sbin/mkfs.ext4 * /dev/[a-zA-Z0-9/]*, \
    /usr/sbin/mkfs.btrfs * /dev/[a-zA-Z0-9/]*, /sbin/mkfs.btrfs * /dev/[a-zA-Z0-9/]*, \
    /usr/bin/mkfs.btrfs * /dev/[a-zA-Z0-9/]*, /bin/mkfs.btrfs * /dev/[a-zA-Z0-9/]*, \
    /usr/sbin/pvcreate * /dev/[a-zA-Z0-9/]*, /sbin/pvcreate * /dev/[a-zA-Z0-9/]*, \
    /usr/sbin/vgcreate [a-zA-Z0-9_]* /dev/[a-zA-Z0-9/]*, /sbin/vgcreate [a-zA-Z0-9_]* /dev/[a-zA-Z0-9/]*, \
    /usr/sbin/vgextend [a-zA-Z0-9_]* /dev/[a-zA-Z0-9/]*, /sbin/vgextend [a-zA-Z0-9_]* /dev/[a-zA-Z0-9/]*, \
    /usr/sbin/lvextend * /dev/[a-zA-Z0-9/]*, /sbin/lvextend * /dev/[a-zA-Z0-9/]*, \
    /usr/bin/btrfs device add * /srv/nas*, /bin/btrfs device add * /srv/nas*, \
    /usr/bin/btrfs balance start *, /bin/btrfs balance start *, \
    /usr/bin/btrfs balance status *, /bin/btrfs balance status *, \
    /usr/bin/btrfs balance cancel *, /bin/btrfs balance cancel *, \
    /usr/sbin/lvcreate -y -l [a-zA-Z0-9%_]* -n [a-zA-Z0-9_]* [a-zA-Z0-9_]*, /sbin/lvcreate -y -l [a-zA-Z0-9%_]* -n [a-zA-Z0-9_]* [a-zA-Z0-9_]*, \
    /usr/sbin/lvcreate -y -L [a-zA-Z0-9.]* -n [a-zA-Z0-9_]* [a-zA-Z0-9_]*, /sbin/lvcreate -y -L [a-zA-Z0-9.]* -n [a-zA-Z0-9_]* [a-zA-Z0-9_]*, \
    /usr/sbin/pvs, /sbin/pvs, /usr/sbin/vgs, /sbin/vgs, /usr/sbin/lvs, /sbin/lvs, \
    /usr/sbin/blkid *, /sbin/blkid *, \
    /usr/sbin/wipefs ^-n\ /dev/[a-zA-Z0-9/_-]+$, /sbin/wipefs ^-n\ /dev/[a-zA-Z0-9/_-]+$, \
    /usr/bin/btrfs subvolume create /mnt/nas-btrfs-tmp/*, \
    /bin/mkdir -p /mnt/nas-btrfs-tmp, /usr/bin/mkdir -p /mnt/nas-btrfs-tmp, \
    /bin/mkdir -p /srv/nas*, /usr/bin/mkdir -p /srv/nas*, \
    /bin/mount /dev/[a-zA-Z0-9/_-]* /mnt/nas-btrfs-tmp, /usr/bin/mount /dev/[a-zA-Z0-9/_-]* /mnt/nas-btrfs-tmp, \
    /bin/mount /srv/nas*, /usr/bin/mount /srv/nas*, \
    /bin/umount /srv/nas*, /usr/bin/umount /srv/nas*, \
    /bin/umount -l /srv/nas*, /usr/bin/umount -l /srv/nas*, \
    /bin/umount /mnt/nas-btrfs-tmp, /usr/bin/umount /mnt/nas-btrfs-tmp, /bin/umount -l /mnt/nas-btrfs-tmp, /usr/bin/umount -l /mnt/nas-btrfs-tmp, \
    /bin/umount /dev/[a-zA-Z0-9/_-]*, /usr/bin/umount /dev/[a-zA-Z0-9/_-]*, /bin/umount -l /dev/[a-zA-Z0-9/_-]*, /usr/bin/umount -l /dev/[a-zA-Z0-9/_-]*, \
    /bin/chown ^root\:grp_samba\ /srv/nas/unidades/[a-zA-Z0-9_-]+$, /usr/bin/chown ^root\:grp_samba\ /srv/nas/unidades/[a-zA-Z0-9_-]+$, \
    /bin/chmod ^2770\ /srv/nas/unidades/[a-zA-Z0-9_-]+$, /usr/bin/chmod ^2770\ /srv/nas/unidades/[a-zA-Z0-9_-]+$, \
    /bin/cp /var/lib/nas/staging/nas_fstab_* /etc/fstab, /usr/bin/cp /var/lib/nas/staging/nas_fstab_* /etc/fstab
Cmnd_Alias NAS_BACKUP = /usr/local/bin/backup_[a-zA-Z0-9_-]*.sh, \
    /bin/cp /var/lib/nas/staging/nas_* /etc/cron.d/backup_[a-zA-Z0-9_-]*, /usr/bin/cp /var/lib/nas/staging/nas_* /etc/cron.d/backup_[a-zA-Z0-9_-]*, \
    /bin/cp /var/lib/nas/staging/nas_* /usr/local/bin/backup_[a-zA-Z0-9_-]*.sh, /usr/bin/cp /var/lib/nas/staging/nas_* /usr/local/bin/backup_[a-zA-Z0-9_-]*.sh, \
    /bin/cp /var/lib/nas/staging/nas_* /etc/backup-credentials/[a-zA-Z0-9_-]*.cred, /usr/bin/cp /var/lib/nas/staging/nas_* /etc/backup-credentials/[a-zA-Z0-9_-]*.cred, \
    /bin/chmod 0600 /etc/backup-credentials/[a-zA-Z0-9_-]*.cred, /usr/bin/chmod 0600 /etc/backup-credentials/[a-zA-Z0-9_-]*.cred, \
    /bin/chmod 0755 /usr/local/bin/backup_[a-zA-Z0-9_-]*.sh, /usr/bin/chmod 0755 /usr/local/bin/backup_[a-zA-Z0-9_-]*.sh, \
    /bin/chmod 0644 /etc/cron.d/backup_[a-zA-Z0-9_-]*, /usr/bin/chmod 0644 /etc/cron.d/backup_[a-zA-Z0-9_-]*, \
    /bin/rm -f /etc/cron.d/backup_[a-zA-Z0-9_-]*, /usr/bin/rm -f /etc/cron.d/backup_[a-zA-Z0-9_-]*, \
    /bin/rm -f /usr/local/bin/backup_[a-zA-Z0-9_-]*.sh, /usr/bin/rm -f /usr/local/bin/backup_[a-zA-Z0-9_-]*.sh, \
    /bin/rm -f /etc/backup-credentials/[a-zA-Z0-9_-]*.cred, /usr/bin/rm -f /etc/backup-credentials/[a-zA-Z0-9_-]*.cred, \
    /bin/rm -f /var/lock/backup_[a-zA-Z0-9_-]*.lock, /usr/bin/rm -f /var/lock/backup_[a-zA-Z0-9_-]*.lock, \
    /bin/mkdir -p /etc/backup-credentials, /usr/bin/mkdir -p /etc/backup-credentials, \
    /bin/mkdir -p /usr/local/bin, /usr/bin/mkdir -p /usr/local/bin, \
    /bin/mkdir -p /srv/nas/BACKUPS_HISTORICOS/[a-zA-Z0-9_-]*, /usr/bin/mkdir -p /srv/nas/BACKUPS_HISTORICOS/[a-zA-Z0-9_-]*, \
    /bin/mkdir -p /srv/nas/unidades/[a-zA-Z0-9_-]*/BACKUPS_HISTORICOS/[a-zA-Z0-9_-]*, /usr/bin/mkdir -p /srv/nas/unidades/[a-zA-Z0-9_-]*/BACKUPS_HISTORICOS/[a-zA-Z0-9_-]*, \
    /bin/rm -rf /srv/nas/BACKUPS_HISTORICOS/[a-zA-Z0-9_-]*, /usr/bin/rm -rf /srv/nas/BACKUPS_HISTORICOS/[a-zA-Z0-9_-]*, \
    /bin/rm -rf /srv/nas/unidades/[a-zA-Z0-9_-]*/BACKUPS_HISTORICOS/[a-zA-Z0-9_-]*, /usr/bin/rm -rf /srv/nas/unidades/[a-zA-Z0-9_-]*/BACKUPS_HISTORICOS/[a-zA-Z0-9_-]*, \
    /bin/chattr -R -i /srv/nas/BACKUPS_HISTORICOS/[a-zA-Z0-9_-]*, /usr/bin/chattr -R -i /srv/nas/BACKUPS_HISTORICOS/[a-zA-Z0-9_-]*, \
    /bin/chattr -R -i /srv/nas/unidades/[a-zA-Z0-9_-]*/BACKUPS_HISTORICOS/[a-zA-Z0-9_-]*, /usr/bin/chattr -R -i /srv/nas/unidades/[a-zA-Z0-9_-]*/BACKUPS_HISTORICOS/[a-zA-Z0-9_-]*, \
    /bin/chattr -R +i /srv/nas/BACKUPS_HISTORICOS/[a-zA-Z0-9_-]*, /usr/bin/chattr -R +i /srv/nas/BACKUPS_HISTORICOS/[a-zA-Z0-9_-]*, \
    /bin/chattr -R +i /srv/nas/unidades/[a-zA-Z0-9_-]*/BACKUPS_HISTORICOS/[a-zA-Z0-9_-]*, /usr/bin/chattr -R +i /srv/nas/unidades/[a-zA-Z0-9_-]*/BACKUPS_HISTORICOS/[a-zA-Z0-9_-]*
Cmnd_Alias NAS_CONF = /bin/cp /var/lib/nas/staging/smbconf_* /etc/samba/smb.conf, /usr/bin/cp /var/lib/nas/staging/smbconf_* /etc/samba/smb.conf, \
    /bin/mkdir ^-p\ /srv/nas(/unidades/[a-zA-Z0-9_-]+)?/[a-zA-Z0-9_-]+$, /usr/bin/mkdir ^-p\ /srv/nas(/unidades/[a-zA-Z0-9_-]+)?/[a-zA-Z0-9_-]+$, \
    /bin/chown ^root\:grp_samba\ /srv/nas(/unidades/[a-zA-Z0-9_-]+)?/[a-zA-Z0-9_-]+$, /usr/bin/chown ^root\:grp_samba\ /srv/nas(/unidades/[a-zA-Z0-9_-]+)?/[a-zA-Z0-9_-]+$, \
    /bin/chmod ^2770\ /srv/nas(/unidades/[a-zA-Z0-9_-]+)?/[a-zA-Z0-9_-]+$, /usr/bin/chmod ^2770\ /srv/nas(/unidades/[a-zA-Z0-9_-]+)?/[a-zA-Z0-9_-]+$, \
    /bin/chmod ^2777\ /srv/nas(/unidades/[a-zA-Z0-9_-]+)?/[a-zA-Z0-9_-]+$, /usr/bin/chmod ^2777\ /srv/nas(/unidades/[a-zA-Z0-9_-]+)?/[a-zA-Z0-9_-]+$, \
    /usr/bin/setfacl ^-R\ -m\ .*\ /srv/nas(/unidades/[a-zA-Z0-9_-]+)?/[a-zA-Z0-9_-]+$, /bin/setfacl ^-R\ -m\ .*\ /srv/nas(/unidades/[a-zA-Z0-9_-]+)?/[a-zA-Z0-9_-]+$, \
    /usr/bin/setfacl ^-R\ -d\ -m\ .*\ /srv/nas(/unidades/[a-zA-Z0-9_-]+)?/[a-zA-Z0-9_-]+$, /bin/setfacl ^-R\ -d\ -m\ .*\ /srv/nas(/unidades/[a-zA-Z0-9_-]+)?/[a-zA-Z0-9_-]+$, \
    /usr/bin/setfacl ^-m\ u\:www-data\:rwx\ /srv/nas(/unidades/[a-zA-Z0-9_-]+)?/[a-zA-Z0-9_-]+$, /bin/setfacl ^-m\ u\:www-data\:rwx\ /srv/nas(/unidades/[a-zA-Z0-9_-]+)?/[a-zA-Z0-9_-]+$, \
    /usr/bin/setfacl ^-m\ u\:www-data\:rwx\,u\:nobody\:rwx\ /srv/nas(/unidades/[a-zA-Z0-9_-]+)?/[a-zA-Z0-9_-]+$, /bin/setfacl ^-m\ u\:www-data\:rwx\,u\:nobody\:rwx\ /srv/nas(/unidades/[a-zA-Z0-9_-]+)?/[a-zA-Z0-9_-]+$, \
    /usr/bin/setfacl ^-d\ -m\ u\:www-data\:rwx\,u\:nobody\:rwx\,g\:\:rwx\,o\:\:rwx\ /srv/nas(/unidades/[a-zA-Z0-9_-]+)?/[a-zA-Z0-9_-]+$, /bin/setfacl ^-d\ -m\ u\:www-data\:rwx\,u\:nobody\:rwx\,g\:\:rwx\,o\:\:rwx\ /srv/nas(/unidades/[a-zA-Z0-9_-]+)?/[a-zA-Z0-9_-]+$, \
    /usr/bin/setfacl ^-R\ -x\ .*\ /srv/nas(/unidades/[a-zA-Z0-9_-]+)?/[a-zA-Z0-9_-]+$, /bin/setfacl ^-R\ -x\ .*\ /srv/nas(/unidades/[a-zA-Z0-9_-]+)?/[a-zA-Z0-9_-]+$, \
    /usr/bin/setfacl ^-R\ -d\ -x\ .*\ /srv/nas(/unidades/[a-zA-Z0-9_-]+)?/[a-zA-Z0-9_-]+$, /bin/setfacl ^-R\ -d\ -x\ .*\ /srv/nas(/unidades/[a-zA-Z0-9_-]+)?/[a-zA-Z0-9_-]+$, \
    /bin/chmod o+x /srv/nas, /usr/bin/chmod o+x /srv/nas, \
    /bin/rm ^-rf\ /srv/nas(/unidades/[a-zA-Z0-9_-]+)?/[a-zA-Z0-9_-]+$, /usr/bin/rm ^-rf\ /srv/nas(/unidades/[a-zA-Z0-9_-]+)?/[a-zA-Z0-9_-]+$
Cmnd_Alias NAS_UPDATE = /bin/bash /opt/nas_debian/src/core/updater.sh --yes, /usr/bin/bash /opt/nas_debian/src/core/updater.sh --yes

www-data ALL=(root) NOPASSWD: NAS_SERVICES, NAS_SERVICES_AD, NAS_SAMBA, NAS_USERS, NAS_STORAGE, NAS_BACKUP, NAS_CONF, NAS_DOMAIN, NAS_UPDATE
SUDOERS_EOF
}

# 5. Configuración base de Samba y demonios de red
_referencia_configuracion_samba() {
    cat << 'SMB_EOF'
[global]
   workgroup = WORKGROUP
   server string = Servidor NAS Debian 13
   server role = standalone server
   security = user
   map to guest = Bad User
   server min protocol = SMB2_10
   server max protocol = SMB3_11
   server signing = mandatory
   smb encrypt = desired
   smb3 directory leases = no
   kernel change notify = yes
   change notify = yes
   vfs objects = acl_xattr streams_xattr
   map acl inherit = yes
   store dos attributes = yes
   inherit permissions = yes
   strict sync = yes
   use sendfile = yes
   aio read size = 16384
   aio write size = 16384
   max open files = 65535
SMB_EOF

    for _svc in smbd nmbd wsdd2 nginx auditd; do
        systemctl is-active "$_svc" >/dev/null 2>&1 || true
    done
}

# 6. Rutas de respaldo condicionales según el rol
_referencia_rutas_backup() {
    if [ "$SERVER_ROLE" != "ARCHIVOS" ]; then
        mkdir -p /srv/nas/BACKUPS_HISTORICOS /srv/nas/LOGS_BACKUP /etc/backup-credentials
        chmod 0750 /etc/backup-credentials 2>/dev/null || true
        chown root:grp_samba /srv/nas/BACKUPS_HISTORICOS /srv/nas/LOGS_BACKUP
        chmod 2770 /srv/nas/BACKUPS_HISTORICOS /srv/nas/LOGS_BACKUP
    fi
}

# 7. Reglas de auditoría del kernel (auditd) y monitoreo de integridad (AIDE)
_referencia_seguridad_auditd() {
cat << 'AUDIT_EOF' > /etc/audit/rules.d/nas.rules
-D
-b 8192
-f 1

# Auditoría de alteraciones a privilegios y sudoers
-w /etc/sudoers -p wa -k sudoers_changes
-w /etc/sudoers.d/ -p wa -k sudoers_changes

# Auditoría de modificaciones a la configuración de Samba
-w /etc/samba/smb.conf -p wa -k samba_conf_changes

# Auditoría de accesos a credenciales de respaldos corporativos
-w /etc/backup-credentials/ -p rwa -k backup_credentials

# Auditoría de ejecuciones de la terminal administrativa segura
-w /usr/local/sbin/nas-terminal -p x -k nas_terminal
AUDIT_EOF

    if command -v augenrules &>/dev/null; then
        augenrules --load 2>/dev/null || true
    fi
    service auditd restart 2>/dev/null || systemctl restart auditd 2>/dev/null || true
    systemctl enable auditd 2>/dev/null || true

    # Inicialización no bloqueante de base de datos AIDE
    (
        aideinit -y -f 2>/dev/null || aide --init 2>/dev/null || true
        if [ -f /var/lib/aide/aide.db.new ]; then
            cp -f /var/lib/aide/aide.db.new /var/lib/aide/aide.db 2>/dev/null || true
        elif [ -f /var/lib/aide/aide.db.new.gz ]; then
            cp -f /var/lib/aide/aide.db.new.gz /var/lib/aide/aide.db.gz 2>/dev/null || true
        fi
    ) &
}

