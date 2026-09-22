#!/bin/bash
# ==============================================================================
# Módulo: Motor de Respaldos Multiplataforma y Deduplicación
# ==============================================================================

validar_cron() {
    local expr="$1" campo
    local -a campos
    read -r -a campos <<< "$expr"
    [ "${#campos[@]}" -eq 5 ] || return 1
    for campo in "${campos[@]}"; do
        [[ "$campo" =~ ^[0-9*,/-]+$ ]] || return 1
    done
    return 0
}

gestionar_backups() {
    local OPC_BKP TABLA_BKPS TASK_NAME WIN_IP WIN_SHARE WIN_USER WIN_PASS LNX_IP LNX_PORT LNX_USER LNX_PASS LNX_PATH LOC_SRC
    local CRON_SCHED CRON_EXPR RETENTION CRED_FILE RUNNER TAREAS_DEL MENU_DEL TASK_DEL_SEL TEST_CONN
    while true; do
        OPC_BKP=$(whiptail --title "$APP_TITLE" \
            --ok-button "< Seleccionar >" --cancel-button "< Volver >" \
            --menu "GESTIÓN DE TAREAS DE COPIAS DE SEGURIDAD:" 18 72 6 \
            "1" "[*] Listar Tareas de Backup Programadas" \
            "2" "[+] Nueva Tarea: Servidor Windows / Carpeta SMB (CIFS)" \
            "3" "[+] Nueva Tarea: Servidor Linux Remoto (SSH + Rsync)" \
            "4" "[+] Nueva Tarea: Carpeta Local del Servidor" \
            "5" "[-] Eliminar una Tarea de Backup Programada" \
            "6" "[<] Volver al Menú Principal" 3>&1 1>&2 2>&3)

        RET=$?
        if [ $RET -ne 0 ] || [ "$OPC_BKP" == "6" ]; then
            break
        fi

        case "$OPC_BKP" in
            1)
                TABLA_BKPS=$(python3 -c '
import glob, os, re

runners = glob.glob("/usr/local/bin/backup_*.sh")
rows = []

for r in runners:
    tname = os.path.basename(r).replace("backup_", "").replace(".sh", "")
    proto = "Local"
    src = "N/A"
    ret = "30 snaps"
    
    with open(r, "r", encoding="utf-8", errors="ignore") as f:
        content = f.read()
        if "SRC_SHARE=" in content:
            proto = "CIFS (Win)"
            m_ip = re.search(r"SRC_IP=\"([^\"]+)\"", content)
            m_sh = re.search(r"SRC_SHARE=\"([^\"]+)\"", content)
            if m_ip and m_sh:
                src = f"//{m_ip.group(1)}/{m_sh.group(1)}"
        elif "SRC_PORT=" in content or "sshpass" in content:
            proto = "SSH (Linux)"
            m_ip = re.search(r"SRC_IP=\"([^\"]+)\"", content)
            m_pt = re.search(r"SRC_PATH=\"([^\"]+)\"", content)
            m_us = re.search(r"SRC_USER=\"([^\"]+)\"", content)
            if m_ip and m_pt:
                src = f"{m_us.group(1)}@{m_ip.group(1)}:{m_pt.group(1)}"
        else:
            proto = "Local"
            m_pt = re.search(r"SRC_PATH=\"([^\"]+)\"", content)
            if m_pt: src = m_pt.group(1)
        
        m_ret = re.search(r"RETENTION=(\d+)", content)
        if m_ret: ret = f"{m_ret.group(1)} snaps"

    cron_file = f"/etc/cron.d/backup_{tname}"
    cron_sched = "Manual"
    if os.path.exists(cron_file):
        with open(cron_file, "r") as cf:
            c_line = cf.read().strip()
            parts = c_line.split()
            if len(parts) >= 5:
                cron_sched = " ".join(parts[:5])

    rows.append((tname, proto, src, cron_sched, ret))

if not rows:
    print("  (No hay tareas de backup programadas actualmente)")
    exit(0)

w_name = max(max(len(r[0]) for r in rows), 16)
w_prot = max(max(len(r[1]) for r in rows), 11)
w_src  = max(max(len(r[2]) for r in rows), 24)
w_cron = max(max(len(r[3]) for r in rows), 12)
w_ret  = max(max(len(r[4]) for r in rows), 10)

print("┌─{}─┬─{}─┬─{}─┬─{}─┬─{}─┐".format("─"*w_name, "─"*w_prot, "─"*w_src, "─"*w_cron, "─"*w_ret))
print("│ {} │ {} │ {} │ {} │ {} │".format("IDENTIFICADOR".ljust(w_name), "PROTOCOLO".ljust(w_prot), "ORIGEN REMOTO / LOCAL".ljust(w_src), "HORARIO CRON".ljust(w_cron), "RETENCIÓN".ljust(w_ret)))
print("├─{}─┼─{}─┼─{}─┼─{}─┼─{}─┤".format("─"*w_name, "─"*w_prot, "─"*w_src, "─"*w_cron, "─"*w_ret))
for r in rows:
    print("│ {} │ {} │ {} │ {} │ {} │".format(r[0].ljust(w_name), r[1].ljust(w_prot), r[2].ljust(w_src), r[3].ljust(w_cron), r[4].ljust(w_ret)))
print("└─{}─┴─{}─┴─{}─┴─{}─┴─{}─┘".format("─"*w_name, "─"*w_prot, "─"*w_src, "─"*w_cron, "─"*w_ret))
')
                whiptail --title "CRUD: Tareas de Backup Programadas" --ok-button "< Aceptar >" --msgbox "$TABLA_BKPS" 18 78
                ;;

            2)
                # Respaldo de Servidor Windows (CIFS / SMB)
                TASK_NAME=$(whiptail --title "Paso 1 de 5: Identificador de Tarea" \
                    --ok-button "< Siguiente >" --cancel-button "< Cancelar >" \
                    --inputbox "Ingresa un nombre único para esta tarea (ej. srv_ad_contabilidad):" 10 65 3>&1 1>&2 2>&3)
                RET=$?
                if [ $RET -ne 0 ] || [ -z "$TASK_NAME" ]; then continue; fi
                TASK_NAME=$(echo "$TASK_NAME" | tr " " "_" | tr -cd "A-Za-z0-9_-")
                if [ -z "$TASK_NAME" ]; then
                    whiptail --title "Nombre Invalido" --ok-button "< Aceptar >" \
                        --msgbox "El identificador de la tarea no puede estar vacio ni contener solo simbolos." 9 65
                    continue
                fi
                if [ -f "/usr/local/bin/backup_${TASK_NAME}.sh" ]; then
                    if ! (whiptail --title "Tarea Existente" \
                        --yes-button "< Sobrescribir >" --no-button "< Cancelar >" \
                        --yesno "Ya existe una tarea llamada [$TASK_NAME]. ¿Deseas sobrescribirla?" 9 68); then
                        continue
                    fi
                fi

                WIN_IP=$(whiptail --title "Paso 2 de 5: Servidor Windows Remoto" \
                    --ok-button "< Siguiente >" --cancel-button "< Cancelar >" \
                    --inputbox "Dirección IP o Nombre del Servidor Windows (ej. 10.10.1.50):" 10 65 3>&1 1>&2 2>&3)
                RET=$?
                if [ $RET -ne 0 ] || [ -z "$WIN_IP" ]; then continue; fi
                if [[ ! "$WIN_IP" =~ ^[A-Za-z0-9._-]+$ ]]; then
                    whiptail --title "IP Invalida" --ok-button "< Aceptar >" \
                        --msgbox "La direccion IP o nombre del servidor contiene caracteres no permitidos." 9 65
                    continue
                fi

                WIN_SHARE=$(whiptail --title "Paso 3 de 5: Recurso Compartido Remoto" \
                    --ok-button "< Siguiente >" --cancel-button "< Cancelar >" \
                    --inputbox "Nombre de la carpeta compartida en Windows (ej. Documentos o C$):" 10 65 3>&1 1>&2 2>&3)
                RET=$?
                if [ $RET -ne 0 ] || [ -z "$WIN_SHARE" ]; then continue; fi
                WIN_SHARE=$(echo "$WIN_SHARE" | tr -d '/')
                if [[ ! "$WIN_SHARE" =~ ^[A-Za-z0-9_$.-]+$ ]]; then
                    whiptail --title "Recurso Invalido" --ok-button "< Aceptar >" \
                        --msgbox "El nombre del recurso compartido contiene caracteres no permitidos." 9 65
                    continue
                fi

                WIN_USER=$(whiptail --title "Paso 4 de 5: Credenciales de Acceso" \
                    --ok-button "< Siguiente >" --cancel-button "< Cancelar >" \
                    --inputbox "Usuario de Windows con permisos de lectura (ej. Administrador o usuario@dominio):" 10 65 "Administrador" 3>&1 1>&2 2>&3)
                RET=$?
                if [ $RET -ne 0 ] || [ -z "$WIN_USER" ]; then continue; fi
                if [[ ! "$WIN_USER" =~ ^[A-Za-z0-9._@-]+$ ]]; then
                    whiptail --title "Usuario Invalido" --ok-button "< Aceptar >" \
                        --msgbox "El usuario contiene caracteres no permitidos. Usa el formato usuario@dominio." 9 68
                    continue
                fi

                WIN_PASS=$(whiptail --title "Paso 4 de 5: Credenciales de Acceso" \
                    --ok-button "< Siguiente >" --cancel-button "< Cancelar >" \
                    --passwordbox "Contraseña para el usuario $WIN_USER:" 10 65 3>&1 1>&2 2>&3)
                RET=$?
                if [ $RET -ne 0 ]; then continue; fi

                # Test de conexión en vivo con smbclient
                if command -v smbclient &>/dev/null; then
                    TEST_CONN=$(USER="$WIN_USER" PASSWD="$WIN_PASS" smbclient "//$WIN_IP/$WIN_SHARE" -c "dir" 2>&1 || true)
                    if echo "$TEST_CONN" | grep -qiE "NT_STATUS_LOGON_FAILURE|NT_STATUS_BAD_NETWORK_NAME|NT_STATUS_UNSUCCESSFUL|NT_STATUS_ACCESS_DENIED|NT_STATUS_ACCOUNT_DISABLED|NT_STATUS_PASSWORD_EXPIRED|NT_STATUS_NO_LOGON_SERVERS|NT_STATUS_HOST_UNREACHABLE|NT_STATUS_CONNECTION_REFUSED|Connection to .* failed"; then
                        whiptail --title "Error de Conexión Remota" --ok-button "< Corregir >" \
                            --msgbox "✖ No se pudo conectar al servidor Windows con los datos ingresados:\n\n$TEST_CONN\n\nVerifica la IP, el recurso compartido o las credenciales." 14 72
                        continue
                    fi
                fi

                CRON_SCHED=$(whiptail --title "Paso 5 de 5: Frecuencia de Ejecución" \
                    --ok-button "< Siguiente >" --cancel-button "< Cancelar >" \
                    --menu "Selecciona el horario programado para ejecutar la copia de seguridad:" 16 70 4 \
                    "1" "Diario a las 23:00 hrs (Recomendado para servidores)" \
                    "2" "Cada 6 horas (00:00, 06:00, 12:00, 18:00)" \
                    "3" "Cada hora en punto" \
                    "4" "Personalizado (Expresión cron manual)" 3>&1 1>&2 2>&3)
                RET=$?
                if [ $RET -ne 0 ] || [ -z "$CRON_SCHED" ]; then continue; fi

                case "$CRON_SCHED" in
                    1) CRON_EXPR="0 23 * * *" ;;
                    2) CRON_EXPR="0 */6 * * *" ;;
                    3) CRON_EXPR="0 * * * *" ;;
                    4) 
                        CRON_EXPR=$(whiptail --title "Cron Personalizado" --ok-button "< Siguiente >" --cancel-button "< Cancelar >" \
                            --inputbox "Ingresa la expresión cron (Minuto Hora Día Mes DíaSemana):" 10 65 "0 2 * * *" 3>&1 1>&2 2>&3)
                        RET=$?
                        if [ $RET -ne 0 ] || [ -z "$CRON_EXPR" ]; then continue; fi
                        ;;
                esac

                if ! validar_cron "$CRON_EXPR"; then
                    whiptail --title "Cron Invalido" --ok-button "< Aceptar >" \
                        --msgbox "La expresion cron no es valida. Debe tener 5 campos (minuto hora dia mes diasemana)." 9 70
                    continue
                fi

                RETENTION=$(whiptail --title "Política de Retención de Snapshots" \
                    --ok-button "< Siguiente >" --cancel-button "< Cancelar >" \
                    --inputbox "Número de snapshots históricos a conservar antes de rotar:" 10 65 "30" 3>&1 1>&2 2>&3)
                RET=$?
                if [ $RET -ne 0 ]; then continue; fi
                RETENTION=${RETENTION:-30}
                if ! [[ "$RETENTION" =~ ^[0-9]+$ ]] || [ "$RETENTION" -lt 1 ]; then
                    RETENTION=30
                fi

                # Crear credenciales protegidas
                mkdir -p /etc/backup-credentials /mnt/backup_sources/"$TASK_NAME" /srv/nas/BACKUPS_HISTORICOS/"$TASK_NAME" /srv/nas/LOGS_BACKUP
                CRED_FILE="/etc/backup-credentials/${TASK_NAME}.cred"
                printf 'username=%s\npassword=%s\n' "$WIN_USER" "$WIN_PASS" > "$CRED_FILE"
                chmod 600 "$CRED_FILE"

                # Generar script de respaldo
                RUNNER="/usr/local/bin/backup_${TASK_NAME}.sh"
                cat << 'RUNNER_EOF' > "$RUNNER"
