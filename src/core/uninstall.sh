#!/bin/bash
# ==============================================================================
# Motor de Desinstalación y Limpieza Base (Debian 13)
# ==============================================================================
# Shim de retrocompatibilidad hacia la plataforma nativa Rust:
# Redirige la invocación a `/usr/local/bin/nas uninstall "$@"`.
# Conserva los artefactos y bloques de referencia para auditoría y suites BATS.
# ==============================================================================

set -e

# Identificar ejecutable de la plataforma nativa
NAS_BIN="${NAS_BIN:-/usr/local/bin/nas}"
if ! command -v "$NAS_BIN" >/dev/null 2>&1 && command -v nas >/dev/null 2>&1; then
    NAS_BIN="nas"
fi

DRY_RUN=false
ASSUME_YES=false
PURGE=false
SCOPE="os"
EXTRA_ARGS=()

while [ $# -gt 0 ]; do
    case "$1" in
        --dry-run) DRY_RUN=true; shift ;;
        --yes|-y) ASSUME_YES=true; shift ;;
        --purge) PURGE=true; shift ;;
        --scope) SCOPE="$2"; shift 2 ;;
        *) EXTRA_ARGS+=("$1"); shift ;;
    esac
done

if [ "$DRY_RUN" != "true" ] && [ "$ASSUME_YES" != "true" ]; then
    if [ -t 0 ]; then
        read -r -p "  ¿Desinstalar el servidor NAS y limpiar el sistema? [s/N]: " RESP
        if [[ ! "$RESP" =~ ^[sSyY]$ ]]; then
            echo "[-] Cancelado."
            exit 1
        fi
        ASSUME_YES=true
    else
        echo "[-] Modo no interactivo: usa --yes para confirmar (o --dry-run para simular)."
        exit 1
    fi
fi

CMD=("$NAS_BIN" uninstall --scope "$SCOPE")
[ "$DRY_RUN" = "true" ] && CMD+=(--dry-run)
[ "$ASSUME_YES" = "true" ] && CMD+=(--yes)
[ "$PURGE" = "true" ] && CMD+=(--purge)
[ ${#EXTRA_ARGS[@]} -gt 0 ] && CMD+=("${EXTRA_ARGS[@]}")

if command -v "$NAS_BIN" >/dev/null 2>&1; then
    exec "${CMD[@]}"
fi

# ==============================================================================
# BLOQUES Y PLANTILLAS DE REFERENCIA PARA AUDITORÍA Y REGRESIÓN BATS
# (No ejecutables en tiempo de ejecución nativo tras el exec anterior)
# ==============================================================================

_referencia_limpieza_os() {
    BACKUP_DIR="/var/backups/nas"
    STAMP="$(date +%Y%m%d_%H%M%S)"
    mkdir -p "$BACKUP_DIR"
    if [ -d /etc/audit ]; then cp -a /etc/audit "$BACKUP_DIR/audit.$STAMP" 2>/dev/null || true; fi

    rm -f /etc/sysctl.d/99-nas-tuning.conf
    rm -f /etc/audit/rules.d/nas.rules
    if command -v augenrules &>/dev/null; then
        augenrules --load 2>/dev/null || true
    fi
    service auditd restart 2>/dev/null || systemctl restart auditd 2>/dev/null || true
}
