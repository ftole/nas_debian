#!/usr/bin/env bats

# Pruebas de la CLI de administración unificada (bin/nas-cli).

setup() {
    export APP_ENV=testing
}

@test "nas-cli help muestra la lista completa de modulos y retorna 0" {
    run php bin/nas-cli help
    [ "$status" -eq 0 ]
    [[ "$output" =~ "storage" ]]
    [[ "$output" =~ "share" ]]
    [[ "$output" =~ "backup" ]]
    [[ "$output" =~ "trash" ]]
    [[ "$output" =~ "user" ]]
    [[ "$output" =~ "group" ]]
    [[ "$output" =~ "domain" ]]
    [[ "$output" =~ "log" ]]
    [[ "$output" =~ "service" ]]
    [[ "$output" =~ "system" ]]
}

@test "nas-cli storage overview --json retorna JSON valido con /srv/nas" {
    run php bin/nas-cli storage overview --json
    [ "$status" -eq 0 ]
    [[ "$output" =~ '"success": true' ]]
    [[ "$output" =~ '"path": "/srv/nas"' ]]
}

@test "nas-cli storage disks --json retorna listado valido" {
    run php bin/nas-cli storage disks --json
    [ "$status" -eq 0 ]
    [[ "$output" =~ '"success": true' ]]
}

@test "nas-cli share list --json retorna listado estructurado de recursos" {
    run php bin/nas-cli share list --json
    [ "$status" -eq 0 ]
    [[ "$output" =~ '"success": true' ]]
}

@test "nas-cli user list --json retorna listado de cuentas" {
    run php bin/nas-cli user list --json
    [ "$status" -eq 0 ]
    [[ "$output" =~ '"success": true' ]]
}

@test "nas-cli group list --json retorna grupos de seguridad" {
    run php bin/nas-cli group list --json
    [ "$status" -eq 0 ]
    [[ "$output" =~ '"success": true' ]]
}

@test "nas-cli domain status --json retorna estado de Directorio Activo" {
    run php bin/nas-cli domain status --json
    [ "$status" -eq 0 ]
    [[ "$output" =~ '"success": true' ]]
}

@test "nas-cli service status --json reporta servicios de red" {
    run php bin/nas-cli service status --json
    [ "$status" -eq 0 ]
    [[ "$output" =~ '"success": true' ]]
    [[ "$output" =~ '"service": "smbd"' ]]
}

@test "nas-cli system metrics --json reporta metricas de rendimiento" {
    run php bin/nas-cli system metrics --json
    [ "$status" -eq 0 ]
    [[ "$output" =~ '"success": true' ]]
    [[ "$output" =~ '"cpu_usage_pct"' ]]
}

@test "nas-cli system updates --json reporta version y commit" {
    run php bin/nas-cli system updates --json
    [ "$status" -eq 0 ]
    [[ "$output" =~ '"success": true' ]]
    [[ "$output" =~ '"version"' ]]
}

@test "nas-cli trash count --json reporta contador de papelera" {
    run php bin/nas-cli trash count --json
    [ "$status" -eq 0 ]
    [[ "$output" =~ '"success": true' ]]
    [[ "$output" =~ '"count"' ]]
}

@test "nas-cli log all --json retorna bitacoras unificadas" {
    run php bin/nas-cli log all --json --limit=5
    [ "$status" -eq 0 ]
    [[ "$output" =~ '"success": true' ]]
}

@test "nas-cli status muestra el diagnostico de salud y retorna 0" {
    run php bin/nas-cli status
    [ "$status" -eq 0 ]
    [[ "$output" =~ "DIAGNÓSTICO EN VIVO" ]]
    [[ "$output" =~ "INFORMACIÓN Y SALUD DEL SISTEMA" ]]
}

@test "nas-cli status --json retorna diagnostico estructurado con success true" {
    run php bin/nas-cli status --json
    [ "$status" -eq 0 ]
    [[ "$output" =~ '"success": true' ]]
    [[ "$output" =~ '"system"' ]]
    [[ "$output" =~ '"storage"' ]]
    [[ "$output" =~ '"services"' ]]
}

@test "nas-cli system info --json es alias valido de metrics" {
    run php bin/nas-cli system info --json
    [ "$status" -eq 0 ]
    [[ "$output" =~ '"success": true' ]]
    [[ "$output" =~ '"cpu_usage_pct"' ]]
}

@test "nas-cli log audit --json y backups son aliases validos" {
    run php bin/nas-cli log audit --json --limit=3
    [ "$status" -eq 0 ]
    [[ "$output" =~ '"success": true' ]]

    run php bin/nas-cli log backups --json --limit=3
    [ "$status" -eq 0 ]
    [[ "$output" =~ '"success": true' ]]
}

@test "nas-cli storage format sin parametros rechaza con codigo 1" {
    run php bin/nas-cli storage format
    [ "$status" -eq 1 ]
    [[ "$output" =~ "Debes especificar el disco" ]]
}

