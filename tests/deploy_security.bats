#!/usr/bin/env bats

# ==============================================================================
# Pruebas Unitarias de Endurecimiento de Seguridad (Kernel sysctl, auditd, AIDE)
# ==============================================================================

teardown() {
    if [ -n "$tmp" ] && [ -f "$tmp" ]; then
        rm -f "$tmp"
    fi
}

_extraer_bloque_sysctl() {
    awk '
        /cat << '\''SYSCTL_EOF'\'' > \/etc\/sysctl\.d\/99-nas-tuning\.conf$/ { capture=1; next }
        capture && /^SYSCTL_EOF$/ { capture=0; exit }
        capture { print }
    ' src/core/deploy.sh
}

_extraer_bloque_audit() {
    awk '
        /cat << '\''AUDIT_EOF'\'' > \/etc\/audit\/rules\.d\/nas\.rules$/ { capture=1; next }
        capture && /^AUDIT_EOF$/ { capture=0; exit }
        capture { print }
    ' src/core/deploy.sh
}

@test "deploy.sh instala los paquetes de seguridad auditd y aide" {
    # Validar que auditd y aide esten incluidos en la invocacion de apt-get install
    local bloque_apt
    bloque_apt=$(awk '
        /apt-get install/ { capture=1 }
        capture { print }
        capture && />\/dev\/null 2>&1; then/ { capture=0; exit }
    ' src/core/deploy.sh)
    [ -n "$bloque_apt" ]

    echo "$bloque_apt" | grep -qw "auditd"
    echo "$bloque_apt" | grep -qw "aide"
}

@test "el bloque sysctl de deploy.sh contiene directivas de red anti-spoofing y mitigacion syn flood" {
    local tmp
    tmp="$(mktemp)"
    _extraer_bloque_sysctl > "$tmp"
    [ -s "$tmp" ]

    run grep -q '^net\.ipv4\.conf\.all\.rp_filter = 1$' "$tmp"
    [ "$status" -eq 0 ]

    run grep -q '^net\.ipv4\.conf\.default\.rp_filter = 1$' "$tmp"
    [ "$status" -eq 0 ]

    run grep -q '^net\.ipv4\.tcp_syncookies = 1$' "$tmp"
    [ "$status" -eq 0 ]

    rm -f "$tmp"
}

@test "el bloque sysctl de deploy.sh contiene directivas de aislamiento de kernel y ptrace" {
    local tmp
    tmp="$(mktemp)"
    _extraer_bloque_sysctl > "$tmp"
    [ -s "$tmp" ]

    run grep -q '^kernel\.kptr_restrict = 2$' "$tmp"
    [ "$status" -eq 0 ]

    run grep -q '^kernel\.dmesg_restrict = 1$' "$tmp"
    [ "$status" -eq 0 ]

    run grep -q '^kernel\.yama\.ptrace_scope = 2$' "$tmp"
    [ "$status" -eq 0 ]

    rm -f "$tmp"
}

@test "todas las directivas en el bloque sysctl de deploy.sh tienen sintaxis clave = valor valida" {
    local tmp
    tmp="$(mktemp)"
    _extraer_bloque_sysctl > "$tmp"
    [ -s "$tmp" ]

    # Filtrar lineas no vacias y que no sean comentarios
    local invalidas
    invalidas=$(grep -v '^[[:space:]]*#' "$tmp" | grep -v '^[[:space:]]*$' | grep -vE '^[a-z0-9_.]+ *= *[0-9a-zA-Z_.]+$' || true)
    rm -f "$tmp"
    [ -z "$invalidas" ]
}

@test "deploy.sh genera reglas auditd para vigilar sudoers smb.conf credenciales y nas-terminal" {
    local tmp
    tmp="$(mktemp)"
    _extraer_bloque_audit > "$tmp"
    [ -s "$tmp" ]

    # Modificaciones en /etc/sudoers y /etc/sudoers.d/
    run grep -Eq '^-w /etc/sudoers -p wa' "$tmp"
    [ "$status" -eq 0 ]
    run grep -Eq '^-w /etc/sudoers\.d/ -p wa' "$tmp"
    [ "$status" -eq 0 ]

    # Modificaciones en /etc/samba/smb.conf
    run grep -Eq '^-w /etc/samba/smb\.conf -p wa' "$tmp"
    [ "$status" -eq 0 ]

    # Accesos a /etc/backup-credentials/
    run grep -Eq '^-w /etc/backup-credentials/ -p rwa' "$tmp"
    [ "$status" -eq 0 ]

    # Ejecuciones de /usr/local/sbin/nas-terminal
    run grep -Eq '^-w /usr/local/sbin/nas-terminal -p x' "$tmp"
    [ "$status" -eq 0 ]

    rm -f "$tmp"
}

@test "deploy.sh incluye recarga de reglas auditd e inicializacion no bloqueante de AIDE" {
    run grep -q 'augenrules --load' src/core/deploy.sh
    [ "$status" -eq 0 ]

    run grep -Eq 'service auditd restart|systemctl restart auditd' src/core/deploy.sh
    [ "$status" -eq 0 ]

    run grep -Eq 'aideinit -y -f.*aide --init' src/core/deploy.sh
    [ "$status" -eq 0 ]

    run grep -q 'cp -f /var/lib/aide/aide.db.new /var/lib/aide/aide.db' src/core/deploy.sh
    [ "$status" -eq 0 ]
}

@test "uninstall.sh elimina /etc/audit/rules.d/nas.rules y recarga la configuracion de auditoria" {
    run grep -q 'rm -f /etc/audit/rules.d/nas.rules' src/core/uninstall.sh
    [ "$status" -eq 0 ]

    run grep -q 'rm -f /etc/sysctl.d/99-nas-tuning.conf' src/core/uninstall.sh
    [ "$status" -eq 0 ]

    run grep -Eq 'service auditd restart|systemctl restart auditd' src/core/uninstall.sh
    [ "$status" -eq 0 ]

    run grep -q 'if \[ -d /etc/audit \]; then cp -a /etc/audit' src/core/uninstall.sh
    [ "$status" -eq 0 ]
}

@test "uninstall.sh invoca augenrules para regenerar reglas de auditoria tras la eliminacion" {
    run grep -q 'augenrules --load' src/core/uninstall.sh
    [ "$status" -eq 0 ]
}

@test "deploy.sh verifica el estado activo de auditd y aplica configuracion sysctl" {
    run grep -Eq 'for _svc in .*auditd' src/core/deploy.sh
    [ "$status" -eq 0 ]

    run grep -q 'sysctl -p /etc/sysctl.d/99-nas-tuning.conf' src/core/deploy.sh
    [ "$status" -eq 0 ]

    run grep -q 'systemctl enable auditd' src/core/deploy.sh
    [ "$status" -eq 0 ]
}

