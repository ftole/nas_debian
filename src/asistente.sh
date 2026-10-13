#!/bin/bash
# ==============================================================================
# ASISTENTE PRINCIPAL: Servidor NAS & Central de Respaldos (Debian 13)
# ==============================================================================

# Asegurar emulación de terminal
export TERM="${TERM:-xterm-256color}"

# Reclamar el teclado para menús interactivos si se ejecuta a través de pipe
if [ ! -t 0 ]; then
    exec < /dev/tty 2>/dev/null || true
fi

# Validar privilegios de administrador
if [ "$EUID" -ne 0 ]; then
    echo "[-] Este asistente requiere privilegios de superusuario (root)."
    echo "    Ejecuta: sudo nas  o  sudo bash $0"
    exit 1
fi
# Validar tamaño mínimo de la terminal (Whiptail requiere espacio)
if [ -t 0 ]; then
    filas=$(tput lines 2>/dev/null || echo 24)
    cols=$(tput cols 2>/dev/null || echo 80)
    if [ "$filas" -lt 18 ] || [ "$cols" -lt 70 ]; then
        echo -e "\033[1;33m[!] Tu terminal es muy pequeña ($cols x $filas).\033[0m"
        echo -e "    Maximiza la ventana (Mínimo recomendado: 72x20) para ejecutar el asistente."
        exit 1
    fi
fi

SRC_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# Cargar librerías del sistema
# shellcheck source=src/lib/colors.sh
source "$SRC_DIR/lib/colors.sh"
# shellcheck source=src/lib/helpers.sh
source "$SRC_DIR/lib/helpers.sh"

# Cargar motores del núcleo
# shellcheck source=src/core/updater.sh
source "$SRC_DIR/core/updater.sh"

# Cargar módulos del menú
# shellcheck source=src/modules/deploy_wizard.sh
source "$SRC_DIR/modules/deploy_wizard.sh"
# shellcheck source=src/modules/storage.sh
source "$SRC_DIR/modules/storage.sh"
# shellcheck source=src/modules/shares.sh
source "$SRC_DIR/modules/shares.sh"
# shellcheck source=src/modules/trash.sh
source "$SRC_DIR/modules/trash.sh"
# shellcheck source=src/modules/backups.sh
source "$SRC_DIR/modules/backups.sh"
# shellcheck source=src/modules/users.sh
source "$SRC_DIR/modules/users.sh"
# shellcheck source=src/modules/groups.sh
source "$SRC_DIR/modules/groups.sh"
# shellcheck source=src/modules/domain.sh
source "$SRC_DIR/modules/domain.sh"
# shellcheck source=src/modules/logs.sh
source "$SRC_DIR/modules/logs.sh"
# shellcheck source=src/modules/diagnostics.sh
source "$SRC_DIR/modules/diagnostics.sh"

# Si se pasa el argumento --status, mostrar diagnóstico directo
if [ "$1" == "--status" ] || [ "$1" == "status" ]; then
    diagnostico_nas
    exit 0
fi

# Función de desinstalación guiada
desinstalar_guiado() {
    if (whiptail --title "ALERTA DE DESINSTALACIÓN CRÍTICA" \
        --yes-button "< Sí, Desinstalar Todo >" --no-button "< Cancelar >" \
        --yesno "¡CUIDADO! Esta acción desinstalará todos los paquetes de Samba, el Panel Web (Nginx/PHP), desmontará el disco y limpiará las configuraciones.\n\n¿Confirmas que deseas restablecer el servidor a su estado base limpio?" 12 72); then
        clear 2>/dev/null || true
        bash "$SRC_DIR/core/uninstall.sh" --yes
        whiptail --title "$APP_TITLE" --ok-button "< Aceptar >" \
            --msgbox "✔ El servidor ha sido desinstalado y el sistema quedó completamente limpio." 8 65
    fi
}

# Función para verificar si el servidor ya fue desplegado
obtener_estado_despliegue() {
    if [ -f /etc/samba/smb.conf ] && getent group grp_samba &>/dev/null && [ -d /srv/nas ]; then
        local rol="ARCHIVOS"
        if [ -s /etc/nas/role ]; then
            rol=$(tr -cd 'A-Za-z_' < /etc/nas/role | tr '[:lower:]' '[:upper:]')
            [ "$rol" == "HIBRIDO" ] && rol="ARCHIVOS_BACKUP"
            [ "$rol" == "ARCHIVOSBACKUP" ] && rol="ARCHIVOS_BACKUP"
        elif grep -qi "Servidor BACKUP" /etc/samba/smb.conf 2>/dev/null; then
            rol="BACKUP"
        fi
        echo "$rol"
    else
        echo ""
    fi
}

