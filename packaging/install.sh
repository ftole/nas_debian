#!/bin/bash
# ==============================================================================
# Instalador del panel NAS en Rust (nas-web + CLI).
# Idempotente: puede ejecutarse varias veces sin duplicar configuración.
# Uso: sudo bash packaging/install.sh [--prefix DIR] [--no-nginx]
# ==============================================================================
set -euo pipefail

PREFIX="${PREFIX:-/usr/local/bin}"
ENV_FILE="/etc/nas-web.env"
UNIT_FILE="/etc/systemd/system/nas-web.service"
NGINX_SITE="/etc/nginx/sites-available/nas-web"
SUDOERS_FILE="/etc/sudoers.d/nas-web-rust"
STATE_DIR="/var/lib/nas"
DATA_ROOT="/srv/nas"
SERVICE_USER="nas-web"
CONFIGURE_NGINX=1

while [ $# -gt 0 ]; do
    case "$1" in
        --prefix) PREFIX="${2:?falta directorio}"; shift 2 ;;
        --no-nginx) CONFIGURE_NGINX=0; shift ;;
        -h|--help) sed -n '2,6p' "$0"; exit 0 ;;
        *) echo "opción desconocida: $1" >&2; exit 2 ;;
    esac
done

[ "$(id -u)" -eq 0 ] || { echo "[-] ejecute como root" >&2; exit 1; }

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
BUILD_DIR="${BUILD_DIR:-$REPO_DIR/target/release}"

if [ ! -d "$BUILD_DIR" ] || [ ! -x "$BUILD_DIR/nas" ]; then
    if [ -d "$REPO_DIR/bin" ] && [ -x "$REPO_DIR/bin/nas" ]; then
        BUILD_DIR="$REPO_DIR/bin"
    elif [ -x "$PREFIX/nas" ]; then
        BUILD_DIR="$PREFIX"
    fi
fi

for bin in nas nas-web nas-tui nasctl nas-dev; do
    if [ ! -x "$BUILD_DIR/$bin" ]; then
        echo "[-] falta el binario $BUILD_DIR/$bin (ejecute: cargo build --release --workspace)" >&2
        exit 1
    fi
done

echo "[1/7] Instalando binarios en $PREFIX"
install -d -m 0755 "$PREFIX"
for bin in nas nas-web nas-tui nasctl nas-dev; do
    install -m 0755 "$BUILD_DIR/$bin" "$PREFIX/$bin"
done
rm -f "$PREFIX/nas-cli"
ln -sf "$PREFIX/nas" "$PREFIX/nas-cli"

echo "[2/7] Creando usuario de servicio y directorios de estado"
if ! id -u "$SERVICE_USER" >/dev/null 2>&1; then
    useradd --system --home-dir "$STATE_DIR" --shell /usr/sbin/nologin "$SERVICE_USER"
fi

# Grupos necesarios: grp_samba para operar sobre /srv/nas y systemd-journal para bitácoras.
groupadd -f grp_samba >/dev/null 2>&1 || true
groupadd -f grp_web >/dev/null 2>&1 || true
groupadd -f grp_superadmin >/dev/null 2>&1 || true
usermod -aG grp_samba "$SERVICE_USER" >/dev/null 2>&1 || true
usermod -aG systemd-journal "$SERVICE_USER" >/dev/null 2>&1 || true

install -d -m 0750 -o "$SERVICE_USER" -g "$SERVICE_USER" "$STATE_DIR"
chown "$SERVICE_USER:$SERVICE_USER" "$STATE_DIR" 2>/dev/null || true
chmod 0750 "$STATE_DIR" 2>/dev/null || true

install -d -m 2770 -o "$SERVICE_USER" -g "$SERVICE_USER" "$STATE_DIR/audit" 2>/dev/null || true
chown -R "$SERVICE_USER:$SERVICE_USER" "$STATE_DIR/audit" 2>/dev/null || true

if [ -f "$STATE_DIR/nas.sqlite" ]; then
    chown "$SERVICE_USER:$SERVICE_USER" "$STATE_DIR"/nas.sqlite* 2>/dev/null || true
    chmod 0640 "$STATE_DIR"/nas.sqlite* 2>/dev/null || true
fi

