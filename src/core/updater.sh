#!/bin/bash
# ==============================================================================
# Motor de Auto-Actualización desde GitHub
# ==============================================================================

# Nota: no se usa `set -e` porque este archivo se importa con `source` en el
# asistente TUI. Los errores se manejan de forma explícita en cada paso.

LIB_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../lib" && pwd)"
# shellcheck source=src/lib/colors.sh
source "$LIB_DIR/colors.sh" 2>/dev/null || true

PROJECT_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"

# Origen y rama esperados. Pueden sobrescribirse mediante variables de entorno
# para entornos con forks propios (NAS_UPDATE_REPO / NAS_UPDATE_BRANCH).
EXPECTED_REPO="${NAS_UPDATE_REPO:-ftole/nas_debian}"
EXPECTED_BRANCH="${NAS_UPDATE_BRANCH:-main}"

# Normaliza la URL del remoto a la forma "propietario/repositorio" para admitir
# tanto el formato HTTPS como el formato SSH sin comparaciones frágiles.
_normalizar_remoto() {
    local url="$1" repo
    if [[ "$url" == *"://"* ]]; then
        # Formato HTTPS: https://github.com/propietario/repo.git
        repo="${url#*://}"
        repo="${repo#*/}"
    else
        # Formato SSH: git@github.com:propietario/repo.git
        repo="${url#*:}"
    fi
    repo="${repo%.git}"
    printf '%s\n' "$repo"
}

# Muestra un aviso respetando el contexto TUI o consola.
_aviso() {
    local msg="$1"
    if [ -t 0 ] && command -v whiptail &>/dev/null; then
        whiptail --title "${APP_TITLE:-Actualizador NAS}" --ok-button "< Aceptar >" --msgbox "$msg" 12 70
    else
        echo -e "${C_YELLOW}[!] $msg${C_RESET}"
    fi
}

# Valida la sintaxis de todos los scripts Bash del proyecto. Devuelve 0 si
# todos son válidos y un código distinto de cero si alguno falla.
_validar_sintaxis() {
    local f
    local -i fallos
    fallos=0
    while IFS= read -r f; do
        if ! bash -n "$f" 2>/dev/null; then
            echo "  [X] Error de sintaxis en: $f"
            fallos+=1
        fi
    done < <(find "$PROJECT_ROOT" -type f -name "*.sh" -print)
    [ "$fallos" -eq 0 ]
}

