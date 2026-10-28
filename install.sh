#!/bin/bash
# ==============================================================================
# INSTALADOR OFICIAL: Servidor NAS & Central de Respaldos EAD-COL (Debian 13)
# ==============================================================================
# Instala la plataforma Rust (nas, nas-web, nas-tui, nasctl) y el panel web.
#
# Uso remoto con One-Liner:
#   curl -fsSL https://raw.githubusercontent.com/ftole/nas_debian/main/install.sh | sudo bash
#
# Desinstalación:
#   sudo bash install.sh --uninstall
# ==============================================================================

set -e

export GIT_TERMINAL_PROMPT=0
export PATH="/root/.cargo/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin"

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
INSTALL_DIR="/opt/nas_debian"
BIN_DIR="/usr/local/bin"
CARGO_HOME="${CARGO_HOME:-/root/.cargo}"

SSH_CHECK=$(ssh -T -o StrictHostKeyChecking=accept-new -o BatchMode=yes -o ConnectTimeout=5 git@github.com 2>&1 || true)
if echo "$SSH_CHECK" | grep -qi "successfully authenticated"; then
    REPO_URL="git@github.com:ftole/nas_debian.git"
else
    REPO_URL="https://github.com/ftole/nas_debian.git"
fi

# Colores de consola
C_RESET="\033[0m"
C_BOLD="\033[1m"
C_CYAN="\033[1;36m"
C_GREEN="\033[1;32m"
C_YELLOW="\033[1;33m"
C_RED="\033[1;31m"

# Verifica la firma GPG de un tag contra la huella esperada.
verificar_firma_tag() {
    local tag="$1" esperado="$2" salida fp
    if ! salida=$(git verify-tag --raw "$tag" 2>&1); then
        return 1
    fi
    fp=$(echo "$salida" | awk '/^\[GNUPG:\] VALIDSIG /{print $3; exit}')
    [ -n "$fp" ] || return 1
    if [ -n "$esperado" ]; then
        local esperado_norm fp_norm
        esperado_norm=$(printf '%s' "$esperado" | tr -d ' ' | tr '[:lower:]' '[:upper:]')
        fp_norm=$(printf '%s' "$fp" | tr -d ' ' | tr '[:lower:]' '[:upper:]')
        if [ "$fp_norm" != "$esperado_norm" ] && [ "${fp_norm: -16}" != "$esperado_norm" ] && [ "$fp_norm" != "${esperado_norm: -16}" ]; then
            return 1
        fi
    fi
    return 0
}

# Devuelve la tag firmada más reciente alcanzable desde la referencia dada.
seleccionar_tag_firmada() {
    local esperado="$1" ref="$2" tag
    while IFS= read -r tag; do
        [ -z "$tag" ] && continue
        [[ "$tag" =~ ^v[0-9]+\.[0-9]+\.[0-9]+$ ]] || continue
        git merge-base --is-ancestor "$tag" "$ref" 2>/dev/null || continue
        if verificar_firma_tag "$tag" "$esperado"; then
            printf '%s\n' "$tag"
            return 0
        fi
    done < <(git tag --sort=-version:refname 2>/dev/null)
    return 1
}

# Aplica la política de tags firmadas tras clonar.
aplicar_politica_tags() {
    local esperado="${NAS_UPDATE_SIGNER:-}"
    local obligatorio="${NAS_REQUIRE_SIGNED_TAGS:-false}"
    if [ "$obligatorio" != "true" ] && [ -z "$esperado" ]; then
        return 0
    fi
    git fetch -q origin --tags 2>/dev/null || true
    local tag
    if tag=$(seleccionar_tag_firmada "$esperado" "origin/main"); then
        if git checkout -q -B main "$tag" 2>/dev/null; then
            echo -e "${C_GREEN}  [✔] Instalación fijada a la tag firmada: $tag${C_RESET}"
            return 0
        fi
        echo -e "${C_YELLOW}  [!] No se pudo posicionar el árbol en $tag.${C_RESET}"
        return 1
    fi
    echo -e "${C_RED}[-] No se encontró una tag firmada válida. Abortando por seguridad.${C_RESET}"
    return 1
}

if [ "$EUID" -ne 0 ]; then
    echo -e "${C_RED}[-] Este instalador requiere privilegios de administrador.${C_RESET}"
    echo -e "    Por favor ejecuta: ${C_BOLD}sudo bash $0${C_RESET}"
    exit 1
fi

if [ "$1" == "--uninstall" ] || [ "$1" == "uninstall" ]; then
    echo -e "${C_YELLOW}[!] Desinstalando la plataforma NAS...${C_RESET}"
    if [ -d "$INSTALL_DIR" ] && [ -f "$INSTALL_DIR/src/core/uninstall.sh" ]; then
        bash "$INSTALL_DIR/src/core/uninstall.sh" --yes || true
    elif [ -x "$BIN_DIR/nas" ]; then
        "$BIN_DIR/nas" uninstall --scope os --yes 2>/dev/null || true
    fi
    systemctl disable --now nas-web.service 2>/dev/null || true
    rm -f "$BIN_DIR/nas" "$BIN_DIR/nas-web" "$BIN_DIR/nas-tui" "$BIN_DIR/nasctl" "$BIN_DIR/nas-dev"
    rm -rf "$INSTALL_DIR"
    echo -e "${C_GREEN}✔ Plataforma desinstalada.${C_RESET}"
    exit 0
