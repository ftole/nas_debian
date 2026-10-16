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

@test "_hook_post_actualizacion despliega nas-terminal y sudoers de forma segura en rutas configurables" {
    local tmp_dir
    tmp_dir="$(mktemp -d)"
    local fake_term="$tmp_dir/bin/nas-terminal"
    local fake_sudoers_dir="$tmp_dir/sudoers.d"
    mkdir -p "$(dirname "$fake_term")" "$fake_sudoers_dir"
    local fake_sudoers="$fake_sudoers_dir/nas-web"

    NAS_TERMINAL_DEST="$fake_term" \
    NAS_SUDOERS_DIR="$fake_sudoers_dir" \
    NAS_SUDOERS_DEST="$fake_sudoers" \
    run _hook_post_actualizacion

    [ "$status" -eq 0 ]
    [ -s "$fake_term" ]
    [ -x "$fake_term" ]
    run grep -q 'SESSION="nas-web-term-\${TARGET_USER}"' "$fake_term"
    [ "$status" -eq 0 ]

    [ -s "$fake_sudoers" ]
    run grep -q 'NAS_STORAGE' "$fake_sudoers"
    [ "$status" -eq 0 ]
    run grep -q '/dev/\[a-zA-Z0-9/_-\]\*' "$fake_sudoers"
    [ "$status" -eq 0 ]

    rm -rf "$tmp_dir"
}

@test "_hook_post_actualizacion extrae la definicion de nas-chpasswd desde deploy.sh" {
    local tmp_dir
    tmp_dir="$(mktemp -d)"
    local tmp_chp="$tmp_dir/nas-chpasswd"

    awk '
        /^cat << '\''NAS_CHPASSWD_EOF'\'' > \/usr\/local\/sbin\/nas-chpasswd$/ { capture=1; next }
        capture && /^NAS_CHPASSWD_EOF$/ { capture=0; exit }
        capture { print }
    ' src/core/deploy.sh > "$tmp_chp"

    [ -s "$tmp_chp" ]
    run grep -q 'no se permite modificar contraseñas de cuentas protegidas o root' "$tmp_chp"
    [ "$status" -eq 0 ]
    rm -rf "$tmp_dir"
}

@test "_hook_post_actualizacion despliega nas-chpasswd de forma segura en ruta configurable" {
    local tmp_dir
    tmp_dir="$(mktemp -d)"
    local fake_chp="$tmp_dir/bin/nas-chpasswd"
    mkdir -p "$(dirname "$fake_chp")"

    NAS_CHPASSWD_DEST="$fake_chp" \
    run _hook_post_actualizacion

    [ "$status" -eq 0 ]
    [ -s "$fake_chp" ]
    [ -x "$fake_chp" ]
    run grep -q '/usr/sbin/chpasswd' "$fake_chp"
    [ "$status" -eq 0 ]

    rm -rf "$tmp_dir"
}

@test "_normalizar_remoto rechaza esquema HTTP inseguro (vuln-0003)" {
    run _normalizar_remoto "http://github.com/ftole/nas_debian.git"
    [ "$status" -ne 0 ]
    [ -z "$output" ]

    run _normalizar_remoto "http://github.com/ftole/nas_debian"
    [ "$status" -ne 0 ]
    [ -z "$output" ]
}

@test "nas-chpasswd extraido rechaza root, usuarios invalidos y cuentas del sistema (vuln-0006)" {
    local tmp_dir
    tmp_dir="$(mktemp -d)"
    local tmp_chp="$tmp_dir/nas-chpasswd"

    awk '
        /^cat << '\''NAS_CHPASSWD_EOF'\'' > \/usr\/local\/sbin\/nas-chpasswd$/ { capture=1; next }
        capture && /^NAS_CHPASSWD_EOF$/ { capture=0; exit }
        capture { print }
    ' src/core/deploy.sh > "$tmp_chp"
    chmod +x "$tmp_chp"

    run bash -c "echo 'root:pass123' | '$tmp_chp'"
    [ "$status" -ne 0 ]
    [[ "$output" =~ "no se permite modificar contraseñas de cuentas protegidas o root" ]]

    run bash -c "echo 'bad;user:pass123' | '$tmp_chp'"
    [ "$status" -ne 0 ]
    [[ "$output" =~ "no se permite modificar contraseñas de cuentas protegidas o root" ]]

    run bash -c "echo 'daemon:pass123' | '$tmp_chp'"
    [ "$status" -ne 0 ]
    [[ "$output" =~ "el usuario no existe o es una cuenta de sistema" ]]

    rm -rf "$tmp_dir"
}

@test "_hook_post_actualizacion extrae la definicion de nas-user-gecos desde deploy.sh" {
    local tmp_dir
    tmp_dir="$(mktemp -d)"
    local tmp_gecos="$tmp_dir/nas-user-gecos"

    awk '
        /^cat << '\''NAS_GECOS_EOF'\'' > \/usr\/local\/sbin\/nas-user-gecos$/ { capture=1; next }
        capture && /^NAS_GECOS_EOF$/ { capture=0; exit }
        capture { print }
    ' src/core/deploy.sh > "$tmp_gecos"

    [ -s "$tmp_gecos" ]
    run grep -q 'nas-user-gecos: cuenta protegida o invalida' "$tmp_gecos"
    [ "$status" -eq 0 ]
    rm -rf "$tmp_dir"
}

@test "_hook_post_actualizacion despliega nas-user-gecos de forma segura en ruta configurable" {
    local tmp_dir
    tmp_dir="$(mktemp -d)"
    local fake_gecos="$tmp_dir/bin/nas-user-gecos"
    mkdir -p "$(dirname "$fake_gecos")"

    NAS_GECOS_DEST="$fake_gecos" \
    run _hook_post_actualizacion

    [ "$status" -eq 0 ]
    [ -s "$fake_gecos" ]
    [ -x "$fake_gecos" ]
    run grep -q '/usr/sbin/usermod -c' "$fake_gecos"
    [ "$status" -eq 0 ]

    rm -rf "$tmp_dir"
}

@test "nas-user-gecos extraido valida argumentos y rechaza root o cuentas invalidas (vuln-0011)" {
    local tmp_dir
    tmp_dir="$(mktemp -d)"
    local tmp_gecos="$tmp_dir/nas-user-gecos"

    awk '
        /^cat << '\''NAS_GECOS_EOF'\'' > \/usr\/local\/sbin\/nas-user-gecos$/ { capture=1; next }
        capture && /^NAS_GECOS_EOF$/ { capture=0; exit }
        capture { print }
    ' src/core/deploy.sh > "$tmp_gecos"
    chmod +x "$tmp_gecos"

    run bash "$tmp_gecos"
    [ "$status" -ne 0 ]

    run bash "$tmp_gecos" "root" "Super Root"
    [ "$status" -ne 0 ]
    [[ "$output" =~ "cuenta protegida o invalida" ]]

    run bash "$tmp_gecos" "user;id" "Injected Name"
    [ "$status" -ne 0 ]
    [[ "$output" =~ "cuenta protegida o invalida" ]]

    run bash "$tmp_gecos" "daemon" "Daemon User"
    [ "$status" -ne 0 ]
    [[ "$output" =~ "el usuario no existe o es una cuenta de sistema" ]]

    rm -rf "$tmp_dir"
}



