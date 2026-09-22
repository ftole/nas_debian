#!/usr/bin/env bats

# Pruebas de inyeccion de fallos sobre los runners de backup generados.
# Genera los runners reales con backup_api.py en directorios temporales y los
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
    python3 - "$SANDBOX" <<'PY'
import sys, importlib.util as u
root = sys.argv[1]
spec = u.spec_from_file_location('b', 'src/web/backups/backup_api.py')
m = u.module_from_spec(spec)
spec.loader.exec_module(m)
m.BIN_DIR = root + '/runners'
m.CRON_DIR = root + '/cron'
m.CRED_DIR = root + '/cred'
m.BKP_ROOT = root + '/bkp'
m.LOG_ROOT = root + '/log'
m.KNOWN_HOSTS = root + '/known_hosts'
m.create_task({"id": "t_local", "proto": "local", "cron": "0 23 * * *", "retention": 30, "path": root + "/src"})
m.create_task({"id": "t_cifs", "proto": "cifs", "cron": "0 23 * * *", "retention": 30, "ip": "10.0.0.1", "share": "docs", "user": "Administrador", "password": "x"})
m.create_task({"id": "t_ssh", "proto": "ssh", "cron": "0 2 * * *", "retention": 15, "ip": "10.0.0.2", "port": "22", "path": "/var/www", "user": "root", "password": "x"})
PY
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
