#!/bin/bash
# ==============================================================================
# Motor de Auto-Actualización desde GitHub
# ==============================================================================

# set -e eliminado porque infecta a todo el asistente cuando se importa con source
LIB_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../lib" && pwd)"
# shellcheck source=src/lib/colors.sh
source "$LIB_DIR/colors.sh" 2>/dev/null || true

PROJECT_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"

actualizar_desde_git() {
    local CURRENT_REV REMOTE_REV
    clear 2>/dev/null || true
    echo -e "${C_CYAN}"
    echo "  ╭──────────────────────────────────────────────────────────────────────────╮"
    echo "  │             BUSCANDO ACTUALIZACIONES DEL PROYECTO EN GITHUB              │"
    printf "  ╰──────────────────────────────────────────────────────────────────────────╯%b\n\n" "${C_RESET}"

    git config --system --add safe.directory "$PROJECT_ROOT" 2>/dev/null || git config --global --add safe.directory "$PROJECT_ROOT" 2>/dev/null || true

    if [ -d "$PROJECT_ROOT/.git" ]; then
        cd "$PROJECT_ROOT" || return
        echo -e " [•] Conectando con GitHub..."
        if ! git fetch origin main 2>/dev/null; then
            if [ -t 0 ] && command -v whiptail &>/dev/null; then
                whiptail --title "Sin conexión" --ok-button "< Aceptar >" \
                    --msgbox "No se pudo contactar con GitHub. Verifica tu conexión e inténtalo de nuevo." 9 68
            else
                echo -e "${C_YELLOW}[!] No se pudo contactar con GitHub (sin conexión).${C_RESET}"
            fi
            return
        fi
        CURRENT_REV=$(git rev-parse HEAD 2>/dev/null || echo "1")
        REMOTE_REV=$(git rev-parse origin/main 2>/dev/null || echo "2")

        if [ "$CURRENT_REV" == "$REMOTE_REV" ]; then
            if [ -t 0 ] && command -v whiptail &>/dev/null; then
                whiptail --title "$APP_TITLE" --ok-button "< Aceptar >" \
                    --msgbox "✔ Tu versión ya está completamente actualizada a la última versión de GitHub.\n\nCommit: $(git log -1 --format='%h - %s (%cd)' --date=short)" 10 70
            else
                echo -e "${C_GREEN}✔ Ya tienes la última versión instalada ($(git log -1 --format='%h - %s (%cd)' --date=short)).${C_RESET}"
            fi
        else
            local changelog
            changelog=$(git log HEAD..origin/main --oneline -n 5 2>/dev/null || echo "Nuevas mejoras disponibles.")
            if [ -t 0 ] && command -v whiptail &>/dev/null; then
                if (whiptail --title "Actualización Disponible" \
                    --yes-button "< Actualizar Ahora >" --no-button "< Cancelar >" \
                    --yesno "Hay una nueva versión disponible en GitHub:\n\n$changelog\n\n¿Deseas descargar e instalar la actualización ahora?" 15 72); then
                    git reset --hard origin/main
                    find "$PROJECT_ROOT" -type f -name "*.sh" -exec chmod +x {} + 2>/dev/null || true
                    whiptail --title "$APP_TITLE" --ok-button "< Reiniciar Asistente >" \
                        --msgbox "✔ ¡Actualización instalada con éxito!\n\nEl asistente se reiniciará con las nuevas mejoras." 9 65
                    exec bash "$PROJECT_ROOT/src/asistente.sh"
                fi
            else
                git reset --hard origin/main
                find "$PROJECT_ROOT" -type f -name "*.sh" -exec chmod +x {} + 2>/dev/null || true
                echo -e "${C_GREEN}✔ ¡Actualización completada con éxito a la versión $(git log -1 --format='%h - %s')!${C_RESET}"
            fi
        fi
    else
        echo -e "${C_YELLOW}[!] Este directorio no es un repositorio Git.${C_RESET}"
    fi
}

if [[ "${BASH_SOURCE[0]}" == "${0}" ]]; then
    actualizar_desde_git
fi