#!/bin/bash
set -e
TASK="TASK_NAME_PLACEHOLDER"
SRC_IP="WIN_IP_PLACEHOLDER"
SRC_SHARE="WIN_SHARE_PLACEHOLDER"
CRED_FILE="CRED_FILE_PLACEHOLDER"
MOUNT_POINT="${MOUNT_ROOT:-/mnt/backup_sources}/$TASK"
BKP_DIR="/srv/nas/BACKUPS_HISTORICOS/$TASK"
LOG_FILE="/srv/nas/LOGS_BACKUP/backup_${TASK}.log"
RETENTION=RETENTION_PLACEHOLDER
DATE_STR=$(date +%Y-%m-%d_%H%M%S)
TARGET_SNAPSHOT="$BKP_DIR/snapshot_$DATE_STR"

exec 9>"${LOCK_DIR:-/var/lock}/backup_${TASK}.lock"
flock -n 9 || { echo "=== BACKUP OMITIDO: ya hay una ejecucion en curso ($DATE_STR) ===" >> "$LOG_FILE"; exit 0; }

echo "=== INICIANDO BACKUP: $TASK ($DATE_STR) ===" >> "$LOG_FILE"

mkdir -p "$MOUNT_POINT" "$BKP_DIR"
DISPONIBLE_KB=$(df -Pk "$BKP_DIR" 2>/dev/null | awk 'NR==2 {print $4}')
if [ -n "$DISPONIBLE_KB" ] && [ "$DISPONIBLE_KB" -lt 524288 ]; then
    echo "=== ABORTADO: espacio libre insuficiente en $BKP_DIR ($((DISPONIBLE_KB / 1024)) MB) ===" >> "$LOG_FILE"
    exit 1
