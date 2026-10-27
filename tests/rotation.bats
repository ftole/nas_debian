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
    cat > "$SANDBOX/runners/backup_t_rot.sh" <<EOF
#!/bin/bash
set -e
TASK='t_rot'
SRC_PATH='$SANDBOX/src'
BKP_DIR='$SANDBOX/bkp/t_rot'
LOG_FILE='$SANDBOX/log/backup_t_rot.log'
RETENTION=2
DATE_STR=\$(date +%Y-%m-%d_%H%M%S)
STAGE_SNAPSHOT="\$BKP_DIR/.inprogress_\$DATE_STR"
FINAL_SNAPSHOT="\$BKP_DIR/snapshot_\$DATE_STR"

mkdir -p "\$STAGE_SNAPSHOT" "\$(dirname "\$LOG_FILE")"
rsync -a "\$SRC_PATH/" "\$STAGE_SNAPSHOT/" || exit 1
mv "\$STAGE_SNAPSHOT" "\$FINAL_SNAPSHOT"

ALL_SNAPS=\$(find "\$BKP_DIR" -maxdepth 1 -type d -name "snapshot_*" 2>/dev/null | sort || true)
COUNT=\$(echo "\$ALL_SNAPS" | grep -c "snapshot_" || echo 0)
if [ "\$COUNT" -gt "\$RETENTION" ]; then
    DIFF=\$((COUNT - RETENTION))
    echo "\$ALL_SNAPS" | head -n "\$DIFF" | while read -r old_snap; do
        if [ -n "\$old_snap" ]; then
            rm -rf "\$old_snap"
        fi
    done
fi
EOF
    chmod +x "$SANDBOX/runners/backup_t_rot.sh"

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
    if [ -d "$SANDBOX" ]; then
        chattr -R -i "$SANDBOX" 2>/dev/null || true
        rm -rf "$SANDBOX"
    fi
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
