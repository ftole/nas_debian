# Posibles Problemas, Soluciones y Modos de Fallo (FMEA) • NAS Debian 13

Este documento recopila las situaciones anómalas más comunes que pueden presentarse durante la instalación, despliegue o explotación del servidor, explicando cómo diagnosticarlas y resolverlas paso a paso. También incluye el análisis formal de modos de fallo y resiliencia (FMEA).

---

## 1. Guía Rápida de Solución de Problemas (Troubleshooting)

### 1.1 "Windows no detecta el servidor en el Explorador ('Red')"
- **Síntoma:** Otros equipos en la red local no ven el servidor NAS en la pestaña "Red" de Windows 10/11.
- **Causas habituales:**
  1. El servicio `wsdd2` no está en ejecución.
  2. El firewall UFW está bloqueando los paquetes de descubrimiento.
- **Diagnóstico y solución:**
  ```bash
  # 1. Comprobar el estado del servicio WSDD2
  sudo systemctl status wsdd2

  # 2. Si falló por DynamicUser en Debian 13, verificar el override de systemd
  cat /etc/systemd/system/wsdd2.service.d/override.conf

  # 3. Comprobar que los puertos de WSDD2 estén abiertos en UFW
  sudo ufw status | grep -E '3702|5355|5357'
  ```
  Si los puertos no estaban permitidos, habilítalos:
  ```bash
  sudo ufw allow 3702/udp comment 'WSD Discovery'
  sudo ufw allow 3702/tcp comment 'WSD Discovery TCP'
  sudo ufw allow 5355/udp comment 'LLMNR UDP'
  sudo ufw allow 5357/tcp comment 'WSD HTTP'
  ```

---

### 1.2 "El montaje CIFS falla en la tarea de respaldo de Windows"
- **Síntoma:** Al ejecutar o probar una tarea de backup hacia una carpeta compartida de Windows, la conexión es rechazada.
- **Causas habituales:**
  1. La cuenta pertenece a un dominio corporativo pero se omitió el dominio en las credenciales.
  2. El servidor Windows exige un dialecto SMB diferente o firmas obligatorias.
- **Diagnóstico y solución:**
  - Si el usuario es de Directorio Activo, asegúrate de escribir `DOMINIO\usuario` en el asistente o panel web. El sistema desglosará automáticamente los campos en `/etc/backup-credentials/<tarea>.cred`.
  - Comprueba la accesibilidad del recurso remoto con `smbclient`:
    ```bash
    smbclient -L //<IP_WINDOWS> -U "DOMINIO\usuario"
    ```
  - Si el host remoto ejecuta una versión antigua de Windows Server, ajusta temporalmente el dialecto en la llamada a `mount.cifs` probando con `vers=3.0` o `vers=2.1`.

---

### 1.3 "La tarea de respaldo aborta con error de espacio insuficiente"
- **Síntoma:** El registro en `/srv/nas/LOGS_BACKUP/` indica que la tarea fue abortada por falta de espacio libre.
- **Por qué ocurre:** Por seguridad, los runners evalúan la capacidad con `df -Pk` antes de iniciar. Si el almacenamiento supera el 95% de uso o restan menos de 2 GB libres, la tarea se cancela preventivamente para evitar la corrupción del sistema de archivos.
- **Solución:**
  1. Comprueba el uso real de disco: `df -h /srv/nas`.
  2. Revisa la política de retención de las tareas. Si está acumulando demasiados snapshots, reduce el valor (ej. a 15 o 30).
  3. Vacía la papelera de reciclaje web si contiene archivos pesados eliminados.
  4. Si utilizas Btrfs, comprueba el espacio libre y ejecuta un balanceo si es necesario:
     ```bash
     btrfs filesystem usage /srv/nas
     ```

---

### 1.4 "Un disco aparece marcado como 'EN USO' y el asistente no permite formatear"
- **Síntoma:** Al intentar desplegar el servidor sobre un disco secundario, el asistente advierte que el dispositivo está en uso.
- **Causas y comportamiento de seguridad:**
  - **Si el disco forma parte de un arreglo RAID o es un volumen físico LVM (PV):** El sistema **bloquea incondicionalmente el formateo**. Esto previene la destrucción catastrófica de volúmenes de almacenamiento existentes.
  - **Si el disco simplemente tiene una partición montada en otra ruta:** El sistema exige ejecutar el despliegue con la bandera `--ignore-in-use` y escribir explícitamente `SI-FORMATEAR`.
  - **Si el disco ya contiene datos que deseas conservar:** Utiliza la opción `--keep-data` para montarlo en `/srv/nas` sin formatear ni perder información.

---

### 1.5 "Bloqueos o errores de solo lectura al abrir archivos de Office / Excel"
- **Síntoma:** Varios usuarios abren una hoja de cálculo compartida en red y reciben mensajes de "archivo en uso por otro usuario" o no pueden guardar.
- **Diagnóstico y solución:**
  - Comprueba que la sección `[global]` de `/etc/samba/smb.conf` incluya los módulos VFS corporativos:
    ```ini
    vfs objects = acl_xattr streams_xattr
    store dos attributes = yes
    inherit permissions = yes
    strict sync = yes
    ```
  - Comprueba que los permisos del recurso en `/srv/nas/<RECURSO>` tengan el bit SGID (`2770`) y las ACLs heredadas:
    ```bash
    getfacl /srv/nas/<RECURSO>
    ```

---

