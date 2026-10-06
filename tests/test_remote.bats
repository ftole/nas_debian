#!/usr/bin/env bats

# Pruebas de la suite de pruebas remotas en Bash (test_remote.sh).

@test "test_remote.sh help muestra la lista completa de comandos y retorna 0" {
    run bash test_remote.sh help
    [ "$status" -eq 0 ]
    [[ "$output" =~ "test_remote.sh install" ]]
    [[ "$output" =~ "test_remote.sh uninstall" ]]
    [[ "$output" =~ "test_remote.sh update" ]]
    [[ "$output" =~ "test_remote.sh web" ]]
    [[ "$output" =~ "test_remote.sh samba" ]]
    [[ "$output" =~ "test_remote.sh backups" ]]
    [[ "$output" =~ "test_remote.sh suite" ]]
    [[ "$output" =~ "test_remote.sh console" ]]
    [[ "$output" =~ "test_remote.sh status" ]]
    [[ "$output" =~ "test_remote.sh config" ]]
}

@test "test_remote.sh rechaza comandos no reconocidos con código 1" {
    run bash test_remote.sh comando_invalido_xyz
    [ "$status" -eq 1 ]
    [[ "$output" =~ "Comando desconocido" ]]
}

@test "test_remote.sh cumple con la sintaxis de bash (bash -n)" {
    run bash -n test_remote.sh
    [ "$status" -eq 0 ]
}

@test "Invariante de seguridad: ni test_remote.sh ni test_remote.py usan StrictHostKeyChecking=no" {
    run grep "StrictHostKeyChecking=no" test_remote.sh test_remote.py
    [ "$status" -ne 0 ]
}

@test "test_remote.sh usa la directiva segura StrictHostKeyChecking=accept-new" {
    run grep "StrictHostKeyChecking=accept-new" test_remote.sh
    [ "$status" -eq 0 ]
}