fi

clear 2>/dev/null || true
echo -e "${C_CYAN}"
echo "  ╭──────────────────────────────────────────────────────────────────────────╮"
echo "  │        INSTALADOR OFICIAL • SERVIDOR NAS & BACKUP EAD-COL (DEBIAN 13)    │"
echo "  │                    Plataforma Rust (axum + ratatui)                      │"
echo -e "  ╰──────────────────────────────────────────────────────────────────────────╯${C_RESET}\n"

echo -e " ${C_BOLD}[1/5]${C_RESET} Instalando herramientas base..."
DEBIAN_FRONTEND=noninteractive apt-get update -qq >/dev/null 2>&1 || true
DEBIAN_FRONTEND=noninteractive apt-get install -y -qq sudo git curl ca-certificates \
    build-essential pkg-config btrfs-progs samba-vfs-modules cifs-utils rsync >/dev/null 2>&1

if ! command -v cargo >/dev/null 2>&1; then
    echo -e "  Instalando toolchain de Rust..."
    curl -fsSL https://sh.rustup.rs -o /tmp/rustup-init.sh
    sh /tmp/rustup-init.sh -y --profile minimal --default-toolchain stable >/dev/null 2>&1
    export PATH="$CARGO_HOME/bin:$PATH"
fi
cargo --version

echo -e " ${C_BOLD}[2/5]${C_RESET} Sincronizando el repositorio en ${C_CYAN}$INSTALL_DIR${C_RESET}..."
if [ -d "$INSTALL_DIR/.git" ]; then
    cd "$INSTALL_DIR"
    git remote set-url origin "$REPO_URL" 2>/dev/null || true
    if ! git fetch -q origin main 2>/dev/null; then
        echo -e "${C_YELLOW}  [!] No se pudo contactar con GitHub; se conserva la versión instalada.${C_RESET}"
    else
        git fetch -q origin --tags 2>/dev/null || true
        CURRENT_REV=$(git rev-parse HEAD 2>/dev/null || echo "")
        REMOTE_REV=$(git rev-parse origin/main 2>/dev/null || echo "")
        if [ -n "${NAS_UPDATE_SIGNER:-}" ] || [ "${NAS_REQUIRE_SIGNED_TAGS:-false}" == "true" ]; then
            if ! LATEST_TAG=$(seleccionar_tag_firmada "${NAS_UPDATE_SIGNER:-}" "origin/main"); then
                echo -e "${C_RED}[-] No se encontró una versión firmada válida. Abortando por seguridad.${C_RESET}"
                exit 1
            fi
            REMOTE_REV=$(git rev-list -n 1 "$LATEST_TAG" 2>/dev/null || echo "$REMOTE_REV")
        fi
        if [ -n "$CURRENT_REV" ] && [ -n "$REMOTE_REV" ] && [ "$CURRENT_REV" != "$REMOTE_REV" ]; then
            if ! git merge --ff-only "${LATEST_TAG:-origin/main}"; then
                echo -e "${C_RED}[-] La actualización no es fast-forward.${C_RESET}"
                exit 1
            fi
        fi
    fi
elif [ -d "$SCRIPT_DIR/.git" ] && [ "$SCRIPT_DIR" != "$INSTALL_DIR" ]; then
    rm -rf "$INSTALL_DIR"
    git clone -q "$SCRIPT_DIR" "$INSTALL_DIR"
    cd "$INSTALL_DIR"
    aplicar_politica_tags || exit 1
else
    rm -rf "$INSTALL_DIR"
    git clone -q "$REPO_URL" "$INSTALL_DIR"
    cd "$INSTALL_DIR"
    aplicar_politica_tags || exit 1
fi
cd "$INSTALL_DIR"
git config --system --add safe.directory "$INSTALL_DIR" 2>/dev/null || true

echo -e " ${C_BOLD}[3/5]${C_RESET} Compilando la plataforma Rust (puede tardar varios minutos)..."
cargo build --release --locked --workspace

echo -e " ${C_BOLD}[4/5]${C_RESET} Instalando binarios y el servicio del panel..."
for bin in nas nas-web nas-tui nasctl nas-dev; do
    if [ -f "target/release/$bin" ]; then
        install -m 0755 "target/release/$bin" "$BIN_DIR/$bin"
    fi
done
bash packaging/install.sh

echo -e " ${C_BOLD}[5/5]${C_RESET} Estado final..."
systemctl --no-pager --lines=0 status nas-web.service || true
printf '  %s\n' "Panel:  https://$(hostname -I 2>/dev/null | awk '{print $1}')/"
printf '  %s\n' "CLI:    sudo nas --help"
printf '  %s\n' "TUI:    sudo nas-tui"
echo
echo -e "${C_GREEN}✔ Instalación completada.${C_RESET}"
