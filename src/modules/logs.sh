#!/bin/bash
# ==============================================================================
# Módulo: Visor de Auditoría y Registros Unificados (Samba, Admin, Backups, Journal)
# ==============================================================================

gestionar_registros_auditoria() {
    local OPC_LOG CLI_RUNNER OUT SEARCH_QUERY RET

    CLI_RUNNER=""
    if [ -x "/usr/local/bin/nas" ]; then
        CLI_RUNNER="/usr/local/bin/nas"
    elif [ -f "$SRC_DIR/../bin/nas-cli" ]; then
        CLI_RUNNER="php $SRC_DIR/../bin/nas-cli"
    elif [ -f "/opt/nas_debian/bin/nas-cli" ]; then
        CLI_RUNNER="php /opt/nas_debian/bin/nas-cli"
    fi

    if [ -z "$CLI_RUNNER" ]; then
        whiptail --title "$APP_TITLE" --ok-button "< Aceptar >" \
            --msgbox "Error: No se encontró el CLI de administración nas-cli." 8 60
        return 1
    fi

    while true; do
        OPC_LOG=$(whiptail --title "$APP_TITLE" \
            --ok-button "< Seleccionar >" --cancel-button "< Volver >" \
            --menu "AUDITORÍA UNIFICADA Y BITÁCORAS DEL SISTEMA:" 18 72 7 \
            "1" "[S] Auditoría de Archivos Samba (full_audit: apertura, borrado, renombrado)" \
            "2" "[A] Auditoría de Acciones Administrativas del Servidor (nas-admin.log)" \
            "3" "[B] Registros y Bitácoras de Copias de Seguridad (backups_master.log)" \
            "4" "[J] Registros de Journald del Sistema (systemd journalctl)" \
            "5" "[*] Vista Unificada Multidimensional (Todos los eventos recientes)" \
            "6" "[Q] Buscar en Registros con Filtro de Texto o Usuario" \
            "7" "[<] Volver al Menú Principal" 3>&1 1>&2 2>&3)

        RET=$?
        if [ $RET -ne 0 ] || [ "$OPC_LOG" == "7" ]; then
            break
        fi

        case "$OPC_LOG" in
            1)
                OUT=$($CLI_RUNNER log samba --limit=30 2>&1 || true)
                whiptail --title "Auditoría de Archivos Samba (full_audit)" --ok-button "< Aceptar >" \
                    --msgbox "$OUT" 22 78
                ;;

            2)
                OUT=$($CLI_RUNNER log admin --limit=30 2>&1 || true)
                whiptail --title "Auditoría de Acciones Administrativas" --ok-button "< Aceptar >" \
                    --msgbox "$OUT" 22 78
                ;;

            3)
                OUT=$($CLI_RUNNER log backup --limit=30 2>&1 || true)
                whiptail --title "Bitácoras de Copias de Seguridad" --ok-button "< Aceptar >" \
                    --msgbox "$OUT" 22 78
                ;;

            4)
                OUT=$($CLI_RUNNER log journal --limit=30 2>&1 || true)
                whiptail --title "Registros de Journald (systemd)" --ok-button "< Aceptar >" \
                    --msgbox "$OUT" 22 78
                ;;

            5)
                OUT=$($CLI_RUNNER log all --limit=30 2>&1 || true)
                whiptail --title "Auditoría Unificada del Sistema" --ok-button "< Aceptar >" \
                    --msgbox "$OUT" 22 78
                ;;

            6)
                SEARCH_QUERY=$(whiptail --title "Buscar en Registros" \
                    --ok-button "< Buscar >" --cancel-button "< Cancelar >" \
                    --inputbox "Ingresa término de búsqueda (ej. usuario, IP, nombre de archivo o acción):" 10 65 "" 3>&1 1>&2 2>&3)
                RET=$?
                if [ $RET -eq 0 ] && [ -n "$SEARCH_QUERY" ]; then
                    OUT=$($CLI_RUNNER log all --query="$SEARCH_QUERY" --limit=40 2>&1 || true)
                    whiptail --title "Resultados de Búsqueda: $SEARCH_QUERY" --ok-button "< Aceptar >" \
                        --msgbox "$OUT" 22 78
                fi
                ;;
        esac
    done
}
