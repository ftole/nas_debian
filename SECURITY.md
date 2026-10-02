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
8. Si hay tags firmados, prefiere el último tag con firma GPG válida y fusiona el
   tag exacto (no la punta de la rama).

### Verificación criptográfica (tags GPG)

Para producción, configura la huella del firmante de confianza:

```bash
export NAS_UPDATE_SIGNER="<huella-GPG-de-40-hex>"
```

Para producción, configura **ambas** variables:

```bash
export NAS_REQUIRE_SIGNED_TAGS=true
export NAS_UPDATE_SIGNER="<huella-GPG-de-40-hex>"
```

`NAS_REQUIRE_SIGNED_TAGS=true` sin firmante exige una firma válida, pero **acepta
cualquier clave** que el sistema considere de confianza; por eso la huella del
firmante es un **requisito**, no una opción.

Con `NAS_UPDATE_SIGNER` definido (o `NAS_REQUIRE_SIGNED_TAGS=true`), la
**instalación inicial**, la **reinstalación** y `nas update` exigen una tag firmada
con esquema `vMAJOR.MINOR.PATCH` y fijan el árbol en la tag exacta (se elige la
más reciente por versión que sea alcanzable desde la rama). Sin ninguna de las dos
variables, se usa la rama (con advertencia de "sin verificación"). Publica versiones
con `git tag -s vX.Y.Z -m "..."`.

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

> **Nota:** `accept-new` confía en el primer contacto (TOFU). Para producción,
> registra la huella explícitamente con `ssh-keyscan` verificado antes del primer
> backup, en lugar de aceptar la primera conexión automáticamente.

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

## 3. Credenciales y ejecución de backup

- Ubicación: `/etc/backup-credentials/<tarea>.cred` (propietario root, permisos 0600).
- Los runners verifican que los permisos sigan siendo `600` antes de usarlas.
- Las contraseñas viajan por variables de entorno o archivos protegidos, nunca como
  argumentos de línea de comandos, y se redactan de los mensajes de error y logs.
- **Soporte para dominios Active Directory:** El parser del asistente y de la API web
  desglosa automáticamente formatos `DOMINIO\usuario` y `DOMINIO/usuario`, almacenando
  los campos `username`, `password` y `domain` de forma separada en el archivo protegido.
  Esto previene inyecciones de comandos, evita fallos de autenticación NTLM/Kerberos y
  elimina cualquier exposición de claves en la lista global de procesos (`ps`).
- **Resiliencia en montaje CIFS:** El conector de respaldo Windows opera con montaje en
  solo lectura (`ro`), dialecto `vers=3.1.1`, `noserverino`, `cache=none` y temporizador
  blando `soft,timeo=30`. Si el host Windows se apaga o reinicia durante el proceso, el
  kernel del NAS no se bloquea indefinidamente en operaciones de I/O.
- **Staging atómico y control de interrupciones:** Cada respaldo escribe inicialmente en
  `$BKP_DIR/.inprogress_$DATE_STR`. Los interceptores de señales (`trap cleanup EXIT TERM INT`)
  aseguran que ante interrupción manual, corte eléctrico o falla de red, el punto de
  montaje se desmonte de inmediato y el snapshot parcial se destruya. Solo tras un código
  de salida exitoso (0) se ejecuta la promoción atómica mediante `mv` a `snapshot_$DATE_STR`.

### Deuda de seguridad conocida

- Las credenciales se guardan en **texto plano** en `/etc/backup-credentials/`
  (protegidas por permisos `0600` y propietario `root`). Es una compatibilidad con
  `mount.cifs` y `sshpass`. Cuando sea posible, se recomienda migrar los respaldos
  por SSH a **llaves SSH** para no depender de `sshpass` ni de contraseñas en disco.
- La escritura de credenciales, runners y archivos de cron es **atómica y duradera**
  (temporal + `fsync` + `os.replace`), y la creación/actualización de una tarea es
  **transaccional** (se revierte al estado anterior si alguna etapa falla).

## 4. Privilegios y Aislamiento del Entorno Web (Nginx + PHP-FPM)

El panel web nativo del servidor opera bajo un modelo de privilegios mínimos y aislamiento estricto:

- **Usuario no privilegiado:** Nginx y PHP-FPM ejecutan bajo la cuenta de sistema `www-data`.
- **Modo ondemand:** El pool PHP-FPM (`/etc/php/*/fpm/pool.d/nas-web.conf`) opera con `pm = ondemand`, apagando procesos ociosos y reduciendo el consumo de memoria en reposo a ~0 MB.
- **Escalada acotada mediante Sudoers:** El archivo `/etc/sudoers.d/nas-web` concede acceso administrativo exclusivamente a la lista blanca de comandos necesarios para la operación (`systemctl`, `journalctl`, `smbpasswd`, `pdbedit`, etc.), validado con `visudo -c`.
- **Prevención de inyección de comandos:** Las clases de servicio en PHP 8 (`SystemService`, `UserService`, `StorageService`, etc.) utilizan obligatoriamente `proc_open` con arrays de parámetros para interactuar con utilidades del sistema operativo, eliminando la interpretación de shell y los riesgos de inyección.
- **Entorno 100% Offline:** Todas las hojas de estilo, scripts y 36 iconos SVG residen localmente en el servidor, garantizando funcionamiento autónomo y protección contra vectores de ataque basados en CDNs externas o dependencias remotas.

## 5. Formateo y reutilización de almacenamiento

El motor de despliegue (`deploy.sh`) contempla dos políticas para los discos de datos:

1. **Reutilización segura de datos (`--keep-data`):**
   - Permite asociar una unidad o partición existente al punto de montaje `/srv/nas` sin formatear ni destruir datos preexistentes.
   - Detecta la partición adecuada (buscando etiqueta `NAS_DATA` o partición válida preexistente), comprueba el sistema de archivos (`ext4`, `btrfs`, etc.) y aplica las directivas de montaje optimizadas según el hardware.

2. **Formateo de disco dedicado:**
   - Exige confirmación explícita antes de formatear cualquier unidad:
     - Muestra el dispositivo, modelo, tamaño, particiones y puntos de montaje (`lsblk`).
     - Excluye el disco raíz del sistema operativo mediante comprobación del kernel y udev (`80-udisks2-hide-os.rules`).
     - **Discos en uso crítico (RAID / LVM PV):** Se bloquean de forma incondicional para impedir la destrucción de volúmenes compartidos.
     - **Discos montados:** Requieren obligatoriamente `--ignore-in-use` y confirmación interactiva textual (`SI-FORMATEAR`). La bandera `--force` **no** ignora el estado de uso; únicamente confirma sin preguntar en unidades libres.