@test "nas-cli share create sin parametros rechaza con codigo 1" {
    run php bin/nas-cli share create
    [ "$status" -eq 1 ]
    [[ "$output" =~ "Faltan parámetros obligatorios" ]]
}

@test "nas-cli backup create sin parametros rechaza con codigo 1" {
    run php bin/nas-cli backup create
    [ "$status" -eq 1 ]
    [[ "$output" =~ "Faltan parámetros obligatorios" ]]
}

@test "nas-cli backup rechaza operaciones en rol ARCHIVOS con error estructurado" {
    NAS_SERVER_ROLE=ARCHIVOS run php bin/nas-cli backup list
    [ "$status" -eq 1 ]
    [[ "$output" =~ "El módulo de respaldos no está disponible en el rol ARCHIVOS." ]]

    NAS_SERVER_ROLE=ARCHIVOS run php bin/nas-cli backup list --json
    [ "$status" -eq 1 ]
    [[ "$output" =~ '"success": false' ]]
    [[ "$output" =~ '"error": "El módulo de respaldos no está disponible en el rol ARCHIVOS."' ]]

    NAS_SERVER_ROLE=ARCHIVOS run php bin/nas-cli backup create --json --id=tarea1 --proto=cifs --target=10.10.1.50 --path=C$
    [ "$status" -eq 1 ]
    [[ "$output" =~ '"success": false' ]]
    [[ "$output" =~ '"error": "El módulo de respaldos no está disponible en el rol ARCHIVOS."' ]]
}

@test "nas-cli backup list --json opera exitosamente en roles con soporte de backup" {
    NAS_SERVER_ROLE=BACKUP run php bin/nas-cli backup list --json
    [ "$status" -eq 0 ]
    [[ "$output" =~ '"success": true' ]]

    NAS_SERVER_ROLE=ARCHIVOS_BACKUP run php bin/nas-cli backup list --json
    [ "$status" -eq 0 ]
    [[ "$output" =~ '"success": true' ]]
}

@test "nas-cli storage trim --json retorna success en modo test" {
    run php bin/nas-cli storage trim --json
    [ "$status" -eq 0 ]
    [[ "$output" =~ '"success": true' ]]
}

@test "nas-cli storage scrub --json retorna success en modo test" {
    run php bin/nas-cli storage scrub --json
    [ "$status" -eq 0 ]
    [[ "$output" =~ '"success": true' ]]
}

@test "nas-cli storage subvol --json retorna listado valido" {
    run php bin/nas-cli storage subvol --json
    [ "$status" -eq 0 ]
    [[ "$output" =~ '"success": true' ]]
}

@test "nas-cli trash list --json retorna listado estructurado" {
    run php bin/nas-cli trash list --json
    [ "$status" -eq 0 ]
    [[ "$output" =~ '"success": true' ]]
}

@test "nas-cli comando no reconocido rechaza y muestra ayuda con codigo 0" {
    run php bin/nas-cli modulo_inexistente_xyz
    [ "$status" -eq 0 ]
    [[ "$output" =~ "Módulos y comandos disponibles" ]]
}

@test "nas-cli user show sin usuario rechaza con codigo 1" {
    run php bin/nas-cli user show
    [ "$status" -eq 1 ]
    [[ "$output" =~ "Uso: nas user show <usuario>" ]]
}

@test "nas-cli user shares sin usuario rechaza con codigo 1" {
    run php bin/nas-cli user shares
    [ "$status" -eq 1 ]
    [[ "$output" =~ "Uso: nas user shares <usuario>" ]]
}

@test "nas-cli user assign sin parametros rechaza con codigo 1" {
    run php bin/nas-cli user assign
    [ "$status" -eq 1 ]
    [[ "$output" =~ "Uso: nas user assign <usuario> <grp1,grp2...>" ]]
}

@test "nas-cli group members sin grupo rechaza con codigo 1" {
    run php bin/nas-cli group members
    [ "$status" -eq 1 ]
    [[ "$output" =~ "Uso: nas group members <grupo>" ]]
}

@test "nas-cli group add-member sin parametros rechaza con codigo 1" {
    run php bin/nas-cli group add-member
    [ "$status" -eq 1 ]
    [[ "$output" =~ "Uso: nas group add-member <grupo> <usuario1,usuario2...>" ]]
}

@test "nas-cli group remove-member sin parametros rechaza con codigo 1" {
    run php bin/nas-cli group remove-member
    [ "$status" -eq 1 ]
    [[ "$output" =~ "Uso: nas group remove-member <grupo> <usuario1,usuario2...>" ]]
}

@test "nas-cli user show --json retorna ficha estructurada del usuario" {
    run php bin/nas-cli user show sistemas --json
    [ "$status" -eq 0 ]
    [[ "$output" =~ '"success": true' ]]
    [[ "$output" =~ '"username": "sistemas"' ]]
    [[ "$output" =~ '"role"' ]]
}

@test "nas-cli user shares --json retorna mapa de accesos Samba" {
    run php bin/nas-cli user shares sistemas --json
    [ "$status" -eq 0 ]
    [[ "$output" =~ '"success": true' ]]
}

