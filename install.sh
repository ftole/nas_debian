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
        --yes|-y)
            # Acepta explícitamente flag de actualización/instalación desatendida
            shift
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

install_runtime_deps() {
    local mode="${1:-runtime}"
    local pkgs=(
        sudo curl wget ca-certificates tar gzip acl openssl
        btrfs-progs lvm2 parted e2fsprogs util-linux psmisc procps iputils-ping
        smartmontools hdparm logrotate auditd aide ufw fail2ban unattended-upgrades cron
        samba samba-vfs-modules smbclient wsdd2 cifs-utils rsync openssh-client sshpass
        sqlite3 python3 nginx-light
        tmux realmd adcli krb5-user packagekit sssd sssd-tools sssd-ad libnss-sss libpam-sss
    )
    if [ "$mode" = "build" ]; then
        pkgs+=(git build-essential pkg-config)
    fi
    DEBIAN_FRONTEND=noninteractive apt-get update -qq >/dev/null 2>&1 || true
    if command -v ionice >/dev/null 2>&1; then
        DEBIAN_FRONTEND=noninteractive nice -n 10 ionice -c 2 -n 7 \
            apt-get install -y -qq --no-install-recommends "${pkgs[@]}" >/dev/null 2>&1
    else
        DEBIAN_FRONTEND=noninteractive nice -n 10 \
            apt-get install -y -qq --no-install-recommends "${pkgs[@]}" >/dev/null 2>&1
    fi
}

clear 2>/dev/null || true
echo -e "${C_CYAN}"
echo "  ╭──────────────────────────────────────────────────────────────────────────╮"
echo "  │        INSTALADOR OFICIAL • SERVIDOR NAS & BACKUP EAD-COL (DEBIAN 13)    │"
echo "  │                    Plataforma Rust (axum + ratatui)                      │"
echo -e "  ╰──────────────────────────────────────────────────────────────────────────╯${C_RESET}\n"

if [ "$BUILD_FROM_SOURCE" -eq 1 ]; then
    echo -e "${C_YELLOW}[!] Modo de compilación desde código fuente activado.${C_RESET}"

    echo -e " ${C_BOLD}[1/5]${C_RESET} Instalando herramientas base y de compilación..."
    install_runtime_deps build

    if ! cargo --version >/dev/null 2>&1; then
        if [ -n "${SUDO_USER:-}" ] && [ -d "/home/$SUDO_USER/.rustup" ]; then
            ln -sf "/home/$SUDO_USER/.rustup" /root/.rustup 2>/dev/null || true
            ln -sf "/home/$SUDO_USER/.cargo" /root/.cargo 2>/dev/null || true
            export PATH="/root/.cargo/bin:$PATH"
        fi
        if ! cargo --version >/dev/null 2>&1; then
            echo -e "  Instalando toolchain de Rust..."
            curl -fsSL https://sh.rustup.rs -o /tmp/rustup-init.sh
            sh /tmp/rustup-init.sh -y --profile minimal --default-toolchain stable >/dev/null 2>&1
            rm -f /tmp/rustup-init.sh
            export PATH="$CARGO_HOME/bin:$PATH"
        fi
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
    if [ -z "${CARGO_BUILD_JOBS:-}" ]; then
        NAS_CPUS="$(nproc 2>/dev/null || echo 1)"
        NAS_AVAIL_MB="$(awk '/MemAvailable:/ {printf "%d", $2/1024; found=1} END {if (!found) print 2048}' /proc/meminfo 2>/dev/null || echo 2048)"
        NAS_MEM_JOBS=$(( NAS_AVAIL_MB / 1536 ))
        [ "$NAS_MEM_JOBS" -lt 1 ] && NAS_MEM_JOBS=1
        if [ "$NAS_CPUS" -gt 2 ]; then
            NAS_CPU_JOBS=$(( NAS_CPUS - 1 ))
        else
            NAS_CPU_JOBS=1
        fi
        CARGO_BUILD_JOBS="$NAS_CPU_JOBS"
        [ "$NAS_MEM_JOBS" -lt "$CARGO_BUILD_JOBS" ] && CARGO_BUILD_JOBS="$NAS_MEM_JOBS"
        [ "$CARGO_BUILD_JOBS" -lt 1 ] && CARGO_BUILD_JOBS=1
        export CARGO_BUILD_JOBS
    fi
    echo "    Presupuesto de compilación: CARGO_BUILD_JOBS=${CARGO_BUILD_JOBS}"
    if command -v ionice >/dev/null 2>&1; then
        nice -n 10 ionice -c 2 -n 7 cargo build --release --locked --workspace
    else
        nice -n 10 cargo build --release --locked --workspace
    fi

    echo -e " ${C_BOLD}[4/5]${C_RESET} Instalando binarios y el servicio del panel..."
    for bin in nas nas-web nas-tui nasctl nas-dev; do
        if [ -f "target/release/$bin" ]; then
            install -m 0755 "target/release/$bin" "$BIN_DIR/$bin"
        fi
    done
    cat > "$INSTALL_DIR/version.json" <<EOF
{
  "version": "$(grep -m 1 '^version = ' "$INSTALL_DIR/Cargo.toml" | cut -d'"' -f2)",
  "commit": "$(git -C "$INSTALL_DIR" rev-parse HEAD 2>/dev/null || echo "local")",
  "date": "$(date -u +"%Y-%m-%d")",
  "channel": "stable",
  "signature_scheme": "Ed25519",
  "ed25519_verified": false,
  "notes": "Compilación local desde código fuente (sin manifiesto Ed25519)"
}
EOF
    mkdir -p /etc/nas 2>/dev/null || true
    cp -f "$INSTALL_DIR/version.json" /etc/nas/version.json 2>/dev/null || true
    install -d -m 0700 -o root -g root /etc/backup-credentials 2>/dev/null || true
    install -d -m 0755 /etc/cron.d 2>/dev/null || true
    rm -f /etc/nginx/sites-enabled/default 2>/dev/null || true
    BUILD_DIR="$INSTALL_DIR/target/release" NAS_INSTALL_CALLER="install.sh" bash packaging/install.sh