# Función para gestión granular de servicios de red
gestionar_servicios_red() {
    local OPC_SVC SVC_SEL ACCION OUT RET
    while true; do
        OPC_SVC=$(whiptail --title "$APP_TITLE" \
            --ok-button "< Seleccionar >" --cancel-button "< Volver >" \
            --menu "CONTROL DE SERVICIOS DE RED Y SISTEMA:" 18 72 6 \
            "1" "[*] Ver estado detallado de todos los servicios" \
            "2" "[~] Reiniciar todos los servicios de red (Samba/WSDD2/Web)" \
            "3" "[R] Reiniciar un servicio individual" \
            "4" "[+] Iniciar un servicio detenido" \
            "5" "[-] Detener un servicio activo" \
            "6" "[<] Regresar al Menú Principal" 3>&1 1>&2 2>&3)

        RET=$?
        if [ $RET -ne 0 ] || [ "$OPC_SVC" == "6" ]; then
            break
        fi

        case "$OPC_SVC" in
            1)
                OUT=$(systemctl status smbd nmbd wsdd2 nginx cron 2>&1 || true)
                whiptail --title "Estado de Servicios" --ok-button "< Aceptar >" \
                    --msgbox "$OUT" 22 78
                ;;

            2)
                if (whiptail --title "Confirmar Reinicio" \
                    --yes-button "< Sí, Reiniciar >" --no-button "< Cancelar >" \
                    --yesno "¿Deseas reiniciar los servicios de red de Samba, WSDD2, Nginx y PHP-FPM ahora?" 9 65); then
                    systemctl restart smbd nmbd wsdd2 nginx 2>/dev/null || true
                    systemctl restart 'php*-fpm' 2>/dev/null || true
                    whiptail --title "$APP_TITLE" --ok-button "< Aceptar >" \
                        --msgbox "✔ Servicios de Samba, WSDD2, Nginx y PHP-FPM reiniciados correctamente." 8 60
                fi
                ;;

            3|4|5)
                ACCION="restart"
                [ "$OPC_SVC" == "4" ] && ACCION="start"
                [ "$OPC_SVC" == "5" ] && ACCION="stop"

                SVC_SEL=$(whiptail --title "Seleccionar Servicio" \
                    --ok-button "< Siguiente >" --cancel-button "< Cancelar >" \
                    --menu "Servicio sobre el cual ejecutar $ACCION:" 16 65 6 \
                    "smbd"     "Servidor de Archivos Samba (SMB/CIFS)" \
                    "nmbd"     "Servicio de Nombres NetBIOS" \
                    "wsdd2"    "Descubrimiento de Red Windows (WSD)" \
                    "nginx"    "Servidor Web Nginx" \
                    "cron"     "Planificador de Tareas Cron" \
                    "ufw"      "Firewall de Red UFW" 3>&1 1>&2 2>&3)
                RET=$?
                if [ $RET -eq 0 ] && [ -n "$SVC_SEL" ]; then
                    if systemctl "$ACCION" "$SVC_SEL" 2>/dev/null; then
                        whiptail --title "$APP_TITLE" --ok-button "< Aceptar >" \
                            --msgbox "✔ Servicio $SVC_SEL: acción $ACCION ejecutada correctamente." 8 55
                    else
                        whiptail --title "Error" --ok-button "< Aceptar >" \
                            --msgbox "Fallo al ejecutar $ACCION en el servicio $SVC_SEL." 8 55
                    fi
                fi
                ;;
        esac
    done
}

