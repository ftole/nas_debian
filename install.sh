#!/bin/bash
# ==============================================================================
# INSTALADOR OFICIAL: Servidor NAS & Central de Respaldos EAD-COL (Debian 13)
# ==============================================================================
# Instala la plataforma Rust (nas, nas-web, nas-tui, nasctl, nas-dev) y el panel.
#
# Uso remoto ultrarrápido (Método 2: Release oficial precompilado):
#   curl -fsSL https://raw.githubusercontent.com/ftole/nas_debian/main/install.sh | sudo bash
#
# Con contraseña web predefinida:
#   curl -fsSL https://raw.githubusercontent.com/ftole/nas_debian/main/install.sh | sudo NAS_WEB_PASSWORD="mi_clave_segura" bash
#
# Compilación desde código fuente (Método 1):
#   sudo bash install.sh --build-from-source
#
# Desinstalación:
#   sudo bash install.sh --uninstall
# ==============================================================================

set -e

export GIT_TERMINAL_PROMPT=0
export PATH="/root/.cargo/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin"

INSTALL_DIR="/opt/nas_debian"
BIN_DIR="/usr/local/bin"
CARGO_HOME="${CARGO_HOME:-/root/.cargo}"
BUILD_FROM_SOURCE=0
SPECIFIED_VERSION=""
PASSWORD_AUTO_GENERATED=0

# Colores de consola
C_RESET="\033[0m"
C_BOLD="\033[1m"
C_CYAN="\033[1;36m"
C_GREEN="\033[1;32m"
C_YELLOW="\033[1;33m"
C_RED="\033[1;31m"

# Procesar argumentos
while [ $# -gt 0 ]; do
    case "$1" in
        --build-from-source)
            BUILD_FROM_SOURCE=1
            shift
            ;;
        --version)
            SPECIFIED_VERSION="${2:?falta versión}"; shift 2
            ;;
        --uninstall|uninstall)
            echo -e "${C_YELLOW}[!] Desinstalando la plataforma NAS...${C_RESET}"
            if [ -d "$INSTALL_DIR" ] && [ -f "$INSTALL_DIR/src/core/uninstall.sh" ]; then
                bash "$INSTALL_DIR/src/core/uninstall.sh" --yes || true
            elif [ -x "$BIN_DIR/nas" ]; then
                "$BIN_DIR/nas" uninstall --scope os --yes 2>/dev/null || true
            fi
            systemctl disable --now nas-web.service 2>/dev/null || true
            rm -f /etc/systemd/system/nas-web.service /etc/sudoers.d/nas-web-rust /etc/nginx/sites-enabled/nas-web /etc/nginx/sites-available/nas-web 2>/dev/null || true
            systemctl daemon-reload 2>/dev/null || true
            rm -f "$BIN_DIR/nas" "$BIN_DIR/nas-web" "$BIN_DIR/nas-tui" "$BIN_DIR/nasctl" "$BIN_DIR/nas-dev" "$BIN_DIR/nas-cli"
            rm -rf "$INSTALL_DIR"
            echo -e "${C_GREEN}✔ Plataforma desinstalada.${C_RESET}"
            exit 0
            ;;
        -h|--help)
            echo "Uso: sudo bash install.sh [opciones]"
            echo "Opciones:"
            echo "  --build-from-source    Compila desde el repositorio Git con Rust/Cargo"
            echo "  --version <tag>        Instala una versión específica (ej: v0.1.4)"
            echo "  --uninstall            Desinstala completamente el servidor NAS"
            echo "  -h, --help             Muestra esta ayuda"
            exit 0
            ;;
        *)
            echo "Opción desconocida: $1" >&2
            exit 2
            ;;
    esac
done

if [ "$EUID" -ne 0 ]; then
    echo -e "${C_RED}[-] Este instalador requiere privilegios de administrador.${C_RESET}"
    echo -e "    Por favor ejecuta: ${C_BOLD}sudo bash $0${C_RESET}"
    exit 1
fi

# Detección de contraseña desatendida en pipelines (curl ... | sudo bash)
if [ -z "${NAS_WEB_PASSWORD:-}" ] && { [ ! -f /etc/nas-web.env ] || [ ! -f /var/lib/nas/nas.sqlite ]; }; then
    if [ ! -t 0 ]; then
        NAS_WEB_PASSWORD="$(openssl rand -base64 12 2>/dev/null || head -c 16 /dev/urandom | tr -dc 'A-Za-z0-9!#%&*' | head -c 16)"
        export NAS_WEB_PASSWORD
        PASSWORD_AUTO_GENERATED=1
    fi