install -d -m 2770 -o root -g grp_samba "$DATA_ROOT" "$DATA_ROOT/BACKUPS_HISTORICOS" "$DATA_ROOT/BACKUPS_PERMANENTES" "$DATA_ROOT/LOGS_BACKUP" 2>/dev/null || true
install -d -m 0755 /etc/nas
install -d -m 0700 -o root -g root /etc/backup-credentials
install -d -m 0755 /etc/cron.d
ln -sf "$ENV_FILE" /etc/nas/nas-web.env
if [ -f /etc/nas/role ]; then
    chmod 0644 /etc/nas/role 2>/dev/null || true
    chown root:root /etc/nas/role 2>/dev/null || true
fi

echo "[3/7] Configurando variables de entorno"
GENERATE_NEW_PASS=0
if [ ! -f "$ENV_FILE" ]; then
    GENERATE_NEW_PASS=1
elif [ -n "${NAS_WEB_PASSWORD:-}" ]; then
    GENERATE_NEW_PASS=1
elif [ ! -f "$STATE_DIR/nas.sqlite" ]; then
    # La base de datos nunca fue inicializada (instalación previa interrumpida)
    GENERATE_NEW_PASS=1
fi

if [ "$GENERATE_NEW_PASS" -eq 1 ]; then
    secret="$(openssl rand -hex 32 2>/dev/null || head -c 32 /dev/urandom | od -An -tx1 | tr -d ' \n')"
    if [ -n "${NAS_WEB_PASSWORD:-}" ]; then
        password="$NAS_WEB_PASSWORD"
        echo "    Contraseña tomada de NAS_WEB_PASSWORD"
    elif [ ! -t 0 ]; then
        password="$(openssl rand -base64 12 2>/dev/null || head -c 16 /dev/urandom | tr -dc 'A-Za-z0-9!#%&*' | head -c 16)"
        echo "    [i] Instalación desatendida: contraseña generada automáticamente."
    else
        echo "    Defina la contraseña del panel (se actualizará a PBKDF2-HMAC-SHA512 en el primer acceso):"
        read -r -s -p "    Contraseña: " password; echo
        if [ -z "$password" ]; then
            password="$(openssl rand -base64 12 2>/dev/null || head -c 16 /dev/urandom | tr -dc 'A-Za-z0-9!#%&*' | head -c 16)"
            echo "    Contraseña en blanco; se generó automáticamente."
        fi
    fi
    hash="$(printf '%s' "$password" | sha256sum | cut -d' ' -f1)"
    install -m 0600 -o root -g root /dev/null "$ENV_FILE"
    {
        echo "NAS_WEB_BIND=127.0.0.1:8080"
        echo "NAS_WEB_USER=admin"
        echo "NAS_WEB_SECRET=$secret"
        echo "NAS_WEB_PASSWORD_SHA256=$hash"
        echo "NAS_WEB_TTL_SECS=28800"
        echo "NAS_REPO_DIR=/opt/nas_debian"
        echo "NAS_STATE_DIR=$STATE_DIR"
        echo "NAS_DATA_ROOT=$DATA_ROOT"
        echo "NAS_SAMBA_CONF=/etc/samba/smb.conf"
        echo "NAS_ROLE_FILE=/etc/nas/role"
        echo "NAS_BACKUP_DIR=/usr/local/bin"
        echo "NAS_INVENTORY=/etc/nas/inventory.yaml"
    } > "$ENV_FILE"

    # Si la base de datos ya existía y se proporcionó una nueva contraseña explícita, actualizar hash
    if [ -f "$STATE_DIR/nas.sqlite" ] && [ -n "${NAS_WEB_PASSWORD:-}" ]; then
        sqlite3 "$STATE_DIR/nas.sqlite" "UPDATE users SET password_hash='$hash' WHERE username='admin';" 2>/dev/null || true
    fi

    # Guardar contraseña temporalmente para presentarse una sola vez al usuario
    printf '%s' "$password" > /etc/nas/.initial_admin_password
    chmod 0600 /etc/nas/.initial_admin_password
else
    echo "    $ENV_FILE ya existe: se conserva"
fi

echo "[4/7] Instalando helpers privilegiados de Samba y reglas del sistema"
install -m 0755 -o root -g root "$SCRIPT_DIR/nas-samba-apply" /usr/local/sbin/nas-samba-apply
install -m 0755 -o root -g root "$SCRIPT_DIR/nas-share-dir" /usr/local/sbin/nas-share-dir
install -m 0755 -o root -g root "$SCRIPT_DIR/nas-share-acl" /usr/local/sbin/nas-share-acl
install -m 0755 -o root -g root "$SCRIPT_DIR/nas-accounts" /usr/local/sbin/nas-accounts
install -m 0755 -o root -g root "$SCRIPT_DIR/nas-backup-manage" /usr/local/sbin/nas-backup-manage
install -m 0755 -o root -g root "$SCRIPT_DIR/nas-terminal" /usr/local/sbin/nas-terminal
install -m 0755 -o root -g root "$SCRIPT_DIR/nas-storage-apply" /usr/local/sbin/nas-storage-apply
install -d -m 2770 -o root -g "$SERVICE_USER" /var/lib/nas/staging 2>/dev/null || true

