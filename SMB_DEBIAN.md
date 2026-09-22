# Manual Técnico: Servidor NAS y Central de Respaldos (Debian 13)

Este documento describe la arquitectura, el motor de copias de seguridad, el modelo de seguridad y los procedimientos de operación del proyecto. Está dirigido a administradores que necesiten entender el funcionamiento interno o replicar el despliegue. Para la instalación y el uso general, consulta el `README.md`.

## 1. Introducción y alcance

El sistema cubre dos funciones excluyentes:

1. **Servidor de archivos (NAS departamental):** almacenamiento en red para clientes Windows mediante Samba, con descubrimiento WSDD2 y panel Cockpit.
2. **Central de copias de seguridad:** repositorio dedicado a respaldar servidores Windows, servidores Linux, estaciones de trabajo y carpetas locales, con snapshots deduplicados y retención configurable.

El despliegue base es idéntico para ambos roles: crea el grupo `grp_sistemas` y el directorio `/srv/nas`, sin recursos compartidos. Los grupos y recursos se añaden después desde el asistente según las necesidades del entorno.

## 2. Arquitectura

```text
                         +---------------------------+
                         |     Debian 13 (Trixie)    |
                         +-------------+-------------+
                                       |
          +----------------------------+----------------------------+
          |                            |                            |
  +-------+-------+          +---------+---------+        +---------+---------+
  |  Samba/WSDD2  |          |  Cockpit + web    |        |  Motor de backups |
  |  (SMB / WSD)  |          |  (panel Backups)  |        |  (CIFS/SSH/local) |
  +---------------+          +-------------------+        +-------------------+
```

Componentes:

- **Instalador y CLI `nas`** (`install.sh`): despliega el proyecto en `/opt/nas_debian` y crea el comando global.
- **Asistente de terminal** (`src/asistente.sh` y `src/modules/`): menú de 9 módulos.
- **Motor de despliegue** (`src/core/deploy.sh`): prepara almacenamiento, Samba, Cockpit y parches.
- **Motor de backups** (`src/modules/backups.sh` y `src/web/backups/`): crea y ejecuta las tareas.
- **Panel web** (`src/web/backups/`): interfaz Cockpit para gestionar las tareas.

## 3. Motor de copias de seguridad

### 3.1 Snapshots y deduplicación

- Cada ejecución crea una carpeta `snapshot_YYYY-MM-DD_HHMMSS` dentro de `/srv/nas/BACKUPS_HISTORICOS/<tarea>/`.
- `rsync --link-dest` reutiliza los inodos (enlaces duros) de los archivos sin cambios, por lo que cada snapshot ocupa poco espacio adicional.
- La retención conserva los últimos N snapshots y elimina los más antiguos. El orden se determina por el nombre (cronológico), no por la fecha de modificación.

### 3.2 Orígenes soportados

- **Windows (CIFS):** montaje en solo lectura con credenciales en `/etc/backup-credentials/<tarea>.cred` (`0600`). El montaje usa `vers=3.0,sec=ntlmssp`.
- **Linux (SSH):** `rsync` sobre un túnel `sshpass`, preservando permisos POSIX, propietarios, grupos y fechas.
- **Local:** `rsync` sobre una ruta del propio servidor.

### 3.3 Programación y concurrencia

- `cron` lanza cada tarea mediante `systemd-run --collect` con prioridad baja.
- Un bloqueo `flock` por tarea evita ejecuciones simultáneas.
- Antes de copiar, se comprueba el espacio libre disponible.

## 4. Modelo de seguridad

- **Validación de entradas:** IP, recurso, ruta, usuario, puerto y expresión cron se validan tanto en el asistente como en la API web.
- **Manejo de secretos:** las contraseñas no se pasan como argumentos; se envían por `stdin` o variables de entorno, y las credenciales se guardan en archivos con permisos `0600`.
- **Samba:** `map to guest = Bad User`, cifrado negociado (`desired`) y protocolo mínimo SMB2.
- **Recursos de respaldo ocultos:** sufijo `$` y, opcionalmente, `browseable = no`.
- **Permisos:** ACL de POSIX (`setfacl`) para el esquema de escritura por varios grupos.
- **Integridad del sistema:** protección del disco del sistema operativo (LVM, RAID, LUKS y Btrfs), aviso ante discos en uso, respaldo de `smb.conf` antes de regenerarlo y `dpkg-divert` para binarios del sistema.
- **Cadena de suministro:** verificación SHA256 de las extensiones descargadas y CI con acciones fijadas por SHA.

## 5. Métodos de despliegue

- **Asistente:** `sudo nas` (recomendado).
- **Panel web:** `https://<IP_DEL_SERVIDOR>:9090`, en el módulo Backups.
- **Línea de comandos:** `sudo bash src/core/deploy.sh [DISCO/LOCAL] [WORKGROUP] [NETBIOS] [ADMIN_USER] [ADMIN_PASS] [ROL]`.
  - La clave puede enviarse por `stdin` usando `-` en el campo de contraseña.
  - El despliegue aborta si el disco dedicado está en uso; añade `--force` para forzarlo.
- **Desinstalación:** `sudo nas uninstall`.

## 6. Gestión

- **Grupos (`grp_*`):** se crean desde el módulo [2]; `grp_sistemas` es el grupo maestro.
- **Recursos compartidos:** módulo [3], con cuatro esquemas de permisos:
  1. Lectura y escritura por grupo.
  2. Solo lectura general con escritura exclusiva.
  3. Solo lectura estricta.
  4. Acceso público o de invitados.
- **Usuarios:** módulo [5]. Quienes pertenecen a `grp_sistemas` obtienen shell y acceso web; el resto solo tiene acceso de red.

## 7. Restauración de archivos

1. Ingresar a la carpeta de snapshots:

   ```bash
   cd /srv/nas/BACKUPS_HISTORICOS/<nombre_tarea>/
   ls -la
   ```

2. Seleccionar el snapshot deseado (por ejemplo `snapshot_2026-08-28_230000`).

3. Copiar el archivo o la carpeta al destino:

   ```bash
   cp snapshot_2026-08-28_230000/Contabilidad/Reporte.xlsx /srv/nas/VENTAS/
   ```

## 8. Operación y mantenimiento

- **Registros:** `/srv/nas/LOGS_BACKUP/backup_<tarea>.log`, con rotación semanal mediante `logrotate`.
- **Diagnóstico:** `sudo nas status`.
- **Actualización:** `sudo nas update`.
- **Verificación de servicios:** `systemctl status smbd nmbd wsdd2 cockpit.socket cron`.

## 9. Solución de problemas y glosario

**Problemas frecuentes**

- **El montaje CIFS falla:** el servidor puede exigir otra versión de SMB; edita el runner generado en `/usr/local/bin/backup_<tarea>.sh`.
- **El backup se omite:** hay otra ejecución en curso (bloqueo `flock`) o el espacio libre es inferior a 500 MB.
- **La tarea no se ejecuta:** verifica el servicio `cron` y el registro de la tarea.
- **Un disco aparece como "EN USO":** está montado, es un volumen LVM o un miembro de RAID.

**Glosario**

- **Snapshot:** copia de un origen en un instante concreto.
- **Enlace duro (hardlink):** referencia adicional a un mismo bloque de datos; no duplica el espacio.
- **`--link-dest`:** opción de `rsync` que reutiliza los archivos ya presentes en un snapshot anterior.
- **Retención:** número de snapshots que se conservan antes de eliminar los más antiguos.
- **WSDD2:** servicio de descubrimiento de equipos en redes Windows (WSD/LLMNR).
