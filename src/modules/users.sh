#!/bin/bash
# ==============================================================================
# Módulo: Gestión Integral de Usuarios, Roles, Estados y Samba
# ==============================================================================

crear_usuario_guiado() {
    local SERVER_IP USER_NAME FULL_NAME GRUPOS_DISP LISTA_OPCIONES GRUPOS_SELEC GRUPO_FINAL USER_PW SHELL_TYPE PERM_TXT STATUS g
    SERVER_IP=$(obtener_ip_local)

    # 1. Nombre de usuario
    USER_NAME=$(whiptail --title "Paso 1 de 4: Nombre de Usuario" \
        --ok-button "< Siguiente >" --cancel-button "< Cancelar >" \
        --inputbox "Ingresa el identificador de usuario para la red (ej. carlos_m):" 10 65 3>&1 1>&2 2>&3)
    RET=$?
    if [ $RET -ne 0 ] || [ -z "$USER_NAME" ]; then return; fi

    if ! [[ "$USER_NAME" =~ ^[a-z_][a-z0-9_-]*$ ]]; then
        whiptail --title "Error de Formato" --ok-button "< Aceptar >" \
            --msgbox "El identificador solo puede contener letras minúsculas, números y guión bajo." 9 65
        return
    fi

    if id "$USER_NAME" &>/dev/null; then
        whiptail --title "Usuario Existente" --ok-button "< Aceptar >" \
            --msgbox "El usuario \"$USER_NAME\" ya existe en el sistema." 8 50
        return
    fi

    # 2. Nombre Real / Cargo / Descripción
    FULL_NAME=$(whiptail --title "Paso 2 de 4: Nombre Real y Cargo" \
        --ok-button "< Siguiente >" --cancel-button "< Cancelar >" \
        --inputbox "Nombre completo y departamento/cargo del empleado:" 10 65 "Empleado EAD" 3>&1 1>&2 2>&3)
    RET=$?
    if [ $RET -ne 0 ]; then return; fi
    FULL_NAME=${FULL_NAME:-"Empleado EAD"}

    # 3. Selección de Grupos de Seguridad (grp_*)
    GRUPOS_DISP=$(awk -F: '$1 ~ /^grp_/ {print $1}' /etc/group | sort -u)

    if [ -z "$GRUPOS_DISP" ]; then
        whiptail --title "Sin Grupos de Seguridad" --ok-button "< Aceptar >" \
            --msgbox "No hay grupos de seguridad creados.\nPor favor crea un grupo primero desde el menú [2] Gestión de Grupos." 10 65
        return
    fi

    local -a LISTA_OPCIONES=()
    for g in $GRUPOS_DISP; do
        [ "$g" == "$USER_NAME" ] && continue
        STATUS="OFF"
        LISTA_OPCIONES+=("$g" "Grupo_$g" "$STATUS")
    done

    GRUPOS_SELEC=$(whiptail --title "Paso 3 de 4: Asignación de Grupos" \
        --ok-button "< Siguiente >" --cancel-button "< Cancelar >" \
        --checklist "Marca con la BARRA ESPACIADORA [X] los grupos a los que pertenecerá '$USER_NAME':" 18 74 8 \
        "${LISTA_OPCIONES[@]}" 3>&1 1>&2 2>&3)

    RET=$?
    if [ $RET -ne 0 ] || [ -z "$GRUPOS_SELEC" ]; then
        whiptail --title "Aviso" --ok-button "< Aceptar >" --msgbox "Debes seleccionar al menos un grupo para el usuario." 8 50
        return
    fi

    GRUPO_FINAL=$(echo "$GRUPOS_SELEC" | tr -d '\"' | tr ' ' ',')

    # 4. Contraseña de red Samba
    while true; do
        USER_PW=$(whiptail --title "Paso 4 de 4: Contraseña de Red Samba" \
            --ok-button "< Siguiente >" --cancel-button "< Cancelar >" \
            --passwordbox "Ingresa la contraseña de red Samba para $USER_NAME:" 10 65 3>&1 1>&2 2>&3)
        RET=$?
        if [ $RET -ne 0 ]; then return; fi
        if [ -n "$USER_PW" ]; then break; fi
        whiptail --title "Error" --ok-button "< Aceptar >" --msgbox "La contraseña no puede estar vacía." 8 45
    done

    # Política de aislamiento: Admin (Web+SSH) vs Empleado (Solo red)
    if echo "$GRUPO_FINAL" | grep -qw "grp_sistemas"; then
        SHELL_TYPE="/bin/bash"
        PERM_TXT="Administrador del Servidor (Acceso Web Cockpit + Consola SSH + Red)"
    else
        SHELL_TYPE="/usr/sbin/nologin"
        PERM_TXT="Empleado de Red (Acceso exclusivo a carpetas Samba, sin consola ni web)"
    fi

    if (whiptail --title "Confirmar Registro de Usuario" \
        --yes-button "< Sí, Registrar Usuario >" --no-button "< Cancelar >" \
        --yesno "¿Confirmas el registro del usuario con los siguientes datos?\n\n• Usuario:          $USER_NAME\n• Nombre / Cargo:   $FULL_NAME\n• Grupos Asignados: $GRUPO_FINAL\n• Nivel de Acceso:  $PERM_TXT" 16 72); then
        
        if [ "$SHELL_TYPE" == "/bin/bash" ]; then
            adduser --disabled-password --gecos "$FULL_NAME" --shell "$SHELL_TYPE" "$USER_NAME" 2>/dev/null || useradd -c "$FULL_NAME" -s "$SHELL_TYPE" -m "$USER_NAME"
        else
            adduser --disabled-password --gecos "$FULL_NAME" --no-create-home --shell "$SHELL_TYPE" "$USER_NAME" 2>/dev/null || useradd -c "$FULL_NAME" -s "$SHELL_TYPE" -M "$USER_NAME"
        fi

        IFS=',' read -ra GRPS <<< "$GRUPO_FINAL"
        for g in "${GRPS[@]}"; do
            groupadd -f "$g"
        done

        usermod -aG "$GRUPO_FINAL" "$USER_NAME"
        
        if [ "$SHELL_TYPE" == "/bin/bash" ]; then
            usermod -aG sudo,adm "$USER_NAME"
            echo "${USER_NAME}:${USER_PW}" | chpasswd
        fi

        printf '%s\n%s\n' "$USER_PW" "$USER_PW" | smbpasswd -a -s "$USER_NAME"

        whiptail --title "$APP_TITLE" --ok-button "< Aceptar >" \
            --msgbox "✔ ¡Usuario Registrado con Éxito!\n\n• Usuario:          $USER_NAME\n• Nombre / Cargo:   $FULL_NAME\n• Grupos Asignados: $GRUPO_FINAL\n• Acceso:           $PERM_TXT\n\nYa puede conectar desde la red a: \\\\${SERVER_IP}" 15 72
    fi
}

