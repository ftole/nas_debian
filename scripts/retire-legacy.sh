#!/bin/bash
# ==============================================================================
# Retiro escalonado del stack anterior (PHP + Bash + Python).
#
# Uso:
#   bash scripts/retire-legacy.sh            # simulación (no modifica nada)
#   bash scripts/retire-legacy.sh --apply    # aplica (requiere cutover validado)
#
# Guardas: no aplica si el panel Rust no está activo o si falta el respaldo de
# la configuración de Nginx. Cada paso imprime los `git mv`/`git rm` a ejecutar.
# ==============================================================================
set -euo pipefail

APPLY=0
[ "${1:-}" = "--apply" ] && APPLY=1

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

FAILED=0
say() { printf '  %s\n' "$*"; }
stage() { printf '\n== %s ==\n' "$*"; }
problem() { printf '  [!] %s\n' "$*"; FAILED=1; }

check_guards() {
    stage "Guardas previas"
    if [ "${NAS_RETIRE_SKIP_GUARDS:-0}" = "1" ]; then
        say "guardas de servidor omitidas (NAS_RETIRE_SKIP_GUARDS=1)"
        return 0
    fi
    if systemctl is-active --quiet nas-web 2>/dev/null; then
        say "nas-web activo"
    else
        problem "nas-web no está activo: ejecute el cutover (docs/despliegue_rust.md) antes de retirar el panel PHP"
    fi

    if ls /etc/nginx/sites-available/nas-web.bak.* >/dev/null 2>&1; then
        say "respaldo de Nginx presente"
    else
        problem "no hay respaldo de Nginx (packaging/install.sh lo crea al instalar)"
    fi

    if [ -f /etc/nas-web.env ]; then
        say "/etc/nas-web.env presente"
    else
        problem "falta /etc/nas-web.env (ejecute packaging/install.sh)"
    fi
}

plan_stage() {
    local title="$1"; shift
    stage "$title"
    for path in "$@"; do
        if [ -e "$path" ]; then
            say "retirar: $path"
        else
            say "ya retirado: $path"
        fi
    done
}

move_stage() {
    local title="$1"; shift
    stage "$title"
    local dest="$1"; shift
    mkdir -p "$dest"
    for path in "$@"; do
        [ -e "$path" ] || continue
        if [ "$APPLY" -eq 1 ]; then
            git mv "$path" "$dest/"
            say "movido: $path -> $dest/"
        else
            say "git mv $path $dest/"
        fi
    done
}

check_guards

plan_stage "Fase 1 · Panel PHP (mover a legacy/ para rollback)" \
    web

plan_stage "Fase 2 · CLI PHP y asistente Python (retirar)" \
    bin/nas-cli nas_admin.py nas_admin.bat nas-assistant.spec test_remote.py test_remote.sh

plan_stage "Fase 3 · Asistente whiptail (mover a legacy/)" \
    src/asistente.sh src/modules

plan_stage "Fase 4 · Pruebas del stack anterior (mover a legacy/)" \
    tests/test_web.php tests/test_qa_web.py tests/test_mockup.py tests/test_live_api.py \
    tests/test_nas_admin.py tests/test_remote_runner.py tests/test_cli.bats tests/test_remote.bats \
    src/web

echo
echo "Se conservan (aún no portados a Rust):"
say "src/core/deploy.sh, src/core/uninstall.sh, src/core/updater.sh, install.sh"

if [ "$FAILED" -ne 0 ]; then
    echo
    echo "Retiro bloqueado: resuelva las guardas antes de aplicar."
    exit 1
fi

if [ "$APPLY" -eq 1 ]; then
    move_stage "Aplicando Fase 1 (panel PHP -> legacy/)" legacy \
        web
    move_stage "Aplicando Fase 3 (whiptail -> legacy/)" legacy \
        src/asistente.sh src/modules

    stage "Aplicando Fase 2 (CLI PHP y Python)"
    for path in bin/nas-cli nas_admin.py nas_admin.bat nas-assistant.spec test_remote.py test_remote.sh; do
        [ -e "$path" ] || continue
        git rm -q "$path"
        say "eliminado: $path"
    done

    move_stage "Aplicando Fase 4 (pruebas legacy -> legacy/tests)" legacy/tests \
        tests/test_web.php tests/test_qa_web.py tests/test_mockup.py tests/test_live_api.py \
        tests/test_nas_admin.py tests/test_remote_runner.py tests/test_cli.bats tests/test_remote.bats \
        src/web

    stage "Siguientes pasos manuales"
    say "1. Actualizar .github/workflows/ci.yml (quitar jobs PHP/pytest del stack retirado)"
    say "2. Ajustar install.sh/nas_admin.* que referencien rutas retiradas"
    say "3. cargo test --workspace && pytest tests/ -q"
    say "4. Commit por archivo (regla del proyecto)"
    echo
    echo "Retiro aplicado en el árbol de trabajo."
else
    echo
    echo "Simulación completada. Ejecute con --apply tras validar el cutover."
fi
