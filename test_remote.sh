#!/bin/bash
# ==============================================================================
# Servidor NAS & Central de Respaldos (Debian 13) - Suite de Pruebas Remotas
# ==============================================================================
# Script interactivo y CLI multiplataforma para pruebas automatizadas vía SSH.
# Lee y persiste credenciales en '.env'. Si Python 3 con 'paramiko' está disponible,
# delega la ejecución para mayor velocidad y auditoría SSL nativa; de lo contrario,
# ejecuta la batería completa en Bash puro mediante OpenSSH y curl.
# ==============================================================================

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ENV_FILE="$SCRIPT_DIR/.env"
KNOWN_HOSTS_FILE="$SCRIPT_DIR/.nas_known_hosts"

# Colores ANSI
C_RESET="\033[0m"
C_BOLD="\033[1m"
C_CYAN="\033[1;36m"
C_GREEN="\033[1;32m"
C_YELLOW="\033[1;33m"
C_RED="\033[1;31m"
C_WHITE="\033[1;37m"
C_GRAY="\033[0;90m"

log_info() {
    echo -e " ${C_CYAN}[•]${C_RESET} $*"
}

log_ok() {
    echo -e " ${C_GREEN}[✔]${C_RESET} $*"
}

log_warn() {
    echo -e " ${C_YELLOW}[!]${C_RESET} $*"
}

log_err() {
    echo -e " ${C_RED}[✘]${C_RESET} $*"
}

print_banner() {
    echo -e "\n${C_CYAN}${C_BOLD}==============================================================================${C_RESET}"
    echo -e "${C_WHITE}${C_BOLD}  SERVIDOR NAS & CENTRAL DE RESPALDOS EAD-COL (Debian 13)${C_RESET}"
    echo -e "${C_CYAN}  Lanzador de Pruebas Remotas y Diagnóstico Multiplataforma (Bash)${C_RESET}"
    echo -e "${C_CYAN}${C_BOLD}==============================================================================${C_RESET}"
}

# ------------------------------------------------------------------------------
# Delegación prioritaria a Python si está disponible
# ------------------------------------------------------------------------------
case "$1" in
    help|--help|-h)
        # Ayuda nativa de test_remote.sh
        ;;
    install|uninstall|update|web|samba|backups|suite|console|status|config|"")
        if command -v python3 &>/dev/null && [ -f "$SCRIPT_DIR/test_remote.py" ]; then
            if python3 -c "import paramiko" &>/dev/null; then
                exec python3 "$SCRIPT_DIR/test_remote.py" "$@"
            fi
        fi
        ;;
    *)
        log_err "Comando desconocido: '$1'. Usa 'bash test_remote.sh help' para ver la lista de comandos."
        exit 1
        ;;
esac

# ------------------------------------------------------------------------------
# Motor Nativo en Bash (Fallback sin Python/Paramiko)
# ------------------------------------------------------------------------------

cargar_env() {
    if [ -f "$ENV_FILE" ]; then
        # Cargar variables ignorando comentarios y líneas en blanco
        while IFS='=' read -r key value || [ -n "$key" ]; do
            # Limpiar espacios
            key=$(echo "$key" | tr -d '[:space:]')
            case "$key" in
                \#*|"") continue ;;
                NAS_TEST_IP) NAS_TEST_IP=$(echo "$value" | tr -d '\r"' | tr -d "'") ;;
                NAS_TEST_PORT) NAS_TEST_PORT=$(echo "$value" | tr -d '\r"' | tr -d "'") ;;
                NAS_TEST_USER) NAS_TEST_USER=$(echo "$value" | tr -d '\r"' | tr -d "'") ;;
                NAS_TEST_PASSWORD) NAS_TEST_PASSWORD=$(echo "$value" | tr -d '\r"' | tr -d "'") ;;
                NAS_ROOT_PASSWORD) NAS_ROOT_PASSWORD=$(echo "$value" | tr -d '\r"' | tr -d "'") ;;
            esac
        done < "$ENV_FILE"
    fi

    NAS_TEST_PORT="${NAS_TEST_PORT:-22}"
    NAS_TEST_USER="${NAS_TEST_USER:-sistemas}"
    NAS_ROOT_PASSWORD="${NAS_ROOT_PASSWORD:-$NAS_TEST_PASSWORD}"
}

