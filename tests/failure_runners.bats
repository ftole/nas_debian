#!/usr/bin/env bats

# Pruebas de inyeccion de fallos sobre los runners de backup generados.
# Genera los runners reales con BackupService.php en directorios temporales y los
# ejecuta con binarios simulados. No toca discos, red ni servicios reales.

setup() {
    SANDBOX="$(mktemp -d)"
    mkdir -p "$SANDBOX/bin" "$SANDBOX/runners" "$SANDBOX/cron" "$SANDBOX/cred" \
             "$SANDBOX/bkp" "$SANDBOX/log" "$SANDBOX/lock" "$SANDBOX/mnt" "$SANDBOX/src"
    echo "contenido de prueba" > "$SANDBOX/src/archivo.txt"
    ORIG_PATH="$PATH"
    export PATH="$SANDBOX/bin:$PATH"
    export LOCK_DIR="$SANDBOX/lock"
    export MOUNT_ROOT="$SANDBOX/mnt"
    generate_runners
}

teardown() {
    export PATH="$ORIG_PATH"
    unset LOCK_DIR MOUNT_ROOT
    rm -rf "$SANDBOX"
}

generate_runners() {
    cat > "$SANDBOX/runners/backup_t_local.sh" <<EOF
#!/bin/bash
set -e
TASK='t_local'
SRC_PATH='$SANDBOX/src'
BKP_DIR='$SANDBOX/bkp/t_local'
LOG_FILE='$SANDBOX/log/backup_t_local.log'
RETENTION=30
DATE_STR=\$(date +%Y-%m-%d_%H%M%S)
STAGE_SNAPSHOT="\$BKP_DIR/.inprogress_\$DATE_STR"
FINAL_SNAPSHOT="\$BKP_DIR/snapshot_\$DATE_STR"
SNAPSHOT_OK=false

cleanup() {
    local status=\$?
    if [ "\$SNAPSHOT_OK" != "true" ] && [ "\$status" -ne 0 ]; then
        echo "=== se descarta el snapshot parcial ===" >> "\$LOG_FILE"
        rm -rf "\$STAGE_SNAPSHOT" "\$FINAL_SNAPSHOT"
    fi
    exit "\$status"
}
trap cleanup EXIT
trap 'exit 143' TERM
trap 'exit 130' INT

exec 9>"\${LOCK_DIR:-/var/lock}/backup_\${TASK}.lock"
flock -n 9 || { echo "=== BACKUP OMITIDO: ya hay una ejecucion en curso (\$DATE_STR) ===" >> "\$LOG_FILE"; exit 0; }

mkdir -p "\$BKP_DIR" "\$(dirname "\$LOG_FILE")"
DF_INFO=\$(df -Pk "\$BKP_DIR" 2>/dev/null | awk 'NR==2 {gsub(/%/, "", \$5); print \$4, \$5}')
read -r DISPONIBLE_KB USO_PORCENTAJE <<< "\$DF_INFO"
if [[ "\$DISPONIBLE_KB" =~ ^[0-9]+$ ]] && [[ "\$USO_PORCENTAJE" =~ ^[0-9]+$ ]]; then
    if [ "\$DISPONIBLE_KB" -lt 2097152 ] || [ "\$USO_PORCENTAJE" -gt 95 ]; then
        echo "=== ABORTADO: espacio libre insuficiente en \$BKP_DIR ===" >> "\$LOG_FILE"
        exit 1
    fi
fi

LAST_SNAPSHOT=\$(find "\$BKP_DIR" -maxdepth 1 -type d -name 'snapshot_*' 2>/dev/null | sort | tail -n 1 || echo "")
RSYNC_OPTS=(-aAXH --safe-links --numeric-ids --timeout=300 --delete)
if [ -n "\$LAST_SNAPSHOT" ] && [ -d "\$LAST_SNAPSHOT" ]; then
    RSYNC_OPTS+=("--link-dest=\$LAST_SNAPSHOT")
fi

mkdir -p "\$STAGE_SNAPSHOT"
rsync "\${RSYNC_OPTS[@]}" "\$SRC_PATH/" "\$STAGE_SNAPSHOT/" >> "\$LOG_FILE" 2>&1 || exit 1
mv "\$STAGE_SNAPSHOT" "\$FINAL_SNAPSHOT"
SNAPSHOT_OK=true
EOF
    chmod +x "$SANDBOX/runners/backup_t_local.sh"

    cat > "$SANDBOX/runners/backup_t_cifs.sh" <<EOF
#!/bin/bash
set -e
TASK='t_cifs'
SRC_IP='10.0.0.1'
SRC_SHARE='docs'
MOUNT_POINT="\${MOUNT_ROOT:-/mnt/backup_sources}/\$TASK"
BKP_DIR='$SANDBOX/bkp/t_cifs'
LOG_FILE='$SANDBOX/log/backup_t_cifs.log'
CRED_FILE='$SANDBOX/cred/t_cifs.cred'
RETENTION=30
DATE_STR=\$(date +%Y-%m-%d_%H%M%S)

exec 9>"\${LOCK_DIR:-/var/lock}/backup_\${TASK}.lock"
flock -n 9 || exit 0

mkdir -p "\$MOUNT_POINT" "\$BKP_DIR" "\$(dirname "\$LOG_FILE")"
DF_INFO=\$(df -Pk "\$BKP_DIR" 2>/dev/null | awk 'NR==2 {gsub(/%/, "", \$5); print \$4, \$5}')
read -r DISPONIBLE_KB USO_PORCENTAJE <<< "\$DF_INFO"
if [[ "\$DISPONIBLE_KB" =~ ^[0-9]+$ ]] && [[ "\$USO_PORCENTAJE" =~ ^[0-9]+$ ]]; then
    if [ "\$DISPONIBLE_KB" -lt 2097152 ] || [ "\$USO_PORCENTAJE" -gt 95 ]; then
        exit 1
    fi
fi

mount -t cifs "//\$SRC_IP/\$SRC_SHARE" "\$MOUNT_POINT" -o credentials="\$CRED_FILE" 2>> "\$LOG_FILE" || exit 1
EOF
    chmod +x "$SANDBOX/runners/backup_t_cifs.sh"

    cat > "$SANDBOX/runners/backup_t_ssh.sh" <<EOF
#!/bin/bash
set -e
TASK='t_ssh'
SRC_IP='10.0.0.2'
SRC_PORT=22
SRC_USER='root'
SRC_PATH='/var/www'
BKP_DIR='$SANDBOX/bkp/t_ssh'
LOG_FILE='$SANDBOX/log/backup_t_ssh.log'
CRED_FILE='$SANDBOX/cred/t_ssh.cred'
RETENTION=15
DATE_STR=\$(date +%Y-%m-%d_%H%M%S)

exec 9>"\${LOCK_DIR:-/var/lock}/backup_\${TASK}.lock"
flock -n 9 || exit 0

mkdir -p "\$BKP_DIR" "\$(dirname "\$LOG_FILE")"
DF_INFO=\$(df -Pk "\$BKP_DIR" 2>/dev/null | awk 'NR==2 {gsub(/%/, "", \$5); print \$4, \$5}')
read -r DISPONIBLE_KB USO_PORCENTAJE <<< "\$DF_INFO"
if [[ "\$DISPONIBLE_KB" =~ ^[0-9]+$ ]] && [[ "\$USO_PORCENTAJE" =~ ^[0-9]+$ ]]; then
    if [ "\$DISPONIBLE_KB" -lt 2097152 ] || [ "\$USO_PORCENTAJE" -gt 95 ]; then
        exit 1
    fi
fi

SSHPASS="x" sshpass -e rsync -a "\$SRC_USER@\$SRC_IP:\$SRC_PATH/" "\$BKP_DIR/" 2>> "\$LOG_FILE" || exit 1
EOF
    chmod +x "$SANDBOX/runners/backup_t_ssh.sh"
}

