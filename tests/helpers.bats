#!/usr/bin/env bats

setup() {
    # Cargar las funciones a probar
    source "src/lib/helpers.sh"
}

@test "obtener_ip_local debe retornar una IP valida o 127.0.0.1" {
    run obtener_ip_local
    [ "$status" -eq 0 ]
    # Verificar que el output parezca una IP (x.x.x.x)
    [[ "$output" =~ ^[0-9]+\.[0-9]+\.[0-9]+\.[0-9]+$ ]]
}

@test "obtener_netbios_defecto no debe estar vacio y estar en mayusculas" {
    run obtener_netbios_defecto
    [ "$status" -eq 0 ]
    [ -n "$output" ]
    # Debe coincidir con caracteres en mayúscula, números, o guiones
    [[ "$output" =~ ^[A-Z0-9_-]+$ ]]
}

@test "obtener_workgroup_defecto debe retornar WORKGROUP si no hay smb.conf" {
    # Hacemos un backup temporal de smb.conf si existe
    if [ -f /etc/samba/smb.conf ]; then
        mv /etc/samba/smb.conf /tmp/smb.conf.bak || true
    fi

    run obtener_workgroup_defecto
    [ "$status" -eq 0 ]
    [ "$output" = "WORKGROUP" ]

    # Restaurar
    if [ -f /tmp/smb.conf.bak ]; then
        mv /tmp/smb.conf.bak /etc/samba/smb.conf || true
    fi
}

@test "detect_default_user no debe estar vacio" {
    run detect_default_user
    [ "$status" -eq 0 ]
    [ -n "$output" ]
    # No debe retornar root si hay opciones
    if [ -z "$SUDO_USER" ] && ! awk -F: '$3 >= 1000 && $3 < 60000 && $1 != "nobody" {found=1; exit} END {if(found) exit 0; else exit 1}' /etc/passwd; then
        [ "$output" = "nas" ]
    fi
}

@test "resolver_disco_base retorna un disco base no vacio" {
    local root_dev
    root_dev=$(findmnt -n -o SOURCE / 2>/dev/null | head -n1)
    if [ -z "$root_dev" ] || [ ! -b "$root_dev" ]; then
        skip "la raiz no es un dispositivo de bloque en este entorno"
    fi
    run resolver_disco_base "$root_dev"
    [ "$status" -eq 0 ]
    [ -n "$output" ]
}

@test "resolver_discos_raiz retorna al menos un disco para la raiz" {
    local root_dev
    root_dev=$(findmnt -n -o SOURCE / 2>/dev/null | head -n1)
    if [ -z "$root_dev" ] || [ ! -b "$root_dev" ]; then
        skip "la raiz no es un dispositivo de bloque en este entorno"
    fi
    run resolver_discos_raiz "$root_dev"
    [ "$status" -eq 0 ]
    [ -n "$output" ]
}

@test "validar_cron acepta expresiones validas y rechaza invalidas" {
    # shellcheck source=src/modules/backups.sh
    source "src/modules/backups.sh"
    run validar_cron "0 23 * * *"
    [ "$status" -eq 0 ]
    run validar_cron "0 23 * *"
    [ "$status" -ne 0 ]
    run validar_cron "0 23 * * *; rm -rf /"
    [ "$status" -ne 0 ]
}
