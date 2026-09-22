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
# shellcheck source=src/modules/groups.sh
source "$SRC_DIR/modules/groups.sh"
# shellcheck source=src/modules/shares.sh
source "$SRC_DIR/modules/shares.sh"
# shellcheck source=src/modules/backups.sh
source "$SRC_DIR/modules/backups.sh"
# shellcheck source=src/modules/users.sh
source "$SRC_DIR/modules/users.sh"
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
        --yesno "¡CUIDADO! Esta acción desinstalará todos los paquetes de Samba, Cockpit, desmontará el disco y limpiará las configuraciones.\n\n¿Confirmas que deseas restablecer el servidor a su estado base limpio?" 12 72); then
        clear 2>/dev/null || true
        bash "$SRC_DIR/core/uninstall.sh" --yes
        whiptail --title "$APP_TITLE" --ok-button "< Aceptar >" \
            --msgbox "✔ El servidor ha sido desinstalado y el sistema quedó completamente limpio." 8 65
    fi
}

# Función para verificar si el servidor ya fue desplegado
obtener_estado_despliegue() {
    if [ -f /etc/samba/smb.conf ] && getent group grp_sistemas &>/dev/null && [ -d /srv/nas ]; then
        local rol="ARCHIVOS"
        if grep -qi "Servidor BACKUP" /etc/samba/smb.conf 2>/dev/null; then
            rol="BACKUP"
        fi
        echo "$rol"
    else
        echo ""
    fi
}

# ==============================================================================
# MENÚ PRINCIPAL INTERACTIVO
# ==============================================================================
while true; do
    ROL_ACTUAL=$(obtener_estado_despliegue)
    if [ -n "$ROL_ACTUAL" ]; then
        TAG_OPC1="[1]  [✔] Servidor Configurado (Rol: $ROL_ACTUAL)"
    else
        TAG_OPC1="[1]  Desplegar Servidor (NAS de Archivos o Central de Backup)"
    fi

    OPCION=$(whiptail --title "$APP_TITLE" \
        --ok-button "< Seleccionar >" --cancel-button "< Salir >" \
        --menu "Selecciona una opción usando las flechas y presiona Enter:" 21 74 9 \
        "1" "$TAG_OPC1" \
        "2" "[2]  Gestión de Grupos de Seguridad (Crear / Listar / Eliminar)" \
        "3" "[3]  Gestión de Recursos Compartidos (Ver / Crear / Deshabilitar / Borrar)" \
        "4" "[4]  Gestión de Tareas de Backup (Windows / Linux / Local)" \
        "5" "[5]  Gestión de Usuarios y Empleados (Crear, Grupos y Claves)" \
        "6" "[6]  Ver Diagnóstico, Discos y Recursos Compartidos" \
        "7" "[7]  Reiniciar Servicios de Red (Samba / Cockpit)" \
        "8" "[8]  Buscar Actualizaciones desde GitHub (Auto-Update)" \
        "9" "[9]  Desinstalar y Limpiar Servidor" 3>&1 1>&2 2>&3)

    RET=$?
    if [ $RET -ne 0 ]; then
        clear 2>/dev/null || true
        break
    fi

    case "$OPCION" in
        1)
            if [ -n "$ROL_ACTUAL" ]; then
                whiptail --title "Servidor Ya Configurado" --ok-button "< Aceptar >" \
                    --msgbox "✔ Este servidor ya se encuentra DESPLEGADO y EN LÍNEA (Rol: $ROL_ACTUAL).\n\nPara administrar el servidor utiliza las siguientes opciones:\n • Crear grupos de seguridad:       Opción [2]\n • Crear carpetas compartidas ($):   Opción [3]\n • Programar copias de seguridad:   Opción [4]\n • Gestionar usuarios y accesos:    Opción [5]\n\n💡 Si deseas reconfigurar desde cero o cambiar el rol, primero debes ejecutar la opción [9] Desinstalar y Limpiar Servidor." 16 72
            else
                instalar_nas
            fi
            ;;
        2) gestionar_grupos ;;
        3) gestionar_recursos_compartidos ;;
        4) gestionar_backups ;;
        5) gestionar_usuarios ;;
        6) diagnostico_nas ;;
        7) 
            if (whiptail --title "Confirmar Reinicio" \
                --yes-button "< Sí, Reiniciar >" --no-button "< Cancelar >" \
                --yesno "¿Deseas reiniciar los servicios de red de Samba, WSDD2 y Cockpit ahora?" 9 65); then
                systemctl restart smbd nmbd wsdd2 cockpit.socket cockpit.service 2>/dev/null || true
                whiptail --title "$APP_TITLE" --ok-button "< Aceptar >" \
                    --msgbox "✔ Servicios de Samba, WSDD2 y Cockpit reiniciados correctamente." 8 60
            fi
            ;;
        8) actualizar_desde_git ;;
        9) desinstalar_guiado ;;
    esac
done
