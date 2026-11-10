#!/bin/bash
# ==============================================================================
# Despliegue local rápido de componentes compilados en Rust.
# Compila, instala en /usr/local/bin y reinicia el servicio nas-web.
# Uso: [sudo] bash scripts/deploy_local.sh [--all | --bin <nombre>] [--no-restart]
# ==============================================================================
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"

PREFIX="${PREFIX:-/usr/local/bin}"
TARGET_BIN="nas-web"
BUILD_ALL=0
RESTART_SERVICE=1
HAS_WEB=0

usage() {
    cat <<EOF
Uso: $0 [opciones]

Opciones:
  -b, --bin <nombre>   Compilar e instalar un binario específico (por defecto: nas-web)
                       Opciones: nas, nas-web, nas-tui, nasctl, nas-dev
  -a, --all            Compilar e instalar todos los binarios del workspace
      --no-restart     No reiniciar el servicio nas-web tras la instalación
  -h, --help           Muestra esta ayuda

Ejemplos:
  $0                   # Recompila nas-web, lo instala y reinicia el servicio
  $0 --all             # Recompila todo el workspace y actualiza /usr/local/bin
EOF
    exit 0
}

while [ $# -gt 0 ]; do
    case "$1" in
        -b|--bin)
            TARGET_BIN="${2:?falta el nombre del binario}"
            shift 2
            ;;
        -a|--all)
            BUILD_ALL=1
            shift
            ;;
        --no-restart)
            RESTART_SERVICE=0
            shift
            ;;
        -h|--help)
            usage
            ;;
        *)
            echo "[-] Opción desconocida: $1" >&2
            echo "    Ejecute '$0 --help' para ver las opciones disponibles." >&2
            exit 2
            ;;
    esac
done

# Determinar comando sudo si no somos root
SUDO_CMD=()
if [ "$(id -u)" -ne 0 ]; then
    if command -v sudo >/dev/null 2>&1; then
        SUDO_CMD=(sudo)
    else
        echo "[-] Error: Se requieren permisos de superusuario para escribir en $PREFIX" >&2
        exit 1
    fi
fi

# Validar cargo en PATH o directorios estándar de toolchain
if ! command -v cargo >/dev/null 2>&1; then
    if [ -x "$HOME/.cargo/bin/cargo" ]; then
        PATH="$HOME/.cargo/bin:$PATH"
        export PATH
    elif [ -x "/root/.cargo/bin/cargo" ]; then
        PATH="/root/.cargo/bin:$PATH"
        export PATH
    fi
fi

if ! command -v cargo >/dev/null 2>&1; then
    echo "[-] Error: cargo no está en el PATH." >&2
    exit 1
fi

cd "$REPO_DIR"

if [ "$BUILD_ALL" -eq 1 ]; then
    echo "[1/3] Compilando workspace completo en modo --release..."
    cargo build --workspace --release --locked
    BINS_TO_INSTALL=("nas" "nas-web" "nas-tui" "nasctl" "nas-dev")
else
    echo "[1/3] Compilando paquete '$TARGET_BIN' en modo --release..."
    cargo build --release --locked -p "$TARGET_BIN"
    BINS_TO_INSTALL=("$TARGET_BIN")
fi

echo "[2/3] Instalando binario(s) en $PREFIX..."
"${SUDO_CMD[@]}" install -d -m 0755 "$PREFIX"
for bin in "${BINS_TO_INSTALL[@]}"; do
    SRC="$REPO_DIR/target/release/$bin"
    if [ ! -f "$SRC" ]; then
        echo "[-] Error: No se encontró el binario compilado $SRC" >&2
        exit 1
    fi
    "${SUDO_CMD[@]}" install -m 0755 "$SRC" "$PREFIX/$bin"
    echo "  -> Instalado: $PREFIX/$bin"
    if [ "$bin" = "nas" ]; then
        "${SUDO_CMD[@]}" rm -f "$PREFIX/nas-cli"
        "${SUDO_CMD[@]}" ln -sf "$PREFIX/nas" "$PREFIX/nas-cli"
    fi
done

# Desplegar reglas de auditoría del kernel si auditd está presente en el servidor
if [ -f "$REPO_DIR/packaging/nas.rules" ] && [ -d /etc/audit ]; then
    "${SUDO_CMD[@]}" install -d -m 0750 /etc/audit/rules.d 2>/dev/null || true
    "${SUDO_CMD[@]}" install -m 0640 "$REPO_DIR/packaging/nas.rules" /etc/audit/rules.d/nas.rules 2>/dev/null || true
    if command -v augenrules >/dev/null 2>&1; then
        "${SUDO_CMD[@]}" augenrules --load >/dev/null 2>&1 || true
    fi
    if command -v systemctl >/dev/null 2>&1 && systemctl is-active --quiet auditd; then
        "${SUDO_CMD[@]}" systemctl restart auditd >/dev/null 2>&1 || true
    fi
fi

if [ "$RESTART_SERVICE" -eq 1 ]; then
    for bin in "${BINS_TO_INSTALL[@]}"; do
        if [ "$bin" = "nas-web" ]; then
            HAS_WEB=1
            break
        fi
    done

    if [ "$HAS_WEB" -eq 1 ]; then
        echo "[3/3] Reiniciando servicio nas-web..."
        if command -v systemctl >/dev/null 2>&1 && systemctl list-unit-files nas-web.service >/dev/null 2>&1; then
            "${SUDO_CMD[@]}" systemctl restart nas-web
            if "${SUDO_CMD[@]}" systemctl is-active --quiet nas-web; then
                echo "  -> Servicio nas-web activo y en ejecución."
            else
                echo "[!] Advertencia: nas-web no reporta estado 'active'." >&2
            fi
        else
            echo "  (systemctl o nas-web.service no disponible; omite reinicio)"
        fi
    else
        echo "[3/3] Omite reinicio de nas-web (no se actualizó nas-web)."
    fi
else
    echo "[3/3] Omite reinicio de servicios (--no-restart activo)."
fi

echo ""
echo "=================================================================="
echo " Despliegue local finalizado exitosamente."
if [ "$HAS_WEB" -eq 1 ]; then
    echo " IMPORTANTE: Si modificó la interfaz web, recargue el navegador"
    echo " con Ctrl + Shift + R (o Ctrl + F5) para invalidar la caché."
fi
echo "=================================================================="