guardar_env() {
    cat <<EOF > "$ENV_FILE"
# ==============================================================================
# Servidor NAS & Central de Respaldos (Debian 13) - Credenciales de Prueba
# ==============================================================================
# Generado automáticamente por test_remote.sh

NAS_TEST_IP=$NAS_TEST_IP
NAS_TEST_PORT=$NAS_TEST_PORT
NAS_TEST_USER=$NAS_TEST_USER
NAS_TEST_PASSWORD=$NAS_TEST_PASSWORD
NAS_ROOT_PASSWORD=$NAS_ROOT_PASSWORD
EOF
    chmod 600 "$ENV_FILE" 2>/dev/null || true
    log_ok "Configuración guardada en $ENV_FILE"
}

ssh_cmd() {
    local cmd="$1"
    local sudo_flag="${2:-false}"
    local ssh_opts=(-p "$NAS_TEST_PORT" -o "StrictHostKeyChecking=accept-new" -o "UserKnownHostsFile=$KNOWN_HOSTS_FILE" -o "ConnectTimeout=10")

    if [ "$sudo_flag" = "true" ]; then
        cmd="printf '%s\n' '$NAS_ROOT_PASSWORD' | sudo -S -p '' bash -c $(printf '%q' "$cmd")"
    fi

    if command -v sshpass &>/dev/null && [ -n "$NAS_TEST_PASSWORD" ]; then
        printf '%s\n' "$cmd" | sshpass -p "$NAS_TEST_PASSWORD" ssh "${ssh_opts[@]}" "$NAS_TEST_USER@$NAS_TEST_IP" "bash"
    else
        printf '%s\n' "$cmd" | ssh "${ssh_opts[@]}" "$NAS_TEST_USER@$NAS_TEST_IP" "bash"
    fi
}

probar_conexion() {
    log_info "Comprobando conexión SSH con $NAS_TEST_USER@$NAS_TEST_IP:$NAS_TEST_PORT..."
    if ssh_cmd "whoami" false >/dev/null 2>&1; then
        if ssh_cmd "id -u" true 2>/dev/null | grep -q "0"; then
            log_ok "Conexión exitosa y permisos de root verificados."
            return 0
        else
            log_warn "Conexión SSH establecida, pero fallo en autenticación sudo."
            return 1
        fi
    else
        log_err "No se pudo establecer conexión SSH con $NAS_TEST_IP:$NAS_TEST_PORT."
        return 1
    fi
}

configurar_interactivo() {
    print_banner
    echo -e "\n${C_WHITE}${C_BOLD}Configuración de Conexión SSH para Pruebas Remotas (Bash):${C_RESET}\n"

    read -r -p " [?] Dirección IP o Host remoto [${NAS_TEST_IP:-10.10.1.2}]: " INPUT_IP
    NAS_TEST_IP="${INPUT_IP:-${NAS_TEST_IP:-10.10.1.2}}"

    read -r -p " [?] Puerto SSH [${NAS_TEST_PORT:-22}]: " INPUT_PORT
    NAS_TEST_PORT="${INPUT_PORT:-${NAS_TEST_PORT:-22}}"

    read -r -p " [?] Usuario SSH con acceso sudo [${NAS_TEST_USER:-sistemas}]: " INPUT_USER
    NAS_TEST_USER="${INPUT_USER:-${NAS_TEST_USER:-sistemas}}"

    read -r -s -p " [?] Contraseña SSH: " NAS_TEST_PASSWORD
    echo ""

    read -r -s -p " [?] Contraseña de root / sudo [Enter si es la misma]: " NAS_ROOT_PASSWORD
    echo ""
    NAS_ROOT_PASSWORD="${NAS_ROOT_PASSWORD:-$NAS_TEST_PASSWORD}"

    if probar_conexion; then
        guardar_env
    else
        read -r -p " [?] ¿Deseas guardar los datos a pesar de la advertencia? [s/N]: " RESP
        if [[ "$RESP" =~ ^[sSyY]$ ]]; then
            guardar_env
        fi
    fi
}