fi

clear 2>/dev/null || true
echo -e "${C_CYAN}"
echo "  ╭──────────────────────────────────────────────────────────────────────────╮"
echo "  │        INSTALADOR OFICIAL • SERVIDOR NAS & BACKUP EAD-COL (DEBIAN 13)    │"
echo "  │                    Plataforma Rust (axum + ratatui)                      │"
echo -e "  ╰──────────────────────────────────────────────────────────────────────────╯${C_RESET}\n"

if [ "$BUILD_FROM_SOURCE" -eq 1 ]; then
    echo -e "${C_YELLOW}[!] Modo de compilación desde código fuente activado.${C_RESET}"

    echo -e " ${C_BOLD}[1/5]${C_RESET} Instalando herramientas base y de compilación..."
    DEBIAN_FRONTEND=noninteractive apt-get update -qq >/dev/null 2>&1 || true
    DEBIAN_FRONTEND=noninteractive apt-get install -y -qq sudo git curl ca-certificates \
        build-essential pkg-config acl openssl btrfs-progs lvm2 samba samba-vfs-modules cifs-utils rsync \
        nginx-light parted auditd tmux realmd adcli sssd sssd-tools sssd-ad libnss-sss libpam-sss >/dev/null 2>&1

    if ! command -v cargo >/dev/null 2>&1; then
        echo -e "  Instalando toolchain de Rust..."
        curl -fsSL https://sh.rustup.rs -o /tmp/rustup-init.sh
        sh /tmp/rustup-init.sh -y --profile minimal --default-toolchain stable >/dev/null 2>&1
        rm -f /tmp/rustup-init.sh
        export PATH="$CARGO_HOME/bin:$PATH"
    fi
    cargo --version

    SSH_CHECK=$(ssh -T -o StrictHostKeyChecking=accept-new -o BatchMode=yes -o ConnectTimeout=5 git@github.com 2>&1 || true)
    if echo "$SSH_CHECK" | grep -qi "successfully authenticated"; then
        REPO_URL="git@github.com:ftole/nas_debian.git"
    else
        REPO_URL="https://github.com/ftole/nas_debian.git"
    fi

    echo -e " ${C_BOLD}[2/5]${C_RESET} Sincronizando el repositorio en ${C_CYAN}$INSTALL_DIR${C_RESET}..."
    SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" 2>/dev/null && pwd || echo "")"
    if [ -d "$INSTALL_DIR/.git" ]; then
        cd "$INSTALL_DIR"
        git remote set-url origin "$REPO_URL" 2>/dev/null || true
        git fetch -q origin main 2>/dev/null || true
        git fetch -q origin --tags 2>/dev/null || true
        git checkout -q main 2>/dev/null || true
        git pull --ff-only origin main 2>/dev/null || true
    elif [ -n "$SCRIPT_DIR" ] && [ -d "$SCRIPT_DIR/.git" ] && [ "$SCRIPT_DIR" != "$INSTALL_DIR" ]; then
        rm -rf "$INSTALL_DIR"
        git clone -q "$SCRIPT_DIR" "$INSTALL_DIR"
    else
        rm -rf "$INSTALL_DIR"
        git clone -q "$REPO_URL" "$INSTALL_DIR"
    fi

    cd "$INSTALL_DIR"
    if [ -n "$SPECIFIED_VERSION" ]; then
        git checkout -q "$SPECIFIED_VERSION" 2>/dev/null || true
    fi
    git config --system --add safe.directory "$INSTALL_DIR" 2>/dev/null || true

    echo -e " ${C_BOLD}[3/5]${C_RESET} Compilando la plataforma Rust (release)..."
    cargo build --release --locked --workspace

    echo -e " ${C_BOLD}[4/5]${C_RESET} Instalando binarios y el servicio del panel..."
    for bin in nas nas-web nas-tui nasctl nas-dev; do
        if [ -f "target/release/$bin" ]; then
            install -m 0755 "target/release/$bin" "$BIN_DIR/$bin"
        fi
    done
    install -d -m 0700 -o root -g root /etc/backup-credentials 2>/dev/null || true
    install -d -m 0755 /etc/cron.d 2>/dev/null || true
    rm -f /etc/nginx/sites-enabled/default 2>/dev/null || true
    BUILD_DIR="$INSTALL_DIR/target/release" NAS_INSTALL_CALLER="install.sh" bash packaging/install.sh

