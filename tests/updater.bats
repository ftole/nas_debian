#!/usr/bin/env bats

# Pruebas de las funciones puras del motor de actualización (sin acceso a red).

setup() {
    # shellcheck source=src/core/updater.sh
    source "src/core/updater.sh"
}

@test "_normalizar_remoto normaliza una URL HTTPS" {
    result=$(_normalizar_remoto "https://github.com/ftole/nas_debian.git")
    [ "$result" = "ftole/nas_debian" ]
}

@test "_normalizar_remoto normaliza una URL SSH" {
    result=$(_normalizar_remoto "git@github.com:ftole/nas_debian.git")
    [ "$result" = "ftole/nas_debian" ]
}

@test "_normalizar_remoto distingue un repositorio distinto" {
    result=$(_normalizar_remoto "https://github.com/otro/repo.git")
    [ "$result" != "ftole/nas_debian" ]
    [ "$result" = "otro/repo" ]
}

@test "_normalizar_remoto rechaza hosts ajenos a github.com" {
    run _normalizar_remoto "https://gitlab.com/ftole/nas_debian.git"
    [ "$status" -ne 0 ]
    [ -z "$output" ]

    run _normalizar_remoto "git@evil.com:ftole/nas_debian.git"
    [ "$status" -ne 0 ]
    [ -z "$output" ]

    run _normalizar_remoto "ssh://git@otro.com/ftole/nas_debian.git"
    [ "$status" -ne 0 ]
    [ -z "$output" ]
}

@test "_validar_sintaxis valida los scripts del proyecto" {
    run _validar_sintaxis
    [ "$status" -eq 0 ]
}

@test "_verificar_firma_tag rechaza un tag inexistente" {
    run _verificar_firma_tag "tag-que-no-existe-xyz" ""
    [ "$status" -ne 0 ]
}

@test "_hook_post_actualizacion se ejecuta sin errores de forma no destructiva" {
    run _hook_post_actualizacion
    [ "$status" -eq 0 ]
}

@test "_hook_post_actualizacion extrae la definicion de nas-terminal desde deploy.sh" {
    local tmp_dir
    tmp_dir="$(mktemp -d)"
    local tmp_term="$tmp_dir/nas-terminal"

    awk '
        /^cat << '\''NAS_TERM_EOF'\'' > \/usr\/local\/sbin\/nas-terminal$/ { capture=1; next }
        capture && /^NAS_TERM_EOF$/ { capture=0; exit }
        capture { print }
    ' src/core/deploy.sh > "$tmp_term"

    [ -s "$tmp_term" ]
    run grep -q 'SESSION="nas-web-term-\${TARGET_USER}"' "$tmp_term"
    [ "$status" -eq 0 ]
    rm -rf "$tmp_dir"
}

