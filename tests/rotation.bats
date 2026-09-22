#!/usr/bin/env bats

# Pruebas de rotación de snapshots (política de retención) sobre los runners
# generados. Genera un runner local con retención baja en un sandbox y ejecuta
# la rotación con binarios simulados. No toca discos, red ni servicios reales.

setup() {
    SANDBOX="$(mktemp -d)"
    mkdir -p "$SANDBOX/bin" "$SANDBOX/runners" "$SANDBOX/bkp" "$SANDBOX/log" \
             "$SANDBOX/lock" "$SANDBOX/cron" "$SANDBOX/cred" "$SANDBOX/src"
    ORIG_PATH="$PATH"
    export PATH="$SANDBOX/bin:$PATH"
    export LOCK_DIR="$SANDBOX/lock"

    echo "contenido de prueba" > "$SANDBOX/src/archivo.txt"

    # Generar un runner local con retención baja (2 snapshots)
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
m.create_task({"id": "t_rot", "proto": "local", "cron": "0 23 * * *", "retention": 2, "path": root + "/src"})
PY

    # df con espacio suficiente
    printf '#!/bin/bash\necho "Filesystem 1024-blocks Used Available Capacity Mounted on"\necho "fake 10000000 1000 9999000 1%% /"\n' > "$SANDBOX/bin/df"
    chmod +x "$SANDBOX/bin/df"

    # rsync que simula éxito creando el snapshot de destino
    cat > "$SANDBOX/bin/rsync" <<'EOF'
#!/bin/bash
for last; do :; done
mkdir -p "$last"
echo "ok" > "$last/ok.txt"
exit 0
EOF
    chmod +x "$SANDBOX/bin/rsync"
}

teardown() {
    export PATH="$ORIG_PATH"
    unset LOCK_DIR
    rm -rf "$SANDBOX"
}

@test "la rotación elimina los snapshots más antiguos al superar la retención" {
    mkdir -p "$SANDBOX/bkp/t_rot/snapshot_2020-01-01_000000"
    mkdir -p "$SANDBOX/bkp/t_rot/snapshot_2020-01-02_000000"
    mkdir -p "$SANDBOX/bkp/t_rot/snapshot_2020-01-03_000000"

    run bash "$SANDBOX/runners/backup_t_rot.sh"
    [ "$status" -eq 0 ]

    # 3 previos + 1 nuevo = 4; con retención 2 deben quedar 2 (los más recientes)
    count=$(find "$SANDBOX/bkp/t_rot" -maxdepth 1 -type d -name 'snapshot_*' | wc -l)
    [ "$count" -eq 2 ]
    [ ! -d "$SANDBOX/bkp/t_rot/snapshot_2020-01-01_000000" ]
    [ ! -d "$SANDBOX/bkp/t_rot/snapshot_2020-01-02_000000" ]
}

@test "no se eliminan snapshots cuando hay menos que la retención" {
    mkdir -p "$SANDBOX/bkp/t_rot/snapshot_2020-01-01_000000"

    run bash "$SANDBOX/runners/backup_t_rot.sh"
    [ "$status" -eq 0 ]

    # 1 previo + 1 nuevo = 2; con retención 2 no se elimina nada
    count=$(find "$SANDBOX/bkp/t_rot" -maxdepth 1 -type d -name 'snapshot_*' | wc -l)
    [ "$count" -eq 2 ]
    [ -d "$SANDBOX/bkp/t_rot/snapshot_2020-01-01_000000" ]
}