if [ -f "$SCRIPT_DIR/99-nas-tuning.conf" ]; then
    install -d -m 0755 /etc/sysctl.d
    install -m 0644 "$SCRIPT_DIR/99-nas-tuning.conf" /etc/sysctl.d/99-nas-tuning.conf
    sysctl -p /etc/sysctl.d/99-nas-tuning.conf >/dev/null 2>&1 || true
fi

if [ -f "$SCRIPT_DIR/60-nas-readahead.rules" ]; then
    install -d -m 0755 /etc/udev/rules.d
    install -m 0644 "$SCRIPT_DIR/60-nas-readahead.rules" /etc/udev/rules.d/60-nas-readahead.rules
    udevadm control --reload-rules >/dev/null 2>&1 || true
fi

echo "    Instalando tarea diaria de auto-purga de papelera Samba..."
install -d -m 0755 /etc/cron.daily
cat > /etc/cron.daily/nas-recycle-purge <<'PURGE_EOF'
#!/bin/bash
# nas-recycle-purge: Purga automática de archivos en papeleras Samba con más de 15 días
if [ -x /usr/local/bin/nas ]; then
    /usr/local/bin/nas helper purge-recycle --days 15 >/dev/null 2>&1 || true
else
    find /srv/nas -type f -path '*/.recycle/*' -mtime +15 -delete 2>/dev/null || true
    find /srv/nas -type d -path '*/.recycle/*' -empty -delete 2>/dev/null || true
fi
PURGE_EOF
chmod 0755 /etc/cron.daily/nas-recycle-purge

echo "[5/7] Instalando unidad systemd"
install -m 0644 "$SCRIPT_DIR/nas-web.service" "$UNIT_FILE"
systemctl daemon-reload
systemctl enable --now nas-web.service
systemctl restart nas-web.service
systemctl --no-pager --lines=0 status nas-web.service || true

