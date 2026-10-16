#!/usr/bin/env bats

# Regresión: el fragmento sudoers generado por deploy.sh debe ser válido.
# Un fragmento inválido en /etc/sudoers.d rompe sudo (y con ello toda la gestión web),
# por lo que este test valida sintaxis real con visudo.

_extraer_bloque_sudoers() {
    awk '
        /^cat << SUDOERS_EOF > \/etc\/sudoers\.d\/nas-web$/ { capture=1; next }
        capture && /^SUDOERS_EOF$/ { capture=0; exit }
        capture { print }
    ' src/core/deploy.sh
}

@test "el fragmento sudoers de deploy.sh es valido para visudo" {
    command -v visudo >/dev/null 2>&1 || skip "visudo no disponible"

    local tmp
    tmp="$(mktemp)"
    _extraer_bloque_sudoers > "$tmp"
    [ -s "$tmp" ]

    # El heredoc es expandido por bash: ${PHP_VER} -> 8.4 y \\ -> \
    sed -i -e 's/\${PHP_VER}/8.4/g' -e 's/\\\\/\\/g' "$tmp"

    run visudo -c -f "$tmp"
    rm -f "$tmp"
    [ "$status" -eq 0 ]
}

@test "el fragmento sudoers evita patrones que rompen el parser" {
    local tmp
    tmp="$(mktemp)"
    _extraer_bloque_sudoers > "$tmp"
    [ -s "$tmp" ]

    # Comas dentro de clases de caracteres (p. ej. [a-zA-Z0-9_,.-]) rompen sudoers.
    run grep -Eq '\[[^]]*,[^]]*\]' "$tmp"
    [ "$status" -ne 0 ]

    # Un ':' sin escapar en los argumentos (p. ej. chown root:grupo) es error de sintaxis.
    run grep -q 'root:grp_samba' "$tmp"
    [ "$status" -ne 0 ]

    rm -f "$tmp"
}

@test "el alias NAS_STORAGE autoriza desmontaje por ruta de dispositivo /dev/*" {
    local tmp
    tmp="$(mktemp)"
    _extraer_bloque_sudoers > "$tmp"
    [ -s "$tmp" ]

    run grep -q '/bin/umount /dev/\[a-zA-Z0-9/_-\]\*' "$tmp"
    [ "$status" -eq 0 ]
    run grep -q '/usr/bin/umount /dev/\[a-zA-Z0-9/_-\]\*' "$tmp"
    [ "$status" -eq 0 ]
    run grep -q '/bin/umount -l /dev/\[a-zA-Z0-9/_-\]\*' "$tmp"
    [ "$status" -eq 0 ]
    run grep -q '/usr/bin/umount -l /dev/\[a-zA-Z0-9/_-\]\*' "$tmp"
    [ "$status" -eq 0 ]

    rm -f "$tmp"
}

@test "el fragmento sudoers no contiene comodines inseguros ni alias obsoletos (vuln-0006 vuln-0008 vuln-0011)" {
    local tmp
    tmp="$(mktemp)"
    _extraer_bloque_sudoers > "$tmp"
    [ -s "$tmp" ]

    # NAS_TERMINAL no debe existir en sudoers (el helper corre directamente sin sudo)
    run grep -q 'NAS_TERMINAL' "$tmp"
    [ "$status" -ne 0 ]

    # No debe contener chattr * sin anclar
    run grep -Eq 'chattr[[:space:]]+\*' "$tmp"
    [ "$status" -ne 0 ]

    # No debe contener usermod -aG * ni usermod -c *
    run grep -Eq 'usermod[[:space:]]+-aG[[:space:]]+\*' "$tmp"
    [ "$status" -ne 0 ]
    run grep -Eq 'usermod[[:space:]]+-c[[:space:]]+\*' "$tmp"
    [ "$status" -ne 0 ]

    # No debe permitir /usr/sbin/chpasswd irrestricto
    run grep -Eq '/usr/sbin/chpasswd\b' "$tmp"
    [ "$status" -ne 0 ]
    run grep -q '/usr/local/sbin/nas-chpasswd' "$tmp"
    [ "$status" -eq 0 ]

    # No debe permitir gpasswd sobre sudo o adm directamente
    run grep -Eq 'gpasswd[[:space:]]+-[ad][[:space:]]+.*[[:space:]]+(sudo|adm)\b' "$tmp"
    [ "$status" -ne 0 ]

    # No debe copiar archivos desde /tmp hacia /etc
    run grep -Eq 'cp[[:space:]]+/tmp/' "$tmp"
    [ "$status" -ne 0 ]
    run grep -q '/var/lib/nas/staging/' "$tmp"
    [ "$status" -eq 0 ]

    # lvcreate no debe tener comodin libre
    run grep -Eq 'lvcreate[[:space:]]+\*' "$tmp"
    [ "$status" -ne 0 ]
    run grep -q 'lvcreate -y -l' "$tmp"
    [ "$status" -eq 0 ]

    rm -f "$tmp"
}

@test "el fragmento sudoers autoriza nas-user-gecos de forma acotada (vuln-0011)" {
    local tmp
    tmp="$(mktemp)"
    _extraer_bloque_sudoers > "$tmp"
    [ -s "$tmp" ]

    run grep -q '/usr/local/sbin/nas-user-gecos \[a-zA-Z0-9_.-]\*' "$tmp"
    [ "$status" -eq 0 ]

    rm -f "$tmp"
}

@test "el fragmento sudoers autoriza nas-admin-manage de forma acotada (vuln-0003)" {
    local tmp
    tmp="$(mktemp)"
    _extraer_bloque_sudoers > "$tmp"
    [ -s "$tmp" ]

    run grep -q '/usr/local/sbin/nas-admin-manage add \[a-zA-Z0-9_.-]\*' "$tmp"
    [ "$status" -eq 0 ]
    run grep -q '/usr/local/sbin/nas-admin-manage remove \[a-zA-Z0-9_.-]\*' "$tmp"
    [ "$status" -eq 0 ]

    rm -f "$tmp"
}

@test "deploy.sh configura firma SMB obligatoria para mitigar NTLM relay (vuln-0004)" {
    run grep -q 'server signing = mandatory' src/core/deploy.sh
    [ "$status" -eq 0 ]
}