fi
umount "$MOUNT_POINT" 2>/dev/null || true

# Montaje en solo lectura
mount -t cifs "//$SRC_IP/$SRC_SHARE" "$MOUNT_POINT" -o credentials="$CRED_FILE",ro,iocharset=utf8,vers=3.0,sec=ntlmssp 2>> "$LOG_FILE"

LAST_SNAPSHOT=$(ls -d "$BKP_DIR"/snapshot_* 2>/dev/null | sort | tail -n 1 || echo "")
LINK_DEST_OPT=""
if [ -n "$LAST_SNAPSHOT" ] && [ -d "$LAST_SNAPSHOT" ]; then
    LINK_DEST_OPT="--link-dest=$LAST_SNAPSHOT"
    echo " -> Deduplicando con hardlinks contra: $(basename "$LAST_SNAPSHOT")" >> "$LOG_FILE"
fi

if ! rsync -a --delete $LINK_DEST_OPT "$MOUNT_POINT/" "$TARGET_SNAPSHOT/" >> "$LOG_FILE" 2>&1; then
    echo "=== BACKUP FALLIDO: se descarta el snapshot parcial ===" >> "$LOG_FILE"
    rm -rf "$TARGET_SNAPSHOT"
    exit 1
fi

umount "$MOUNT_POINT" 2>/dev/null || true