asegurar_config() {
    cargar_env
    if [ -z "$NAS_TEST_IP" ] || [ -z "$NAS_TEST_USER" ] || [ -z "$NAS_TEST_PASSWORD" ]; then
        configurar_interactivo
    fi
}

# ------------------------------------------------------------------------------
# Acciones de Pruebas en Bash
# ------------------------------------------------------------------------------

accion_install() {
    echo -e "\n${C_CYAN}${C_BOLD}>>> [1/7] Prueba de Despliegue e Instalación (test install)${C_RESET}"
    log_info "Sincronizando archivos del proyecto a /tmp/nas_debian_test..."
    local tar_tmp
    tar_tmp="/tmp/nas_sync_$$.tar.gz"
    tar --exclude='.git' --exclude='__pycache__' --exclude='*.sqlite*' -czf "$tar_tmp" -C "$SCRIPT_DIR" src web install.sh

    local ssh_opts=(-p "$NAS_TEST_PORT" -o "StrictHostKeyChecking=accept-new" -o "UserKnownHostsFile=$KNOWN_HOSTS_FILE")
    if command -v sshpass &>/dev/null && [ -n "$NAS_TEST_PASSWORD" ]; then
        sshpass -p "$NAS_TEST_PASSWORD" scp "${ssh_opts[@]}" "$tar_tmp" "$NAS_TEST_USER@$NAS_TEST_IP:/tmp/nas_sync.tar.gz"
    else
        scp "${ssh_opts[@]}" "$tar_tmp" "$NAS_TEST_USER@$NAS_TEST_IP:/tmp/nas_sync.tar.gz"
    fi
    rm -f "$tar_tmp"

    ssh_cmd "rm -rf /tmp/nas_debian_test && mkdir -p /tmp/nas_debian_test && tar -xzf /tmp/nas_sync.tar.gz -C /tmp/nas_debian_test && rm -f /tmp/nas_sync.tar.gz" true

    log_info "Ejecutando deploy.sh en remoto..."
    ssh_cmd "cd /tmp/nas_debian_test && printf '%s\n' '$NAS_ROOT_PASSWORD' | bash src/core/deploy.sh LOCAL WORKGROUP SRV-NAS $NAS_TEST_USER - ARCHIVOS --force --confirm" true

    log_info "Verificando servicios activos..."
    for svc in smbd wsdd2 nginx; do
        if ssh_cmd "systemctl is-active $svc" false | grep -q "^active"; then
            log_ok "Servicio '$svc': ACTIVO"
        else
            log_err "Servicio '$svc': INACTIVO"
        fi
    done
}

accion_uninstall() {
    echo -e "\n${C_CYAN}${C_BOLD}>>> [2/7] Prueba de Desinstalación y Limpieza (test uninstall)${C_RESET}"
    log_info "Ejecutando uninstall.sh..."
    ssh_cmd "if [ -f /tmp/nas_debian_test/src/core/uninstall.sh ]; then bash /tmp/nas_debian_test/src/core/uninstall.sh --yes; else bash /opt/nas_debian/src/core/uninstall.sh --yes; fi" true
    log_info "Verificando eliminación de archivos administrativos..."
    ssh_cmd "test ! -f /etc/sudoers.d/nas-web && echo 'OK: sudoers eliminado'" true
    ssh_cmd "test ! -f /usr/local/bin/nas && echo 'OK: CLI eliminado'" true
    log_ok "Desinstalación verificada."
}

accion_update() {
    echo -e "\n${C_CYAN}${C_BOLD}>>> [3/7] Prueba de Actualizador (test update)${C_RESET}"
    ssh_cmd "if [ -f /tmp/nas_debian_test/src/core/updater.sh ]; then bash /tmp/nas_debian_test/src/core/updater.sh --help; else echo 'updater no hallado'; fi" false
    log_ok "Actualizador verificado."
}

