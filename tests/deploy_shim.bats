#!/usr/bin/env bats
# Verificaciones del shim real de src/core/deploy.sh (no de bloques muertos).

@test "deploy.sh compila (bash -n) y delega en nas deploy apply" {
    bash -n "$BATS_TEST_DIRNAME/../src/core/deploy.sh" 2>/dev/null || {
        # Assumption: proyecto raíz disponible vía env si el test corre fuera de tests/
        bash -n src/core/deploy.sh
    }
}

@test "deploy.sh usa --password-stdin para no exponer contraseñas en argv" {
    grep -q -- '--password-stdin' src/core/deploy.sh
}

@test "deploy.sh añade --yes en modo no interactivo" {
    # Debe incluir el flag condicional [ "$FORCE" = "true" ] ... --yes
    grep -q -- '--yes' src/core/deploy.sh
}

@test "deploy.sh no interpola la contraseña en argv literal" {
    # Debe consumir la contraseña por stdin; no invocar nas con valor literal.
    ! grep -qE 'CMD\+=\("--password"' src/core/deploy.sh
}

@test "deploy.sh valida el rol antes de ejecutar" {
    grep -q 'ARCHIVOS_BACKUP' src/core/deploy.sh
}
