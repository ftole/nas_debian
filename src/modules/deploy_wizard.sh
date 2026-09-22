#!/bin/bash
# ==============================================================================
# Módulo: Asistente Interactivo de Despliegue Inicial (5 Pasos)
# ==============================================================================

instalar_nas() {
    local SERVER_IP DEFAULT_USER DEFAULT_NETBIOS DEFAULT_WG ROL_OPCION ROL_SERVER ROL_NETBIOS ROOT_DEV ROOT_DEVS
    local MENU_DISCOS DISCO_SELECCIONADO SMB_NETBIOS SMB_WORKGROUP USUARIO_ACTUAL OPCION_USER ADMIN_USER ADMIN_PASS
    local RESUMEN CORE_DEPLOY name size type dev_path
    
    SERVER_IP=$(obtener_ip_local)
    DEFAULT_USER=$(detect_default_user)
    DEFAULT_NETBIOS=$(obtener_netbios_defecto)
    DEFAULT_WG=$(obtener_workgroup_defecto)

    # --------------------------------------------------------------------------
    # PASO 1: SELECCIÓN DEL ROL
    # --------------------------------------------------------------------------
    ROL_OPCION=$(whiptail --title "Paso 1 de 5: Rol Principal del Servidor" \
        --ok-button "< Siguiente >" --cancel-button "< Cancelar >" \
        --menu "Selecciona la función principal que cumplirá este servidor:" 16 74 2 \
        "1" "Servidor NAS de Archivos (Almacenamiento Departamental Compartido)" \
        "2" "Central de Backup (Inmune a Ransomware - Carpetas Ocultas $)" 3>&1 1>&2 2>&3)
    RET=$?
    if [ $RET -ne 0 ] || [ -z "$ROL_OPCION" ]; then return; fi

    if [ "$ROL_OPCION" == "2" ]; then
        ROL_SERVER="BACKUP"
        ROL_NETBIOS="SRV-EAD-BKP"
    else
        ROL_SERVER="ARCHIVOS"
        ROL_NETBIOS="$DEFAULT_NETBIOS"
    fi

    # --------------------------------------------------------------------------
    # PASO 2: SELECCIÓN DEL DISCO DE ALMACENAMIENTO
    # --------------------------------------------------------------------------
    ROOT_DEV=$(findmnt -n -o SOURCE / 2>/dev/null || df / | tail -1 | awk '{print $1}')
    ROOT_DEVS=$(resolver_discos_raiz "$ROOT_DEV")

    MENU_DISCOS=()
    MENU_DISCOS+=("LOCAL" "Usar espacio de partición raíz ($ROOT_DEV)")

    while read -r name size type _; do
        if [ "$type" == "disk" ]; then
            dev_path="/dev/$name"
            if ! printf '%s\n' "$ROOT_DEVS" | grep -qx "$dev_path"; then
                if disco_en_uso "$dev_path"; then
                    MENU_DISCOS+=("$dev_path" "Disco dedicado ($size) - EN USO (no recomendado)")
                else
                    MENU_DISCOS+=("$dev_path" "Disco dedicado ($size) - Formato BTRFS automático (auto-tuning)")
                fi
            fi
        fi
    done < <(lsblk -dn -o NAME,SIZE,TYPE,MOUNTPOINT)

    DISCO_SELECCIONADO=$(whiptail --title "Paso 2 de 5: Disco de Almacenamiento" \
        --ok-button "< Siguiente >" --cancel-button "< Cancelar >" \
        --menu "Selecciona dónde se alojará el directorio /srv/nas:" 18 74 6 \
        "${MENU_DISCOS[@]}" 3>&1 1>&2 2>&3)
    RET=$?
    if [ $RET -ne 0 ] || [ -z "$DISCO_SELECCIONADO" ]; then return; fi

    # --------------------------------------------------------------------------
    # PASO 3: IDENTIFICADORES DE RED (NETBIOS Y WORKGROUP / DOMINIO)
    # --------------------------------------------------------------------------
    SMB_NETBIOS=$(whiptail --title "Paso 3 de 5: Nombre del Servidor" \
        --ok-button "< Siguiente >" --cancel-button "< Cancelar >" \
        --inputbox "Ingresa el nombre de red NetBIOS para este servidor:" 10 65 "$ROL_NETBIOS" 3>&1 1>&2 2>&3)
    RET=$?
    if [ $RET -ne 0 ] || [ -z "$SMB_NETBIOS" ]; then return; fi
    SMB_NETBIOS=$(echo "$SMB_NETBIOS" | tr '[:lower:]' '[:upper:]' | tr -cd '[:upper:]0-9_-')

    SMB_WORKGROUP=$(whiptail --title "Paso 3 de 5: Grupo de Trabajo o Dominio AD" \
        --ok-button "< Siguiente >" --cancel-button "< Cancelar >" \
        --inputbox "Ingresa el Workgroup o Dominio NetBIOS (ej. WORKGROUP o EMPRESA):" 10 65 "$DEFAULT_WG" 3>&1 1>&2 2>&3)
    RET=$?
    if [ $RET -ne 0 ] || [ -z "$SMB_WORKGROUP" ]; then return; fi
    SMB_WORKGROUP=$(echo "$SMB_WORKGROUP" | tr '[:lower:]' '[:upper:]' | tr -cd '[:upper:]0-9_-')

    # --------------------------------------------------------------------------
    # PASO 4: ADMINISTRADOR DE COCKPIT Y SAMBA
    # --------------------------------------------------------------------------
    USUARIO_ACTUAL="$DEFAULT_USER"
    OPCION_USER=$(whiptail --title "Paso 4 de 5: Administrador de Cockpit" \
        --ok-button "< Siguiente >" --cancel-button "< Cancelar >" \
        --menu "Selecciona la cuenta que administrará el panel web Cockpit y el servidor:" 14 70 2 \
        "1" "Usar usuario detectado: [$USUARIO_ACTUAL] (Recomendado)" \
        "2" "Crear o especificar otro usuario administrador" 3>&1 1>&2 2>&3)
    RET=$?
    if [ $RET -ne 0 ] || [ -z "$OPCION_USER" ]; then return; fi

    ADMIN_USER="$USUARIO_ACTUAL"
    if [ "$OPCION_USER" == "2" ]; then
        ADMIN_USER=$(whiptail --title "Nuevo Administrador" \
            --ok-button "< Siguiente >" --cancel-button "< Cancelar >" \
            --inputbox "Ingresa el nombre de usuario para el nuevo administrador:" 10 65 "admin_nas" 3>&1 1>&2 2>&3)
        RET=$?
        if [ $RET -ne 0 ] || [ -z "$ADMIN_USER" ]; then return; fi
    fi

    while true; do
        ADMIN_PASS=$(whiptail --title "Contraseña de Red Samba ($ADMIN_USER)" \
            --ok-button "< Siguiente >" --cancel-button "< Cancelar >" \
            --passwordbox "Ingresa la contraseña de red para $ADMIN_USER (Requerida por Samba/Windows):" 10 70 3>&1 1>&2 2>&3)
        RET=$?
        if [ $RET -ne 0 ]; then return; fi
        if [ -n "$ADMIN_PASS" ]; then break; fi
        whiptail --title "Contraseña Requerida" --ok-button "< Aceptar >" \
            --msgbox "Samba requiere una contraseña para que Windows pueda autenticar y conectar a las carpetas compartidas." 9 65
    done

    # --------------------------------------------------------------------------
    # PASO 5: RESUMEN Y CONFIRMACIÓN
    # --------------------------------------------------------------------------
    RESUMEN="PARAMETROS DE CONFIGURACION:
* Funcion Principal    : $ROL_SERVER
* Direccion IP Red     : $SERVER_IP
* Disco Almacenamiento : $DISCO_SELECCIONADO
* Nombre del Servidor  : $SMB_NETBIOS
* Grupo / Dominio      : $SMB_WORKGROUP
* Administrador Web    : $ADMIN_USER (Permisos sudo y Samba)

INCLUYE PARCHES AUTOMATICOS:
- Integracion Cockpit File Sharing y difusion WSDD2 / LLMNR
- Wrappers de compatibilidad en espanol (chage / passwd / lastb)
- Herramientas multiplataforma de Backup (CIFS, Rsync, SSHPass, Cron)

¿Confirmas la configuracion y el despliegue completo?"

    if [ "$DISCO_SELECCIONADO" != "LOCAL" ]; then
        RESUMEN="$RESUMEN

ADVERTENCIA: se formateara el disco $DISCO_SELECCIONADO y se borraran TODOS sus datos."
    fi

    if (whiptail --title "Paso 5 de 5: Confirmación Crítica" \
        --yes-button "< Sí, Iniciar Despliegue >" --no-button "< Cancelar >" \
        --yesno "$RESUMEN" 20 74); then
        
        clear 2>/dev/null || true
        printf "%b" "${C_CYAN}"
        echo "  ╭──────────────────────────────────────────────────────────────────────╮"
        echo "  │        INICIANDO DESPLIEGUE AUTOMATIZADO DEL SERVIDOR ($ROL_SERVER)   │"
        printf "  ╰──────────────────────────────────────────────────────────────────────╯%b\n\n" "${C_RESET}"
        
        CORE_DEPLOY="$(cd "$(dirname "${BASH_SOURCE[0]}")/../core" && pwd)/deploy.sh"
        printf '%s\n' "$ADMIN_PASS" | bash "$CORE_DEPLOY" "$DISCO_SELECCIONADO" "$SMB_WORKGROUP" "$SMB_NETBIOS" "$ADMIN_USER" "-" "$ROL_SERVER"
        local ret_exec=$?
        
        if [ $ret_exec -eq 0 ]; then
            whiptail --title "$APP_TITLE" --ok-button "< Finalizar >" \
                --msgbox "✔ ¡Despliegue del Servidor ($ROL_SERVER) Completado con Éxito!\n\n• Panel Web Cockpit: https://${SERVER_IP}:9090\n• Administrador:     $ADMIN_USER (con permisos sudo y Samba)\n• Grupo Maestro:     grp_sistemas (Permisos totales sobre /srv/nas)\n• Redes Compartidas: 0 (Servidor base 100% limpio)\n\n💡 SIGUIENTE PASO:\nUtiliza las opciones [2] y [3] del menú para crear tus grupos y definir tus carpetas compartidas (visibles u ocultas $) a medida." 17 74
        else
            whiptail --title "Error en el Despliegue" --ok-button "< Aceptar >" \
                --msgbox "✖ Ocurrió un error durante la ejecución del script de despliegue (Código de salida: $ret_exec).\n\nRevisa los mensajes anteriores en la consola." 12 70
        fi
    fi
}