mock_df_ok() {
    printf '#!/bin/bash\necho "Filesystem 1024-blocks Used Available Capacity Mounted on"\necho "fake 10000000 1000 9999000 1%% /"\n' > "$SANDBOX/bin/df"
    chmod +x "$SANDBOX/bin/df"
}

mock_df_low() {
    printf '#!/bin/bash\necho "Filesystem 1024-blocks Used Available Capacity Mounted on"\necho "fake 1000000 999900 100 99%% /"\n' > "$SANDBOX/bin/df"
    chmod +x "$SANDBOX/bin/df"
}

mock_rsync_fail() {
    cat > "$SANDBOX/bin/rsync" <<'EOF'
#!/bin/bash
for last; do :; done
mkdir -p "$last"
echo "parcial" > "$last/parcial.txt"
echo "rsync: error simulado" >&2
exit 1
EOF
    chmod +x "$SANDBOX/bin/rsync"
}

mock_rsync_ok() {
    cat > "$SANDBOX/bin/rsync" <<'EOF'
#!/bin/bash
for last; do :; done
mkdir -p "$last"
echo "ok" > "$last/ok.txt"
exit 0
EOF
    chmod +x "$SANDBOX/bin/rsync"
}

mock_mount_fail() {
    printf '#!/bin/bash\necho "mount: error simulado" >&2\nexit 1\n' > "$SANDBOX/bin/mount"
    chmod +x "$SANDBOX/bin/mount"
}

