#!/bin/bash
# ==============================================================================
# Verificaciones estáticas del empaquetado (se ejecutan en CI).
# ==============================================================================
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
FAILED=0

check() {
    local file="$1" needle="$2" label="$3"
    if grep -qF -- "$needle" "$file"; then
        echo "[OK]    $label"
    else
        echo "[FALLO] $label (falta '$needle' en $file)"
        FAILED=1
    fi
}

for script in "$SCRIPT_DIR"/*.sh; do
    if bash -n "$script"; then
        echo "[OK]    sintaxis $(basename "$script")"
    else
        echo "[FALLO] sintaxis $(basename "$script")"
        FAILED=1
    fi
done

UNIT="$SCRIPT_DIR/nas-web.service"
check "$UNIT" "ProtectSystem=yes" "unit: ProtectSystem"
check "$UNIT" "PrivateTmp=yes" "unit: PrivateTmp"
check "$UNIT" "RestrictAddressFamilies=" "unit: RestrictAddressFamilies"
check "$UNIT" "EnvironmentFile=/etc/nas-web.env" "unit: EnvironmentFile"
check "$UNIT" "ReadWritePaths=/var/lib/nas /srv/nas" "unit: ReadWritePaths"

NGINX="$SCRIPT_DIR/nginx-nas-web.conf"
check "$NGINX" "proxy_pass http://127.0.0.1:8080" "nginx: proxy_pass"
check "$NGINX" "ssl_certificate" "nginx: TLS"
check "$NGINX" "Strict-Transport-Security" "nginx: HSTS"
check "$NGINX" "client_max_body_size 512M" "nginx: límite de subida"
check "$NGINX" "listen 443 ssl" "nginx: HTTPS"

INSTALL="$SCRIPT_DIR/install.sh"
check "$INSTALL" "visudo -cf" "install: valida sudoers"
check "$INSTALL" "nginx -t" "install: valida nginx"
check "$INSTALL" "systemctl enable --now nas-web.service" "install: habilita servicio"

if [ "$FAILED" -ne 0 ]; then
    echo
    echo "Empaquetado incompleto."
    exit 1
fi

echo
echo "Empaquetado verificado."
