# Asistente de Administración Remota NAS (Windows)

Herramienta TUI para administrar de forma remota desde Windows el **Servidor NAS &
Central de Respaldos EAD-COL (Debian 13)**, reutilizando el motor existente de
`test_remote.py` (SSH/paramiko) y los scripts del servidor (`install.sh`,
`deploy.sh`, `asistente.sh`, panel web). No modifica nada del funcionamiento actual:
agrega un **front-end de producción** adicional.

---

## 1. Qué aporta

| Acción | Descripción |
| :--- | :--- |
| Desplegar | Asistente guiado (rol ARCHIVOS/BACKUP, disco, Workgroup, NetBIOS, admin, `--keep-data`) que clona el proyecto desde GitHub y ejecuta `deploy.sh`. |
| Estado / diagnóstico | `nas status` y comprobaciones de servicios, SQLite y almacenamiento. |
| Actualizar | Actualizador existente (`updater.sh --yes`). |
| Servicios | Reiniciar / recargar / revisar smbd, nmbd, wsdd2, nginx, php-fpm y cron. |
| Logs | Bitácoras de systemd, administrativa y de respaldos. |
| Consola SSH | Shell interactiva directa al servidor. |
| Desinstalar | Limpieza total con doble confirmación (`DESINSTALAR`). |
| Credenciales | Conexión guiada con verificación en vivo. |

---

## 2. Requisitos

- **Windows 10 o 11** con conexión de red al servidor Debian 13.
- **Modo empaquetado:** `NAS-Assistant.exe` (no requiere Python).
- **Modo script:** Python 3.10+ y `pip install -r requirements-assistant.txt`.

Opción recomendada: **Windows OpenSSH Client** para la consola interactiva
(`ssh`). Si no está, el asistente usa internamente `paramiko` como respaldo.

---

## 3. Configuración inicial

Al primer inicio pide IP, puerto, usuario SSH y contraseñas, probando la conexión
y los permisos `sudo` en vivo.

- Las contraseñas se guardan en el **Credential Manager de Windows** (`keyring`).
- En `.env` solo se persisten host, puerto y usuario (no secretos).

El archivo de configuración por defecto es `.env` junto al ejecutable. Puedes
usar otro con `--env-file`.

---

## 4. Uso

### Menú interactivo
```text
python nas_admin.py            (o NAS-Assistant.exe)
```
```text
[1] Desplegar servidor (NAS / Backup)
[2] Estado y diagnóstico
[3] Actualizar software
[4] Reiniciar / reload de servicios
[5] Ver logs / bitácoras
[6] Consola SSH interactiva
[7] Desinstalar servidor
[8] Credenciales y conexión
[0] Salir
```

### Acciones directas (CLI)
```text
python nas_admin.py config        # Credenciales y conexión
python nas_admin.py deploy        # Asistente de despliegue
python nas_admin.py status        # Estado y diagnóstico
python nas_admin.py update        # Actualizar software
python nas_admin.py services      # Gestión de servicios
python nas_admin.py logs          # Ver bitácoras
python nas_admin.py console       # Consola SSH
python nas_admin.py uninstall     # Desinstalar (con confirmación)
```

---

## 5. Despliegue

1. Selecciona **Desplegar**.
2. Elige el almacenamiento:
   - `LOCAL`: usa la partición raíz (idóneo para pruebas o para reutilizar datos).
   - Algún disco físico detectado con `lsblk` (el disco del SO se excluye; se indica
     SSD/HDD según `rota`).
3. Define **Rol** (`ARCHIVOS` o `BACKUP`), **Workgroup**, **NetBIOS**, **usuario
   administrador** y si usas `--keep-data` (reutilizar datos sin formatear).
4. Confirma el resumen. El asistente clona o actualiza `ftole/nas_debian` en
   `/opt/nas_debian` y ejecuta `deploy.sh` con tus parámetros.
5. Al terminar verifica servicios, SQLite y `/srv/nas`.

> El panel web queda accesible en `https://<IP>` tras el despliegue.

---

## 6. Credenciales y seguridad

- **Claves de host**: política estricta `accept-new` persistente (`.nas_known_hosts`).
  Si la huella del servidor cambia, la conexión se rechaza (protección MITM).
- **Contraseñas**: en el Credential Manager de Windows; no se escriben en `.env`.
- **Sudo y despliegue**: la clave viaja por `stdin`, nunca por argumentos de `ps`.
- **Acciones destructivas** (desinstalar): confirmación reforzada con la palabra
  `DESINSTALAR`.

---

## 7. Empaquetado (build del `.exe`)

En CI de Windows (workflow `.github/workflows/build-windows.yml`), al publicar un
tag `v*`, se construye `NAS-Assistant.exe` y se adjunta al Release.

Construcción local (opcional):
```text
pip install -r requirements-assistant.txt pyinstaller
pyinstaller --clean --noconfirm nas-assistant.spec
```

---

## 8. Solución de problemas

| Problema | Causa probable | Solución |
| :--- | :--- | :--- |
| "Configuración incompleta" | `.env` sin host/usuario o clave | `python nas_admin.py config` |
| No conecta por SSH | Firewall, clave de host cambiada o puerto | Verifica `systemctl status ssh` y revisa `.nas_known_hosts`. |
| Despliegue lento | `git clone` de GitHub en el servidor | Verifica conectividad a Internet del servidor. |
| `update` no funciona | Repositorio no desplegado | Ejecuta primero `deploy`. |