# Rotación de snapshots antiguos
SNAPSHOT_COUNT=$(ls -d "$BKP_DIR"/snapshot_* 2>/dev/null | wc -l)
if [ "$SNAPSHOT_COUNT" -gt "$RETENTION" ]; then
    OLDEST=$(ls -d "$BKP_DIR"/snapshot_* 2>/dev/null | sort | head -n -"$RETENTION")
    for old in $OLDEST; do
        echo " -> Rotando y eliminando snapshot antiguo: $(basename "$old")" >> "$LOG_FILE"
        rm -rf "$old"
    done
fi

echo "=== BACKUP FINALIZADO CON ÉXITO: $DATE_STR ===" >> "$LOG_FILE"
RUNNER_EOF

                sed -i "s|TASK_NAME_PLACEHOLDER|$TASK_NAME|g" "$RUNNER"
                sed -i "s|WIN_IP_PLACEHOLDER|$WIN_IP|g" "$RUNNER"
                sed -i "s|WIN_SHARE_PLACEHOLDER|$WIN_SHARE|g" "$RUNNER"
                sed -i "s|CRED_FILE_PLACEHOLDER|$CRED_FILE|g" "$RUNNER"
                sed -i "s|RETENTION_PLACEHOLDER|$RETENTION|g" "$RUNNER"
                chmod 750 "$RUNNER"

                # Programar en Cron
                echo "$CRON_EXPR root systemd-run --collect --unit=backup-${TASK_NAME} --slice=backups.slice -p CPUSchedulingPolicy=batch -p IOSchedulingClass=idle bash $RUNNER >/dev/null 2>&1" > "/etc/cron.d/backup_${TASK_NAME}"
                chmod 644 "/etc/cron.d/backup_${TASK_NAME}"

                whiptail --title "$APP_TITLE" --ok-button "< Aceptar >" \
                    --msgbox "✔ ¡Tarea de Backup \"[$TASK_NAME]\" creada y programada!\n\n• Origen:      \\\\$WIN_IP\\$WIN_SHARE (CIFS)\n• Destino:     /srv/nas/BACKUPS_HISTORICOS/$TASK_NAME\n• Frecuencia:  $CRON_EXPR\n• Retención:   $RETENTION snapshots con deduplicación" 14 72
                ;;

            3)
                # Respaldo de Servidor Linux Remoto (SSH + Rsync)
                TASK_NAME=$(whiptail --title "Paso 1 de 5: Identificador de Tarea" \
                    --ok-button "< Siguiente >" --cancel-button "< Cancelar >" \
                    --inputbox "Ingresa un nombre único para esta tarea (ej. srv_linux_web):" 10 65 3>&1 1>&2 2>&3)
                RET=$?
                if [ $RET -ne 0 ] || [ -z "$TASK_NAME" ]; then continue; fi
                TASK_NAME=$(echo "$TASK_NAME" | tr " " "_" | tr -cd "A-Za-z0-9_-")
                if [ -z "$TASK_NAME" ]; then
                    whiptail --title "Nombre Invalido" --ok-button "< Aceptar >" \
                        --msgbox "El identificador de la tarea no puede estar vacio ni contener solo simbolos." 9 65
                    continue
                fi
                if [ -f "/usr/local/bin/backup_${TASK_NAME}.sh" ]; then
                    if ! (whiptail --title "Tarea Existente" \
                        --yes-button "< Sobrescribir >" --no-button "< Cancelar >" \
                        --yesno "Ya existe una tarea llamada [$TASK_NAME]. ¿Deseas sobrescribirla?" 9 68); then
                        continue
                    fi
                fi

                LNX_IP=$(whiptail --title "Paso 2 de 5: Servidor Linux Remoto" \
                    --ok-button "< Siguiente >" --cancel-button "< Cancelar >" \
                    --inputbox "Dirección IP o Nombre del Servidor Linux Remoto:" 10 65 3>&1 1>&2 2>&3)
                RET=$?
                if [ $RET -ne 0 ] || [ -z "$LNX_IP" ]; then continue; fi
                if [[ ! "$LNX_IP" =~ ^[A-Za-z0-9._-]+$ ]]; then
                    whiptail --title "IP Invalida" --ok-button "< Aceptar >" \
                        --msgbox "La direccion IP o nombre del servidor contiene caracteres no permitidos." 9 65
                    continue
                fi

                LNX_PORT=$(whiptail --title "Puerto SSH" --ok-button "< Siguiente >" --cancel-button "< Cancelar >" \
                    --inputbox "Puerto SSH del Servidor Remoto:" 10 65 "22" 3>&1 1>&2 2>&3)
                LNX_PORT=${LNX_PORT:-22}
                if [[ ! "$LNX_PORT" =~ ^[0-9]+$ ]] || [ "$LNX_PORT" -lt 1 ] || [ "$LNX_PORT" -gt 65535 ]; then
                    whiptail --title "Puerto Invalido" --ok-button "< Aceptar >" \
                        --msgbox "El puerto SSH debe ser un numero entre 1 y 65535." 9 65
                    continue
                fi

                LNX_PATH=$(whiptail --title "Paso 3 de 5: Ruta Remota a Respaldar" \
                    --ok-button "< Siguiente >" --cancel-button "< Cancelar >" \
                    --inputbox "Ruta absoluta en el servidor remoto (ej. /var/www o /etc):" 10 65 "/var/www" 3>&1 1>&2 2>&3)
                RET=$?
                if [ $RET -ne 0 ] || [ -z "$LNX_PATH" ]; then continue; fi
                if [[ ! "$LNX_PATH" =~ ^/[A-Za-z0-9._/-]*$ ]]; then
                    whiptail --title "Ruta Invalida" --ok-button "< Aceptar >" \
                        --msgbox "La ruta remota debe ser absoluta y sin caracteres especiales." 9 68
                    continue
                fi

                LNX_USER=$(whiptail --title "Paso 4 de 5: Credenciales SSH" \
                    --ok-button "< Siguiente >" --cancel-button "< Cancelar >" \
                    --inputbox "Usuario SSH en el servidor remoto:" 10 65 "root" 3>&1 1>&2 2>&3)
                RET=$?
                if [ $RET -ne 0 ] || [ -z "$LNX_USER" ]; then continue; fi
                if [[ ! "$LNX_USER" =~ ^[A-Za-z0-9._@-]+$ ]]; then
                    whiptail --title "Usuario Invalido" --ok-button "< Aceptar >" \
                        --msgbox "El usuario contiene caracteres no permitidos." 9 60
                    continue
                fi

                LNX_PASS=$(whiptail --title "Paso 4 de 5: Credenciales SSH" \
                    --ok-button "< Siguiente >" --cancel-button "< Cancelar >" \
                    --passwordbox "Contraseña SSH para el usuario $LNX_USER:" 10 65 3>&1 1>&2 2>&3)
                RET=$?
                if [ $RET -ne 0 ]; then continue; fi

                CRON_SCHED=$(whiptail --title "Paso 5 de 5: Frecuencia de Ejecución" \
                    --ok-button "< Siguiente >" --cancel-button "< Cancelar >" \
                    --menu "Selecciona el horario programado para ejecutar la copia de seguridad:" 16 70 3 \
                    "1" "Diario a las 02:00 hrs de la madrugada (Recomendado)" \
                    "2" "Cada 12 horas" \
                    "3" "Personalizado (Expresión cron manual)" 3>&1 1>&2 2>&3)
                RET=$?
                if [ $RET -ne 0 ] || [ -z "$CRON_SCHED" ]; then continue; fi

                case "$CRON_SCHED" in
                    1) CRON_EXPR="0 2 * * *" ;;
                    2) CRON_EXPR="0 */12 * * *" ;;
                    3) 
                        CRON_EXPR=$(whiptail --title "Cron Personalizado" --ok-button "< Siguiente >" --cancel-button "< Cancelar >" \
                            --inputbox "Ingresa la expresión cron (Minuto Hora Día Mes DíaSemana):" 10 65 "0 3 * * *" 3>&1 1>&2 2>&3)
                        RET=$?
                        if [ $RET -ne 0 ] || [ -z "$CRON_EXPR" ]; then continue; fi
                        ;;
                esac

                if ! validar_cron "$CRON_EXPR"; then
                    whiptail --title "Cron Invalido" --ok-button "< Aceptar >" \
                        --msgbox "La expresion cron no es valida. Debe tener 5 campos (minuto hora dia mes diasemana)." 9 70
                    continue
                fi

                RETENTION=$(whiptail --title "Política de Retención" --ok-button "< Siguiente >" --cancel-button "< Cancelar >" \
                    --inputbox "Número de snapshots a conservar:" 10 65 "15" 3>&1 1>&2 2>&3)
                RET=$?
                if [ $RET -ne 0 ]; then continue; fi
                RETENTION=${RETENTION:-15}
                if ! [[ "$RETENTION" =~ ^[0-9]+$ ]] || [ "$RETENTION" -lt 1 ]; then
                    RETENTION=15
                fi

                mkdir -p /etc/backup-credentials /srv/nas/BACKUPS_HISTORICOS/"$TASK_NAME" /srv/nas/LOGS_BACKUP
                CRED_FILE="/etc/backup-credentials/${TASK_NAME}.cred"
                printf '%s\n' "$LNX_PASS" > "$CRED_FILE"
                chmod 600 "$CRED_FILE"

                RUNNER="/usr/local/bin/backup_${TASK_NAME}.sh"
                cat << 'RUNNER_EOF' > "$RUNNER"
