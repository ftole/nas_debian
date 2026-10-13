#!/bin/bash
# ==============================================================================
# Módulo: Gestión de Almacenamiento, Discos, LVM, Btrfs, Scrub y TRIM
# ==============================================================================

gestionar_almacenamiento() {
    local OPC_STG CLI_RUNNER OUT DISCO_SEL FS_SEL CONFIRM_TXT
    local VG_NAME LV_NAME LV_SIZE SUB_OPC SUB_NAME UMOUNT_TARGET
    local RET

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
        OPC_STG=$(whiptail --title "$APP_TITLE" \
            --ok-button "< Seleccionar >" --cancel-button "< Volver >" \
            --menu "GESTIÓN DE ALMACENAMIENTO Y DISCOS:" 19 72 8 \
            "1" "[*] Resumen y Capacidad del Almacenamiento (/srv/nas)" \
            "2" "[D] Listar Discos y Particiones Físicas" \
            "3" "[F] Formatear y Montar Disco en /srv/nas (ext4 / btrfs)" \
            "4" "[L] Crear Volumen LVM (PV/VG/LV) y Montar" \
            "5" "[S] Gestionar Subvolúmenes Btrfs" \
            "6" "[C] Iniciar Auditoría BTRFS Scrub (Detección Bit Rot)" \
            "7" "[T] Ejecutar Optimización SSD TRIM (fstrim)" \
            "8" "[U] Desmontar Almacenamiento de Forma Segura" \
            "9" "[<] Volver al Menú Principal" 3>&1 1>&2 2>&3)

        RET=$?
        if [ $RET -ne 0 ] || [ "$OPC_STG" == "9" ]; then
            break
        fi

        case "$OPC_STG" in
            1)
                OUT=$($CLI_RUNNER storage overview 2>&1 || true)
                whiptail --title "Capacidad y Sistema de Archivos" --ok-button "< Aceptar >" \
                    --msgbox "$OUT" 16 74
                ;;

            2)
                OUT=$($CLI_RUNNER storage disks 2>&1 || true)
                whiptail --title "Discos y Particiones del Sistema" --ok-button "< Aceptar >" \
                    --msgbox "$OUT" 20 78
                ;;

            3)
                DISCO_SEL=$(whiptail --title "Formatear Almacenamiento" \
                    --ok-button "< Siguiente >" --cancel-button "< Cancelar >" \
                    --inputbox "Ingresa la ruta absoluta del disco o partición (ej. /dev/sdb o /dev/nvme0n1p1):" 10 68 "" 3>&1 1>&2 2>&3)
                RET=$?
                if [ $RET -ne 0 ] || [ -z "$DISCO_SEL" ]; then
                    continue
                fi

                FS_SEL=$(whiptail --title "Tipo de Sistema de Archivos" \
                    --ok-button "< Siguiente >" --cancel-button "< Cancelar >" \
                    --menu "Selecciona el filesystem a aplicar:" 12 60 2 \
                    "ext4"  "Recomendado para servidores de archivos (rápido y estable)" \
                    "btrfs" "Recomendado para servidores de backups (compresión zstd)" 3>&1 1>&2 2>&3)
                RET=$?
                if [ $RET -ne 0 ] || [ -z "$FS_SEL" ]; then
                    continue
                fi

                CONFIRM_TXT=$(whiptail --title "¡ADVERTENCIA CRÍTICA DE FORMATEO!" \
                    --ok-button "< Confirmar Formateo >" --cancel-button "< Cancelar >" \
                    --inputbox "¡ESTA ACCIÓN BORRARÁ TODOS LOS DATOS EN $DISCO_SEL!\nPara confirmar escribe exactamente: SI-FORMATEAR" 12 68 "" 3>&1 1>&2 2>&3)
                RET=$?
                if [ $RET -ne 0 ] || [ "$CONFIRM_TXT" != "SI-FORMATEAR" ]; then
                    whiptail --title "Operación Cancelada" --ok-button "< Aceptar >" \
                        --msgbox "Formateo cancelado. No se realizaron cambios en el disco." 8 60
                    continue
                fi

                OUT=$($CLI_RUNNER storage format "$DISCO_SEL" "$FS_SEL" --confirm=SI-FORMATEAR 2>&1 || true)
                whiptail --title "Resultado del Formateo" --ok-button "< Aceptar >" \
                    --msgbox "$OUT" 12 70
                ;;

            4)
                DISCO_SEL=$(whiptail --title "Crear Volumen LVM" \
                    --ok-button "< Siguiente >" --cancel-button "< Cancelar >" \
                    --inputbox "Ingresa el disco para inicializar como Physical Volume (ej. /dev/sdb):" 10 68 "" 3>&1 1>&2 2>&3)
                RET=$?
                if [ $RET -ne 0 ] || [ -z "$DISCO_SEL" ]; then
                    continue
                fi

                VG_NAME=$(whiptail --title "Nombre del Grupo de Volúmenes (VG)" \
                    --ok-button "< Siguiente >" --cancel-button "< Cancelar >" \
                    --inputbox "Ingresa el nombre del Volume Group:" 10 65 "vg_nas" 3>&1 1>&2 2>&3)
                RET=$?
                if [ $RET -ne 0 ] || [ -z "$VG_NAME" ]; then
                    continue
                fi

                LV_NAME=$(whiptail --title "Nombre del Volumen Lógico (LV)" \
                    --ok-button "< Siguiente >" --cancel-button "< Cancelar >" \
                    --inputbox "Ingresa el nombre del Logical Volume:" 10 65 "lv_datos" 3>&1 1>&2 2>&3)
                RET=$?
                if [ $RET -ne 0 ] || [ -z "$LV_NAME" ]; then
                    continue
                fi

                FS_SEL=$(whiptail --title "Filesystem para LVM" \
                    --ok-button "< Siguiente >" --cancel-button "< Cancelar >" \
                    --menu "Selecciona el filesystem a aplicar:" 12 60 2 \
                    "ext4"  "Filesystem transaccional ext4" \
                    "btrfs" "Filesystem Btrfs con compresión" 3>&1 1>&2 2>&3)
                RET=$?
                if [ $RET -ne 0 ] || [ -z "$FS_SEL" ]; then
                    continue
                fi

                LV_SIZE=$(whiptail --title "Tamaño del Volumen Lógico" \
                    --ok-button "< Siguiente >" --cancel-button "< Cancelar >" \
                    --inputbox "Ingresa el tamaño (ej. 100%FREE, 50%VG, 500G):" 10 65 "100%FREE" 3>&1 1>&2 2>&3)
                RET=$?
                if [ $RET -ne 0 ] || [ -z "$LV_SIZE" ]; then
                    continue
                fi

                CONFIRM_TXT=$(whiptail --title "Confirmación LVM" \
                    --ok-button "< Aprovisionar LVM >" --cancel-button "< Cancelar >" \
                    --inputbox "Se inicializará LVM en $DISCO_SEL ($VG_NAME/$LV_NAME).\nEscribe exactamente: SI-FORMATEAR" 11 68 "" 3>&1 1>&2 2>&3)
                RET=$?
                if [ $RET -ne 0 ] || [ "$CONFIRM_TXT" != "SI-FORMATEAR" ]; then
                    whiptail --title "Operación Cancelada" --ok-button "< Aceptar >" \
                        --msgbox "Aprovisionamiento LVM cancelado." 8 50
                    continue
                fi

                OUT=$($CLI_RUNNER storage lvm "$DISCO_SEL" "$VG_NAME" "$LV_NAME" "$FS_SEL" "$LV_SIZE" --confirm=SI-FORMATEAR 2>&1 || true)
                whiptail --title "Resultado Aprovisionamiento LVM" --ok-button "< Aceptar >" \
                    --msgbox "$OUT" 12 70
                ;;

            5)
                SUB_OPC=$(whiptail --title "Subvolúmenes Btrfs" \
                    --ok-button "< Seleccionar >" --cancel-button "< Volver >" \
                    --menu "GESTIÓN DE SUBVOLÚMENES:" 13 65 3 \
                    "1" "[*] Listar subvolúmenes existentes en /srv/nas" \
                    "2" "[+] Crear un nuevo subvolumen Btrfs" \
                    "3" "[<] Regresar" 3>&1 1>&2 2>&3)
                RET=$?
                if [ $RET -ne 0 ] || [ "$SUB_OPC" == "3" ]; then
                    continue
                fi

                if [ "$SUB_OPC" == "1" ]; then
                    OUT=$($CLI_RUNNER storage subvol list 2>&1 || true)
                    whiptail --title "Subvolúmenes Btrfs" --ok-button "< Aceptar >" \
                        --msgbox "$OUT" 16 72
                elif [ "$SUB_OPC" == "2" ]; then
                    SUB_NAME=$(whiptail --title "Nuevo Subvolumen Btrfs" \
                        --ok-button "< Crear >" --cancel-button "< Cancelar >" \
                        --inputbox "Ingresa el nombre del subvolumen (ej. backups_finanzas):" 10 65 "" 3>&1 1>&2 2>&3)
                    RET=$?
                    if [ $RET -eq 0 ] && [ -n "$SUB_NAME" ]; then
                        OUT=$($CLI_RUNNER storage subvol create "$SUB_NAME" 2>&1 || true)
                        whiptail --title "Resultado" --ok-button "< Aceptar >" \
                            --msgbox "$OUT" 10 65
                    fi
                fi
                ;;

            6)
                if (whiptail --title "Auditoría BTRFS Scrub" \
                    --yes-button "< Iniciar Scrub >" --no-button "< Cancelar >" \
                    --yesno "¿Deseas iniciar una auditoría de integridad BTRFS scrub sobre /srv/nas?\n\nEsta tarea verificará las sumas de comprobación criptográficas de cada bloque para detectar y corregir corrupción silenciosa (Bit Rot)." 12 70); then
                    OUT=$($CLI_RUNNER storage scrub 2>&1 || true)
                    whiptail --title "BTRFS Scrub" --ok-button "< Aceptar >" \
                        --msgbox "$OUT" 10 65
                fi
                ;;

            7)
                if (whiptail --title "Optimización SSD TRIM" \
                    --yes-button "< Ejecutar TRIM >" --no-button "< Cancelar >" \
                    --yesno "¿Deseas ejecutar fstrim sobre /srv/nas ahora mismo?\n\nEsta operación descartará bloques libres en unidades de estado sólido para mantener velocidades sostenidas y alargar la vida útil del almacenamiento flash." 12 70); then
                    OUT=$($CLI_RUNNER storage trim 2>&1 || true)
                    whiptail --title "SSD TRIM" --ok-button "< Aceptar >" \
                        --msgbox "$OUT" 10 65
                fi
                ;;

            8)
                UMOUNT_TARGET=$(whiptail --title "Desmontar Almacenamiento" \
                    --ok-button "< Desmontar >" --cancel-button "< Cancelar >" \
                    --inputbox "Ingresa el punto de montaje o dispositivo a desmontar:" 10 65 "/srv/nas" 3>&1 1>&2 2>&3)
                RET=$?
                if [ $RET -eq 0 ] && [ -n "$UMOUNT_TARGET" ]; then
                    OUT=$($CLI_RUNNER storage unmount "$UMOUNT_TARGET" 2>&1 || true)
                    whiptail --title "Resultado de Desmontaje" --ok-button "< Aceptar >" \
                        --msgbox "$OUT" 10 65
                fi
                ;;
        esac
    done
}
