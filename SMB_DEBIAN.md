# Guía de Implementación y Replicación: Servidor NAS & Central de Backup Multiplataforma (Debian Linux)

Esta guía documenta el procedimiento completo, probado y replicable para desplegar servidores empresariales bajo Debian 13 para dos funciones principales:
1. **Servidor de Archivos (NAS Principal):** Almacenamiento en red departamental para clientes Windows con Samba, WSDD2 y Cockpit.
2. **Servidor de Copias de Seguridad (Backup Centralizado):** Repositorio dedicado diseñado para resistir ransomware (montajes de solo lectura, recursos ocultos y snapshots versionados) para respaldar servidores Windows, servidores Linux, estaciones de trabajo y carpetas locales mediante snapshots incrementales y deduplicación.

---

## 1. Arquitectura y Métodos de Respaldo

### A. Método de Copia de Seguridad y Deduplicación:
* **Incremental Versionada con Snapshots y Hardlinks:**
  * Cada ejecución genera una carpeta con fecha y hora (`snapshot_YYYY-MM-DD_HHMMSS`).
  * Los archivos que no han sido modificados **comparten el mismo bloque físico en el disco** (*hardlinks*).
  * **Ahorro de espacio:** Significativo (típicamente superior al **85%**) frente a copias completas repetitivas, gracias a los hardlinks.
  * **Retención histórica:** El snapshot más reciente siempre refleja el origen; las versiones anteriores se conservan hasta que la rotación elimina los snapshots más antiguos que excedan la retención configurada (N snapshots).
* **Exactitud punto en el tiempo:**
  * Cada snapshot es una réplica exacta del origen en el instante de la ejecución (`rsync -a --delete`); las versiones previas se conservan como snapshots anteriores.

> [!NOTE]
> Antes de crear una tarea, el asistente **prueba la conexión** (CIFS/SSH) con las credenciales ingresadas. Las ejecuciones programadas se lanzan con `systemd-run` y un bloqueo `flock` que evita solapamientos.

---

### B. Respaldo de Servidores Windows (Active Directory, SQL, File Server):
* **Alcance:** copia a nivel de archivos del recurso compartido SMB. Para aplicaciones como AD o SQL usa herramientas nativas/conscientes de VSS.
* **Protocolo:** SMB / CIFS con montaje en modo **Solo Lectura (`ro`)**.
* **Seguridad de Credenciales:** El usuario y contraseña de Windows se almacenan en `/etc/backup-credentials/<tarea>.cred` con permisos estrictos `0600 root:root` (inaccesible para usuarios normales).
* **Flujo de Ejecución:**
  1. El servidor de backup monta temporalmente la carpeta de Windows en `/mnt/backup_sources/<tarea>`.
  2. Ejecuta el snapshot incremental con deduplicación.
  3. Desmonta el recurso inmediatamente (`umount`).
  4. Genera el registro detallado en `/srv/nas/LOGS_BACKUP/`.

---

### C. Respaldo de Servidores Linux / NAS Principal:
* **Protocolo:** Túnel SSH cifrado con `rsync` y `sshpass` (autenticación por contraseña).
* **Verificación de host:** el runner desactiva la validación de la clave del host remoto (`StrictHostKeyChecking=no`); úsalo solo en redes de confianza (riesgo de *man-in-the-middle*).
* **Flujo de Ejecución:**
  1. Conexión segura por SSH con un usuario autorizado (configurable; por defecto `root`).
  2. Preservación exacta de permisos POSIX, propietarios, grupos y fechas de modificación.

---

### D. Respaldo de Carpetas Locales del Servidor:
* **Protocolo:** `rsync` local (sin credenciales de red).
* **Origen:** cualquier ruta absoluta del propio servidor (p. ej. `/srv/nas/SISTEMAS`).
* **Uso típico:** proteger directorios locales o consolidar copias ya presentes en el NAS.

---

## 2. Métodos de Gestión y Despliegue

### Método 1: Asistente Gráfico Interactivo en Terminal (Recomendado)
```bash
sudo nas
```
* **Detección Automática del Entorno:**
  * Escanea dinámicamente los discos del servidor, identifica el disco del sistema operativo (`/`, incluidos LVM/RAID/LUKS) para protegerlo contra formateo accidental, y ofrece discos secundarios o la partición local.
  * **Aviso:** el disco dedicado seleccionado se **formatea por completo** (BTRFS) y se borran sus datos; el asistente solicita confirmación explícita antes de hacerlo.
  * Detecta la dirección IP real del servidor en la red local para paneles web y accesos SMB.
  * Detecta el usuario administrador actual para asignarle permisos en Cockpit y Samba.
