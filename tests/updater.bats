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
}

@test "_validar_sintaxis valida los scripts del proyecto" {
    run _validar_sintaxis
    [ "$status" -eq 0 ]
}
