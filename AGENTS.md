# AGENTS.md • Servidor NAS & Central de Respaldos Multiplataforma EAD-COL (Debian 13)

> [!IMPORTANT]
> Este documento contiene la **especificación técnica completa, inventario de scripts, arquitectura de seguridad, soluciones a peculiaridades del sistema y estado del proyecto** para que cualquier agente de IA o ingeniero pueda retomar y continuar este trabajo en cualquier equipo.


---

## 1. Identidad y Propósito del Proyecto

* **Sistema Operativo Base:** Debian 13 (Trixie) GNU/Linux x86_64.
* **Organización / Grupo de Trabajo:** `TEAM-JOFRATO`.
* **Identificadores NetBIOS por Defecto:**
  * Servidor NAS de Archivos: `SRV-NAS`
  * Servidor Central de Backup: `SRV-BKP`
* **Dirección IP de Red Local de Prueba:** `10.10.1.2` (Subred `10.10.1.0/24`).
* **Credenciales Administrativas de Entorno:**
  * Usuario: `usuario asignado` / `root`
  * Contraseña predeterminada: `la que se asigne`
* **Convención de Commits de Git:**
  * **REGLA ESTRICTA:** Los commits deben ser atómicos y granulares. **Debe existir un commit independiente por cada archivo modificado.** Si se alteran 3 archivos, se deben registrar 3 commits por separado.
  * Todos los mensajes de commit deben redactarse en **español** siguiendo el estándar convencional:
    * `feat: <descripción en español>` (nuevas características)
    * `fix: <descripción en español>` (corrección de errores)
    * `docs: <descripción en español>` (documentación)
    * `refactor: <descripción en español>` (mejoras de código)
    * `ci: <descripción en español>` (cambios en integración continua)
  * **REGLA DE INTEGRACIÓN CONTINUA (CI):** Cada *push* debe validar el CI de GitHub Actions. En caso de fallo, se debe corregir el código y volver a validar. **Bajo ninguna circunstancia se pueden utilizar directivas para ignorar advertencias en el CI** (`# shellcheck disable` o banderas `-e`). El código debe ajustarse a la regla, no la regla al código.

---

## 2. Inventario de Archivos del Proyecto

Todos los archivos del proyecto son portables y se adaptan dinámicamente al directorio donde se alojen y al hardware del servidor:

| Archivo / Ruta | Tipo | Descripción |
| :--- | :--- | :--- |
| `install.sh` | Script Bash CLI | **Instalador Remoto Oficial y Gestor CLI** para desplegar el comando `nas`, con auto-actualización (`update`) y desinstalación limpia. |
| `src/asistente.sh` | Script Bash (TUI `whiptail`) | **Asistente Visual Interactivo** con colores nativos, detección dinámica de discos/IP/usuario, validación en vivo, ciclo de edición y 9 módulos de gestión. |
| `src/core/deploy.sh` | Script Bash CLI | **Motor de Despliegue Automatizado** con detección inteligente de entorno, protección de partición raíz, soporte de roles (`ARCHIVOS` o `BACKUP`), formateo, Samba, Cockpit y parches. |
| `src/core/uninstall.sh` | Script Bash CLI | **Desinstalador y Limpiador Total** para restablecer el servidor a su estado base limpio. |
| `src/core/updater.sh` | Script Bash CLI | **Motor de actualización remota desde GitHub** para entornos simplificados. |
| `src/lib/{colors,helpers}.sh` | Bash Lib | Paleta ANSI y funciones de detección de entorno (IP, NetBIOS, Workgroup, usuario, disco base). |
| `src/modules/*.sh` | Bash (TUI `whiptail`) | Módulos del asistente: `deploy_wizard`, `groups`, `shares`, `backups`, `users`, `diagnostics`. |
| `src/web/backups/` | Web (Cockpit) | Plugin de respaldos: `index.html`, `main.js`, `style.css`, `backup_api.py` y FontAwesome. |
| `tests/helpers.bats` | BATS | Pruebas unitarias de las funciones auxiliares de entorno. |
| `tests/failure_*.bats` | BATS | Pruebas de inyección de fallos (discos en uso y runners de backup). |
| `tests/test_api.py` | pytest | Pruebas de validación y fallos del backend web. |
| `FAILURE_MODES.md` | Markdown | Modos de fallo y su verificación (FMEA). |
| `.github/workflows/ci.yml` | CI | Pipeline de GitHub Actions: ShellCheck, BATS y Flake8. |
| `.gitattributes` | Config | Normalización de fin de línea (LF) y tratamiento de binarios. |
| `README.md` | Markdown | **Guía de Puesta a Punto Paso a Paso** para preparación y hardening de Debian 13. |
| `SMB_DEBIAN.md` | Markdown | **Manual Técnico y Guía de Replicación** para usuarios y administradores. |
| `SECURITY.md` | Markdown | **Modelo de seguridad**, rollback de actualizaciones, identidades SSH, credenciales y parches de Cockpit. |
| `AGENTS.md` | Markdown | **Este documento maestro de contexto para agentes de IA**. |