* **Otros comandos del CLI:** `sudo nas update` (actualiza desde GitHub), `sudo nas status` (diagnóstico) y `sudo nas version` (versión/commit instalado).

---

### Método 2: Panel Web Cockpit (Backups)
Accede a `https://<IP_DEL_SERVIDOR>:9090` → módulo **Backups** para crear, probar la conexión, listar, ejecutar y eliminar tareas y consultar sus registros desde el navegador (usa la misma API que el asistente).

---

### Método 3: Despliegue Automatizado por Línea de Comandos
```bash
# Sintaxis (los parámetros son opcionales con auto-detección):
sudo bash src/core/deploy.sh [DISCO/LOCAL] [WORKGROUP] [NETBIOS] [ADMIN_USER] [ADMIN_PASS] [ROL]

# Ejemplo para Servidor NAS de Archivos:
sudo bash src/core/deploy.sh LOCAL EAD-COL SRV-EAD-NAS admin <CLAVE_ADMIN> ARCHIVOS

# Ejemplo para Servidor de Backup con disco secundario:
sudo bash src/core/deploy.sh /dev/sda EAD-COL SRV-EAD-BKP admin <CLAVE_ADMIN> BACKUP
```

> [!TIP]
> Pasar la clave como argumento la expone temporalmente en `ps`. Para evitarlo, usa `-` en el campo de contraseña y envíala por `stdin`:
> ```bash
> printf '%s\n' '<CLAVE_ADMIN>' | sudo bash src/core/deploy.sh LOCAL EAD-COL SRV-EAD-NAS admin - ARCHIVOS
> ```

### Método 4: Desinstalación y Limpieza Rápida
```bash
sudo nas uninstall
```

---

## 3. ¿Cómo Restaurar Archivos desde un Backup?

Para restaurar archivos o carpetas de cualquier fecha conservada por la retención:

1. **Ingresar a la carpeta de snapshots:**
   ```bash
   cd /srv/nas/BACKUPS_HISTORICOS/<nombre_tarea>/
   ls -la
   ```
2. **Seleccionar el snapshot deseado:**
   * Cada carpeta corresponde a un punto exacto en el tiempo (ej. `snapshot_2026-08-28_230000`).
3. **Copiar el archivo hacia el servidor de destino:**
   ```bash
   # Ejemplo restaurando un archivo hacia Windows o NAS:
   cp snapshot_2026-08-28_230000/Contabilidad/Reporte.xlsx /srv/nas/VENTAS/
   ```

---

## 4. Diferencias de Arquitectura: Servidor NAS vs Servidor de Backup

> [!IMPORTANT]
> El despliegue base es **idéntico y limpio** para ambos roles: crea únicamente `grp_sistemas` y `/srv/nas`, con **0 recursos compartidos**. Los grupos y carpetas de ejemplo siguientes se crean después desde el asistente (menús [2] y [3]) según las necesidades del entorno.

> [!NOTE]
> El asistente ofrece **4 esquemas de permisos** por recurso (Lectura/Escritura por grupo, Solo Lectura + Escritura exclusiva, Solo Lectura estricta y Público/Invitados) y una **retención por defecto** de 30 snapshots (15 para tareas Linux por SSH).

### Rol ARCHIVOS (NAS Departamental):
* **Grupos de ejemplo:** `grp_sistemas`, `grp_c1_admin`, `grp_c1_analista`, `grp_c2_admin`, etc. (creados por el administrador).
* **Carpetas Visibles de ejemplo:** `[SISTEMAS]`, `[C1_*]`, `[C2_*]` accesibles según matriz de permisos.

### Rol BACKUP (100% Oculto y Resistente a Ransomware):
* **Grupos de ejemplo:** `grp_sistemas` (TI) y, opcionalmente, `grp_backups` (servicio técnico). Ningún usuario común debería existir en este servidor.
* **Recursos Ocultos:** Se crean con el sufijo `$` (y opcionalmente `browseable = no`) para quedar **invisibles en el explorador de Windows**:
  * `[BACKUPS_WINDOWS$]`: Destino oculto para agentes Windows (Veeam / Windows Backup).
  * `[BACKUPS_LINUX$]`: Destino oculto para servidores Linux.
  * `[BACKUPS_SERVIDORES$]`: Repositorio de imágenes y snapshots.
* **Acceso Estricto:** Solo accesible por credenciales autorizadas escribiendo la ruta UNC directa (ej. `\\<IP_SERVIDOR>\BACKUPS_WINDOWS$`).

---

## 5. Acceso Web y Conexión de Red

* **Panel Web Cockpit:** `https://<IP_DEL_SERVIDOR>:9090`
* **Red Windows:** `\\<IP_DEL_SERVIDOR>` (o `\\<NOMBRE_NETBIOS>`)