### 1.6 "El panel web devuelve 502 Bad Gateway o no carga"
- **Síntoma:** El navegador no puede conectar al puerto 80/443 o muestra un error de pasarela de Nginx.
- **Diagnóstico y solución:**
  ```bash
  # Verificar el estado del servidor web y del procesador PHP
  sudo systemctl status nginx
  sudo systemctl status php*-fpm

  # Revisar registros de error
  sudo tail -n 30 /var/log/nginx/error.log
  ```
  Si PHP-FPM está detenido, inícialo con `sudo systemctl restart php*-fpm`. Recuerda que en reposo el pool `nas-web` utiliza `pm = ondemand` y solo levanta procesos trabajadores cuando recibe peticiones HTTP.

---

## 2. Matriz de Modos de Fallo y Resiliencia (FMEA)

El proyecto incluye pruebas de inyección de fallos automatizadas en GitHub Actions (`bats tests/`, `pytest tests/`, `php tests/test_web.php`). A continuación se resume cómo responde el sistema ante cada escenario crítico:

### 2.1 Modos de Fallo en Copias de Seguridad

| Escenario de Fallo | Comportamiento del Sistema | Mecanismo de Protección | Verificación |
| :--- | :--- | :--- | :--- |
| **Espacio insuficiente (<2 GB o >95%)** | Aborta antes de transferir datos y registra alerta clara. | Chequeo preventivo `df -Pk`. | `failure_runners.bats` |
| **`rsync` falla a mitad de copia** | Sale con código de error y destruye el staging temporal. | Trampa `trap cleanup EXIT TERM INT`. | `failure_runners.bats` |
| **Host remoto se desconecta a mitad de copia** | Sale con error; no cuelga el kernel del NAS. | Montaje con `soft,timeo=30` y `noserverino`. | `failure_runners.bats` |
| **Credenciales rechazadas** | Falla con mensaje explícito sin reintentos infinitos. | Validación previa de salida de autenticación. | `failure_runners.bats` |
| **Ejecución simultánea de la misma tarea** | La segunda ejecución se descarta sin error ("BACKUP OMITIDO"). | Cerrojo exclusivo con `flock`. | `failure_runners.bats` |
| **Corte de energía / Apagón repentino** | El snapshot parcial queda en `.inprogress_*` y se purga al reiniciar; el histórico solo expone copias 100% íntegras. | Staging atómico + promoción `mv`. | Inspección en laboratorio |
| **Cancelación manual en caliente** | Detiene subprocesos, libera el cerrojo y purga el directorio temporal. | Interceptores de señal `SIGTERM` / `SIGINT`. | Panel Web / CLI |

---

### 2.2 Modos de Fallo en Almacenamiento y Despliegue

| Escenario de Fallo | Comportamiento del Sistema | Mecanismo de Protección | Verificación |
| :--- | :--- | :--- | :--- |
| **Intento de formatear disco con RAID o LVM** | Aborta incondicionalmente sin opción a formatear. | Detección estricta de subsistemas de bloques. | `failure_helpers.bats` |
| **Intento de formatear disco montado** | Aborta salvo que se proporcione `--ignore-in-use` y confirmación `SI-FORMATEAR`. | Doble confirmación interactiva. | `failure_helpers.bats` |
| **Reutilizar disco con datos (`--keep-data`)** | Detecta la partición y el filesystem; monta sin formatear respetando datos. | Identificación de etiquetas y tipo de FS. | Pruebas de integración |
| **Disco del SO identificado erróneamente** | Se excluye de la lista de discos y se aísla en UDisks2. | Regla udev `80-udisks2-hide-os.rules`. | `helpers.bats` |
| **Degradación magnética o silenciosa (*Bit Rot*)** | La tarea mensual de `btrfs scrub` detecta sumas SHA256 inconsistentes y repara sectores. | Auditoría Btrfs programada. | `nas-btrfs-scrub` |

---

### 2.3 Modos de Fallo en Panel Web y Seguridad

| Escenario de Fallo | Comportamiento del Sistema | Mecanismo de Protección | Verificación |
| :--- | :--- | :--- | :--- |
| **Intento de salto de directorio (*Path Traversal*)** | Se rechaza con código de error HTTP 400/403 sin exponer el filesystem. | Validación estricta con `realpath()` y `resolveSafePath`. | `test_web.php` |
| **Intento de inyección de comandos en terminal/API** | Los argumentos no se interpretan por shell; se pasan como arreglo puro a `proc_open`. | Invocación estricta sin `/bin/sh -c`. | `test_web.php` |
| **Comandos interactivos en terminal web (`nano`, `vi`)** | Se detectan preventivamente y se emite mensaje educativo guiando al visor web o SSH. | Filtro de binarios TTY en `TerminalService`. | `test_web.php` |
| **Eliminación accidental de un archivo en la web** | El archivo se resguarda en la papelera `.trash/` y puede recuperarse intacto. | Sistema de Papelera con metadatos JSON. | Pruebas web |
| **Caída de conexión a Internet** | El panel y todas sus pantallas abren con normalidad al 100%. | Arquitectura 100% Offline (cero dependencias remotas). | `test_mockup.py` |

---

## 3. Principios de Validación del Proyecto

1. **Se prueba el artefacto real:** Las pruebas unitarias y de estrés evalúan directamente los runners generados, los servicios PHP y los scripts Bash, no maquetas estáticas.
2. **Las pruebas verifican el resultado del fallo:** No basta con comprobar que un script retorne código distinto de cero; se comprueba que el mensaje sea claro, que los descriptores temporales se hayan eliminado y que el sistema permanezca en un estado coherente.
3. **Hermetismo:** Las pruebas automáticas de CI no requieren privilegios de `root` ni tocan servicios reales del equipo de desarrollo.
