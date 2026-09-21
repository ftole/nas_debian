#!/bin/bash
# ==============================================================================
# Módulo: Gestión de Grupos de Seguridad
# ==============================================================================

gestionar_grupos() {
    local OPC_GRP TABLA_GRUPOS NUEVO_GRP GRUPOS_ELIM MENU_ELIM GRP_BORRAR g
    while true; do
        OPC_GRP=$(whiptail --title "$APP_TITLE" \
            --ok-button "< Seleccionar >" --cancel-button "< Volver >" \
            --menu "GESTIÓN DE GRUPOS DE SEGURIDAD:" 16 68 4 \
            "1" "[*] Listar grupos existentes" \
            "2" "[+] Crear un nuevo grupo" \
            "3" "[-] Eliminar un grupo existente" \
            "4" "[<] Volver al Menú Principal" 3>&1 1>&2 2>&3)

        RET=$?
        if [ $RET -ne 0 ] || [ "$OPC_GRP" == "4" ]; then
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
                GRUPOS_ELIM=$(awk -F: '$1 ~ /^grp_/ && $1 != "grp_sistemas" {print $1}' /etc/group | sort)
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
