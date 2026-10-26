#!/bin/bash
# ==============================================================================
# Módulo: Diagnóstico del Sistema y Monitor en Vivo
# ==============================================================================

diagnostico_nas() {
    local SERVER_IP s found_share r has_bkp b c_expr
    SERVER_IP=$(obtener_ip_local)
    clear 2>/dev/null || true
    echo -e "${C_CYAN}"
    echo "  ╭──────────────────────────────────────────────────────────────────────────╮"
    echo "  │               DIAGNÓSTICO EN VIVO • SERVIDOR ($SERVER_IP)                │"
    echo -e "  ╰──────────────────────────────────────────────────────────────────────────╯${C_RESET}\n"

    echo -e "  ${C_BOLD}${C_WHITE}1. ESTADO DE SERVICIOS EN TIEMPO REAL:${C_RESET}"
    local php_svc
    php_svc=$(systemctl list-unit-files --type=service 'php*-fpm.service' 2>/dev/null | awk '/php.*-fpm/ {print $1; exit}')
    [ -z "$php_svc" ] && php_svc="php-fpm"
    for s in smbd nmbd wsdd2 nginx "$php_svc" cron; do
        if systemctl is-active "$s" &>/dev/null; then
            echo -e "     [${C_GREEN}● ACTIVO${C_RESET}] $s"
        else
            echo -e "     [${C_RED}● INACTIVO${C_RESET}] $s"
        fi
    done

    echo -e "\n  ${C_BOLD}${C_WHITE}2. ALMACENAMIENTO DEL SERVIDOR (/srv/nas):${C_RESET}"
    df -h /srv/nas 2>/dev/null | awk 'NR==2 {printf "     Capacidad Total: %s | Utilizado: %s (%s) | Libre: %s\n", $2, $3, $5, $4}'

    echo -e "\n  ${C_BOLD}${C_WHITE}3. RECURSOS COMPARTIDOS EN SAMBA:${C_RESET}"
    if [ -f /etc/samba/smb.conf ]; then
        local found_share=false
        while read -r r; do
            [ -n "$r" ] && found_share=true && echo -e "     [•] ${C_YELLOW}$r${C_RESET}"
        done < <(grep -E "^\[" /etc/samba/smb.conf | grep -v "global" | tr -d "[]")
        if [ "$found_share" = false ]; then
            echo -e "     ${C_GRAY}(No hay recursos compartidos configurados todavía)${C_RESET}"
        fi
    else
        echo -e "     ${C_GRAY}(Samba aún no ha sido instalado o configurado)${C_RESET}"
    fi

    echo -e "\n  ${C_BOLD}${C_WHITE}4. TAREAS DE BACKUP PROGRAMADAS:${C_RESET}"
    local has_bkp=false
    for b in /etc/cron.d/backup_*; do
        if [ -f "$b" ]; then
            has_bkp=true
            local c_expr
            c_expr=$(awk '{print $1,$2,$3,$4,$5}' "$b")
            echo -e "     [⏱] ${C_CYAN}$(basename "$b" | sed "s/backup_//"):${C_RESET} ${C_WHITE}${c_expr}${C_RESET}"
        fi
    done
    if [ "$has_bkp" = false ]; then
        echo -e "     ${C_GRAY}(No hay tareas de backup programadas)${C_RESET}"
    fi

    echo -e "\n  ${C_GRAY}──────────────────────────────────────────────────────────────────────────${C_RESET}"
    if [ -t 0 ]; then
        read -n 1 -s -r -p "  Presiona cualquier tecla para regresar al menú principal..."
    fi
}
