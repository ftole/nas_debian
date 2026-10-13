#!/bin/bash
# ==============================================================================
# Módulo: Gestión de Grupos de Seguridad
# ==============================================================================

gestionar_grupos() {
    local OPC_GRP TABLA_GRUPOS NUEVO_GRP GRUPOS_ELIM MENU_ELIM GRP_BORRAR g
    local GRUPOS_REN MENU_REN GRP_REN_SEL NUEVO_NOMBRE
    while true; do
        OPC_GRP=$(whiptail --title "$APP_TITLE" \
            --ok-button "< Seleccionar >" --cancel-button "< Volver >" \
            --menu "GESTIÓN DE GRUPOS DE SEGURIDAD:" 17 68 5 \
            "1" "[*] Listar grupos existentes" \
            "2" "[+] Crear un nuevo grupo" \
            "3" "[R] Renombrar un grupo existente" \
            "4" "[-] Eliminar un grupo existente" \
            "5" "[<] Volver al Menú Principal" 3>&1 1>&2 2>&3)

        RET=$?
        if [ $RET -ne 0 ] || [ "$OPC_GRP" == "5" ]; then
            break
        fi

        case "$OPC_GRP" in
            1)
                TABLA_GRUPOS=$(python3 -c '
groups = []
with open("/etc/group", "r", encoding="utf-8") as f:
    for line in f:
        parts = line.strip().split(":")
        if len(parts) >= 4 and parts[0].startswith("grp_"):
            gname, gid, members = parts[0], parts[2], parts[3] if parts[3] else "(sin miembros)"
            groups.append((gname, gid, members))

if not groups:
    print("  (No se encontraron grupos de seguridad creados)")
    exit(0)

w_name = max(max(len(g[0]) for g in groups), 20)
w_gid  = max(max(len(g[1]) for g in groups), 6)
w_mem  = max(max(len(g[2]) for g in groups), 28)

print("┌─{}─┬─{}─┬─{}─┐".format("─"*w_name, "─"*w_gid, "─"*w_mem))
print("│ {} │ {} │ {} │".format("NOMBRE DEL GRUPO".ljust(w_name), "GID".ljust(w_gid), "USUARIOS MIEMBROS".ljust(w_mem)))
print("├─{}─┼─{}─┼─{}─┤".format("─"*w_name, "─"*w_gid, "─"*w_mem))
for gname, gid, members in sorted(groups, key=lambda x: x[0]):
    print("│ {} │ {} │ {} │".format(gname.ljust(w_name), gid.ljust(w_gid), members.ljust(w_mem)))
print("└─{}─┴─{}─┴─{}─┘".format("─"*w_name, "─"*w_gid, "─"*w_mem))
')
                whiptail --title "CRUD: Grupos de Seguridad" --ok-button "< Aceptar >" \
                    --msgbox "$TABLA_GRUPOS" 18 76
                ;;

            2)
                NUEVO_GRP=$(whiptail --title "$APP_TITLE" \
                    --ok-button "< Siguiente >" --cancel-button "< Cancelar >" \
                    --inputbox "Ingresa el nombre del grupo (ej. grp_contabilidad o grp_ventas):" 10 65 "grp_" 3>&1 1>&2 2>&3)
                RET=$?
                if [ $RET -eq 0 ] && [ -n "$NUEVO_GRP" ]; then
                    NUEVO_GRP=$(echo "$NUEVO_GRP" | tr ' ' '_' | tr '[:upper:]' '[:lower:]' | tr -cd 'a-z0-9_-')
                    if [[ "$NUEVO_GRP" != grp_* ]]; then
                        NUEVO_GRP="grp_${NUEVO_GRP}"
                    fi
                    if [ "$NUEVO_GRP" == "grp_" ]; then
                        whiptail --title "Nombre Invalido" --ok-button "< Aceptar >" \
                            --msgbox "El nombre del grupo no puede estar vacio ni contener solo simbolos." 9 65
                        continue
                    fi
                    if groupadd "$NUEVO_GRP" 2>/dev/null; then
                        whiptail --title "$APP_TITLE" --ok-button "< Aceptar >" \
                            --msgbox "✔ Grupo \"$NUEVO_GRP\" creado exitosamente." 8 50
                    else
                        whiptail --title "Error" --ok-button "< Aceptar >" \
                            --msgbox "El grupo \"$NUEVO_GRP\" ya existe o el nombre es inválido." 8 55
                    fi
                fi
                ;;

            3)
                GRUPOS_REN=$(awk -F: '$1 ~ /^grp_/ && $1 != "grp_samba" && $1 != "grp_web" && $1 != "grp_superadmin" {print $1}' /etc/group | sort)
                if [ -z "$GRUPOS_REN" ]; then
                    whiptail --title "Aviso" --ok-button "< Aceptar >" \
                        --msgbox "No hay grupos personalizados disponibles para renombrar." 8 55
                    continue
                fi

                MENU_REN=()
                for g in $GRUPOS_REN; do
                    MENU_REN+=("$g" "Grupo_Personalizado")
                done

                GRP_REN_SEL=$(whiptail --title "Renombrar Grupo" \
                    --ok-button "< Siguiente >" --cancel-button "< Cancelar >" \
                    --menu "Selecciona el grupo que deseas renombrar:" 16 68 6 \
                    "${MENU_REN[@]}" 3>&1 1>&2 2>&3)
                RET=$?
                if [ $RET -eq 0 ] && [ -n "$GRP_REN_SEL" ]; then
                    NUEVO_NOMBRE=$(whiptail --title "Nuevo Nombre de Grupo" \
                        --ok-button "< Renombrar >" --cancel-button "< Cancelar >" \
                        --inputbox "Ingresa el nuevo nombre para $GRP_REN_SEL:" 10 65 "$GRP_REN_SEL" 3>&1 1>&2 2>&3)
                    RET=$?
                    if [ $RET -eq 0 ] && [ -n "$NUEVO_NOMBRE" ]; then
                        NUEVO_NOMBRE=$(echo "$NUEVO_NOMBRE" | tr ' ' '_' | tr '[:upper:]' '[:lower:]' | tr -cd 'a-z0-9_-')
                        if [[ "$NUEVO_NOMBRE" != grp_* ]]; then
                            NUEVO_NOMBRE="grp_${NUEVO_NOMBRE}"
                        fi
                        if [ "$NUEVO_NOMBRE" == "grp_superadmin" ] || [ "$NUEVO_NOMBRE" == "grp_samba" ] || [ "$NUEVO_NOMBRE" == "grp_web" ]; then
                            whiptail --title "Error" --ok-button "< Aceptar >" \
                                --msgbox "No puedes renombrar un grupo al nombre reservado $NUEVO_NOMBRE." 8 60
                            continue
                        fi
                        if groupmod -n "$NUEVO_NOMBRE" "$GRP_REN_SEL" 2>/dev/null; then
                            whiptail --title "$APP_TITLE" --ok-button "< Aceptar >" \
                                --msgbox "✔ Grupo \"$GRP_REN_SEL\" renombrado a \"$NUEVO_NOMBRE\" con éxito." 8 55
                        else
                            whiptail --title "Error" --ok-button "< Aceptar >" \
                                --msgbox "Fallo al renombrar grupo (¿el nombre ya existe?)." 8 55
                        fi
                    fi
                fi
                ;;

            4)
                GRUPOS_ELIM=$(awk -F: '$1 ~ /^grp_/ && $1 != "grp_samba" && $1 != "grp_web" && $1 != "grp_superadmin" {print $1}' /etc/group | sort)
                if [ -z "$GRUPOS_ELIM" ]; then
                    whiptail --title "Aviso" --ok-button "< Aceptar >" \
                        --msgbox "No hay grupos personalizados disponibles para eliminar." 8 55
                    continue
                fi

                local -a MENU_ELIM=()
                for g in $GRUPOS_ELIM; do
                    MENU_ELIM+=("$g" "Grupo_Personalizado")
                done

                GRP_BORRAR=$(whiptail --title "Eliminar Grupo" \
                    --ok-button "< Siguiente >" --cancel-button "< Cancelar >" \
                    --menu "Selecciona el grupo que deseas eliminar:" 16 68 6 \
                    "${MENU_ELIM[@]}" 3>&1 1>&2 2>&3)

                RET=$?
                if [ $RET -eq 0 ] && [ -n "$GRP_BORRAR" ]; then
                    if (whiptail --title "Confirmar Eliminación" \
                        --yes-button "< Sí, Eliminar >" --no-button "< Cancelar >" \
                        --yesno "¿Estás seguro de que deseas eliminar el grupo \"$GRP_BORRAR\"?" 8 60); then
                        groupdel "$GRP_BORRAR" 2>/dev/null || true
                        whiptail --title "$APP_TITLE" --ok-button "< Aceptar >" \
                            --msgbox "✔ Grupo \"$GRP_BORRAR\" eliminado correctamente." 8 50
                    fi
                fi
                ;;
        esac
    done
}