echo "[6/7] Instalando reglas sudo acotadas para el panel"
rm -f /etc/sudoers.d/nas-web
cat > "$SUDOERS_FILE" <<'SUDO'
nas-web ALL=(root) NOPASSWD: /usr/bin/systemctl start smbd, /usr/bin/systemctl stop smbd, \
    /usr/bin/systemctl restart smbd, /usr/bin/systemctl reload smbd, \
    /usr/bin/systemctl start nmbd, /usr/bin/systemctl stop nmbd, \
    /usr/bin/systemctl restart nmbd, /usr/bin/systemctl reload nmbd, \
    /usr/bin/systemctl start wsdd2, /usr/bin/systemctl stop wsdd2, \
    /usr/bin/systemctl restart wsdd2, /usr/bin/systemctl reload wsdd2, \
    /usr/bin/systemctl start nginx, /usr/bin/systemctl stop nginx, \
    /usr/bin/systemctl restart nginx, /usr/bin/systemctl reload nginx, \
    /usr/bin/systemctl start nas-web, /usr/bin/systemctl stop nas-web, \
    /usr/bin/systemctl restart nas-web, /usr/bin/systemctl reload nas-web, \
    /usr/bin/systemctl restart sssd, /usr/bin/systemctl reboot, \
    /usr/bin/systemd-run --no-block --collect systemctl reboot, \
    /usr/bin/systemd-run --no-block --collect --unit=nas-system-update /bin/bash /opt/nas_debian/src/core/updater.sh --yes, \
    /usr/bin/smbstatus -b, /usr/bin/testparm -s, \
    /usr/sbin/realm list, /usr/sbin/realm discover *, /usr/sbin/realm join *, /usr/sbin/realm leave, \
    /usr/bin/rm -rf /var/lib/nas/dev-sync/*, \
    /usr/bin/mkdir -p /var/lib/nas/dev-sync/*, \
    /usr/bin/tar -xzf /var/lib/nas/dev-sync/*, /usr/bin/mv -T /var/lib/nas/dev-sync/* /opt/nas_debian/current, \
    /usr/bin/btrfs scrub status /srv/nas, /usr/bin/btrfs scrub start /srv/nas, \
    /usr/bin/fstrim -v /srv/nas, \
    /usr/local/bin/nas helper *, \
    /usr/local/sbin/nas-samba-apply /var/lib/nas/staging/smbconf_*, \
    /usr/local/sbin/nas-share-dir *, /usr/local/sbin/nas-share-acl *, \
    /usr/local/sbin/nas-accounts *, /usr/bin/pdbedit -L -s, /usr/bin/passwd -S *, \
    /usr/local/sbin/nas-backup-manage *, /usr/local/sbin/nas-terminal *, \
    /usr/local/sbin/nas-storage-apply *, \
    /bin/bash /opt/nas_debian/src/core/updater.sh --yes, \
    /usr/bin/true
SUDO
chmod 0440 "$SUDOERS_FILE"
if command -v visudo >/dev/null 2>&1; then
    if ! visudo -cf "$SUDOERS_FILE" >/dev/null; then
        echo "[-] sudoers inválido" >&2
        rm -f "$SUDOERS_FILE"
        exit 1
    fi
fi

if [ "$CONFIGURE_NGINX" -eq 1 ] && command -v nginx >/dev/null 2>&1; then
    echo "[7/7] Configurando Nginx"
    install -d -m 0755 /etc/nginx/sites-available /etc/nginx/sites-enabled
    install -d -m 0755 /etc/ssl/certs
    install -d -m 0700 /etc/ssl/private
    if [ ! -f /etc/ssl/certs/nas-web.crt ] || [ ! -f /etc/ssl/private/nas-web.key ]; then
        echo "    Generando certificado SSL autofirmado para Nginx..."
        openssl req -x509 -nodes -days 3650 -newkey rsa:2048 \
            -keyout /etc/ssl/private/nas-web.key \
            -out /etc/ssl/certs/nas-web.crt \
            -subj "/CN=nas-server/O=NAS/C=CO" 2>/dev/null || true
        chmod 0600 /etc/ssl/private/nas-web.key 2>/dev/null || true
        chmod 0644 /etc/ssl/certs/nas-web.crt 2>/dev/null || true
    fi
    if [ -f "$NGINX_SITE" ]; then
        cp -a "$NGINX_SITE" "$NGINX_SITE.bak.$(date +%Y%m%d%H%M%S)"
    fi
    install -m 0644 "$SCRIPT_DIR/nginx-nas-web.conf" "$NGINX_SITE"
    rm -f /etc/nginx/sites-enabled/default
    ln -sf "$NGINX_SITE" /etc/nginx/sites-enabled/nas-web
    if nginx -t; then
        systemctl reload nginx
    else
        echo "[-] Nginx rechazó la configuración; se conserva el respaldo" >&2
        exit 1
    fi
else
    echo "[7/7] Nginx omitido (--no-nginx o no instalado)"
fi

echo "[verificación]"
if curl -fsS --max-time 5 http://127.0.0.1:8080/healthz >/dev/null; then
    echo "    healthz OK"
else
    echo "    [!] healthz no responde aún"
fi
echo
echo "Instalación completada."
echo "  Panel:      https://<servidor>/  (usuario admin)"
echo "  Variables:  $ENV_FILE"
echo "  Servicio:   systemctl status nas-web"
echo "  Reinicio:   systemctl restart nas-web"

if [ -f /etc/nas/.initial_admin_password ] && [ "${NAS_INSTALL_CALLER:-}" != "install.sh" ]; then
    INIT_PASS="$(cat /etc/nas/.initial_admin_password 2>/dev/null || true)"
    rm -f /etc/nas/.initial_admin_password
    if [ -n "$INIT_PASS" ]; then
        SERVER_HOST="$(hostname -I 2>/dev/null | awk '{print $1}' || echo "127.0.0.1")"
        echo
        echo "  ╭──────────────────────────────────────────────────────────────────╮"
        echo "  │               CREDENCIALES DE ACCESO AL PANEL WEB                │"
        echo "  ├──────────────────────────────────────────────────────────────────┤"
        echo "  │  Panel Web:   https://${SERVER_HOST}/"
        echo "  │  Usuario:     admin"
        echo "  │  Contraseña:  ${INIT_PASS}"
        echo "  ├──────────────────────────────────────────────────────────────────┤"
        echo "  │  [!] GUARDE ESTA CONTRASEÑA AHORA: solo se muestra una única vez.│"
        echo "  ╰──────────────────────────────────────────────────────────────────╯"
        echo
    fi
fi