---

## 3. Roles del Servidor y Matriz de Seguridad

El sistema está diseñado para operar bajo dos roles mutuamente excluyentes:

```text
                               ┌─────────────────────────┐
                               │    Servidor Debian 13   │
                               └────────────┬────────────┘
                     ┌──────────────────────┴──────────────────────┐
                     ▼                                             ▼
        ┌─────────────────────────┐                   ┌─────────────────────────┐
        │      Rol: ARCHIVOS      │                   │       Rol: BACKUP       │
        │    (NAS Departamental)  │                   │  (Central de Respaldos) │
        └────────────┬────────────┘                   └────────────┬────────────┘
                     │                                             │
      ┌──────────────┴──────────────┐               ┌──────────────┴──────────────┐
      ▼                             ▼               ▼                             ▼
Carpetas Visibles:            Grupos:         Carpetas Ocultas ($):         Grupos:
[SISTEMAS]                    grp_sistemas    [BACKUPS_WINDOWS$]            SOLO grp_sistemas
[CAMPANA_UNO_*]               grp_empleados   [BACKUPS_LINUX$]              SOLO grp_backups
[CAMPANA_DOS_*]               grp_c1_*, c2_*  [BACKUPS_SERVIDORES$]         (Cero empleados)
```

### A. Despliegue Base Limpio (Servidor NAS o Central de Backup):
* **0 Redes Compartidas Automáticas:** El archivo `smb.conf` se inicializa únicamente con la sección `[global]` optimizada, sin recursos de prueba ni carpetas innecesarias.
* **Grupo Maestro:** Únicamente se crea `grp_sistemas` (con permisos totales `2775` sobre `/srv/nas`). El administrador del servidor queda asignado a `sudo,adm,grp_sistemas`.
* **Gestión 100% Modular desde el Asistente:**
  * **Creación de Grupos (Menú [2]):** Grupos departamentales o técnicos según las necesidades del entorno.
  * **Creación de Recursos (Menú [3]):** Configuración guiada con elección de visibilidad (Oculto `$` por defecto o Visible) y 4 esquemas de permisos granulares:
    1. *Lectura y Escritura por Grupo:* Todos los grupos autorizados (`read only = no`, `mask 0770`).
    2. *Solo Lectura General + Escritura Exclusiva:* Acceso general de lectura con escritura restringida (`read only = yes`, `write list = @grupo_escritura`, `mask 0775`).
    3. *Solo Lectura Estricta:* Consulta histórica sin modificación (`read only = yes`, `mask 0755`).
    4. *Acceso Público / Invitados:* Libre acceso con o sin clave (`guest ok = yes`).

---

## 4. Motor de Copias de Seguridad Multiplataforma

### A. Respaldo de Servidores Windows:
* **Protocolo:** CIFS / SMB.
* **Seguridad:** Credenciales en `/etc/backup-credentials/<tarea>.cred` con permisos `0600 root:root`.
* **Mecanismo:** Montaje temporal en `/mnt/backup_sources/<tarea>` con flag `ro` (Solo Lectura) ➜ Ejecución de Snapshot ➜ Desmontaje inmediato.

