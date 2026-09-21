#!/bin/bash
# ==============================================================================
# Motor de Desinstalación y Limpieza Base (Debian 13)
# ==============================================================================

set -e

if [ "$EUID" -ne 0 ]; then
  echo "[-] Este script debe ejecutarse con privilegios de root (sudo bash $0)"
  exit 1
fi

echo "=============================================================================="
echo " [!] INICIANDO DESINSTALACIÓN Y LIMPIEZA TOTAL DEL SERVIDOR"
echo "=============================================================================="

echo "[1/7] Deteniendo y deshabilitando servicios..."
systemctl stop smbd nmbd wsdd2 cockpit.socket cockpit.service 2>/dev/null || true
systemctl disable smbd nmbd wsdd2 cockpit.socket 2>/dev/null || true

echo "[2/7] Desinstalando paquetes de Samba, Cockpit y dependencias..."
DEBIAN_FRONTEND=noninteractive apt-get purge -y \
    samba samba-common samba-common-bin wsdd2 smbclient \
    cockpit cockpit-storaged cockpit-networkmanager cockpit-packagekit \
    cockpit-file-sharing cockpit-identities cockpit-navigator 2>/dev/null || true
apt-get autoremove -y 2>/dev/null || true

echo "[3/7] Desmontando almacenamiento y limpiando /etc/fstab..."
umount /srv/nas 2>/dev/null || true
sed -i '\|/srv/nas|d' /etc/fstab
systemctl daemon-reload

echo "[4/7] Eliminando tareas de backup, credenciales y puntos de montaje..."
rm -f /usr/local/bin/backup_*.sh
rm -f /etc/cron.d/backup_*
rm -rf /etc/backup-credentials
rm -rf /mnt/backup_sources

echo "[5/7] Eliminando configuraciones, wrappers y parches del sistema..."
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

echo "[6/7] Eliminando grupos creados..."
grep -E '^grp_' /etc/group | cut -d: -f1 | while read -r grp; do
    groupdel "$grp" 2>/dev/null || true
done

echo "[7/7] Restaurando /etc/motd..."
: > /etc/motd
: > /etc/issue.net

echo ""
echo "=============================================================================="
echo " ✔ ¡DESINSTALACIÓN COMPLETADA CON ÉXITO!"
echo " El servidor ha vuelto a su estado base limpio."
echo "=============================================================================="