else
    # ==========================================================================
    # INSTALACIÓN ULTRARRÁPIDA DESDE RELEASE OFICIAL PRECOMPILADO (PREDETERMINADO)
    # ==========================================================================
    echo -e " ${C_BOLD}[1/5]${C_RESET} Instalando dependencias del sistema operativo..."
    DEBIAN_FRONTEND=noninteractive apt-get update -qq >/dev/null 2>&1 || true
    DEBIAN_FRONTEND=noninteractive apt-get install -y -qq sudo curl ca-certificates tar gzip \
        acl openssl btrfs-progs lvm2 samba samba-vfs-modules cifs-utils rsync \
        nginx-light parted auditd tmux realmd adcli sssd sssd-tools sssd-ad libnss-sss libpam-sss >/dev/null 2>&1

    echo -e " ${C_BOLD}[2/5]${C_RESET} Consultando release oficial de GitHub..."
    TARGET_TAG="$SPECIFIED_VERSION"
    if [ -z "$TARGET_TAG" ]; then
        # 1. Resolver tag mediante redirección HTTP de releases/latest
        EFFECTIVE_URL=$(curl -fsSLI -o /dev/null -w "%{url_effective}" --connect-timeout 10 "https://github.com/ftole/nas_debian/releases/latest" 2>/dev/null || true)
        if [[ "$EFFECTIVE_URL" =~ /releases/tag/(v[0-9]+\.[0-9]+\.[0-9]+.*)$ ]]; then
            TARGET_TAG="${BASH_REMATCH[1]}"
        fi

        # 2. Fallback mediante GitHub API
        if [ -z "$TARGET_TAG" ]; then
            API_RESP=$(curl -sSL -H "Accept: application/vnd.github.v3+json" --connect-timeout 10 "https://api.github.com/repos/ftole/nas_debian/releases/latest" 2>/dev/null || true)
            TARGET_TAG=$(echo "$API_RESP" | grep -o '"tag_name": *"[^"]*"' | head -n 1 | cut -d'"' -f4 || true)
        fi

        # 3. Fallback de contingencia a v0.1.4
        if [ -z "$TARGET_TAG" ]; then
            TARGET_TAG="v0.1.4"
        fi
    fi

    # Normalizar versión y tag
    if [[ "$TARGET_TAG" =~ ^[0-9] ]]; then
        TARGET_TAG="v$TARGET_TAG"
    fi
    TARGET_VER="${TARGET_TAG#v}"

    echo -e "  Versión seleccionada: ${C_GREEN}${TARGET_TAG}${C_RESET} (${TARGET_VER})"

    PKG_FILE="nas_debian-${TARGET_VER}.tar.gz"
    MANIFEST_FILE="nas-${TARGET_VER}.json"
    BASE_URL="https://github.com/ftole/nas_debian/releases/download/${TARGET_TAG}"
    TAR_URL="${BASE_URL}/${PKG_FILE}"
    MANIFEST_URL="${BASE_URL}/${MANIFEST_FILE}"
    SHA_URL="${BASE_URL}/${PKG_FILE}.sha256"

    TMP_DIR=$(mktemp -d /tmp/nas_pkg.XXXXXX 2>/dev/null || mktemp -d)
    trap 'rm -rf "$TMP_DIR"' EXIT

    echo -e "  Descargando paquete de distribución..."
    if ! curl -fsSL --connect-timeout 15 --max-time 300 -o "$TMP_DIR/$PKG_FILE" "$TAR_URL"; then
        echo -e "${C_RED}[-] No se pudo descargar el paquete precompilado desde $TAR_URL${C_RESET}" >&2
        echo -e "    Puede compilar desde fuentes ejecutando: ${C_BOLD}sudo bash $0 --build-from-source${C_RESET}" >&2
        exit 1
    fi

    # Descargar manifiesto o suma SHA-256 para verificación
    curl -fsSL --connect-timeout 10 --max-time 60 -o "$TMP_DIR/$MANIFEST_FILE" "$MANIFEST_URL" 2>/dev/null || true
    curl -fsSL --connect-timeout 10 --max-time 60 -o "$TMP_DIR/${PKG_FILE}.sha256" "$SHA_URL" 2>/dev/null || true

    EXPECTED_SHA=""
    if [ -f "$TMP_DIR/$MANIFEST_FILE" ]; then
        EXPECTED_SHA=$(grep -o '"sha256": *"[^"]*"' "$TMP_DIR/$MANIFEST_FILE" | head -n 1 | cut -d'"' -f4 || true)
    fi
    if [ -z "$EXPECTED_SHA" ] && [ -f "$TMP_DIR/${PKG_FILE}.sha256" ]; then
        EXPECTED_SHA=$(awk '{print $1}' "$TMP_DIR/${PKG_FILE}.sha256")
    fi

    if [ -n "$EXPECTED_SHA" ]; then
        ACTUAL_SHA=$(sha256sum "$TMP_DIR/$PKG_FILE" | awk '{print $1}')
        if [ "$ACTUAL_SHA" != "$EXPECTED_SHA" ]; then
            echo -e "${C_RED}[-] Verificación SHA-256 fallida (esperado $EXPECTED_SHA, obtenido $ACTUAL_SHA). Abortando.${C_RESET}" >&2
            exit 1
        fi
        echo -e "${C_GREEN}  [✔] Integridad SHA-256 verificada:${C_RESET} $ACTUAL_SHA"
    fi

    echo -e " ${C_BOLD}[3/5]${C_RESET} Desempaquetando archivos en ${C_CYAN}$INSTALL_DIR${C_RESET}..."
    mkdir -p "$INSTALL_DIR"
    tar --no-same-owner -xzf "$TMP_DIR/$PKG_FILE" -C "$INSTALL_DIR" --strip-components=1
    chown -R root:root "$INSTALL_DIR" 2>/dev/null || true

    echo -e " ${C_BOLD}[4/5]${C_RESET} Instalando binarios y configurando servicios..."
    install -d -m 0755 "$BIN_DIR"
    for bin in nas nas-web nas-tui nasctl nas-dev; do
        if [ -f "$INSTALL_DIR/bin/$bin" ]; then
            install -m 0755 "$INSTALL_DIR/bin/$bin" "$BIN_DIR/$bin"
        fi
    done

    cd "$INSTALL_DIR"
    install -d -m 0700 -o root -g root /etc/backup-credentials 2>/dev/null || true
    install -d -m 0755 /etc/cron.d 2>/dev/null || true
    rm -f /etc/nginx/sites-enabled/default 2>/dev/null || true
    BUILD_DIR="$INSTALL_DIR/bin" NAS_INSTALL_CALLER="install.sh" bash packaging/install.sh