@test "nas-cli group members --json lista integrantes de un grupo" {
    run php bin/nas-cli group members grp_superadmin --json
    [ "$status" -eq 0 ]
    [[ "$output" =~ '"success": true' ]]
    [[ "$output" =~ '"username"' ]]
}

@test "nas-cli user assign en modo testing asigna grupos exitosamente" {
    run php bin/nas-cli user assign usuario_test grp_marketing --json
    [ "$status" -eq 0 ]
    [[ "$output" =~ '"success": true' ]]
}

@test "nas-cli group add-member y remove-member en modo testing operan exitosamente" {
    run php bin/nas-cli group add-member grp_marketing usuario_test --json
    [ "$status" -eq 0 ]
    [[ "$output" =~ '"success": true' ]]

    run php bin/nas-cli group remove-member grp_marketing usuario_test --json
    [ "$status" -eq 0 ]
    [[ "$output" =~ '"success": true' ]]
}

@test "nas-cli share create rechaza recursos publicos, visibles o grupos no permitidos en rol BACKUP" {
    NAS_SERVER_ROLE=BACKUP run php bin/nas-cli share create --name=VISIBLE_TEST --scheme=1 --groups=grp_samba --json
    [ "$status" -eq 1 ]
    [[ "$output" =~ "recursos de respaldo ocultos" ]]

    NAS_SERVER_ROLE=BACKUP run php bin/nas-cli share create --name=NO_DOLLAR --hidden --scheme=1 --groups=grp_samba --json
    [ "$status" -eq 1 ]
    [[ "$output" =~ "recursos de respaldo ocultos" ]]

    NAS_SERVER_ROLE=BACKUP run php bin/nas-cli share create --name=PUB_TEST$ --scheme=4 --json
    [ "$status" -eq 1 ]
    [[ "$output" =~ "sin acceso público" ]]

    NAS_SERVER_ROLE=BACKUP run php bin/nas-cli share create --name=BADGRP$ --scheme=1 --groups=grp_ventas --json
    [ "$status" -eq 1 ]
    [[ "$output" =~ "solo pueden ser accesibles por grp_samba o grp_backups" ]]

    NAS_SERVER_ROLE=BACKUP run php bin/nas-cli share create --name=VALID_BKP$ --scheme=1 --groups=grp_backups --json
    [ "$status" -eq 0 ]
    [[ "$output" =~ '"success": true' ]]
}

@test "nas-cli share update y access rechazan esquemas o grupos invalidos en rol BACKUP" {
    NAS_SERVER_ROLE=BACKUP run php bin/nas-cli share update BKP_TEST$ --browseable=yes --json
    [ "$status" -eq 1 ]
    [[ "$output" =~ "recursos de respaldo ocultos" ]]

    NAS_SERVER_ROLE=BACKUP run php bin/nas-cli share update BKP_TEST$ --browseable --json
    [ "$status" -eq 1 ]
    [[ "$output" =~ "recursos de respaldo ocultos" ]]

    NAS_SERVER_ROLE=BACKUP run php bin/nas-cli share update BKP_TEST$ --scheme=4 --json
    [ "$status" -eq 1 ]
    [[ "$output" =~ "sin acceso público" ]]

    NAS_SERVER_ROLE=BACKUP run php bin/nas-cli share update BKP_TEST$ --groups=grp_ventas --json
    [ "$status" -eq 1 ]
    [[ "$output" =~ "solo pueden ser accesibles por grp_samba o grp_backups" ]]

    NAS_SERVER_ROLE=BACKUP run php bin/nas-cli share access BKP_TEST$ group grp_ventas rw --json
    [ "$status" -eq 1 ]
    [[ "$output" =~ "solo pueden ser accesibles por grp_samba o grp_backups" ]]
}

@test "nas-cli share create rechaza flag --browseable sin valor en rol BACKUP" {
    NAS_SERVER_ROLE=BACKUP run php bin/nas-cli share create --name=VALID_BKP$ --browseable --json
    [ "$status" -eq 1 ]
    [[ "$output" =~ "recursos de respaldo ocultos" ]]
}

@test "nas-cli group rename protege grp_backups e impide renombrar hacia grp_backups" {
    run php bin/nas-cli group rename grp_backups grp_hacked --json
    [ "$status" -eq 1 ]
    [[ "$output" =~ "no puede ser renombrado" ]]

    run php bin/nas-cli group rename grp_marketing grp_backups --json
    [ "$status" -eq 1 ]
    [[ "$output" =~ "reservado por el sistema" ]]
}

@test "nas-cli log all incluye cabecera de IP ORIGEN en salida ANSI" {
    run php bin/nas-cli log all --limit=3
    [ "$status" -eq 0 ]
    [[ "$output" =~ "IP ORIGEN" ]]
}

@test "nas-cli log backup incluye cabecera de IP ORIGEN en salida ANSI" {
    run php bin/nas-cli log backup --limit=3
    [ "$status" -eq 0 ]
    [[ "$output" =~ "IP ORIGEN" ]]
}



