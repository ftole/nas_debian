#!/bin/bash
# ==============================================================================
# INSTALADOR OFICIAL: Servidor NAS & Central de Respaldos EAD-COL (Debian 13)
# ==============================================================================
# Uso remoto con One-Liner:
#   curl -fsSL https://raw.githubusercontent.com/ftole/nas_debian/main/install.sh | sudo bash
# ==============================================================================

set -e

# Evitar que git pida contraseñas interactivamente y congele el script
export GIT_TERMINAL_PROMPT=0

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
INSTALL_DIR="/opt/nas_debian"
BIN_PATH="/usr/local/bin/nas"

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
C_WHITE="\033[1;37m"

# Verifica la firma GPG de un tag contra la huella esperada (misma lógica que updater.sh).
verificar_firma_tag() {
    local tag="$1" esperado="$2" salida fp
    if ! salida=$(git verify-tag --raw "$tag" 2>&1); then
        return 1
    fi
    fp=$(echo "$salida" | awk '/^\[GNUPG:\] VALIDSIG /{print $3; exit}')
    if [ -z "$fp" ]; then
        return 1
    fi
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

# 1. Comprobación de permisos de superusuario
if [ "$EUID" -ne 0 ]; then
    echo -e "${C_RED}[-] Este instalador requiere privilegios de administrador.${C_RESET}"
    echo -e "    Por favor ejecuta: ${C_BOLD}sudo bash $0${C_RESET} o utiliza ${C_BOLD}sudo${C_RESET} en el comando curl."
    exit 1
fi

# Desinstalación si se especifica el flag --uninstall
if [ "$1" == "--uninstall" ] || [ "$1" == "uninstall" ]; then
    echo -e "${C_YELLOW}[!] Desinstalando CLI 'nas' del sistema...${C_RESET}"
    rm -f "$BIN_PATH" /usr/local/bin/asistente_nas /usr/local/bin/asistente-nas
    if [ -d "$INSTALL_DIR" ]; then
        if [ -f "$INSTALL_DIR/src/core/uninstall.sh" ]; then
            bash "$INSTALL_DIR/src/core/uninstall.sh" --yes
        fi
        rm -rf "$INSTALL_DIR"
    fi
    echo -e "${C_GREEN}✔ El comando 'nas' y sus archivos han sido desinstalados completamente.${C_RESET}"
    exit 0
fi

clear 2>/dev/null || true
echo -e "${C_CYAN}"
echo "  ╭──────────────────────────────────────────────────────────────────────────╮"
echo "  │        INSTALADOR OFICIAL • SERVIDOR NAS & BACKUP EAD-COL (DEBIAN 13)    │"
echo -e "  ╰──────────────────────────────────────────────────────────────────────────╯${C_RESET}\n"

echo -e " ${C_BOLD}[1/4]${C_RESET} Verificando e instalando herramientas base (git, curl, whiptail)..."
DEBIAN_FRONTEND=noninteractive apt-get update -qq >/dev/null 2>&1 || true
DEBIAN_FRONTEND=noninteractive apt-get install -y -qq sudo git curl wget whiptail ca-certificates btrfs-progs samba-vfs-modules >/dev/null 2>&1

echo -e " ${C_BOLD}[2/4]${C_RESET} Descargando y sincronizando componentes en ${C_CYAN}$INSTALL_DIR${C_RESET}..."
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
            LATEST_TAG=$(git describe --tags --abbrev=0 origin/main 2>/dev/null || echo "")
            if [ -z "$LATEST_TAG" ] || ! verificar_firma_tag "$LATEST_TAG" "${NAS_UPDATE_SIGNER:-}"; then
                echo -e "${C_RED}[-] No se encontró una versión firmada válida. Abortando por seguridad.${C_RESET}"
                exit 1
            fi
            REMOTE_REV=$(git rev-list -n 1 "$LATEST_TAG" 2>/dev/null || echo "$REMOTE_REV")
        fi
        if [ -n "$CURRENT_REV" ] && [ -n "$REMOTE_REV" ] && [ "$CURRENT_REV" != "$REMOTE_REV" ]; then
            BACKUP_DIR=$(mktemp -d "${INSTALL_DIR}.install_backup_XXXXXX")
            if ! cp -a "$INSTALL_DIR/." "$BACKUP_DIR/" 2>/dev/null; then
                echo -e "${C_RED}[-] No se pudo crear el backup de la instalación.${C_RESET}"
                exit 1
            fi
            TARGET_REF="origin/main"
            if [ -n "$LATEST_TAG" ]; then
                if [ -n "${NAS_UPDATE_SIGNER:-}" ] || [ "${NAS_REQUIRE_SIGNED_TAGS:-false}" == "true" ]; then
                    TARGET_REF="$LATEST_TAG"
                fi
            fi
            if ! git merge --ff-only "$TARGET_REF"; then
                echo -e "${C_RED}[-] La actualización no es fast-forward. Backup disponible en: $BACKUP_DIR${C_RESET}"
                exit 1
            fi
            if ! find "$INSTALL_DIR" -type f -name "*.sh" -exec bash -n {} \;; then
                echo -e "${C_RED}[-] La nueva versión no superó la validación; restaurando la anterior.${C_RESET}"
                git reset --hard "$CURRENT_REV"
                exit 1
            fi
        fi
    fi
elif [ -f "$SCRIPT_DIR/src/asistente.sh" ] && [ "$SCRIPT_DIR" != "$INSTALL_DIR" ] && [ -d "$SCRIPT_DIR/.git" ]; then
    rm -rf "$INSTALL_DIR"
    if ! git clone -q "$SCRIPT_DIR" "$INSTALL_DIR" 2>/dev/null; then
        cp -a "$SCRIPT_DIR" "$INSTALL_DIR"
        find "$INSTALL_DIR" -type f \( -name "*.sh" -o -name "*.py" -o -name "*.bats" \) -exec sed -i 's/\r$//' {} + 2>/dev/null || true
    fi
    if cd "$INSTALL_DIR"; then
        git remote set-url origin "$REPO_URL" 2>/dev/null || true
    fi
else
    rm -rf "$INSTALL_DIR"
    git clone -q "$REPO_URL" "$INSTALL_DIR"
fi

find "$INSTALL_DIR" -type f -name "*.sh" -exec chmod +x {} + 2>/dev/null || true
git config --system --add safe.directory "$INSTALL_DIR" 2>/dev/null || git config --global --add safe.directory "$INSTALL_DIR" 2>/dev/null || true

echo -e " ${C_BOLD}[3/4]${C_RESET} Creando comando global del sistema ${C_GREEN}nas${C_RESET} en ${C_WHITE}$BIN_PATH${C_RESET}..."
cat << 'EOF' > "$BIN_PATH"
#!/bin/bash
# Wrapper CLI Global: nas
INSTALL_DIR="/opt/nas_debian"
REPO_URL="https://github.com/ftole/nas_debian.git"

if [ "$EUID" -ne 0 ]; then
    echo "[-] El comando 'nas' requiere privilegios de administrador. Ejecuta: sudo nas $@"
    exit 1
fi

export GIT_TERMINAL_PROMPT=0

git config --system --add safe.directory "$INSTALL_DIR" 2>/dev/null || git config --global --add safe.directory "$INSTALL_DIR" 2>/dev/null || true

case "$1" in
    update|--update|-u)
        if [ -f "$INSTALL_DIR/src/core/updater.sh" ]; then
            bash "$INSTALL_DIR/src/core/updater.sh" "${@:2}"
            exit $?
        else
            echo "[-] Error: no se encontró el actualizador en $INSTALL_DIR. Reinstala con install.sh."
            exit 1
        fi
        ;;

    status|--status|-s)
        if [ -f "$INSTALL_DIR/src/asistente.sh" ]; then
            export TERM="${TERM:-xterm-256color}"
            bash "$INSTALL_DIR/src/asistente.sh" --status 2>/dev/null || bash -c "
                echo '=== ESTADO DEL SERVIDOR ==='
                systemctl status smbd wsdd2 cockpit --no-pager
                df -h /srv/nas 2>/dev/null || true
            "
        fi
        exit 0
        ;;

    version|--version|-v)
        if [ -d "$INSTALL_DIR/.git" ]; then
            cd "$INSTALL_DIR"
            echo "Servidor NAS EAD-COL (Debian 13)"
            echo "Versión: $(git log -1 --format='%h (%cd)' --date=short)"
            echo "Commit:  $(git log -1 --format='%s')"
        else
            echo "Servidor NAS EAD-COL (Debian 13) - Versión 1.0"
        fi
        exit 0
        ;;

    uninstall|--uninstall)
        if [ -f "$INSTALL_DIR/install.sh" ]; then
            bash "$INSTALL_DIR/install.sh" --uninstall
        else
            rm -f /usr/local/bin/nas
            rm -rf "$INSTALL_DIR"
            echo "✔ CLI desinstalado."
        fi
        exit 0
        ;;

    help|--help|-h)
        echo "Uso: sudo nas [COMANDO]"
        echo ""
        echo "Comandos disponibles:"
        echo "  nas              Abre el Asistente Visual Interactivo (Menú Whiptail)"
        echo "  nas update       Actualiza el software a la última versión de GitHub"
        echo "  nas status       Muestra el estado de los servicios y discos"
        echo "  nas version      Muestra la versión actual y el commit instalado"
        echo "  nas uninstall    Desinstala el comando 'nas' y limpia el servidor"
        echo "  nas help         Muestra esta ayuda"
        exit 0
        ;;

    *)
        if [ -f "$INSTALL_DIR/src/asistente.sh" ]; then
            export TERM="${TERM:-xterm-256color}"
            if [ ! -t 0 ]; then
                exec < /dev/tty 2>/dev/null || true
            fi
            exec bash "$INSTALL_DIR/src/asistente.sh" "$@"
        else
            echo "[-] Error: No se encontró $INSTALL_DIR/src/asistente.sh"
            exit 1
        fi
        ;;
