#!/usr/bin/env bats

# Pruebas de fallo para las funciones de deteccion de discos (helpers.sh).
# Usan binarios simulados en PATH: no tocan el sistema real.

setup() {
    # shellcheck source=src/lib/helpers.sh
    source "src/lib/helpers.sh"
    REAL_DEV="$(lsblk -dn -o NAME 2>/dev/null | head -n1)"
    if [ -n "$REAL_DEV" ]; then
        REAL_DEV="/dev/$REAL_DEV"
    fi
    MOCK_BIN="$(mktemp -d)"
    ORIG_PATH="$PATH"
    export PATH="$MOCK_BIN:$PATH"
}

teardown() {
    export PATH="$ORIG_PATH"
    unset MDSTAT
    rm -rf "$MOCK_BIN"
}

mock_cmd() {
    printf '#!/bin/bash\n%s\n' "$2" > "$MOCK_BIN/$1"
    chmod +x "$MOCK_BIN/$1"
}

@test "disco_en_uso detecta un disco montado" {
    [ -n "$REAL_DEV" ] || skip "sin dispositivos de bloque en este entorno"
    mock_cmd lsblk 'echo /mnt/algo'
    run disco_en_uso "$REAL_DEV"
    [ "$status" -eq 0 ]
}

@test "disco_en_uso marca como libre un disco sin montaje ni uso" {
    [ -n "$REAL_DEV" ] || skip "sin dispositivos de bloque en este entorno"
    mock_cmd lsblk ':'
    mock_cmd pvs ':'
    run disco_en_uso "$REAL_DEV"
    [ "$status" -eq 1 ]
}

@test "disco_en_uso detecta un disco usado como PV de LVM" {
    [ -n "$REAL_DEV" ] || skip "sin dispositivos de bloque en este entorno"
    mock_cmd lsblk ':'
    printf '#!/bin/bash\necho "%s"\n' "$REAL_DEV" > "$MOCK_BIN/pvs"
    chmod +x "$MOCK_BIN/pvs"
    run disco_en_uso "$REAL_DEV"
    [ "$status" -eq 0 ]
}

@test "disco_en_uso_critico detecta un PV de LVM" {
    [ -n "$REAL_DEV" ] || skip "sin dispositivos de bloque en este entorno"
    mock_cmd lsblk ':'
    printf '#!/bin/bash\necho "%s"\n' "$REAL_DEV" > "$MOCK_BIN/pvs"
    chmod +x "$MOCK_BIN/pvs"
    run disco_en_uso_critico "$REAL_DEV"
    [ "$status" -eq 0 ]
}

@test "disco_en_uso_critico detecta un miembro de RAID" {
    [ -n "$REAL_DEV" ] || skip "sin dispositivos de bloque en este entorno"
    mock_cmd lsblk "echo $(basename "$REAL_DEV")"
    mock_cmd pvs ':'
    printf 'md0 : active raid1 %s[0]\n' "$(basename "$REAL_DEV")" > "$MOCK_BIN/mdstat"
    export MDSTAT="$MOCK_BIN/mdstat"
    run disco_en_uso_critico "$REAL_DEV"
    [ "$status" -eq 0 ]
}

@test "disco_en_uso_critico marca libre un disco sin PV ni RAID" {
    [ -n "$REAL_DEV" ] || skip "sin dispositivos de bloque en este entorno"
    mock_cmd lsblk "echo $(basename "$REAL_DEV")"
    mock_cmd pvs ':'
    : > "$MOCK_BIN/mdstat"
    export MDSTAT="$MOCK_BIN/mdstat"
    run disco_en_uso_critico "$REAL_DEV"
    [ "$status" -eq 1 ]
}