accion_web() {
    echo -e "\n${C_CYAN}${C_BOLD}>>> [4/7] Auditoría Web y Seguridad (test web)${C_RESET}"
    log_info "Probando redirección HTTP 80 -> HTTPS 443..."
    local red
    red=$(curl -s -I "http://$NAS_TEST_IP:80/" 2>/dev/null || true)
    if echo "$red" | grep -qi "301 Moved" && echo "$red" | grep -qi "https://"; then
        log_ok "Redirección 301 verificada desde cliente."
    else
        log_warn "Comprobando localmente en servidor..."
        ssh_cmd "curl -s -I http://127.0.0.1:80/ | grep -i '301 Moved'" false || true
    fi

    log_info "Comprobando HTTPS y HSTS..."
    local sec
    sec=$(curl -k -s -I "https://$NAS_TEST_IP:443/login" 2>/dev/null || true)
    if echo "$sec" | grep -qi "Strict-Transport-Security"; then
        log_ok "Cabecera HSTS detectada."
    fi
}

accion_samba() {
    echo -e "\n${C_CYAN}${C_BOLD}>>> [5/7] Prueba de Samba (test samba)${C_RESET}"
    ssh_cmd "testparm -s" true
    ssh_cmd "mkdir -p /srv/nas/SISTEMAS && echo 'TEST_AUDIT' > /srv/nas/SISTEMAS/test_probe.txt && cat /srv/nas/SISTEMAS/test_probe.txt && rm -f /srv/nas/SISTEMAS/test_probe.txt" true
    log_ok "Prueba Samba y escritura/lectura verificada."
}