actualizar_desde_git() {
    local FORCE_FLAG RAMA_ACTUAL REMOTO_ACTUAL REMOTO_NORM
    local CAMBIOS_LOCALES CURRENT_REV REMOTE_REV changelog BACKUP_DIR fallos
    FORCE_FLAG=""
    if [ "$1" == "--yes" ]; then
        FORCE_FLAG="1"
    fi

    clear 2>/dev/null || true
    echo -e "${C_CYAN}"
    echo "  ╭──────────────────────────────────────────────────────────────────────────╮"
    echo "  │             BUSCANDO ACTUALIZACIONES DEL PROYECTO EN GITHUB              │"
    printf "  ╰──────────────────────────────────────────────────────────────────────────╯%b\n\n" "${C_RESET}"

    git config --system --add safe.directory "$PROJECT_ROOT" 2>/dev/null || git config --global --add safe.directory "$PROJECT_ROOT" 2>/dev/null || true

    if [ ! -d "$PROJECT_ROOT/.git" ]; then
        echo -e "${C_YELLOW}[!] Este directorio no es un repositorio Git.${C_RESET}"
        return 1
    fi
    cd "$PROJECT_ROOT" || return 1

    # 1. Verificar la rama activa y el remoto esperados.
    RAMA_ACTUAL=$(git symbolic-ref --short HEAD 2>/dev/null || echo "")
    if [ "$RAMA_ACTUAL" != "$EXPECTED_BRANCH" ]; then
        _aviso "La rama activa ($RAMA_ACTUAL) no coincide con la esperada ($EXPECTED_BRANCH). Actualización cancelada."
        return 1
    fi
    REMOTO_ACTUAL=$(git remote get-url origin 2>/dev/null || echo "")
    if [ -z "$REMOTO_ACTUAL" ]; then
        _aviso "No se detectó el remoto 'origin'. Actualización cancelada."
        return 1
    fi
    REMOTO_NORM=$(_normalizar_remoto "$REMOTO_ACTUAL")
    if [ "$REMOTO_NORM" != "$EXPECTED_REPO" ]; then
        _aviso "El remoto 'origin' ($REMOTO_NORM) no coincide con el esperado ($EXPECTED_REPO). Actualización cancelada."
        return 1
    fi

    # 2. Detectar cambios locales sin confirmar para no sobrescribir trabajo.
    CAMBIOS_LOCALES=$(git status --porcelain 2>/dev/null || echo "")
    if [ -n "$CAMBIOS_LOCALES" ]; then
        local detalle
        detalle=$(git status --short 2>/dev/null)
        if [ -t 0 ] && command -v whiptail &>/dev/null; then
            whiptail --title "${APP_TITLE:-Actualizador NAS}" --ok-button "< Aceptar >" \
                --msgbox "Existen cambios locales sin confirmar:\n\n$detalle\n\nLa actualización se canceló para no sobrescribir tu trabajo. Confirma o descarta esos cambios y vuelve a intentarlo." 16 70
        else
            echo -e "${C_YELLOW}[!] Existen cambios locales sin confirmar:${C_RESET}"
            echo "$detalle"
            echo -e "${C_YELLOW}[!] La actualización se canceló para no sobrescribir tu trabajo.${C_RESET}"
        fi
        return 1
    fi

    # 3. Obtener la rama remota sin ocultar errores de git.
    echo -e " [•] Conectando con GitHub..."
    if ! git fetch origin "$EXPECTED_BRANCH"; then
        if [ -t 0 ] && command -v whiptail &>/dev/null; then
            whiptail --title "Sin conexión" --ok-button "< Aceptar >" \
                --msgbox "No se pudo contactar con GitHub. Verifica tu conexión e inténtalo de nuevo." 9 68
        else
            echo -e "${C_YELLOW}[!] No se pudo contactar con GitHub (sin conexión).${C_RESET}"
        fi
        return 1
    fi

    CURRENT_REV=$(git rev-parse HEAD 2>/dev/null || echo "")
    REMOTE_REV=$(git rev-parse "origin/$EXPECTED_BRANCH" 2>/dev/null || echo "")
    if [ -z "$CURRENT_REV" ] || [ -z "$REMOTE_REV" ]; then
        _aviso "No se pudo determinar la versión instalada o la candidata. Actualización cancelada."
        return 1
    fi

    if [ "$CURRENT_REV" == "$REMOTE_REV" ]; then
        if [ -t 0 ] && command -v whiptail &>/dev/null; then
            whiptail --title "${APP_TITLE:-Actualizador NAS}" --ok-button "< Aceptar >" \
                --msgbox "✔ Tu versión ya está completamente actualizada a la última versión de GitHub.\n\nCommit: $(git log -1 --format='%h - %s (%cd)' --date=short)" 10 70
        else
            echo -e "${C_GREEN}✔ Ya tienes la última versión instalada ($(git log -1 --format='%h - %s (%cd)' --date=short)).${C_RESET}"
        fi
        return 0
    fi

    # 4. Mostrar versión instalada, candidata y cambios principales.
    changelog=$(git log "$CURRENT_REV..$REMOTE_REV" --oneline -n 5 2>/dev/null || echo "Nuevas mejoras disponibles.")

    if [ -t 0 ] && command -v whiptail &>/dev/null; then
        if ! (whiptail --title "Actualización Disponible" \
            --yes-button "< Actualizar Ahora >" --no-button "< Cancelar >" \
            --yesno "Hay una nueva versión disponible en GitHub.\n\nVersión instalada: $(git log -1 --format='%h - %s' "$CURRENT_REV")\nVersión candidata : $(git log -1 --format='%h - %s' "$REMOTE_REV")\n\nCambios principales:\n$changelog\n\n¿Deseas descargar e instalar la actualización ahora?" 18 74); then
            return 0
        fi
    elif [ "$FORCE_FLAG" == "1" ]; then
        echo -e "${C_CYAN}Versión instalada: $(git log -1 --format='%h - %s' "$CURRENT_REV")${C_RESET}"
        echo -e "${C_CYAN}Versión candidata : $(git log -1 --format='%h - %s' "$REMOTE_REV")${C_RESET}"
        echo -e "${C_CYAN}Cambios principales:${C_RESET}"
        echo "$changelog"
        echo ""
    else
        echo -e "${C_YELLOW}[!] Hay una actualización disponible, pero se requiere confirmación interactiva.${C_RESET}"
        echo -e "${C_YELLOW}    Ejecuta 'sudo nas update --yes' para aplicarla, o abre el asistente.${C_RESET}"
        echo ""
        echo -e "${C_CYAN}Versión instalada: $(git log -1 --format='%h - %s' "$CURRENT_REV")${C_RESET}"
        echo -e "${C_CYAN}Versión candidata : $(git log -1 --format='%h - %s' "$REMOTE_REV")${C_RESET}"
        echo "$changelog"
        return 1
    fi

    # 5. Copia de seguridad del estado actual antes de actualizar.
    BACKUP_DIR=$(mktemp -d "${PROJECT_ROOT}.update_backup_XXXXXX")
    if ! cp -a "$PROJECT_ROOT/." "$BACKUP_DIR" 2>/dev/null; then
        _aviso "No se pudo crear la copia de seguridad local. Actualización cancelada."
        return 1
    fi
    echo -e " [•] Copia de seguridad creada en $BACKUP_DIR"

    # 6. Actualizar únicamente con avance rápido (fast-forward). No se usa
    #    `git reset --hard` para evitar sobrescribir cambios sin control.
    if ! git merge --ff-only "origin/$EXPECTED_BRANCH"; then
        echo -e "${C_RED}[X] No se pudo aplicar la actualización (la historia no es de avance rápido).${C_RESET}"
        echo -e "${C_RED}    El estado previo se conserva en: $BACKUP_DIR${C_RESET}"
        return 1
    fi
    find "$PROJECT_ROOT" -type f -name "*.sh" -exec chmod +x {} + 2>/dev/null || true

    # 7. Validaciones posteriores a la actualización.
    fallos=0
    if ! git fsck --no-progress >/dev/null 2>&1; then
        fallos=$((fallos + 1))
    fi
    if ! _validar_sintaxis; then
        fallos=$((fallos + 1))
    fi

    if [ "$fallos" -ne 0 ]; then
        echo -e "${C_RED}[X] La actualización introdujo errores; restaurando la versión anterior...${C_RESET}"
        git reset --hard "$CURRENT_REV" >/dev/null 2>&1
        find "$PROJECT_ROOT" -type f -name "*.sh" -exec chmod +x {} + 2>/dev/null || true
        _aviso "La actualización falló la validación y se restauró la versión anterior.\n\nRollback manual: git reset --hard $CURRENT_REV\nCopia de seguridad: $BACKUP_DIR"
        return 1
    fi

    if [ -t 0 ] && command -v whiptail &>/dev/null; then
        if [[ "${BASH_SOURCE[0]}" != "${0}" ]]; then
            # Invocado desde el asistente: reiniciar para cargar el código nuevo.
            whiptail --title "${APP_TITLE:-Actualizador NAS}" --ok-button "< Reiniciar Asistente >" \
                --msgbox "✔ ¡Actualización instalada con éxito!\n\nEl asistente se reiniciará con las nuevas mejoras.\n\n(Respaldo conservado en $BACKUP_DIR)" 11 68
            exec bash "$PROJECT_ROOT/src/asistente.sh"
        else
            whiptail --title "${APP_TITLE:-Actualizador NAS}" --ok-button "< Aceptar >" \
                --msgbox "✔ ¡Actualización instalada con éxito!\n\n(Respaldo conservado en $BACKUP_DIR)" 11 68
        fi
    else
        echo -e "${C_GREEN}✔ ¡Actualización completada con éxito a la versión $(git log -1 --format='%h - %s')!${C_RESET}"
        echo -e "${C_GREEN}   Respaldo conservado en: $BACKUP_DIR${C_RESET}"
    fi
    return 0
}

if [[ "${BASH_SOURCE[0]}" == "${0}" ]]; then
    actualizar_desde_git "$@"
fi