fi

echo -e " ${C_BOLD}[5/5]${C_RESET} Estado final del sistema..."
systemctl --no-pager --lines=0 status nas-web.service || true
SERVER_IP=$(hostname -I 2>/dev/null | awk '{print $1}' || echo "127.0.0.1")
echo
printf '  %s\n' "Panel Web: https://${SERVER_IP}/"
printf '  %s\n' "CLI NAS:   sudo nas --help"
printf '  %s\n' "TUI NAS:   sudo nas-tui"
echo

INITIAL_PASS=""
if [ -f /etc/nas/.initial_admin_password ]; then
    INITIAL_PASS="$(cat /etc/nas/.initial_admin_password 2>/dev/null || true)"
    rm -f /etc/nas/.initial_admin_password
elif [ -n "${NAS_WEB_PASSWORD:-}" ] && [ "$PASSWORD_AUTO_GENERATED" -eq 1 ]; then
    INITIAL_PASS="$NAS_WEB_PASSWORD"
fi

if [ -n "$INITIAL_PASS" ]; then
    echo -e "${C_CYAN}  ╭──────────────────────────────────────────────────────────────────╮${C_RESET}"
    echo -e "${C_CYAN}  │               CREDENCIALES DE ACCESO AL PANEL WEB                │${C_RESET}"
    echo -e "${C_CYAN}  ├──────────────────────────────────────────────────────────────────┤${C_RESET}"
    echo -e "${C_CYAN}  │  ${C_RESET}Panel Web:   ${C_BOLD}https://${SERVER_IP}/${C_RESET}"
    echo -e "${C_CYAN}  │  ${C_RESET}Usuario:     ${C_BOLD}admin${C_RESET}"
    echo -e "${C_CYAN}  │  ${C_RESET}Contraseña:  ${C_YELLOW}${C_BOLD}${INITIAL_PASS}${C_RESET}"
    echo -e "${C_CYAN}  ├──────────────────────────────────────────────────────────────────┤${C_RESET}"
    echo -e "${C_CYAN}  │  ${C_YELLOW}[!] GUARDE ESTA CONTRASEÑA AHORA: solo se muestra una única vez.${C_RESET}"
    echo -e "${C_CYAN}  ╰──────────────────────────────────────────────────────────────────╯${C_RESET}\n"
fi

echo -e "${C_GREEN}✔ Instalación completada correctamente.${C_RESET}"