esac
EOF

chmod 755 "$BIN_PATH"
ln -sf "$BIN_PATH" /usr/local/bin/asistente_nas 2>/dev/null || true
ln -sf "$BIN_PATH" /usr/local/bin/asistente-nas 2>/dev/null || true

echo -e " ${C_BOLD}[4/4]${C_RESET} ¡Instalación y configuración completadas con éxito!"
echo ""
echo -e "${C_GREEN}==============================================================================${C_RESET}"
echo -e "${C_BOLD}${C_WHITE} ✔ ¡EL CLI 'nas' ESTÁ LISTO PARA USAR EN TU SERVIDOR!${C_RESET}"
echo -e "${C_GREEN}==============================================================================${C_RESET}"
echo -e " • Para abrir el asistente en cualquier momento:  ${C_CYAN}sudo nas${C_RESET}"
echo -e " • Para actualizar a la última versión:          ${C_CYAN}sudo nas update${C_RESET}"
echo -e " • Para desinstalar por completo:                ${C_CYAN}sudo nas uninstall${C_RESET}"
echo -e "${C_GREEN}==============================================================================${C_RESET}\n"

# Si se ejecuta interactivamente, preguntar si desea iniciar de inmediato
if [ -t 0 ]; then
    read -r -p "¿Deseas abrir el Asistente Visual ahora mismo? [S/n]: " RESP
    RESP=${RESP:-S}
    if [[ "$RESP" =~ ^[sSyY]$ ]]; then
        exec "$BIN_PATH"
    fi
fi