### B. Respaldo de Servidores Linux Remotos:
* **Protocolo:** SSH túnel con `rsync` y `sshpass` (o llaves SSH).
* **Mecanismo:** Preserva propietarios, fechas y permisos POSIX exactos.

### C. Deduplicación por Enlaces Duros (*Hardlinks*):
* **Estructura en disco:** `/srv/nas/BACKUPS_HISTORICOS/<tarea>/snapshot_YYYY-MM-DD_HHMMSS/`.
* **Cómo funciona:** `rsync --link-dest=<ultimo_snapshot>`. Los archivos que no cambiaron apuntan al mismo inodo físico en disco (0% espacio extra duplicado). Ahorro superior al 85% de disco.
* **Política de Retención:** Al crearse un snapshot, el script cuenta los existentes; si superan $N$, elimina el más antiguo. Los archivos vigentes **nunca se borran** gracias al contador de referencias de inodos de Linux.

### D. Test de Conexión en Vivo y Ciclo de Edición:
* Antes de crear una tarea, el asistente prueba la IP, recurso compartido y contraseña en 1 segundo.
* Si falla (ej. error tipográfico o nombre NetBIOS sin DNS), muestra el error detallado y permite **corregir los datos sin perder lo escrito**.

---

## 5. Parches Críticos y Soluciones de Debian 13 Integradas

Si se reinstala el servidor desde cero o en otra máquina, estos parches están incluidos en `src/core/deploy.sh`:

1. **Visibilidad en Red Windows (WSDD2):**
   * *Problema:* `wsdd2` en Debian 13 usa `DynamicUser=true` y falla al ejecutar `testparm` para leer `smb.conf`.
   * *Solución:* Override en `/etc/default/wsdd2` y `/etc/systemd/system/wsdd2.service.d/override.conf` con `WSDD2_OPTS="-N <NETBIOS> -G <WORKGROUP> -H <NETBIOS>"`.
2. **Compatibilidad de Cockpit en Servidores en Español:**
   * *Problema:* Cockpit espera salida en inglés de herramientas del sistema (`chage`, `passwd -S`, `lastb`).
   * *Solución:* Wrappers en `/usr/local/sbin/chage`, `/usr/local/sbin/passwd` y `/usr/local/bin/lastb` que fuerzan `LC_ALL=C LANG=C`.

---

3. **Interfaz Web (Cockpit Backups):**
   * *Arquitectura:* Aplicación web ES5 nativa sin frameworks pesados, inyectada en `/usr/share/cockpit/backups`.
   * *Estilos:* Utiliza PatternFly 4 (`cockpit.css.gz`) importado desde Navigator para mantener coherencia de diseño oscuro/claro nativo de 45Drives.
   * *Backend:* `backup_api.py` actúa como puente JSON. `cockpit.spawn` invoca la API localmente usando escalada de privilegios segura.

> [!NOTE]
> Las directivas globales `set -e` fueron removidas de los módulos importables (`source`) como `updater.sh` para prevenir crashes abruptos del TUI al cancelar diálogos de `whiptail`.

## 6. Comandos de Operación Rápida

### Lanzar el Asistente Interactivo:
```bash
sudo nas
```

### Despliegue Manual por Consola:
```bash
# Servidor NAS (usando partición local o disco dedicado):
sudo bash src/core/deploy.sh LOCAL EAD-COL SRV-EAD-NAS admin <CLAVE_ADMIN> ARCHIVOS

# Servidor de Backup con disco secundario (/dev/sdb o /dev/sda según escaneo):
sudo bash src/core/deploy.sh /dev/sda EAD-COL SRV-EAD-BKP admin <CLAVE_ADMIN> BACKUP
```

> [!IMPORTANT]
> Por seguridad, `deploy.sh` **aborta** si el disco dedicado está en uso (montado, PV de LVM o miembro de RAID); añade `--force` al final para forzarlo. La clave puede enviarse por `stdin` usando `-` en su lugar (evita exponerla en `ps`).

### Limpieza y Desinstalación Total:
```bash
sudo nas uninstall
```

### Verificar Tareas y Logs de Backup:
```bash
ls -la /etc/cron.d/backup_*
tail -f /srv/nas/LOGS_BACKUP/backup_*.log
```