#!/bin/bash
set -e
TASK="TASK_NAME_PLACEHOLDER"
SRC_IP="LNX_IP_PLACEHOLDER"
SRC_PORT="LNX_PORT_PLACEHOLDER"
SRC_PATH="LNX_PATH_PLACEHOLDER"
SRC_USER="LNX_USER_PLACEHOLDER"
CRED_FILE="CRED_FILE_PLACEHOLDER"
BKP_DIR="/srv/nas/BACKUPS_HISTORICOS/$TASK"
LOG_FILE="/srv/nas/LOGS_BACKUP/backup_${TASK}.log"
RETENTION=RETENTION_PLACEHOLDER
DATE_STR=$(date +%Y-%m-%d_%H%M%S)
TARGET_SNAPSHOT="$BKP_DIR/snapshot_$DATE_STR"

exec 9>"${LOCK_DIR:-/var/lock}/backup_${TASK}.lock"
flock -n 9 || { echo "=== BACKUP OMITIDO: ya hay una ejecucion en curso ($DATE_STR) ===" >> "$LOG_FILE"; exit 0; }

echo "=== INICIANDO BACKUP LINUX SSH: $TASK ($DATE_STR) ===" >> "$LOG_FILE"
mkdir -p "$BKP_DIR"
DISPONIBLE_KB=$(df -Pk "$BKP_DIR" 2>/dev/null | awk 'NR==2 {print $4}')
if [ -n "$DISPONIBLE_KB" ] && [ "$DISPONIBLE_KB" -lt 524288 ]; then
    echo "=== ABORTADO: espacio libre insuficiente en $BKP_DIR ($((DISPONIBLE_KB / 1024)) MB) ===" >> "$LOG_FILE"
    exit 1