alternar_estado_usuario() {
    local LISTA_USERS USER_SEL IS_DISABLED u st
    LISTA_USERS=$(pdbedit -L 2>/dev/null | cut -d: -f1 | grep -v "^root$")
    if [ -z "$LISTA_USERS" ]; then
        whiptail --ok-button "< Aceptar >" --msgbox "No hay usuarios registrados en Samba." 8 45
        return
    fi

    local -a MENU_ITEMS=()
    while IFS=$'\t' read -r u st; do
        [ -n "$u" ] && MENU_ITEMS+=("$u" "$st")
    done < <(python3 -c '
import subprocess
out = subprocess.run(["pdbedit", "-L", "-v"], capture_output=True, text=True)
users = {}
current_u = None

for line in out.stdout.splitlines():
    line = line.strip()
    if line.startswith("Unix username:"):
        current_u = line.split(":", 1)[1].strip()
        users[current_u] = {"flags": ""}
    elif current_u and line.startswith("Account Flags:"):
        users[current_u]["flags"] = line.split(":", 1)[1].strip()

for u, data in users.items():
    if u != "root":
        st = "[SUSPENDIDO]" if "D" in data["flags"] else "[ACTIVO]"
        print(f"{u}\t{st}")
')

    USER_SEL=$(whiptail --title "Alternar Estado de Usuario (Activar / Suspender)" \
        --ok-button "< Siguiente >" --cancel-button "< Cancelar >" \
        --menu "Selecciona el usuario para suspender o reactivar su acceso a la red:" 18 72 8 \
        "${MENU_ITEMS[@]}" 3>&1 1>&2 2>&3)

    RET=$?
    if [ $RET -eq 0 ] && [ -n "$USER_SEL" ]; then
        IS_DISABLED=$(pdbedit -v -u "$USER_SEL" 2>/dev/null | grep "Account Flags:" | grep -q "D" && echo "yes" || echo "no")
        
        if [ "$IS_DISABLED" == "yes" ]; then
            if (whiptail --title "Reactivar Usuario" \
                --yes-button "< Sí, Reactivar >" --no-button "< Cancelar >" \
                --yesno "¿Deseas REACTIVAR el acceso a la red para el usuario '$USER_SEL'?" 9 65); then
                smbpasswd -e "$USER_SEL"
                whiptail --title "$APP_TITLE" --ok-button "< Aceptar >" \
                    --msgbox "✔ Usuario '$USER_SEL' REACTIVADO.\nYa puede acceder normalmente a sus carpetas de red." 9 60
            fi
        else
            if (whiptail --title "Suspender Usuario" \
                --yes-button "< Sí, Suspender >" --no-button "< Cancelar >" \
                --yesno "¿Deseas SUSPENDER temporalmente el acceso a la red para el usuario '$USER_SEL'?\n\n(Sus datos y archivos compartidos se conservarán intactos)." 11 68); then
                smbpasswd -d "$USER_SEL"
                whiptail --title "$APP_TITLE" --ok-button "< Aceptar >" \
                    --msgbox "✔ Usuario '$USER_SEL' SUSPENDIDO.\nSu acceso a la red ha sido bloqueado temporalmente." 9 60
            fi
        fi
    fi
}

gestionar_usuarios() {
    local OPC_USR TABLA_USERS LISTA_USERS MENU_USERS TARGET_USER GRUPOS_DISP LISTA_OPC GRPS_ACTUALES NUEVOS_GRPS NUEVOS_GRPS_CSV
    local USER_PW_SEL NUEVA_CLAVE USER_DEL_SEL u g g_prev
    while true; do
        OPC_USR=$(whiptail --title "$APP_TITLE" \
            --ok-button "< Seleccionar >" --cancel-button "< Volver >" \
            --menu "GESTIÓN DE USUARIOS Y ACCESOS:" 18 74 7 \
            "1" "[*] Listar Usuarios (Tabla CRUD con Estados y Cargos)" \
            "2" "[+] Crear Nuevo Usuario (Nombre, Cargo, Grupos y Clave)" \
            "3" "[~] Alternar Estado (Activar / Suspender Acceso a Red)" \
            "4" "[#] Modificar Grupos Asignados a un Usuario" \
            "5" "[*] Cambiar Contraseña de Samba a un Usuario" \
            "6" "[-] Eliminar un Usuario del Sistema" \
            "7" "[<] Volver al Menú Principal" 3>&1 1>&2 2>&3)

        RET=$?
        if [ $RET -ne 0 ] || [ "$OPC_USR" == "7" ]; then
            break
        fi

        case "$OPC_USR" in
            1)
                TABLA_USERS=$(python3 -c '
import subprocess, pwd

out = subprocess.run(["pdbedit", "-L", "-v"], capture_output=True, text=True)
users = {}
current_u = None

for line in out.stdout.splitlines():
    line = line.strip()
    if line.startswith("Unix username:"):
        current_u = line.split(":", 1)[1].strip()
        users[current_u] = {"flags": "", "fullname": ""}
    elif current_u and line.startswith("Account Flags:"):
        users[current_u]["flags"] = line.split(":", 1)[1].strip()
    elif current_u and line.startswith("Full Name:"):
        users[current_u]["fullname"] = line.split(":", 1)[1].strip()

rows = []
for u, data in users.items():
    try:
        p = pwd.getpwnam(u)
        gecos = p.pw_gecos.split(",")[0] if p.pw_gecos else "-"
        shell = p.pw_shell
    except:
        gecos = "-"
        shell = "/usr/sbin/nologin"
    
    grp_out = subprocess.run(["id", "-Gn", u], capture_output=True, text=True)
    all_grps = grp_out.stdout.strip().split()
    sec_grps = [g for g in all_grps if g.startswith("grp_")]
    
    is_disabled = "D" in data["flags"]
    status = "○ SUSPENDIDO" if is_disabled else "● ACTIVO"
    
    if "grp_sistemas" in all_grps or "sudo" in all_grps:
        role = "Admin (Web+SSH)"
    else:
        role = "Solo Red SMB"
        
    grps_str = ", ".join(sec_grps) if sec_grps else "(sin grp_*)"
    rows.append((u, gecos, role, status, grps_str))

if not rows:
    print("  (No hay usuarios registrados en Samba actualmente)")
    exit(0)

w_user = max(max(len(r[0]) for r in rows), 14)
w_name = max(max(len(r[1]) for r in rows), 22)
w_role = max(max(len(r[2]) for r in rows), 16)
w_stat = max(max(len(r[3]) for r in rows), 12)
w_grps = max(max(len(r[4]) for r in rows), 22)

print("┌─{}─┬─{}─┬─{}─┬─{}─┬─{}─┐".format("─"*w_user, "─"*w_name, "─"*w_role, "─"*w_stat, "─"*w_grps))
print("│ {} │ {} │ {} │ {} │ {} │".format("USUARIO".ljust(w_user), "NOMBRE COMPLETO / CARGO".ljust(w_name), "ACCESO / ROL".ljust(w_role), "ESTADO".ljust(w_stat), "GRUPOS ASIGNADOS".ljust(w_grps)))
print("├─{}─┼─{}─┼─{}─┼─{}─┼─{}─┤".format("─"*w_user, "─"*w_name, "─"*w_role, "─"*w_stat, "─"*w_grps))
for r in rows:
    print("│ {} │ {} │ {} │ {} │ {} │".format(r[0].ljust(w_user), r[1].ljust(w_name), r[2].ljust(w_role), r[3].ljust(w_stat), r[4].ljust(w_grps)))
print("└─{}─┴─{}─┴─{}─┴─{}─┴─{}─┘".format("─"*w_user, "─"*w_name, "─"*w_role, "─"*w_stat, "─"*w_grps))
')
                whiptail --title "CRUD: Usuarios Registrados en Samba" --ok-button "< Aceptar >" --msgbox "$TABLA_USERS" 20 96
                ;;

            2)
                crear_usuario_guiado
                ;;

            3)
                alternar_estado_usuario
                ;;

            4)
                LISTA_USERS=$(pdbedit -L 2>/dev/null | cut -d: -f1)
                if [ -z "$LISTA_USERS" ]; then
                    whiptail --ok-button "< Aceptar >" --msgbox "No hay usuarios registrados." 8 45
                    continue
                fi
                local -a MENU_USERS=()
                for u in $LISTA_USERS; do
                    MENU_USERS+=("$u" "Usuario_Samba")
                done
                TARGET_USER=$(whiptail --title "Modificar Grupos de Usuario" \
                    --ok-button "< Siguiente >" --cancel-button "< Cancelar >" \
                    --menu "Selecciona el usuario al que deseas modificar los grupos:" 16 68 6 \
                    "${MENU_USERS[@]}" 3>&1 1>&2 2>&3)
                RET=$?
                if [ $RET -eq 0 ] && [ -n "$TARGET_USER" ]; then
                    GRUPOS_DISP=$(awk -F: '$1 ~ /^grp_/ {print $1}' /etc/group | sort -u)
                    local -a LISTA_OPC=()
                    GRPS_ACTUALES=$(id -Gn "$TARGET_USER" 2>/dev/null)
                    for g in $GRUPOS_DISP; do
                        [ "$g" == "$TARGET_USER" ] && continue
                        STATUS="OFF"
                        if echo "$GRPS_ACTUALES" | grep -qw "$g"; then STATUS="ON"; fi
                        LISTA_OPC+=("$g" "Grupo_$g" "$STATUS")
                    done
                    NUEVOS_GRPS=$(whiptail --title "Modificar Grupos de $TARGET_USER" \
                        --ok-button "< Guardar >" --cancel-button "< Cancelar >" \
                        --checklist "Marca con ESPACIO los grupos asignados a $TARGET_USER:" 18 72 8 \
                        "${LISTA_OPC[@]}" 3>&1 1>&2 2>&3)
                    RET=$?
                    if [ $RET -eq 0 ]; then
                        NUEVOS_GRPS_CSV=$(echo "$NUEVOS_GRPS" | tr -d '\"' | tr ' ' ',')
                        if [ -n "$NUEVOS_GRPS_CSV" ]; then
                            # Remover de grupos grp_* previos
                            for g_prev in $GRUPOS_DISP; do
                                gpasswd -d "$TARGET_USER" "$g_prev" 2>/dev/null || true
                            done
                            usermod -aG "$NUEVOS_GRPS_CSV" "$TARGET_USER"
                            
                            # Ajustar shell y home según pertenezca a grp_sistemas
                            if echo "$NUEVOS_GRPS_CSV" | grep -qw "grp_sistemas"; then
                                usermod -s /bin/bash -aG sudo,adm "$TARGET_USER"
                                if [ ! -d "/home/$TARGET_USER" ]; then
                                    mkdir -p "/home/$TARGET_USER"
                                    cp -r /etc/skel/. "/home/$TARGET_USER/" 2>/dev/null || true
                                    chown -R "$TARGET_USER:$TARGET_USER" "/home/$TARGET_USER"
                                    chmod 700 "/home/$TARGET_USER"
                                fi
                            else
                                usermod -s /usr/sbin/nologin "$TARGET_USER"
                                gpasswd -d "$TARGET_USER" sudo 2>/dev/null || true
                                gpasswd -d "$TARGET_USER" adm 2>/dev/null || true
                            fi
                            
                            whiptail --title "$APP_TITLE" --ok-button "< Aceptar >" \
                                --msgbox "✔ Grupos actualizados para $TARGET_USER:\n$NUEVOS_GRPS_CSV" 9 60
                        fi
                    fi
                fi
                ;;

            5)
                LISTA_USERS=$(pdbedit -L 2>/dev/null | cut -d: -f1)
                if [ -z "$LISTA_USERS" ]; then
                    whiptail --ok-button "< Aceptar >" --msgbox "No hay usuarios registrados." 8 45
                    continue
                fi
                local -a MENU_USERS=()
                for u in $LISTA_USERS; do
                    MENU_USERS+=("$u" "Usuario_Samba")
                done
                USER_PW_SEL=$(whiptail --title "Cambiar Contraseña Samba" \
                    --ok-button "< Siguiente >" --cancel-button "< Cancelar >" \
                    --menu "Selecciona el usuario para cambiar su contraseña:" 16 68 6 \
                    "${MENU_USERS[@]}" 3>&1 1>&2 2>&3)
                RET=$?
                if [ $RET -eq 0 ] && [ -n "$USER_PW_SEL" ]; then
                    NUEVA_CLAVE=$(whiptail --title "Nueva Contraseña" \
                        --ok-button "< Cambiar >" --cancel-button "< Cancelar >" \
                        --passwordbox "Ingresa la nueva contraseña de Samba para $USER_PW_SEL:" 10 65 3>&1 1>&2 2>&3)
                    RET=$?
                    if [ $RET -eq 0 ] && [ -n "$NUEVA_CLAVE" ]; then
                        printf '%s\n%s\n' "$NUEVA_CLAVE" "$NUEVA_CLAVE" | smbpasswd -s "$USER_PW_SEL"
                        if id -Gn "$USER_PW_SEL" 2>/dev/null | grep -qw "grp_sistemas"; then
                            echo "${USER_PW_SEL}:${NUEVA_CLAVE}" | chpasswd 2>/dev/null || true
                        fi
                        whiptail --title "$APP_TITLE" --ok-button "< Aceptar >" \
                            --msgbox "✔ Contraseña de Samba actualizada con éxito para '$USER_PW_SEL'." 8 60
                    fi
                fi
                ;;

            6)
                LISTA_USERS=$(pdbedit -L 2>/dev/null | cut -d: -f1 | grep -v "^root$")
                if [ -z "$LISTA_USERS" ]; then
                    whiptail --ok-button "< Aceptar >" --msgbox "No hay usuarios disponibles para eliminar." 8 45
                    continue
                fi
                local -a MENU_USERS=()
                for u in $LISTA_USERS; do
                    MENU_USERS+=("$u" "Usuario_Samba")
                done
                USER_DEL_SEL=$(whiptail --title "Eliminar Usuario" \
                    --ok-button "< Siguiente >" --cancel-button "< Cancelar >" \
                    --menu "Selecciona el usuario que deseas eliminar:" 16 68 6 \
                    "${MENU_USERS[@]}" 3>&1 1>&2 2>&3)
                RET=$?
                if [ $RET -eq 0 ] && [ -n "$USER_DEL_SEL" ]; then
                    if (whiptail --title "Confirmación de Eliminación" \
                        --yes-button "< Sí, Eliminar >" --no-button "< Cancelar >" \
                        --yesno "¿Estás seguro de eliminar al usuario '$USER_DEL_SEL' de Samba y Linux?" 10 60); then
                        smbpasswd -x "$USER_DEL_SEL" 2>/dev/null || true
                        deluser "$USER_DEL_SEL" 2>/dev/null || userdel "$USER_DEL_SEL" 2>/dev/null || true
                        whiptail --title "$APP_TITLE" --ok-button "< Aceptar >" \
                            --msgbox "✔ Usuario '$USER_DEL_SEL' eliminado exitosamente." 8 50
                    fi
                fi
                ;;
        esac
    done
}