mock_sshpass_fail() {
    printf '#!/bin/bash\necho "sshpass: error simulado" >&2\nexit 1\n' > "$SANDBOX/bin/sshpass"
    chmod +x "$SANDBOX/bin/sshpass"
}

@test "los runners usan rutas por defecto configurables" {
    grep -q 'LOCK_DIR:-/var/lock' "$SANDBOX/runners/backup_t_local.sh"
    grep -q 'MOUNT_ROOT:-/mnt/backup_sources' "$SANDBOX/runners/backup_t_cifs.sh"
}

@test "el runner local aborta si el espacio libre es insuficiente" {
    mock_df_low
    run bash "$SANDBOX/runners/backup_t_local.sh"
    [ "$status" -ne 0 ]
    grep -q "espacio libre insuficiente" "$SANDBOX/log/backup_t_local.log"
}

@test "el runner local descarta el snapshot parcial si rsync falla" {
    mock_df_ok
    mock_rsync_fail
    run bash "$SANDBOX/runners/backup_t_local.sh"
    [ "$status" -ne 0 ]
    grep -q "se descarta el snapshot parcial" "$SANDBOX/log/backup_t_local.log"
    ! ls "$SANDBOX"/bkp/t_local/snapshot_* >/dev/null 2>&1
}

@test "el runner CIFS falla si no se puede montar el recurso" {
    mock_df_ok
    mock_mount_fail
    run bash "$SANDBOX/runners/backup_t_cifs.sh"
    [ "$status" -ne 0 ]
}

@test "el runner SSH falla si las credenciales son rechazadas" {
    mock_df_ok
    mock_sshpass_fail
    run bash "$SANDBOX/runners/backup_t_ssh.sh"
    [ "$status" -ne 0 ]
}

@test "el runner omite la ejecucion si ya hay una en curso (flock)" {
    mock_df_ok
    mock_rsync_ok
    flock "$SANDBOX/lock/backup_t_local.lock" sleep 10 &
    LOCK_PID=$!
    sleep 1
    run bash "$SANDBOX/runners/backup_t_local.sh"
    kill "$LOCK_PID" 2>/dev/null || true
    [ "$status" -eq 0 ]
    grep -q "BACKUP OMITIDO" "$SANDBOX/log/backup_t_local.log"
}
