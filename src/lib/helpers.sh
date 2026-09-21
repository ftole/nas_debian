#!/bin/bash
# ==============================================================================
# Librería: Funciones Auxiliares y Detección de Entorno
# ==============================================================================

# Detección inteligente de la IP local principal de red
obtener_ip_local() {
    local ip
    ip=$(ip route get 1.1.1.1 2>/dev/null | awk '{for(i=1;i<=NF;i++) if($i=="src"){print $(i+1); exit}}')
    if [ -z "$ip" ]; then
        ip=$(ip -4 addr show scope global | awk '$1=="inet" {print $2; exit}' | cut -d/ -f1)
    fi
    echo "${ip:-127.0.0.1}"
}

# Detección del hostname real de la máquina para NetBIOS por defecto
obtener_netbios_defecto() {
    local cur_host
    cur_host=$(hostname -s 2>/dev/null | tr '[:lower:]' '[:upper:]' | tr -cd '[:upper:]0-9_-')
    if [ -n "$cur_host" ] && [ "$cur_host" != "LOCALHOST" ]; then
        echo "$cur_host"
    else
        echo "SRV-EAD-NAS"
    fi
}

# Detección del Workgroup o Dominio configurado actualmente
obtener_workgroup_defecto() {
    local wg
    if [ -f /etc/samba/smb.conf ]; then
        wg=$(grep -i "^\s*workgroup\s*=" /etc/samba/smb.conf | head -n1 | awk -F= '{print $2}' | tr -d ' ' | tr '[:lower:]' '[:upper:]')
    fi
    echo "${wg:-WORKGROUP}"
}

# Detección del usuario estándar que invocó sudo o UID >= 1000
detect_default_user() {
    if [ -n "$SUDO_USER" ] && [ "$SUDO_USER" != "root" ]; then
        echo "$SUDO_USER"
    else
        local u
        u=$(awk -F: '$3 >= 1000 && $3 < 60000 && $1 != "nobody" {print $1; exit}' /etc/passwd)
        echo "${u:-nas}"
    fi
}

# Resolución del disco físico (base) que contiene un dispositivo dado.
# Soporta particiones, LVM, LUKS y subvolúmenes btrfs.
resolver_disco_base() {
    local dev="$1" base parent
    dev="${dev%%[*}"
    base=$(lsblk -no PKNAME "$dev" 2>/dev/null | head -n1)
    while [ -n "$base" ]; do
        parent=$(lsblk -no PKNAME "/dev/$base" 2>/dev/null | head -n1)
        [ -z "$parent" ] && break
        base="$parent"
    done
    if [ -z "$base" ]; then
        base=$(basename "$dev")
    fi
    echo "$base"
}

# Lista los discos físicos (tipo disk) que respaldan un dispositivo dado.
# Soporta md/RAID, LVM, LUKS y subvolúmenes btrfs.
resolver_discos_raiz() {
    local dev="$1"
    dev="${dev%%[*}"
    lsblk -s -n -o NAME,TYPE "$dev" 2>/dev/null | awk '$2=="disk"{print "/dev/"$1}'
}
