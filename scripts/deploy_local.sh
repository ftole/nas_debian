#!/bin/bash
# ==============================================================================
# Despliegue local rápido de componentes compilados en Rust.
# Compila, instala en /usr/local/bin y reinicia el servicio nas-web.
# Uso: [sudo] bash scripts/deploy_local.sh [--all | --bin <nombre>] [--no-restart]
# ==============================================================================
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"

PREFIX="${PREFIX:-/usr/local/bin}"
TARGET_BIN="nas-web"
BUILD_ALL=0
RESTART_SERVICE=1
HAS_WEB=0

usage() {
    cat <<EOF
Uso: $0 [opciones]

Opciones:
  -b, --bin <nombre>   Compilar e instalar un binario específico (por defecto: nas-web)
                       Opciones: nas, nas-web, nas-tui, nasctl, nas-dev
  -a, --all            Compilar e instalar todos los binarios del workspace
      --no-restart     No reiniciar el servicio nas-web tras la instalación
  -h, --help           Muestra esta ayuda

Ejemplos:
  $0                   # Recompila nas-web, lo instala y reinicia el servicio
  $0 --all             # Recompila todo el workspace y actualiza /usr/local/bin
EOF
    exit 0
}

while [ $# -gt 0 ]; do
    case "$1" in
        -b|--bin)
            TARGET_BIN="${2:?falta el nombre del binario}"
            shift 2
            ;;
        -a|--all)
            BUILD_ALL=1
            shift
            ;;
        --no-restart)
            RESTART_SERVICE=0
            shift
            ;;
        -h|--help)
            usage
            ;;
        *)
            echo "[-] Opción desconocida: $1" >&2
            echo "    Ejecute '$0 --help' para ver las opciones disponibles." >&2
            exit 2
            ;;
    esac
done

# Determinar comando sudo si no somos root
SUDO_CMD=()
if [ "$(id -u)" -ne 0 ]; then
    if command -v sudo >/dev/null 2>&1; then
        SUDO_CMD=(sudo)
    else
        echo "[-] Error: Se requieren permisos de superusuario para escribir en $PREFIX" >&2
        exit 1
    fi
fi

# Validar cargo en PATH o directorios estándar de toolchain
if ! command -v cargo >/dev/null 2>&1; then
    if [ -x "$HOME/.cargo/bin/cargo" ]; then
        PATH="$HOME/.cargo/bin:$PATH"
        export PATH
    elif [ -n "${SUDO_USER:-}" ] && [ -x "/home/$SUDO_USER/.cargo/bin/cargo" ]; then
        PATH="/home/$SUDO_USER/.cargo/bin:$PATH"
        export PATH
    elif [ -x "/root/.cargo/bin/cargo" ]; then
        PATH="/root/.cargo/bin:$PATH"
        export PATH
    elif [ -x "/usr/local/cargo/bin/cargo" ]; then
        PATH="/usr/local/cargo/bin:$PATH"
        export PATH
    fi
fi

if ! command -v cargo >/dev/null 2>&1; then
    echo "[-] Error: cargo no está en el PATH." >&2
    exit 1
fi

cd "$REPO_DIR"

compute_cargo_jobs() {
    local cpus avail_mb mem_jobs cpu_jobs jobs
    cpus="$(nproc 2>/dev/null || echo 1)"
    avail_mb="$(awk '/MemAvailable:/ {printf "%d", $2/1024; found=1} END {if (!found) print 2048}' /proc/meminfo 2>/dev/null || echo 2048)"
    mem_jobs=$(( avail_mb / 1536 ))
    [ "$mem_jobs" -lt 1 ] && mem_jobs=1
    if [ "$cpus" -gt 2 ]; then
        cpu_jobs=$(( cpus - 1 ))
    else
        cpu_jobs=1
    fi
    jobs="$cpu_jobs"
    [ "$mem_jobs" -lt "$jobs" ] && jobs="$mem_jobs"
    [ "$jobs" -lt 1 ] && jobs=1
    printf '%s' "$jobs"
}

if [ -z "${CARGO_BUILD_JOBS:-}" ]; then
    CARGO_BUILD_JOBS="$(compute_cargo_jobs)"
    export CARGO_BUILD_JOBS
fi

run_cargo_build() {
    if command -v ionice >/dev/null 2>&1; then
        nice -n 10 ionice -c 2 -n 7 cargo "$@"
    else
        nice -n 10 cargo "$@"
    fi
}

if [ "$BUILD_ALL" -eq 1 ]; then
    echo "[1/3] Compilando workspace completo en modo --release (CARGO_BUILD_JOBS=${CARGO_BUILD_JOBS})..."
    run_cargo_build build --workspace --release --locked
    BINS_TO_INSTALL=("nas" "nas-web" "nas-tui" "nasctl" "nas-dev")
else
    echo "[1/3] Compilando paquete '$TARGET_BIN' en modo --release (CARGO_BUILD_JOBS=${CARGO_BUILD_JOBS})..."
    run_cargo_build build --release --locked -p "$TARGET_BIN"
    BINS_TO_INSTALL=("$TARGET_BIN")
fi