fi

LAST_SNAPSHOT=$(ls -d "$BKP_DIR"/snapshot_* 2>/dev/null | sort | tail -n 1 || echo "")
LINK_DEST_OPT=""
if [ -n "$LAST_SNAPSHOT" ] && [ -d "$LAST_SNAPSHOT" ]; then
    LINK_DEST_OPT="--link-dest=$LAST_SNAPSHOT"
    echo " -> Deduplicando con hardlinks contra: $(basename "$LAST_SNAPSHOT")" >> "$LOG_FILE"
fi

if ! SSHPASS=$(cat "$CRED_FILE") sshpass -e rsync -avz -e "ssh -p $SRC_PORT -o StrictHostKeyChecking=no" --delete $LINK_DEST_OPT "$SRC_USER@$SRC_IP:$SRC_PATH/" "$TARGET_SNAPSHOT/" >> "$LOG_FILE" 2>&1; then
    echo "=== BACKUP FALLIDO: se descarta el snapshot parcial ===" >> "$LOG_FILE"
    rm -rf "$TARGET_SNAPSHOT"
    exit 1
fi

SNAPSHOT_COUNT=$(ls -d "$BKP_DIR"/snapshot_* 2>/dev/null | wc -l)
if [ "$SNAPSHOT_COUNT" -gt "$RETENTION" ]; then
    OLDEST=$(ls -d "$BKP_DIR"/snapshot_* 2>/dev/null | sort | head -n -"$RETENTION")
    for old in $OLDEST; do
        echo " -> Rotando snapshot antiguo: $(basename "$old")" >> "$LOG_FILE"
        rm -rf "$old"
    done
fi

