#!/bin/bash
# ==============================================================================
# Motor de Desinstalación y Limpieza Base (Debian 13)
# ==============================================================================

set -e

if [ "$EUID" -ne 0 ]; then
  echo "[-] Este script debe ejecutarse con privilegios de root (sudo bash $0)"
  exit 1
fi

# Opciones: --dry-run (solo muestra), --yes (no pregunta, para uso desde el asistente)
DRY_RUN=false
ASSUME_YES=false
for arg in "$@"; do
  case "$arg" in
    --dry-run) DRY_RUN=true ;;
    --yes|-y) ASSUME_YES=true ;;
  esac
done

# Confirmación cuando se ejecuta directamente (no desde el asistente, que ya confirmó).
if [ "$DRY_RUN" != "true" ] && [ "$ASSUME_YES" != "true" ]; then
  if [ -t 0 ]; then
    read -r -p "  ¿Desinstalar el servidor NAS y limpiar el sistema? [s/N]: " RESP
    if [[ ! "$RESP" =~ ^[sSyY]$ ]]; then
      echo "[-] Cancelado."
      exit 1
    fi
  else
    echo "[-] Modo no interactivo: usa --yes para confirmar (o --dry-run para simular)."
    exit 1
  fi
fi

echo "=============================================================================="
echo " [!] INICIANDO DESINSTALACIÓN Y LIMPIEZA TOTAL DEL SERVIDOR"
if [ "$DRY_RUN" == "true" ]; then
  echo " [i] MODO SIMULACIÓN (--dry-run): no se modificará el sistema"
fi
echo "=============================================================================="

BACKUP_DIR="/var/backups/nas"
STAMP="$(date +%Y%m%d_%H%M%S)"

echo "[0/7] Respaldando configuraciones antes de eliminar..."
if [ "$DRY_RUN" == "true" ]; then
    echo "  [dry-run] respaldar fstab, samba, credenciales y cron en $BACKUP_DIR"
else
    mkdir -p "$BACKUP_DIR"
    if [ -f /etc/fstab ]; then cp -a /etc/fstab "$BACKUP_DIR/fstab.$STAMP"; fi
    if [ -d /etc/samba ]; then cp -a /etc/samba "$BACKUP_DIR/samba.$STAMP" 2>/dev/null || true; fi
    if [ -d /etc/backup-credentials ]; then cp -a /etc/backup-credentials "$BACKUP_DIR/backup-credentials.$STAMP" 2>/dev/null || true; fi
    if [ -d /etc/cron.d ]; then cp -a /etc/cron.d "$BACKUP_DIR/cron.d.$STAMP" 2>/dev/null || true; fi
    echo "  Respaldos guardados en $BACKUP_DIR"
fi

echo "[1/7] Deteniendo y deshabilitando servicios..."
if [ "$DRY_RUN" == "true" ]; then
    echo "  [dry-run] detener y deshabilitar smbd, nmbd, wsdd2 y cockpit"
else
    systemctl stop smbd nmbd wsdd2 cockpit.socket cockpit.service 2>/dev/null || true
    systemctl disable smbd nmbd wsdd2 cockpit.socket 2>/dev/null || true
fi

echo "[2/7] Desinstalando paquetes de Samba, Cockpit y extensiones..."
if [ "$DRY_RUN" == "true" ]; then
    echo "  [dry-run] apt-get purge de Samba, Cockpit y extensiones"
else
    DEBIAN_FRONTEND=noninteractive apt-get purge -y \
        samba samba-common samba-common-bin wsdd2 smbclient \
        cockpit cockpit-storaged cockpit-networkmanager cockpit-packagekit \
        cockpit-file-sharing cockpit-identities cockpit-navigator 2>/dev/null || true
fi
echo "  Nota: no se ejecuta 'apt-get autoremove' automáticamente (evita borrar"
echo "        dependencias que otros servicios puedan necesitar)."

echo "[3/7] Desmontando almacenamiento y limpiando /etc/fstab..."
if [ "$DRY_RUN" == "true" ]; then
    echo "  [dry-run] desmontar /srv/nas y quitar su línea de /etc/fstab"
else
    umount /srv/nas 2>/dev/null || true
    sed -i '\|/srv/nas|d' /etc/fstab
    systemctl daemon-reload
fi

echo "[4/7] Eliminando tareas de backup, credenciales y puntos de montaje..."
if [ "$DRY_RUN" == "true" ]; then
    echo "  [dry-run] eliminar runners, cron, credenciales y /mnt/backup_sources"
else
    rm -f /usr/local/bin/backup_*.sh
    rm -f /etc/cron.d/backup_*
    rm -rf /etc/backup-credentials
    rm -rf /mnt/backup_sources
fi

echo "[5/7] Eliminando configuraciones, wrappers y parches del sistema..."
if [ "$DRY_RUN" == "true" ]; then
    echo "  [dry-run] eliminar /etc/samba, wrappers, parches y overrides"
else
    rm -rf /etc/samba
    rm -f /usr/local/sbin/chage /usr/local/sbin/passwd /usr/local/bin/lastb
    rm -f /usr/bin/lastb
    if command -v dpkg-divert &>/dev/null; then
        dpkg-divert --remove --rename /usr/bin/lastb 2>/dev/null || true
    fi
    rm -rf /usr/share/cockpit/file-sharing /usr/share/cockpit/identities /usr/share/cockpit/navigator /usr/share/cockpit/backups
    rm -f /etc/udev/rules.d/80-udisks2-hide-os.rules
    rm -f /etc/sysctl.d/99-nas-tuning.conf
    rm -f /etc/default/wsdd2
    rm -rf /etc/systemd/system/wsdd2.service.d
    udevadm control --reload-rules 2>/dev/null || true
    systemctl daemon-reload
fi

echo "[6/7] Eliminando grupos creados..."
if [ "$DRY_RUN" == "true" ]; then
    echo "  [dry-run] eliminar grupos grp_*"
else
    grep -E '^grp_' /etc/group | cut -d: -f1 | while read -r grp; do
        groupdel "$grp" 2>/dev/null || true
    done
fi

echo "[7/7] Restaurando /etc/motd..."
if [ "$DRY_RUN" == "true" ]; then
    echo "  [dry-run] vaciar /etc/motd y /etc/issue.net"
else
    : > /etc/motd
    : > /etc/issue.net
fi

echo ""
echo "=============================================================================="
if [ "$DRY_RUN" == "true" ]; then
    echo " ✔ SIMULACIÓN COMPLETADA (no se modificó nada)."
else
    echo " ✔ ¡DESINSTALACIÓN COMPLETADA CON ÉXITO!"
    echo " El servidor ha vuelto a su estado base limpio."
    echo " Respaldos de configuración en: $BACKUP_DIR"
fi
echo "=============================================================================="