resolve_target_dir() {
    if [ -n "${CARGO_TARGET_DIR:-}" ]; then
        printf '%s' "$CARGO_TARGET_DIR"
        return 0
    fi
    local meta_dir=""
    meta_dir="$(cargo metadata --no-deps --format-version 1 --offline 2>/dev/null \
        | sed -n 's/.*"target_directory":"\([^"]*\)".*/\1/p' | head -n 1 || true)"
    if [ -n "$meta_dir" ]; then
        printf '%s' "$meta_dir"
    else
        printf '%s' "$REPO_DIR/target"
    fi
}

TARGET_ROOT="$(resolve_target_dir)"

echo "[2/3] Instalando binario(s) en $PREFIX..."
"${SUDO_CMD[@]}" install -d -m 0755 "$PREFIX"
for bin in "${BINS_TO_INSTALL[@]}"; do
    SRC="$TARGET_ROOT/release/$bin"
    if [ ! -f "$SRC" ]; then
        echo "[-] Error: No se encontró el binario compilado $SRC" >&2
        exit 1
    fi
    "${SUDO_CMD[@]}" install -m 0755 "$SRC" "$PREFIX/$bin"
    echo "  -> Instalado: $PREFIX/$bin"
    if [ "$bin" = "nas" ]; then
        "${SUDO_CMD[@]}" rm -f "$PREFIX/nas-cli"
        "${SUDO_CMD[@]}" ln -sf "$PREFIX/nas" "$PREFIX/nas-cli"
    fi
done

# Desplegar reglas de auditoría del kernel si auditd está presente en el servidor
if [ -f "$REPO_DIR/packaging/nas.rules" ] && [ -d /etc/audit ]; then
    "${SUDO_CMD[@]}" install -d -m 0750 /etc/audit/rules.d 2>/dev/null || true
    "${SUDO_CMD[@]}" install -m 0640 "$REPO_DIR/packaging/nas.rules" /etc/audit/rules.d/nas.rules 2>/dev/null || true
    if command -v augenrules >/dev/null 2>&1; then
        "${SUDO_CMD[@]}" augenrules --load >/dev/null 2>&1 || true
    fi
    if command -v systemctl >/dev/null 2>&1 && systemctl is-active --quiet auditd; then
        "${SUDO_CMD[@]}" systemctl restart auditd >/dev/null 2>&1 || true
    fi
fi

# Desplegar exclusiones AIDE si el subsistema está instalado
if [ -f "$REPO_DIR/packaging/75_aide_nas" ] && [ -d /etc/aide/aide.conf.d ]; then
    "${SUDO_CMD[@]}" install -m 0644 "$REPO_DIR/packaging/75_aide_nas" /etc/aide/aide.conf.d/75_aide_nas 2>/dev/null || true
fi

if [ "$RESTART_SERVICE" -eq 1 ]; then
    for bin in "${BINS_TO_INSTALL[@]}"; do
        if [ "$bin" = "nas-web" ]; then
            HAS_WEB=1
            break
        fi
    done

    if [ "$HAS_WEB" -eq 1 ]; then
        echo "[3/3] Reiniciando servicio nas-web..."
        if command -v systemctl >/dev/null 2>&1 && systemctl list-unit-files nas-web.service >/dev/null 2>&1; then
            "${SUDO_CMD[@]}" systemctl restart nas-web
            if "${SUDO_CMD[@]}" systemctl is-active --quiet nas-web; then
                echo "  -> Servicio nas-web activo y en ejecución."
            else
                echo "[!] Advertencia: nas-web no reporta estado 'active'." >&2
            fi
        else
            echo "  (systemctl o nas-web.service no disponible; omite reinicio)"
        fi
    else
        echo "[3/3] Omite reinicio de nas-web (no se actualizó nas-web)."
    fi
else
    echo "[3/3] Omite reinicio de servicios (--no-restart activo)."
fi

if [ -x "$PREFIX/nas" ]; then
    "${SUDO_CMD[@]}" "$PREFIX/nas" backup repair-runners >/dev/null 2>&1 || true
    if "${SUDO_CMD[@]}" test ! -f /var/lib/nas/migrations/backup_dedup_v2.done && { "${SUDO_CMD[@]}" test -d /srv/nas/BACKUPS_HISTORICOS || "${SUDO_CMD[@]}" test -d /srv/nas/BACKUPS_PERMANENTES || "${SUDO_CMD[@]}" test -d /srv/nas/unidades; }; then
        if command -v systemd-run >/dev/null 2>&1 && [ -d /run/systemd/system ]; then
            "${SUDO_CMD[@]}" systemd-run --quiet --no-block --collect --unit=nas-backup-dedup-migration \
                -p Nice=19 -p IOSchedulingClass=idle \
                "$PREFIX/nas" backup deduplicate --migration >/dev/null 2>&1 || true
        else
            "${SUDO_CMD[@]}" "$PREFIX/nas" backup deduplicate --migration >/dev/null 2>&1 || true
        fi
    fi
fi

echo ""
echo "=================================================================="
echo " Despliegue local finalizado exitosamente."
if [ "$HAS_WEB" -eq 1 ]; then
    echo " IMPORTANTE: Si modificó la interfaz web, recargue el navegador"
    echo " con Ctrl + Shift + R (o Ctrl + F5) para invalidar la caché."
fi
echo "=================================================================="
