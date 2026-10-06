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
    run grep -q 'root:grp_sistemas' "$tmp"
    [ "$status" -ne 0 ]

    rm -f "$tmp"
}