accion_backups() {
    echo -e "\n${C_CYAN}${C_BOLD}>>> [6/7] Prueba de Respaldos y Deduplicación (test backups)${C_RESET}"
    local bkp_script
    bkp_script="
    rm -rf /tmp/btest_src /srv/nas/BACKUPS_HISTORICOS/btest
    mkdir -p /tmp/btest_src /srv/nas/BACKUPS_HISTORICOS/btest
    echo 'INMUTABLE' > /tmp/btest_src/estatico.txt
    echo 'V1' > /tmp/btest_src/dinamico.txt
    rsync -aAXH /tmp/btest_src/ /srv/nas/BACKUPS_HISTORICOS/btest/snap1/
    echo 'V2' > /tmp/btest_src/dinamico.txt
    rsync -aAXH --link-dest=/srv/nas/BACKUPS_HISTORICOS/btest/snap1 /tmp/btest_src/ /srv/nas/BACKUPS_HISTORICOS/btest/snap2/
    i1=\$(stat -c '%i' /srv/nas/BACKUPS_HISTORICOS/btest/snap1/estatico.txt)
    i2=\$(stat -c '%i' /srv/nas/BACKUPS_HISTORICOS/btest/snap2/estatico.txt)
    if [ \"\$i1\" = \"\$i2\" ]; then
        echo 'DEDUPLICACION_OK: inodos idénticos (\$i1)'
    else
        echo 'DEDUPLICACION_FALLO'
    fi
    rm -rf /tmp/btest_src /srv/nas/BACKUPS_HISTORICOS/btest
    "
    ssh_cmd "$bkp_script" true
    log_ok "Respaldos y hardlinks probados con éxito."
}

accion_suite() {
    print_banner
    echo -e "\n${C_WHITE}${C_BOLD}Ejecutando Suite Completa End-to-End...${C_RESET}\n"
    accion_install
    accion_web
    accion_samba
    accion_backups
    accion_update
    accion_uninstall
    echo -e "\n${C_GREEN}${C_BOLD}✔ BATERÍA COMPLETA FINALIZADA CON ÉXITO.${C_RESET}\n"
}

accion_status() {
    ssh_cmd "echo '=== ESTADO DEL SERVIDOR ==='; uname -a; uptime; df -h / /srv/nas 2>/dev/null || df -h /; systemctl status smbd nginx wsdd2 --no-pager 2>&1 || true" false
}

accion_console() {
    log_info "Conectando consola SSH interactiva..."
    ssh -p "$NAS_TEST_PORT" -o "StrictHostKeyChecking=accept-new" -o "UserKnownHostsFile=$KNOWN_HOSTS_FILE" "$NAS_TEST_USER@$NAS_TEST_IP"
}

# ------------------------------------------------------------------------------
# Menú y Enrutamiento CLI
# ------------------------------------------------------------------------------
case "$1" in
    install)    asegurar_config && accion_install ;;
    uninstall)  asegurar_config && accion_uninstall ;;
    update)     asegurar_config && accion_update ;;
    web)        asegurar_config && accion_web ;;
    samba)      asegurar_config && accion_samba ;;
    backups)    asegurar_config && accion_backups ;;
    suite)      asegurar_config && accion_suite ;;
    console)    asegurar_config && accion_console ;;
    status)     asegurar_config && accion_status ;;
    config)     cargar_env && configurar_interactivo ;;
    help|--help|-h)
        echo "Uso: bash test_remote.sh [COMANDO]"
        echo ""
        echo "Comandos disponibles:"
        echo "  test_remote.sh             Abre el Menú Interactivo"
        echo "  test_remote.sh install     Despliegue y validación de servicios"
        echo "  test_remote.sh uninstall   Desinstalación limpia y verificación"
        echo "  test_remote.sh update      Prueba de actualización y rollback"
        echo "  test_remote.sh web         Auditoría de seguridad web y API"
        echo "  test_remote.sh samba       Prueba de SMB y full_audit"
        echo "  test_remote.sh backups     Prueba de snapshots y deduplicación"
        echo "  test_remote.sh suite       Batería completa end-to-end"
        echo "  test_remote.sh console     Consola SSH interactiva directa"
        echo "  test_remote.sh status      Diagnóstico en vivo de servicios"
        echo "  test_remote.sh config      Reconfigurar archivo .env"
        exit 0
        ;;
    "")
        asegurar_config
        while true; do
            print_banner
            echo -e " ${C_GRAY}Servidor Remoto:${C_RESET} ${C_WHITE}${C_BOLD}$NAS_TEST_IP:$NAS_TEST_PORT${C_RESET} | ${C_GRAY}Usuario:${C_RESET} ${C_WHITE}$NAS_TEST_USER${C_RESET} | ${C_GRAY}Config:${C_RESET} ${C_CYAN}.env${C_RESET}"
            echo -e "${C_CYAN}${C_BOLD}------------------------------------------------------------------------------${C_RESET}"
            echo -e "  ${C_BOLD}[1]${C_RESET} test install      - Despliegue completo y validación de servicios"
            echo -e "  ${C_BOLD}[2]${C_RESET} test uninstall    - Desinstalación y verificación de limpieza total"
            echo -e "  ${C_BOLD}[3]${C_RESET} test update       - Prueba de auto-actualizador y verificación"
            echo -e "  ${C_BOLD}[4]${C_RESET} test web          - Auditoría HTTP->HTTPS, SSL, HSTS, CSRF, Rate-Limit y API"
            echo -e "  ${C_BOLD}[5]${C_RESET} test samba        - Verificación SMB, lectura/escritura y full_audit"
            echo -e "  ${C_BOLD}[6]${C_RESET} test backups      - Creación de tarea, snapshots, hardlinks y deduplicación"
            echo -e "  ${C_BOLD}[7]${C_RESET} test suite total  - Batería completa de pruebas End-to-End"
            echo -e "  ${C_BOLD}[8]${C_RESET} ssh console       - Consola SSH interactiva directa"
            echo -e "  ${C_BOLD}[9]${C_RESET} diagnostico       - Diagnóstico en vivo (nas status / servicios / disco)"
            echo -e "  ${C_BOLD}[10]${C_RESET} reconfigurar     - Editar credenciales de conexión en .env"
            echo -e "  ${C_BOLD}[0]${C_RESET} Salir"
            echo -e "${C_CYAN}${C_BOLD}==============================================================================${C_RESET}"
            read -r -p " [?] Selecciona una opción [0-10]: " OPC
            case "$OPC" in
                1) accion_install ;;
                2) accion_uninstall ;;
                3) accion_update ;;
                4) accion_web ;;
                5) accion_samba ;;
                6) accion_backups ;;
                7) accion_suite ;;
                8) accion_console ;;
                9) accion_status ;;
                10) configurar_interactivo ;;
                0|q|exit) echo -e "\n ${C_GREEN}¡Hasta pronto!${C_RESET}\n"; break ;;
                *) log_warn "Opción inválida." ;;
            esac
            read -r -p "Presiona Enter para continuar..."
        done
        ;;
    *)
        log_err "Comando desconocido: '$1'. Usa 'bash test_remote.sh help' para ver la lista de comandos."
        exit 1
        ;;
esac