# ==============================================================================
# MENÚ PRINCIPAL INTERACTIVO (13 MÓDULOS DE CONTROL TOTAL)
# ==============================================================================
while true; do
    ROL_ACTUAL=$(obtener_estado_despliegue)
    if [ -n "$ROL_ACTUAL" ]; then
        TAG_OPC1="[1]  [✔] Servidor Configurado (Rol: $ROL_ACTUAL)"
    else
        TAG_OPC1="[1]  Desplegar Servidor (NAS de Archivos, Backup o Híbrido)"
    fi

    MENU_OPTS=()
    MENU_OPTS+=("1" "$TAG_OPC1")
    MENU_OPTS+=("2" "[2]  Almacenamiento, Discos, LVM, Btrfs y TRIM")

    if [ "$ROL_ACTUAL" == "BACKUP" ]; then
        MENU_OPTS+=("3" "[3]  Gestión de Recursos de Backup Ocultos ($)")
    else
        MENU_OPTS+=("3" "[3]  Gestión de Recursos Compartidos Samba (Shares)")
    fi

    MENU_OPTS+=("4" "[4]  Papelera de Reciclaje y Restauración (/srv/nas/.trash)")

    if [ "$ROL_ACTUAL" != "ARCHIVOS" ]; then
        MENU_OPTS+=("5" "[5]  Gestión de Tareas de Backup (Windows / Linux / Local)")
    else
        MENU_OPTS+=("5" "[5]  Gestión de Tareas de Backup (No aplicable en rol ARCHIVOS)")
    fi

    MENU_OPTS+=("6" "[6]  Gestión de Usuarios y Roles (Superadmin, Operador, Samba)")
    MENU_OPTS+=("7" "[7]  Gestión de Grupos de Seguridad Departamentales")
    MENU_OPTS+=("8" "[8]  Integración con Dominio Active Directory (AD / SSSD)")
    MENU_OPTS+=("9" "[9]  Visor de Registros y Auditoría Unificada")
    MENU_OPTS+=("10" "[10] Ver Diagnóstico, Discos y Monitor en Vivo")
    MENU_OPTS+=("11" "[11] Control Granular de Servicios de Red (Samba / Nginx / PHP)")
    MENU_OPTS+=("12" "[12] Buscar Actualizaciones desde GitHub (Auto-Update)")
    MENU_OPTS+=("13" "[13] Desinstalar y Limpiar Servidor")

    num_items=$(( ${#MENU_OPTS[@]} / 2 ))
    OPCION=$(whiptail --title "$APP_TITLE" \
        --ok-button "< Seleccionar >" --cancel-button "< Salir >" \
        --menu "Selecciona una opción usando las flechas y presiona Enter:" 22 76 "$num_items" \
        "${MENU_OPTS[@]}" 3>&1 1>&2 2>&3)

    RET=$?
    if [ $RET -ne 0 ]; then
        clear 2>/dev/null || true
        break
    fi

    case "$OPCION" in
        1)
            if [ -n "$ROL_ACTUAL" ]; then
                whiptail --title "Servidor Ya Configurado" --ok-button "< Aceptar >" \
                    --msgbox "✔ Este servidor ya se encuentra DESPLEGADO y EN LÍNEA (Rol: $ROL_ACTUAL).\n\nPara administrar el servidor utiliza las siguientes opciones:\n • Discos y almacenamiento:     Opción [2]\n • Crear carpetas compartidas:   Opción [3]\n • Papelera y restauración:      Opción [4]\n • Programar copias de seguridad: Opción [5]\n • Gestionar usuarios y accesos:  Opción [6]\n • Grupos de seguridad:          Opción [7]\n • Dominio Active Directory:     Opción [8]\n • Auditoría y registros:        Opción [9]\n\n💡 Si deseas reconfigurar desde cero o cambiar el rol, primero debes ejecutar la opción [13] Desinstalar y Limpiar Servidor." 18 74
            else
                instalar_nas
            fi
            ;;
        2) gestionar_almacenamiento ;;
        3) gestionar_recursos_compartidos ;;
        4) gestionar_papelera ;;
        5)
            if [ "$ROL_ACTUAL" == "ARCHIVOS" ]; then
                whiptail --title "Módulo Deshabilitado" --ok-button "< Aceptar >" \
                    --msgbox "El módulo de copias de seguridad no está activo en un servidor con rol exclusivo ARCHIVOS." 8 68
            else
                gestionar_backups
            fi
            ;;
        6) gestionar_usuarios ;;
        7) gestionar_grupos ;;
        8) gestionar_dominio_ad ;;
        9) gestionar_registros_auditoria ;;
        10) diagnostico_nas ;;
        11) gestionar_servicios_red ;;
        12) actualizar_desde_git ;;
        13) desinstalar_guiado ;;
    esac
done
