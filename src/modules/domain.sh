#!/bin/bash
# ==============================================================================
# Módulo: Integración Corporativa con Active Directory (realmd / SSSD / Kerberos)
# ==============================================================================

gestionar_dominio_ad() {
    local OPC_AD CLI_RUNNER OUT DOMAIN_NAME AD_USER AD_PASS AD_OU RET

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
        OPC_AD=$(whiptail --title "$APP_TITLE" \
            --ok-button "< Seleccionar >" --cancel-button "< Volver >" \
            --menu "INTEGRACIÓN CON DIRECTORIO ACTIVO (AD / REALMD):" 16 68 5 \
            "1" "[*] Estado Actual de Integración con Dominio" \
            "2" "[?] Descubrir y Diagnosticar Controlador de Dominio" \
            "3" "[+] Unir este Servidor al Dominio Active Directory" \
            "4" "[-] Desvincular Servidor del Dominio (realm leave)" \
            "5" "[<] Volver al Menú Principal" 3>&1 1>&2 2>&3)

        RET=$?
        if [ $RET -ne 0 ] || [ "$OPC_AD" == "5" ]; then
            break
        fi

        case "$OPC_AD" in
            1)
                OUT=$($CLI_RUNNER domain status 2>&1 || true)
                whiptail --title "Estado de Integración con Dominio" --ok-button "< Aceptar >" \
                    --msgbox "$OUT" 15 72
                ;;

            2)
                DOMAIN_NAME=$(whiptail --title "Descubrir Dominio" \
                    --ok-button "< Descubrir >" --cancel-button "< Cancelar >" \
                    --inputbox "Ingresa el FQDN del dominio (ej. corp.local o ad.empresa.com):" 10 65 "" 3>&1 1>&2 2>&3)
                RET=$?
                if [ $RET -eq 0 ] && [ -n "$DOMAIN_NAME" ]; then
                    OUT=$($CLI_RUNNER domain discover "$DOMAIN_NAME" 2>&1 || true)
                    whiptail --title "Resultado de Descubrimiento" --ok-button "< Aceptar >" \
                        --msgbox "$OUT" 16 74
                fi
                ;;

            3)
                DOMAIN_NAME=$(whiptail --title "Unión a Dominio - Dominio FQDN" \
                    --ok-button "< Siguiente >" --cancel-button "< Cancelar >" \
                    --inputbox "Ingresa el nombre del dominio Active Directory:" 10 65 "" 3>&1 1>&2 2>&3)
                RET=$?
                if [ $RET -ne 0 ] || [ -z "$DOMAIN_NAME" ]; then
                    continue
                fi

                AD_USER=$(whiptail --title "Unión a Dominio - Usuario Administrador" \
                    --ok-button "< Siguiente >" --cancel-button "< Cancelar >" \
                    --inputbox "Ingresa la cuenta con permisos para unir equipos (ej. Administrator o admin_it):" 10 65 "Administrator" 3>&1 1>&2 2>&3)
                RET=$?
                if [ $RET -ne 0 ] || [ -z "$AD_USER" ]; then
                    continue
                fi

                AD_PASS=$(whiptail --title "Unión a Dominio - Contraseña" \
                    --ok-button "< Siguiente >" --cancel-button "< Cancelar >" \
                    --passwordbox "Ingresa la contraseña de la cuenta $AD_USER:" 10 65 3>&1 1>&2 2>&3)
                RET=$?
                if [ $RET -ne 0 ] || [ -z "$AD_PASS" ]; then
                    continue
                fi

                AD_OU=$(whiptail --title "Unidad Organizativa (Opcional)" \
                    --ok-button "< Unir al Dominio >" --cancel-button "< Cancelar >" \
                    --inputbox "OU opcional (ej. OU=Servidores,DC=corp,DC=local o dejar vacío):" 10 65 "" 3>&1 1>&2 2>&3)
                RET=$?
                if [ $RET -ne 0 ]; then
                    continue
                fi

                if [ -n "$AD_OU" ]; then
                    OUT=$($CLI_RUNNER domain join "$DOMAIN_NAME" --user="$AD_USER" --password="$AD_PASS" --ou="$AD_OU" 2>&1 || true)
                else
                    OUT=$($CLI_RUNNER domain join "$DOMAIN_NAME" --user="$AD_USER" --password="$AD_PASS" 2>&1 || true)
                fi

                whiptail --title "Resultado de Unión al Dominio" --ok-button "< Aceptar >" \
                    --msgbox "$OUT" 12 70
                ;;

            4)
                if (whiptail --title "Desvincular del Dominio" \
                    --yes-button "< Sí, Desvincular >" --no-button "< Cancelar >" \
                    --yesno "¿Estás seguro de que deseas desvincular este servidor del dominio Active Directory?\n\nLos usuarios de dominio ya no podrán autenticarse en el NAS." 11 65); then
                    OUT=$($CLI_RUNNER domain leave 2>&1 || true)
                    whiptail --title "Resultado" --ok-button "< Aceptar >" \
                        --msgbox "$OUT" 10 65
                fi
                ;;
        esac
    done
}
