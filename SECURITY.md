# Seguridad del Proyecto NAS Debian (EAD-COL)

Este documento describe el endurecimiento aplicado y los procedimientos operativos
para gestionar actualizaciones, identidades SSH, credenciales y parches.

## 1. Actualización segura y rollback

El actualizador (`sudo nas update` o la opción [8] del asistente) ya **no** aplica
`git reset --hard` de forma desatendida. El flujo es:

1. Verifica que la rama activa sea `main` y que el remoto `origin` corresponda al
   repositorio esperado (`ftole/nas_debian`).
2. Aborta si existen cambios locales sin confirmar.
3. Muestra la versión instalada, la versión candidata y los cambios principales.
4. Pide confirmación explícita (o exige `--yes` en modo no interactivo).
5. Crea una copia de seguridad del estado actual en un directorio hermano
   (`/opt/nas_debian.update_backup_XXXXXX`).
6. Aplica la actualización con `git merge --ff-only` (avance rápido).
7. Valida la integridad del repositorio (`git fsck`) y la sintaxis de los scripts
   (`bash -n`). Si falla, restaura automáticamente la versión anterior.

### Rollback manual

- `sudo nas version` muestra el commit instalado.
- Para volver a un estado previo:
  `cd /opt/nas_debian && git reset --hard <commit>`.
- El respaldo automático queda en `/opt/nas_debian.update_backup_*`.

## 2. Verificación de identidad SSH (known_hosts dedicado)

Las tareas de backup por SSH **ya no** usan `StrictHostKeyChecking=no`. En su lugar
utilizan `StrictHostKeyChecking=accept-new` y un archivo de huellas dedicado y
protegido:

- Ruta: `/root/.ssh/known_hosts_backup` (propietario root, permisos 0600).
- `accept-new` registra la huella en la primera conexión y **rechaza** las conexiones
  posteriores si la huella cambia (protección frente a ataques de intermediario).

### Registrar o cambiar la huella de un servidor remoto

```bash
# Registrar la huella por primera vez
ssh-keyscan -H <IP_o_nombre> >> /root/.ssh/known_hosts_backup

# Cambiar una huella que cambió legítimamente (reinstalación del servidor)
ssh-keygen -R <IP_o_nombre> -f /root/.ssh/known_hosts_backup
ssh-keyscan -H <IP_o_nombre> >> /root/.ssh/known_hosts_backup
```

Si la huella no está registrada o cambió, la tarea falla con un mensaje claro y
**no** se conecta silenciosamente.

## 3. Credenciales de backup

- Ubicación: `/etc/backup-credentials/<tarea>.cred` (propietario root, permisos 0600).
- Los runners verifican que los permisos sigan siendo `600` antes de usarlas.
- Las contraseñas viajan por variables de entorno o archivos protegidos, nunca como
  argumentos de línea de comandos, y se redactan de los mensajes de error y logs.

## 4. Parches de Cockpit y actualizaciones de paquetes

Los parches de Cockpit (Identities y Storage) se aplican con:

- Copia de seguridad previa: `<archivo>.bak-<YYYYMMDD_HHMMSS>`.
- Verificación del patrón esperado: si no existe, el parche se omite y se advierte.
- Escritura atómica (archivo temporal + `mv`), evitando archivos parcialmente corruptos.

> **Importante:** una actualización de paquetes (`apt upgrade` de `cockpit-identities`
> o `cockpit-storaged`) puede reemplazar los archivos parcheados. Tras actualizar
> paquetes, revisa si los parches siguen aplicados.

### Restaurar un parche

`deploy.sh` ofrece un modo de restauración que recupera los archivos desde la copia
de seguridad más reciente sin ejecutar el despliegue:

```bash
sudo bash /opt/nas_debian/src/core/deploy.sh --restore-patches
```

## 5. Formateo de discos

`deploy.sh` exige confirmación explícita antes de formatear un disco dedicado:

- Muestra el dispositivo, modelo, tamaño, particiones y puntos de montaje (`lsblk`).
- Verifica que no sea el disco raíz ni un disco en uso (montado, PV de LVM o RAID).
- Requiere `--confirm` (asistente), `--force` (bajo responsabilidad) o confirmación
  interactiva. En modo no interactivo sin confirmación, aborta.