else
    # ==========================================================================
    # INSTALACIÓN ULTRARRÁPIDA DESDE RELEASE OFICIAL PRECOMPILADO (PREDETERMINADO)
    # ==========================================================================
    echo -e " ${C_BOLD}[1/5]${C_RESET} Instalando dependencias del sistema operativo..."
    install_runtime_deps runtime

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

        # 3. Sin fallback silencioso a versiones antiguas si falla la resolución
        if [ -z "$TARGET_TAG" ]; then
            echo -e "${C_RED}[-] No se pudo determinar la última release oficial en GitHub.${C_RESET}" >&2
            echo -e "    Indique --version <tag> o compile desde fuentes con --build-from-source." >&2
            exit 1
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

    TMP_DIR=$(mktemp -d /tmp/nas_pkg.XXXXXX 2>/dev/null || mktemp -d)
    trap 'rm -rf "$TMP_DIR"' EXIT

    echo -e "  Descargando paquete y manifiesto firmado..."
    if ! curl -fsSL --connect-timeout 15 --max-time 300 -o "$TMP_DIR/$PKG_FILE" "$TAR_URL"; then
        echo -e "${C_RED}[-] No se pudo descargar el paquete precompilado desde $TAR_URL${C_RESET}" >&2
        echo -e "    Puede compilar desde fuentes ejecutando: ${C_BOLD}sudo bash $0 --build-from-source${C_RESET}" >&2
        exit 1
    fi
    if ! curl -fsSL --connect-timeout 10 --max-time 60 -o "$TMP_DIR/$MANIFEST_FILE" "$MANIFEST_URL"; then
        echo -e "${C_RED}[-] No se pudo descargar el manifiesto firmado ($MANIFEST_URL). Instalación abortada.${C_RESET}" >&2
        exit 1
    fi

    DEFAULT_RELEASE_PUB="220d56af9dad91eb44fb9a6fa175adc96cc9ffb25b405a2db2e867b2a9153335"
    SCRIPT_DIR_LOCAL="$(cd "$(dirname "${BASH_SOURCE[0]}")" 2>/dev/null && pwd || echo "")"
    if [ -n "${NAS_RELEASE_PUBKEY:-}" ]; then
        RELEASE_PUBKEY_HEX="$NAS_RELEASE_PUBKEY"
    elif [ -n "$SCRIPT_DIR_LOCAL" ] && [ -f "$SCRIPT_DIR_LOCAL/packaging/release.pub" ]; then
        RELEASE_PUBKEY_HEX="$(tr -d '[:space:]' < "$SCRIPT_DIR_LOCAL/packaging/release.pub")"
    else
        RELEASE_PUBKEY_HEX="$DEFAULT_RELEASE_PUB"
    fi

    if ! printf '%s' "$RELEASE_PUBKEY_HEX" | grep -qE '^[0-9a-fA-F]{64}$'; then
        echo -e "${C_RED}[-] Clave pública Ed25519 inválida.${C_RESET}" >&2
        exit 1
    fi

    json_str_field() {
        local file="$1" key="$2"
        if command -v python3 >/dev/null 2>&1; then
            python3 -c 'import json,sys; d=json.load(open(sys.argv[1])); v=d.get(sys.argv[2]); print("" if v is None else str(v))' "$file" "$key"
        else
            grep -o "\"${key}\"[[:space:]]*:[[:space:]]*\"[^\"]*\"" "$file" | head -n 1 | sed 's/.*:[[:space:]]*"\([^"]*\)"/\1/'
        fi
    }

    hex_to_bin() {
        local hex="$1" out="$2" i=0 len
        len=${#hex}
        : > "$out"
        while [ "$i" -lt "$len" ]; do
            printf '%b' "\\x${hex:i:2}" >> "$out"
            i=$((i + 2))
        done
    }

    M_VERSION="$(json_str_field "$TMP_DIR/$MANIFEST_FILE" "version")"
    M_COMMIT="$(json_str_field "$TMP_DIR/$MANIFEST_FILE" "commit")"
    M_ARTIFACT="$(json_str_field "$TMP_DIR/$MANIFEST_FILE" "artifact")"
    M_SHA256="$(json_str_field "$TMP_DIR/$MANIFEST_FILE" "sha256")"
    M_PUBLISHED_AT="$(json_str_field "$TMP_DIR/$MANIFEST_FILE" "published_at")"
    M_SIGNATURE="$(json_str_field "$TMP_DIR/$MANIFEST_FILE" "signature")"

    if [ -z "$M_VERSION" ] || [ -z "$M_COMMIT" ] || [ -z "$M_ARTIFACT" ] || [ -z "$M_SHA256" ] || [ -z "$M_PUBLISHED_AT" ] || [ -z "$M_SIGNATURE" ]; then
        echo -e "${C_RED}[-] Manifiesto incompleto o sin firma Ed25519. Abortando.${C_RESET}" >&2
        exit 1
    fi
    if [ "${M_VERSION#v}" != "$TARGET_VER" ]; then
        echo -e "${C_RED}[-] Versión del manifiesto (${M_VERSION}) no coincide con la solicitada (${TARGET_VER}). Abortando.${C_RESET}" >&2
        exit 1
    fi
    if [ "$M_ARTIFACT" != "$PKG_FILE" ]; then
        echo -e "${C_RED}[-] Artefacto del manifiesto (${M_ARTIFACT}) no coincide con el paquete (${PKG_FILE}). Abortando.${C_RESET}" >&2
        exit 1
    fi
    if ! printf '%s' "$M_SHA256" | grep -qE '^[0-9a-fA-F]{64}$'; then
        echo -e "${C_RED}[-] SHA-256 inválido en manifiesto. Abortando.${C_RESET}" >&2
        exit 1
    fi
    if ! printf '%s' "$M_SIGNATURE" | grep -qE '^[0-9a-fA-F]{128}$'; then
        echo -e "${C_RED}[-] Firma Ed25519 inválida en manifiesto. Abortando.${C_RESET}" >&2
        exit 1
    fi

    ACTUAL_SHA=$(sha256sum "$TMP_DIR/$PKG_FILE" | awk '{print $1}')
    if [ "${ACTUAL_SHA,,}" != "${M_SHA256,,}" ]; then
        echo -e "${C_RED}[-] Verificación SHA-256 fallida (esperado $M_SHA256, obtenido $ACTUAL_SHA). Abortando.${C_RESET}" >&2
        exit 1
    fi
    echo -e "${C_GREEN}  [✔] Integridad SHA-256 verificada:${C_RESET} $ACTUAL_SHA"

    printf '%s|%s|%s|%s|%s' "$M_VERSION" "$M_COMMIT" "$M_ARTIFACT" "$M_SHA256" "$M_PUBLISHED_AT" > "$TMP_DIR/payload.txt"
    hex_to_bin "302a300506032b6570032100${RELEASE_PUBKEY_HEX}" "$TMP_DIR/pub.der"
    hex_to_bin "$M_SIGNATURE" "$TMP_DIR/sig.bin"

    if ! openssl pkeyutl -verify -pubin -keyform DER -inkey "$TMP_DIR/pub.der" -rawin -in "$TMP_DIR/payload.txt" -sigfile "$TMP_DIR/sig.bin" >/dev/null 2>&1; then
        echo -e "${C_RED}[-] Verificación criptográfica Ed25519 FALLIDA. Posible manipulación del paquete. Abortando.${C_RESET}" >&2
        exit 1
    fi
    echo -e "${C_GREEN}  [✔] Firma criptográfica Ed25519 verificada.${C_RESET}"

    echo -e " ${C_BOLD}[3/5]${C_RESET} Desempaquetando archivos en ${C_CYAN}$INSTALL_DIR${C_RESET}..."
    mkdir -p "$INSTALL_DIR"
    tar --no-same-owner -xzf "$TMP_DIR/$PKG_FILE" -C "$INSTALL_DIR" --strip-components=1
    chown -R root:root "$INSTALL_DIR" 2>/dev/null || true

    if [ ! -f "$INSTALL_DIR/version.json" ]; then
        cat > "$INSTALL_DIR/version.json" <<EOF
{
  "version": "${TARGET_VER}",
  "commit": "${TARGET_TAG}",
  "date": "$(date -u +"%Y-%m-%d")",
  "channel": "stable",
  "signature_scheme": "Ed25519",
  "ed25519_verified": true,
  "notes": "Release oficial ${TARGET_TAG} verificado e instalado"
}
EOF
    fi
    mkdir -p /etc/nas 2>/dev/null || true
    cp -f "$INSTALL_DIR/version.json" /etc/nas/version.json 2>/dev/null || true

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
