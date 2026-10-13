#!/bin/bash
# ==============================================================================
# Módulo: Gestión de Papelera de Reciclaje (/srv/nas/.trash) y Restauración
# ==============================================================================

gestionar_papelera() {
    local OPC_TRASH CLI_RUNNER OUT ITEM_ID RET

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
        OPC_TRASH=$(whiptail --title "$APP_TITLE" \
            --ok-button "< Seleccionar >" --cancel-button "< Volver >" \
            --menu "PAPELERA DE RECICLAJE Y RESTAURACIÓN:" 16 68 5 \
            "1" "[*] Listar Elementos en Papelera (/srv/nas/.trash)" \
            "2" "[R] Restaurar Elemento a su Ubicación Original" \
            "3" "[-] Eliminar Elemento Definitivamente" \
            "4" "[X] Vaciar Papelera de Reciclaje por Completo" \
            "5" "[<] Volver al Menú Principal" 3>&1 1>&2 2>&3)

        RET=$?
        if [ $RET -ne 0 ] || [ "$OPC_TRASH" == "5" ]; then
            break
        fi

        case "$OPC_TRASH" in
            1)
                OUT=$($CLI_RUNNER trash list 2>&1 || true)
                whiptail --title "Elementos en Papelera de Reciclaje" --ok-button "< Aceptar >" \
                    --msgbox "$OUT" 20 78
                ;;

            2)
                ITEM_ID=$(whiptail --title "Restaurar Elemento" \
                    --ok-button "< Restaurar >" --cancel-button "< Cancelar >" \
                    --inputbox "Ingresa el ID numérico del elemento a restaurar (obtenlo en la opción 1):" 10 65 "" 3>&1 1>&2 2>&3)
                RET=$?
                if [ $RET -eq 0 ] && [ -n "$ITEM_ID" ]; then
                    OUT=$($CLI_RUNNER trash restore "$ITEM_ID" 2>&1 || true)
                    whiptail --title "Resultado de Restauración" --ok-button "< Aceptar >" \
                        --msgbox "$OUT" 10 65
                fi
                ;;

            3)
                ITEM_ID=$(whiptail --title "Eliminar Permanentemente" \
                    --ok-button "< Siguiente >" --cancel-button "< Cancelar >" \
                    --inputbox "Ingresa el ID del elemento a borrar definitivamente:" 10 65 "" 3>&1 1>&2 2>&3)
                RET=$?
                if [ $RET -eq 0 ] && [ -n "$ITEM_ID" ]; then
                    if (whiptail --title "Confirmar Eliminación Definitiva" \
                        --yes-button "< Sí, Eliminar >" --no-button "< Cancelar >" \
                        --yesno "¿Estás seguro de que deseas eliminar permanentemente el elemento #$ITEM_ID?\n\nEsta acción no se puede deshacer." 9 65); then
                        OUT=$($CLI_RUNNER trash delete "$ITEM_ID" 2>&1 || true)
                        whiptail --title "Resultado" --ok-button "< Aceptar >" \
                            --msgbox "$OUT" 10 65
                    fi
                fi
                ;;

            4)
                if (whiptail --title "Vaciar Papelera de Reciclaje" \
                    --yes-button "< Sí, Vaciar Todo >" --no-button "< Cancelar >" \
                    --yesno "¡ADVERTENCIA! Se borrarán todos los archivos y carpetas contenidos en la papelera de reciclaje.\n\n¿Deseas continuar?" 10 65); then
                    OUT=$($CLI_RUNNER trash empty 2>&1 || true)
                    whiptail --title "Resultado de Vaciado" --ok-button "< Aceptar >" \
                        --msgbox "$OUT" 10 65
                fi
                ;;
        esac
    done
}