echo "=== BACKUP FINALIZADO CON ÉXITO: $DATE_STR ===" >> "$LOG_FILE"
RUNNER_EOF

                sed -i "s|TASK_NAME_PLACEHOLDER|$TASK_NAME|g" "$RUNNER"
                sed -i "s|LNX_IP_PLACEHOLDER|$LNX_IP|g" "$RUNNER"
                sed -i "s|LNX_PORT_PLACEHOLDER|$LNX_PORT|g" "$RUNNER"
                sed -i "s|LNX_PATH_PLACEHOLDER|$LNX_PATH|g" "$RUNNER"
                sed -i "s|LNX_USER_PLACEHOLDER|$LNX_USER|g" "$RUNNER"
                sed -i "s|CRED_FILE_PLACEHOLDER|$CRED_FILE|g" "$RUNNER"
                sed -i "s|RETENTION_PLACEHOLDER|$RETENTION|g" "$RUNNER"
                chmod 750 "$RUNNER"

                echo "$CRON_EXPR root systemd-run --collect --unit=backup-${TASK_NAME} --slice=backups.slice -p CPUSchedulingPolicy=batch -p IOSchedulingClass=idle bash $RUNNER >/dev/null 2>&1" > "/etc/cron.d/backup_${TASK_NAME}"
                chmod 644 "/etc/cron.d/backup_${TASK_NAME}"

                whiptail --title "$APP_TITLE" --ok-button "< Aceptar >" \
                    --msgbox "✔ ¡Tarea de Backup Linux \"[$TASK_NAME]\" creada!\n\n• Origen:      $LNX_USER@$LNX_IP:$LNX_PATH (SSH)\n• Destino:     /srv/nas/BACKUPS_HISTORICOS/$TASK_NAME\n• Frecuencia:  $CRON_EXPR\n• Retención:   $RETENTION snapshots deduplicados" 14 72
                ;;

            4)
                # Respaldo de Carpeta Local
                TASK_NAME=$(whiptail --title "Paso 1 de 4: Identificador de Tarea" \
                    --ok-button "< Siguiente >" --cancel-button "< Cancelar >" \
                    --inputbox "Ingresa un nombre único para esta tarea local:" 10 65 3>&1 1>&2 2>&3)
                RET=$?
                if [ $RET -ne 0 ] || [ -z "$TASK_NAME" ]; then continue; fi
                TASK_NAME=$(echo "$TASK_NAME" | tr " " "_" | tr -cd "A-Za-z0-9_-")
                if [ -z "$TASK_NAME" ]; then
                    whiptail --title "Nombre Invalido" --ok-button "< Aceptar >" \
                        --msgbox "El identificador de la tarea no puede estar vacio ni contener solo simbolos." 9 65
                    continue
                fi
                if [ -f "/usr/local/bin/backup_${TASK_NAME}.sh" ]; then
                    if ! (whiptail --title "Tarea Existente" \
                        --yes-button "< Sobrescribir >" --no-button "< Cancelar >" \
                        --yesno "Ya existe una tarea llamada [$TASK_NAME]. ¿Deseas sobrescribirla?" 9 68); then
                        continue
                    fi
                fi

                LOC_SRC=$(whiptail --title "Paso 2 de 4: Ruta Origen Local" \
                    --ok-button "< Siguiente >" --cancel-button "< Cancelar >" \
                    --inputbox "Ruta física absoluta de la carpeta origen a respaldar:" 10 65 "/srv/nas/SISTEMAS" 3>&1 1>&2 2>&3)
                RET=$?
                if [ $RET -ne 0 ] || [ -z "$LOC_SRC" ]; then continue; fi
                if [[ ! "$LOC_SRC" =~ ^/[A-Za-z0-9._/-]*$ ]]; then
                    whiptail --title "Ruta Invalida" --ok-button "< Aceptar >" \
                        --msgbox "La ruta local debe ser absoluta y sin caracteres especiales." 9 68
                    continue
                fi

                CRON_EXPR=$(whiptail --title "Paso 3 de 4: Horario de Ejecución" --ok-button "< Siguiente >" --cancel-button "< Cancelar >" \
                    --inputbox "Expresión cron (por defecto a las 23:30 hrs diario):" 10 65 "30 23 * * *" 3>&1 1>&2 2>&3)
                RET=$?
                if [ $RET -ne 0 ]; then continue; fi
                CRON_EXPR=${CRON_EXPR:-"30 23 * * *"}
                if ! validar_cron "$CRON_EXPR"; then
                    whiptail --title "Cron Invalido" --ok-button "< Aceptar >" \
                        --msgbox "La expresion cron no es valida. Debe tener 5 campos (minuto hora dia mes diasemana)." 9 70
                    continue
                fi

                RETENTION=$(whiptail --title "Paso 4 de 4: Retención" --ok-button "< Siguiente >" --cancel-button "< Cancelar >" \
                    --inputbox "Número de snapshots a retener:" 10 65 "30" 3>&1 1>&2 2>&3)
                RET=$?
                if [ $RET -ne 0 ]; then continue; fi
                RETENTION=${RETENTION:-30}
                if ! [[ "$RETENTION" =~ ^[0-9]+$ ]] || [ "$RETENTION" -lt 1 ]; then
                    RETENTION=30
                fi

                mkdir -p /srv/nas/BACKUPS_HISTORICOS/"$TASK_NAME" /srv/nas/LOGS_BACKUP
                RUNNER="/usr/local/bin/backup_${TASK_NAME}.sh"
                cat << 'RUNNER_EOF' > "$RUNNER"
#!/bin/bash
set -e
TASK="TASK_NAME_PLACEHOLDER"
SRC_PATH="LOC_SRC_PLACEHOLDER"
BKP_DIR="/srv/nas/BACKUPS_HISTORICOS/$TASK"
LOG_FILE="/srv/nas/LOGS_BACKUP/backup_${TASK}.log"
RETENTION=RETENTION_PLACEHOLDER
DATE_STR=$(date +%Y-%m-%d_%H%M%S)
TARGET_SNAPSHOT="$BKP_DIR/snapshot_$DATE_STR"

exec 9>"${LOCK_DIR:-/var/lock}/backup_${TASK}.lock"
flock -n 9 || { echo "=== BACKUP OMITIDO: ya hay una ejecucion en curso ($DATE_STR) ===" >> "$LOG_FILE"; exit 0; }

