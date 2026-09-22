#!/bin/bash
# ==============================================================================
# Módulo: Gestión de Recursos Compartidos en Red (Samba)
# ==============================================================================

gestionar_recursos_compartidos() {
    local OPC_SHARE TABLA_SHARES NOMBRE_SHARE OPC_VIS BROWSEABLE VIS_TXT OPC_PUB GUEST_OK
    local NOMBRE_DIR COMENTARIO RUTA_SHARE RUTA_REAL TIPO_PERM GRUPOS_DISP LISTA_OPC GRUPOS_SEL TODOS_GRPS
    local VALID_USERS WRITE_LIST READ_ONLY MASK GRUPO_DUENO TIPO_TXT GRUPOS_RO GRUPOS_RO_ESTRICTO
    local GRUPO_RW MENU_RW STATUS LISTA_SHARES MENU_ITEMS TARGET_SHARE NUEVO_ESTADO
    local LISTA_ELIMINAR MENU_DEL SHARE_A_BORRAR BACKUP_SMB TMP_SMB g
    
    if [ ! -f /etc/samba/smb.conf ]; then
        whiptail --title "Samba no configurado" --ok-button "< Aceptar >" \
            --msgbox "Samba aún no está instalado o configurado.\nEjecuta primero la opción [1] Desplegar Servidor." 9 60
        return
    fi

    while true; do
        OPC_SHARE=$(whiptail --title "$APP_TITLE" \
            --ok-button "< Seleccionar >" --cancel-button "< Volver >" \
            --menu "GESTIÓN DE RECURSOS COMPARTIDOS (SAMBA):" 17 72 5 \
            "1" "[*] Listar y ver detalles de recursos compartidos" \
            "2" "[+] Crear un nuevo recurso compartido (Carpeta de Red)" \
            "3" "[~] Alternar estado (Habilitar / Deshabilitar recurso)" \
            "4" "[-] Eliminar un recurso compartido de la red" \
            "5" "[<] Volver al Menú Principal" 3>&1 1>&2 2>&3)

        RET=$?
        if [ $RET -ne 0 ] || [ "$OPC_SHARE" == "5" ]; then
            break
        fi

        case "$OPC_SHARE" in
            1)
                TABLA_SHARES=$(python3 -c '
import re

with open("/etc/samba/smb.conf", "r", encoding="utf-8") as f:
    lines = f.readlines()

shares = []
current = None
cur_data = {}

for line in lines:
    m = re.match(r"^\s*\[([^\]]+)\]", line)
    if m:
        sec = m.group(1).strip()
        if sec.lower() != "global":
            if current:
                shares.append((current, cur_data))
            current = sec
            cur_data = {"path": "N/A", "valid": "Todos", "available": "yes", "browseable": "yes", "read_only": "no", "write_list": "-"}
        else:
            if current:
                shares.append((current, cur_data))
            current = None
    elif current and "=" in line:
        k, v = line.split("=", 1)
        k, v = k.strip().lower(), v.strip()
        if k == "path": cur_data["path"] = v
        elif k == "valid users": cur_data["valid"] = v
        elif k == "write list": cur_data["write_list"] = v
        elif k == "read only": cur_data["read_only"] = v
        elif k == "browseable": cur_data["browseable"] = v
        elif k == "available": cur_data["available"] = v

if current:
    shares.append((current, cur_data))

rows = []
for name, d in shares:
    estado = "● Activo" if d["available"] != "no" else "○ Deshab."
    vis = "Oculto ($)" if d["browseable"] == "no" or name.endswith("$") else "Visible"
    perm = "Solo Lectura" if d["read_only"] == "yes" else "Lect/Escr"
    path = d["path"]
    rows.append((name, estado, vis, perm, path))

if not rows:
    print("  (No hay recursos compartidos configurados)")
    exit(0)

w_name = max(max(len(r[0]) for r in rows), 14)
w_est  = max(max(len(r[1]) for r in rows), 8)
w_vis  = max(max(len(r[2]) for r in rows), 11)
w_perm = max(max(len(r[3]) for r in rows), 12)
w_path = max(max(len(r[4]) for r in rows), 20)

print("┌─{}─┬─{}─┬─{}─┬─{}─┬─{}─┐".format("─"*w_name, "─"*w_est, "─"*w_vis, "─"*w_perm, "─"*w_path))
print("│ {} │ {} │ {} │ {} │ {} │".format("RECURSO".ljust(w_name), "ESTADO".ljust(w_est), "VISIBILIDAD".ljust(w_vis), "PERMISOS".ljust(w_perm), "RUTA EN DISCO".ljust(w_path)))
print("├─{}─┼─{}─┼─{}─┼─{}─┼─{}─┤".format("─"*w_name, "─"*w_est, "─"*w_vis, "─"*w_perm, "─"*w_path))
for r in rows:
    print("│ {} │ {} │ {} │ {} │ {} │".format(r[0].ljust(w_name), r[1].ljust(w_est), r[2].ljust(w_vis), r[3].ljust(w_perm), r[4].ljust(w_path)))
print("└─{}─┴─{}─┴─{}─┴─{}─┴─{}─┘".format("─"*w_name, "─"*w_est, "─"*w_vis, "─"*w_perm, "─"*w_path))
')
                whiptail --title "CRUD: Recursos Compartidos (Samba)" --ok-button "< Aceptar >" --msgbox "$TABLA_SHARES" 20 78
                ;;

            2)
                NOMBRE_SHARE=$(whiptail --title "$APP_TITLE" \
                    --ok-button "< Siguiente >" --cancel-button "< Cancelar >" \
                    --inputbox "Ingresa el nombre del recurso para Windows (ej. VENTAS o BACKUPS):" 10 65 3>&1 1>&2 2>&3)
                RET=$?
                if [ $RET -ne 0 ] || [ -z "$NOMBRE_SHARE" ]; then continue; fi

                NOMBRE_SHARE=$(echo "$NOMBRE_SHARE" | tr " " "_" | tr -cd "A-Za-z0-9_$-")
                if [ -z "$NOMBRE_SHARE" ]; then
                    whiptail --title "Nombre Invalido" --ok-button "< Aceptar >" \
                        --msgbox "El nombre del recurso no puede estar vacio ni contener solo simbolos." 9 65
                    continue
                fi

                OPC_VIS=$(whiptail --title "Visibilidad en Red (Samba / Windows)" \
                    --ok-button "< Siguiente >" --cancel-button "< Cancelar >" \
                    --menu "Selecciona la visibilidad del recurso en el entorno de red de Windows:" 15 72 2 \
                    "1" "Recurso OCULTO ($) - No visible en explorador (Recomendado)" \
                    "2" "Recurso VISIBLE - Visible en explorador de red para todos" 3>&1 1>&2 2>&3)
                RET=$?
                if [ $RET -ne 0 ] || [ -z "$OPC_VIS" ]; then continue; fi

                if [ "$OPC_VIS" == "1" ]; then
                    [[ "$NOMBRE_SHARE" != *\$ ]] && NOMBRE_SHARE="${NOMBRE_SHARE}\$"
                    BROWSEABLE="no"
                    VIS_TXT="Oculto ($) [Invisible en explorador de red]"
                else
                    NOMBRE_SHARE="${NOMBRE_SHARE%\$}"
                    BROWSEABLE="yes"
                    VIS_TXT="Visible en red"
                fi

                NOMBRE_DIR="${NOMBRE_SHARE%\$}"
                RUTA_SHARE=$(whiptail --title "$APP_TITLE" \
                    --ok-button "< Siguiente >" --cancel-button "< Cancelar >" \
                    --inputbox "Ruta física en el disco del servidor:" 10 65 "/srv/nas/$NOMBRE_DIR" 3>&1 1>&2 2>&3)
                RET=$?
                if [ $RET -ne 0 ] || [ -z "$RUTA_SHARE" ]; then continue; fi
                if [[ ! "$RUTA_SHARE" =~ ^/[A-Za-z0-9._/-]*$ ]]; then
                    whiptail --title "Ruta Invalida" --ok-button "< Aceptar >" \
                        --msgbox "La ruta debe ser absoluta y sin espacios ni caracteres especiales." 9 68
                    continue
                fi

                if [[ "$RUTA_SHARE" != /srv/nas/* ]]; then
                    whiptail --title "Ruta Invalida" --ok-button "< Aceptar >" \
                        --msgbox "La ruta debe estar dentro de /srv/nas." 9 60
                    continue
                fi

                RUTA_REAL=$(realpath -m -- "$RUTA_SHARE")
                case "$RUTA_REAL" in
                    /srv/nas/*)
                        ;;
                    *)
                        whiptail --title "Ruta Invalida" --ok-button "< Aceptar >" \
                            --msgbox "La ruta no puede salir de /srv/nas." 9 60
                        continue
                        ;;
                esac

                if [ "$RUTA_REAL" = "/srv/nas" ]; then
                    whiptail --title "Ruta Invalida" --ok-button "< Aceptar >" \
                        --msgbox "No se puede utilizar la raíz de almacenamiento como recurso." 9 60
                    continue
                fi

                RUTA_SHARE="$RUTA_REAL"

                COMENTARIO=$(whiptail --title "$APP_TITLE" \
                    --ok-button "< Siguiente >" --cancel-button "< Cancelar >" \
                    --inputbox "Descripción o comentario del recurso:" 10 65 "Carpeta compartida $NOMBRE_DIR" 3>&1 1>&2 2>&3)
                [ -z "$COMENTARIO" ] && COMENTARIO="Carpeta compartida $NOMBRE_DIR"
                COMENTARIO=$(printf '%s' "$COMENTARIO" | tr -d '\r\n')

                TIPO_PERM=$(whiptail --title "Esquema de Seguridad y Permisos" \
                    --ok-button "< Siguiente >" --cancel-button "< Cancelar >" \
                    --menu "Selecciona el esquema de permisos para este recurso:" 16 72 4 \
                    "1" "Lectura y Escritura (Todos los grupos autorizados pueden editar)" \
                    "2" "Solo Lectura General + Escritura Exclusiva (write list)" \
                    "3" "Solo Lectura Estricta (Nadie puede modificar desde la red)" \
                    "4" "Acceso Público / Invitados (Sin requerir contraseña)" 3>&1 1>&2 2>&3)
                RET=$?
                if [ $RET -ne 0 ] || [ -z "$TIPO_PERM" ]; then continue; fi

                GRUPOS_DISP=$(awk -F: '$1 ~ /^grp_/ {print $1}' /etc/group | sort -u)
                if [ "$TIPO_PERM" != "4" ] && [ -z "$GRUPOS_DISP" ]; then
                    whiptail --title "Sin Grupos" --ok-button "< Aceptar >" \
                        --msgbox "No hay grupos de seguridad creados.\nCrea primero un grupo desde la opción [2] del menú principal." 9 65
                    continue
                fi

                VALID_USERS=""
                WRITE_LIST=""
                READ_ONLY="no"
                GUEST_OK="no"
                MASK="0770"
                GRUPO_DUENO="grp_sistemas"
                TIPO_TXT=""

                case "$TIPO_PERM" in
                    1)
                        local -a LISTA_OPC=()
                        for g in $GRUPOS_DISP; do
                            STATUS="OFF"
                            [ "$g" == "grp_sistemas" ] && STATUS="ON"
                            LISTA_OPC+=("$g" "$g" "$STATUS")
                        done

                        GRUPOS_SEL=$(whiptail --title "Grupos con Lectura y Escritura" \
                            --ok-button "< Continuar >" --cancel-button "< Cancelar >" \
                            --checklist "Marca con ESPACIO los grupos que tendrán acceso total a [$NOMBRE_SHARE]:" 18 70 8 \
                            "${LISTA_OPC[@]}" 3>&1 1>&2 2>&3)
                        RET=$?
                        if [ $RET -ne 0 ] || [ -z "$GRUPOS_SEL" ]; then continue; fi

                        VALID_USERS=$(echo "$GRUPOS_SEL" | tr -d '\"' | sed 's/^/@/; s/ / @/g; s/,/ @/g')
                        GRUPO_DUENO=$(echo "$GRUPOS_SEL" | tr -d '\"' | awk '{print $1}')
                        READ_ONLY="no"
                        MASK="0770"
                        TIPO_TXT="Lectura y Escritura para Grupos"
                        ;;

                    2)
                        local -a LISTA_OPC=()
                        for g in $GRUPOS_DISP; do
                            LISTA_OPC+=("$g" "$g" "OFF")
                        done

                        GRUPOS_RO=$(whiptail --title "Paso A: Grupos con Permiso de Solo Lectura" \
                            --ok-button "< Siguiente >" --cancel-button "< Cancelar >" \
                            --checklist "Marca los grupos que tendrán permiso de SOLO LECTURA (consultar/descargar):" 18 70 8 \
                            "${LISTA_OPC[@]}" 3>&1 1>&2 2>&3)
                        RET=$?
                        if [ $RET -ne 0 ] || [ -z "$GRUPOS_RO" ]; then continue; fi

                        local -a MENU_RW=()
                        for g in $GRUPOS_DISP; do
                            MENU_RW+=("$g" "Grupo_$g")
                        done

                        GRUPO_RW=$(whiptail --title "Paso B: Grupo con Escritura Exclusiva" \
                            --ok-button "< Siguiente >" --cancel-button "< Cancelar >" \
                            --menu "Selecciona el grupo que tendrá permiso de ESCRITURA (subir/modificar/borrar):" 18 70 8 \
                            "${MENU_RW[@]}" 3>&1 1>&2 2>&3)
                        RET=$?
                        if [ $RET -ne 0 ] || [ -z "$GRUPO_RW" ]; then continue; fi

                        TODOS_GRPS=$(echo "$GRUPOS_RO $GRUPO_RW" | tr -d '\"' | tr ' ' '\n' | sort -u | tr '\n' ' ')
                        VALID_USERS=$(echo "$TODOS_GRPS" | sed 's/^/@/; s/ $//; s/ / @/g')
                        WRITE_LIST="@$GRUPO_RW"
                        READ_ONLY="yes"
                        MASK="0775"
                        GRUPO_DUENO="$GRUPO_RW"
                        TIPO_TXT="Solo Lectura General + Escritura Exclusiva (@$GRUPO_RW)"
                        ;;

                    3)
                        local -a LISTA_OPC=()
                        for g in $GRUPOS_DISP; do
                            STATUS="OFF"
                            [ "$g" == "grp_sistemas" ] && STATUS="ON"
                            LISTA_OPC+=("$g" "$g" "$STATUS")
                        done

                        GRUPOS_RO_ESTRICTO=$(whiptail --title "Grupos con Acceso de Solo Lectura Estricta" \
                            --ok-button "< Continuar >" --cancel-button "< Cancelar >" \
                            --checklist "Marca los grupos autorizados para ver este contenido histórico:" 18 70 8 \
                            "${LISTA_OPC[@]}" 3>&1 1>&2 2>&3)
                        RET=$?
                        if [ $RET -ne 0 ] || [ -z "$GRUPOS_RO_ESTRICTO" ]; then continue; fi

                        VALID_USERS=$(echo "$GRUPOS_RO_ESTRICTO" | tr -d '\"' | sed 's/^/@/; s/ / @/g')
                        GRUPO_DUENO=$(echo "$GRUPOS_RO_ESTRICTO" | tr -d '\"' | awk '{print $1}')
                        READ_ONLY="yes"
                        WRITE_LIST=""
                        MASK="0755"
                        TIPO_TXT="Solo Lectura Estricta (Nadie puede modificar)"
                        ;;

                    4)
                        OPC_PUB=$(whiptail --title "Nivel de Acceso Público" \
                            --ok-button "< Siguiente >" --cancel-button "< Cancelar >" \
                            --menu "Selecciona los permisos para usuarios no autenticados / invitados:" 14 70 2 \
                            "1" "Solo Lectura (Invitados pueden ver y descargar sin borrar)" \
                            "2" "Lectura y Escritura Total (Invitados pueden subir y borrar)" 3>&1 1>&2 2>&3)
                        RET=$?
                        if [ $RET -ne 0 ] || [ -z "$OPC_PUB" ]; then continue; fi

                        GUEST_OK="yes"
                        VALID_USERS=""
                        if [ "$OPC_PUB" == "1" ]; then
                            TIPO_TXT="Acceso Público (Solo Lectura)"
                            READ_ONLY="yes"
                            MASK="0755"
                        else
                            TIPO_TXT="Acceso Público (Lectura y Escritura Libre)"
                            READ_ONLY="no"
                            MASK="0777"
                        fi
                        ;;
                esac

                if (whiptail --title "Confirmar Creación de Recurso" \
                    --yes-button "< Sí, Crear Recurso >" --no-button "< Cancelar >" \
                    --yesno "¿Confirmas la creación del recurso con los siguientes parámetros?\n\n• Nombre:      [$NOMBRE_SHARE]\n• Visibilidad: $VIS_TXT\n• Ruta Disco:   $RUTA_SHARE\n• Esquema:     $TIPO_TXT\n• Acceso:      ${VALID_USERS:-Invitados (Público)}\n• Escritura:   ${WRITE_LIST:-Según Permisos Generales}\n\n• Ruta de red: \\\\${SERVER_IP}\\$NOMBRE_SHARE" 17 72); then
                    
                    mkdir -p "$RUTA_SHARE"
                    if [ "$TIPO_PERM" == "1" ]; then
                        chown -R root:"$GRUPO_DUENO" "$RUTA_SHARE"
                        chmod -R 2770 "$RUTA_SHARE"
                        local -a GRPS_ACL=()
                        read -r -a GRPS_ACL <<< "$(echo "$GRUPOS_SEL" | tr -d '"')"
                        for g in "${GRPS_ACL[@]}"; do
                            setfacl -R -m "g:$g:rwx" "$RUTA_SHARE" 2>/dev/null || true
                            find "$RUTA_SHARE" -type d -exec setfacl -d -m "g:$g:rwx" {} + 2>/dev/null || true
                        done
                    elif [ "$TIPO_PERM" == "2" ]; then
                        chown -R root:"$GRUPO_DUENO" "$RUTA_SHARE"
                        chmod -R 2775 "$RUTA_SHARE"
                    elif [ "$TIPO_PERM" == "3" ]; then
                        chown -R root:"$GRUPO_DUENO" "$RUTA_SHARE"
                        chmod -R 2755 "$RUTA_SHARE"
                    elif [ "$TIPO_PERM" == "4" ]; then
                        if [ "$OPC_PUB" == "1" ]; then
                            chown -R nobody:nogroup "$RUTA_SHARE"
                            chmod -R 0755 "$RUTA_SHARE"
                        else
                            chown -R nobody:nogroup "$RUTA_SHARE"
                            chmod -R 0777 "$RUTA_SHARE"
                        fi
                    fi

                    BACKUP_SMB="/etc/samba/smb.conf.bak-$(date +%Y%m%d_%H%M%S)"
                    cp -a /etc/samba/smb.conf "$BACKUP_SMB"
                    TMP_SMB=$(mktemp)
                    cp -a /etc/samba/smb.conf "$TMP_SMB"
                    {
                        printf '\n# ==============================================================================\n'
                        printf '# RECURSO COMPARTIDO: %s\n' "$NOMBRE_SHARE"
                        printf '# ==============================================================================\n'
                        printf '[%s]\n' "$NOMBRE_SHARE"
                        printf '   comment = %s\n' "$COMENTARIO"
                        printf '   path = %s\n' "$RUTA_SHARE"
                        printf '   browseable = %s\n' "$BROWSEABLE"
                        printf '   read only = %s\n' "$READ_ONLY"
                        printf '   guest ok = %s\n' "$GUEST_OK"
                        if [ "$TIPO_PERM" == "1" ]; then
                            printf '   inherit acls = yes\n'
                        fi
                        if [ -n "$VALID_USERS" ]; then
                            printf '   valid users = %s\n' "$VALID_USERS"
                        fi
                        if [ -n "$WRITE_LIST" ]; then
                            printf '   write list = %s\n' "$WRITE_LIST"
                        fi
                        printf '   create mask = %s\n' "$MASK"
                        printf '   directory mask = %s\n' "$MASK"
                        printf '   force create mode = %s\n' "$MASK"
                        printf '   force directory mode = %s\n' "$MASK"
                    } >> "$TMP_SMB"
                    mv -f "$TMP_SMB" /etc/samba/smb.conf
                    if ! testparm -s >/dev/null 2>&1; then
                        cp -a "$BACKUP_SMB" /etc/samba/smb.conf
                        whiptail --title "Error de Samba" --ok-button "< Aceptar >" \
                            --msgbox "La configuración generada no es válida. Se restauró la copia anterior." 10 70
                        continue
                    fi
                    smbcontrol all reload-config 2>/dev/null || systemctl reload smbd 2>/dev/null || systemctl restart smbd 2>/dev/null || true
                    whiptail --title "$APP_TITLE" --ok-button "< Aceptar >" \
                        --msgbox "✔ ¡Recurso \"[$NOMBRE_SHARE]\" creado con éxito!\n\n• Visibilidad: $VIS_TXT\n• Ruta Disco:  $RUTA_SHARE\n• Esquema:    $TIPO_TXT\n\nAccesible desde Windows en: \\\\${SERVER_IP}\\$NOMBRE_SHARE" 14 72
                fi
                ;;

            3)
                LISTA_SHARES=$(python3 -c '
import re
with open("/etc/samba/smb.conf", "r", encoding="utf-8") as f:
    text = f.read()
for m in re.finditer(r"\[([^\]]+)\]", text):
    s = m.group(1).strip()
    if s.lower() != "global":
        sec_start = m.start()
        next_sec = text.find("[", sec_start + 1)
        block = text[sec_start:next_sec] if next_sec != -1 else text[sec_start:]
        st = "[DESHABILITADO]" if "available = no" in block else "[ACTIVO]"
        print(f"{s} {st}")
')
                if [ -z "$LISTA_SHARES" ]; then
                    whiptail --ok-button "< Aceptar >" --msgbox "No hay recursos compartidos disponibles." 8 45
                    continue
                fi

                local -a MENU_ITEMS=()
                while read -r name status; do
                    MENU_ITEMS+=("$name" "$status")
                done <<< "$LISTA_SHARES"

                TARGET_SHARE=$(whiptail --title "Alternar Estado de Recurso" \
                    --ok-button "< Siguiente >" --cancel-button "< Cancelar >" \
                    --menu "Selecciona el recurso para conmutar su estado en la red:" 18 70 8 \
                    "${MENU_ITEMS[@]}" 3>&1 1>&2 2>&3)

                RET=$?
                if [ $RET -eq 0 ] && [ -n "$TARGET_SHARE" ]; then
                    if (whiptail --title "Confirmar Cambio de Estado" \
                        --yes-button "< Sí, Cambiar Estado >" --no-button "< Cancelar >" \
                        --yesno "¿Estás seguro de que deseas conmutar el estado del recurso \"[$TARGET_SHARE]\" en la red?" 10 68); then
                        
                        BACKUP_SMB="/etc/samba/smb.conf.bak-$(date +%Y%m%d_%H%M%S)"
                        cp -a /etc/samba/smb.conf "$BACKUP_SMB"
                        NUEVO_ESTADO=$(python3 -c '
import sys, re
target = sys.argv[1]
with open("/etc/samba/smb.conf", "r", encoding="utf-8") as f:
    text = f.read()

pattern = re.compile(rf"(\[{re.escape(target)}\][\s\S]*?)(?=\n\[|\Z)")
m = pattern.search(text)
if m:
    block = m.group(1)
    if "available = no" in block:
        block = block.replace("available = no\n", "")
        msg = "HABILITADO (En línea)"
    else:
        block = block.replace(f"[{target}]\n", f"[{target}]\n   available = no\n")
        msg = "DESHABILITADO (Fuera de línea)"
    text = text[:m.start()] + block + text[m.end():]
    with open("/etc/samba/smb.conf", "w", encoding="utf-8") as f:
        f.write(text)
    print(msg)
' "$TARGET_SHARE")
                        if ! testparm -s >/dev/null 2>&1; then
                            cp -a "$BACKUP_SMB" /etc/samba/smb.conf
                            whiptail --title "Error de Samba" --ok-button "< Aceptar >" \
                                --msgbox "La configuración generada no es válida. Se restauró la copia anterior." 10 70
                            continue
                        fi
                        smbcontrol all reload-config 2>/dev/null || systemctl reload smbd 2>/dev/null || true
                        whiptail --title "$APP_TITLE" --ok-button "< Aceptar >" --msgbox "✔ El recurso \"[$TARGET_SHARE]\" ahora está:\n$NUEVO_ESTADO" 9 55
                    fi
                fi
                ;;

            4)
                LISTA_ELIMINAR=$(python3 -c '
import re
with open("/etc/samba/smb.conf", "r", encoding="utf-8") as f:
    lines = f.readlines()
for line in lines:
    m = re.match(r"^\s*\[([^\]]+)\]", line)
    if m and m.group(1).strip().lower() != "global":
        print(m.group(1).strip())
')
                if [ -z "$LISTA_ELIMINAR" ]; then
                    whiptail --ok-button "< Aceptar >" --msgbox "No hay recursos disponibles para eliminar." 8 45
                    continue
                fi

                local -a MENU_DEL=()
                for s in $LISTA_ELIMINAR; do
                    MENU_DEL+=("$s" "Recurso_Samba")
                done

                SHARE_A_BORRAR=$(whiptail --title "Eliminar Recurso Compartido" \
                    --ok-button "< Siguiente >" --cancel-button "< Cancelar >" \
                    --menu "Selecciona el recurso que deseas eliminar definitivamente:" 18 70 8 \
                    "${MENU_DEL[@]}" 3>&1 1>&2 2>&3)

                RET=$?
                if [ $RET -eq 0 ] && [ -n "$SHARE_A_BORRAR" ]; then
                    if (whiptail --title "Confirmación de Eliminación" \
                        --yes-button "< Sí, Eliminar Recurso >" --no-button "< Cancelar >" \
                        --yesno "¿Estás 100% seguro de eliminar la definición del recurso \"[$SHARE_A_BORRAR]\" de la red Samba?\n\n(Nota: Los archivos físicos en el disco se mantendrán protegidos)." 11 68); then
                        
                        BACKUP_SMB="/etc/samba/smb.conf.bak-$(date +%Y%m%d_%H%M%S)"
                        cp -a /etc/samba/smb.conf "$BACKUP_SMB"
                        python3 -c '
import sys, re
target = sys.argv[1]
with open("/etc/samba/smb.conf", "r", encoding="utf-8") as f:
    text = f.read()

text = re.sub(rf"# =+\n# RECURSO COMPARTIDO: {re.escape(target)}\n# =+\n", "", text)
text = re.sub(rf"\[{re.escape(target)}\][\s\S]*?(?=\n\[|\Z)", "", text)

with open("/etc/samba/smb.conf", "w", encoding="utf-8") as f:
    f.write(text.strip() + "\n")
' "$SHARE_A_BORRAR"
                        if ! testparm -s >/dev/null 2>&1; then
                            cp -a "$BACKUP_SMB" /etc/samba/smb.conf
                            whiptail --title "Error de Samba" --ok-button "< Aceptar >" \
                                --msgbox "La configuración generada no es válida. Se restauró la copia anterior." 10 70
                            continue
                        fi
                        smbcontrol all reload-config 2>/dev/null || systemctl reload smbd 2>/dev/null || true
                        whiptail --title "$APP_TITLE" --ok-button "< Aceptar >" --msgbox "✔ El recurso \"[$SHARE_A_BORRAR]\" ha sido eliminado de la red Samba." 8 60
                    fi
                fi
                ;;
        esac
    done
}