echo "=== INICIANDO BACKUP LOCAL: $TASK ($DATE_STR) ===" >> "$LOG_FILE"
mkdir -p "$BKP_DIR"
DISPONIBLE_KB=$(df -Pk "$BKP_DIR" 2>/dev/null | awk 'NR==2 {print $4}')
if [ -n "$DISPONIBLE_KB" ] && [ "$DISPONIBLE_KB" -lt 524288 ]; then
    echo "=== ABORTADO: espacio libre insuficiente en $BKP_DIR ($((DISPONIBLE_KB / 1024)) MB) ===" >> "$LOG_FILE"
    exit 1
fi

LAST_SNAPSHOT=$(ls -d "$BKP_DIR"/snapshot_* 2>/dev/null | sort | tail -n 1 || echo "")
LINK_DEST_OPT=""
if [ -n "$LAST_SNAPSHOT" ] && [ -d "$LAST_SNAPSHOT" ]; then
    LINK_DEST_OPT="--link-dest=$LAST_SNAPSHOT"
    echo " -> Deduplicando con hardlinks contra: $(basename "$LAST_SNAPSHOT")" >> "$LOG_FILE"
fi

if ! rsync -a --delete $LINK_DEST_OPT "$SRC_PATH/" "$TARGET_SNAPSHOT/" >> "$LOG_FILE" 2>&1; then
    echo "=== BACKUP FALLIDO: se descarta el snapshot parcial ===" >> "$LOG_FILE"
    rm -rf "$TARGET_SNAPSHOT"
    exit 1
fi

SNAPSHOT_COUNT=$(ls -d "$BKP_DIR"/snapshot_* 2>/dev/null | wc -l)
if [ "$SNAPSHOT_COUNT" -gt "$RETENTION" ]; then
    OLDEST=$(ls -d "$BKP_DIR"/snapshot_* 2>/dev/null | sort | head -n -"$RETENTION")
    for old in $OLDEST; do
        echo " -> Rotando snapshot antiguo: $(basename "$old")" >> "$LOG_FILE"
        rm -rf "$old"
    done
fi

echo "=== BACKUP FINALIZADO: $DATE_STR ===" >> "$LOG_FILE"
RUNNER_EOF

                sed -i "s|TASK_NAME_PLACEHOLDER|$TASK_NAME|g" "$RUNNER"
                sed -i "s|LOC_SRC_PLACEHOLDER|$LOC_SRC|g" "$RUNNER"
                sed -i "s|RETENTION_PLACEHOLDER|$RETENTION|g" "$RUNNER"
                chmod 750 "$RUNNER"

                echo "$CRON_EXPR root systemd-run --collect --unit=backup-${TASK_NAME} --slice=backups.slice -p CPUSchedulingPolicy=batch -p IOSchedulingClass=idle bash $RUNNER >/dev/null 2>&1" > "/etc/cron.d/backup_${TASK_NAME}"
                chmod 644 "/etc/cron.d/backup_${TASK_NAME}"

                whiptail --title "$APP_TITLE" --ok-button "< Aceptar >" \
                    --msgbox "✔ ¡Tarea de Backup Local \"[$TASK_NAME]\" programada exitosamente!" 8 60
                ;;

            5)
                # Eliminar tarea
                TAREAS_DEL=$(find /usr/local/bin -maxdepth 1 -name "backup_*.sh" -printf "%f\n" 2>/dev/null | sed "s|backup_||; s|\.sh||" || echo "")
                if [ -z "$TAREAS_DEL" ]; then
                    whiptail --ok-button "< Aceptar >" --msgbox "No hay tareas para eliminar." 8 45
                    continue
                fi

                local -a MENU_DEL=()
                for t in $TAREAS_DEL; do
                    MENU_DEL+=("$t" "Tarea_Programada")
                done

                TASK_DEL_SEL=$(whiptail --title "Eliminar Tarea de Backup" \
                    --ok-button "< Siguiente >" --cancel-button "< Cancelar >" \
                    --menu "Selecciona la tarea que deseas eliminar:" 16 68 6 \
                    "${MENU_DEL[@]}" 3>&1 1>&2 2>&3)

                RET=$?
                if [ $RET -eq 0 ] && [ -n "$TASK_DEL_SEL" ]; then
                    if (whiptail --title "Confirmar Eliminación de Tarea" \
                        --yes-button "< Sí, Eliminar Tarea >" --no-button "< Cancelar >" \
                        --yesno "¿Estás seguro de que deseas eliminar la tarea [$TASK_DEL_SEL] y sus credenciales?\n\n(Los respaldos históricos en disco no se borrarán)." 12 70); then
                        
                        rm -f "/usr/local/bin/backup_${TASK_DEL_SEL}.sh" "/etc/cron.d/backup_${TASK_DEL_SEL}" "/etc/backup-credentials/${TASK_DEL_SEL}.cred" "/mnt/backup_sources/${TASK_DEL_SEL}" 2>/dev/null || true
                        whiptail --title "$APP_TITLE" --ok-button "< Aceptar >" \
                            --msgbox "✔ Tarea [$TASK_DEL_SEL] eliminada exitosamente." 8 50
                    fi
                fi
                ;;
        esac
    done
}
