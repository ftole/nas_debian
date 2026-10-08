# Características y Capacidades del Sistema • NAS Debian 13

Este documento resume las capacidades funcionales, operativas y técnicas del servidor. Está redactado con un enfoque directo y práctico para que administradores y equipos técnicos conozcan todo lo que la solución ofrece.

---

## 1. Visión General: Un Servidor, Dos Funciones Especializadas

El sistema fue diseñado desde cero para resolver dos problemas críticos en infraestructuras locales:
1. **Compartir archivos en red a gran velocidad y sin bloqueos** entre decenas o cientos de computadoras Windows, Linux y Mac.
2. **Centralizar copias de seguridad de servidores y puestos de trabajo** con deduplicación extrema, resistencia a cortes eléctricos y aislamiento total contra ransomware.

Para garantizar la máxima seguridad, el sistema opera bajo dos roles mutuamente excluyentes:

![Matriz de Roles y Selección de Almacenamiento](assets/roles_almacenamiento.svg)

| Característica | Rol `ARCHIVOS` (NAS) | Rol `BACKUP` (Central de Respaldos) |
| :--- | :--- | :--- |
| **Propósito principal** | Almacenamiento departamental diario | Repositorio histórico e inmutable de copias |
| **Visibilidad de carpetas** | Recursos visibles en el explorador de red | Recursos ocultos (terminados en `$`) |
| **Filesystem en HDD** | `ext4` con `commit=2` (vaciado en 2s anti-apagón) | `Btrfs` con compresión `zstd:3` + scrub mensual |
| **Filesystem en SSD** | `ext4` con `commit=5` + `fstrim.timer` | `Btrfs` con `zstd:3` + `discard=async` |
| **Acceso a usuarios** | Usuarios departamentales según permisos | Exclusivo para administradores (`grp_samba`) |
| **Reutilización de datos** | Soporte `--keep-data` (montar sin formatear) | Soporte `--keep-data` (montar sin formatear) |

---

## 2. Compartición de Archivos y Optimización para Microsoft Office / Excel

Uno de los problemas más frecuentes en servidores NAS basados en Linux es el bloqueo de hojas de cálculo cuando muchas personas trabajan a la vez en carpetas compartidas. Este proyecto lo resuelve desde el núcleo de Samba:

- **Soporte para más de 100 puestos simultáneos en Excel y Office:**  
  Gracias a la activación de los módulos VFS `acl_xattr` y `streams_xattr`, Samba emula de forma nativa los flujos de datos alternativos de Windows (ADS) y almacena las listas de control de acceso en atributos extendidos de Linux (`xattr`). Esto evita bloqueos con archivos temporales como `~$Presupuesto.xlsx` y garantiza el guardado atómico.
- **Cuatro esquemas de permisos granulares:**
  1. *Lectura y Escritura por Grupo:* Permisos completos para los grupos seleccionados (`mask 0770`).
  2. *Solo Lectura General + Escritura Exclusiva:* Los grupos departamentales consultan el archivo y solo el grupo responsable puede modificarlo. Aplica `default ACL` para que toda subcarpeta o archivo creado herede automáticamente esta regla.
  3. *Solo Lectura Estricta:* Ideal para normativas, manuales y repositorios históricos (`read only = yes`).
  4. *Acceso Público / Invitados:* Acceso directo para intercambio rápido sin requerir usuario ni contraseña.
- **Matriz de permisos integrada en Usuarios y sincronización de ACL POSIX:**  
  La pestaña **Usuarios y grupos** incluye la matriz *grupo/usuario × recurso* (Sin acceso → Solo lectura → Lectura y escritura). Cada cambio recalcula automáticamente las ACL POSIX del directorio (`setfacl`, con herencia por defecto) para que Samba y el sistema de archivos coincidan. El botón **Reparar ACL** corrige desajustes que provocaban accesos denegados pese a estar autorizado en Samba.
- **Descubrimiento instantáneo en Windows 10/11 (WSDD2):**  
  Implementa el demonio ligero `wsdd2` con override de systemd. Los equipos clientes detectan el servidor al instante en su sección "Red" del Explorador de Windows, sin activar protocolos obsoletos ni inseguros como NetBIOS broadcast o SMBv1.

---

## 3. Motor de Copias de Seguridad Multiplataforma

El motor de respaldos trabaja de forma autónoma con una arquitectura pensada para resistir caídas de red, cortes eléctricos y ataques de ransomware:

- **Deduplicación por Enlaces Duros (*Hardlinks* >85% de ahorro):**  
  Utiliza `rsync --link-dest` comparando con el snapshot precedente. Si un archivo de 2 GB no cambió entre el lunes y el martes, el snapshot del martes apunta al mismo inodo físico en disco consumiendo **0 bytes adicionales**, pero permitiendo navegar y restaurar como si fuera una copia completa.
- **Staging Atómico (`.inprogress_*`) y Resistencia a Apagones:**  
  Ninguna copia se escribe directamente en el histórico. Se procesa en una carpeta oculta provisional. Si ocurre un corte de energía, una falla de red o se cancela la tarea, el manejador de señales (`trap cleanup EXIT TERM INT`) purga los datos incompletos. Únicamente las copias 100% íntegras se promueven atómicamente a `snapshot_YYYY-MM-DD_HHMMSS`.
- **Compatibilidad con Orígenes Heterogéneos:**
  - **Servidores Windows:** Conector CIFS SMB 3.1.1 en solo lectura (`ro,soft,timeo=30`) con análisis automático de cuentas locales y de dominio Active Directory (`DOMINIO\usuario`), almacenando credenciales bajo permisos `0600 root:root`.
  - **Servidores Linux:** Replicación mediante SSH y `rsync -aAXH --numeric-ids`, preservando con fidelidad absoluta los propietarios, permisos octales, enlaces duros, atributos extendidos y ACLs POSIX originales. Las firmas de host se validan con `StrictHostKeyChecking=accept-new` en un archivo aislado.
  - **Carpetas Locales:** Sincronización entre rutas internas del servidor a máxima velocidad de disco.
- **Monitoreo Preventivo de Capacidad:**  
  Antes de iniciar la copia, el sistema consulta el espacio disponible con `df -Pk`. Si el uso supera el 85%, genera una advertencia en el registro. Si queda menos de 2 GB libres o la ocupación supera el 95%, cancela la operación de inmediato para evitar que el disco colapse.
- **Cancelación Segura en Caliente:**  
  Cualquier tarea en curso puede abortarse limpiamente desde la interfaz web o desde el asistente, liberando el bloqueo de `flock` y desmontando los recursos sin dejar procesos zombi.

---

## 4. Panel Web de Administración (Slate UI 100% Offline)

El servidor incluye una interfaz web nativa basada en PHP 8 MVC y el sistema de diseño Slate UI:

- **100% Offline (Cero dependencias remotas):**  
  No hace peticiones a Google Fonts, ni descarga librerías desde CDNs como Bootstrap o Tailwind CDN. Cuenta con 36 iconos vectoriales SVG incrustados localmente, garantizando que el panel abra al instante incluso en redes totalmente aisladas de Internet.
- **Consumo casi nulo en reposo (~0 MB):**  
  Funciona mediante Nginx-light y un pool PHP-FPM configurado en modo `pm = ondemand`. Cuando nadie navega en el panel, los procesos de PHP se suspenden por completo.
- **Explorador de Archivos Drag-and-Drop:**
  - Navega con seguridad por las carpetas de `/srv/nas`.
  - Permite arrastrar y soltar archivos desde Windows con barra de progreso interactiva (hasta 512 MB por archivo).
  - Descarga archivos individuales o carpetas completas comprimidas en ZIP al vuelo.
  - **Papelera de Reciclaje Integrada (`.trash/`):** Al eliminar un archivo, se mueve a la papelera con su ruta original registrada. El panel muestra un contador flotante (badge) con los elementos en papelera, permitiendo restaurarlos a su ubicación exacta o vaciarla definitivamente.
  - **Visor Universal de Archivos y Ofimática 100% Offline (Excel, Word, PowerPoint, Texto, PDFs y Multimedia):** Permite previsualizar hojas de cálculo (`.xlsx`, `.xls`, `.ods`, `.csv`, `.tsv`) con navegación por pestañas y tablas estilizadas (SheetJS local), documentos Word (`.docx`) renderizados fielmente en formato página (docx-preview y JSZip locales), presentaciones PowerPoint (`.pptx`) con navegación interactiva por diapositivas y extracción de contenidos, además de visor y editor de texto en caliente con numeración de líneas, reproductor multimedia HTML5 y visor PDF nativo con cero dependencias en la nube.
- **Terminal PTY Real (tmux) con `sudo`:**  
  Ejecuta una sesión `tmux` auténtica *como el usuario de la sesión* (no como `www-data`), habilitando programas interactivos de pantalla completa (`top`, `htop`, `nano`, `vi`) y `Ctrl+C`. Soporta `sudo` solicitando la contraseña igual que en SSH, arranca en el directorio de inicio del usuario, persiste en el servidor y conserva historial con flechas ↑ y ↓ más botones de acceso rápido (`df -h`, `free -m`, `uptime`, `smbstatus`, `realm list`).
- **Integración con Active Directory (AD):**  
  Permite descubrir controladores de dominio en la red corporativa y unir el servidor NAS al dominio Windows mediante `realmd`, `sssd` y `adcli` con un solo formulario.
- **Auditoría y Registros del Sistema:**  
  Todos los eventos administrativos (creación de recursos, gestión de usuarios, copias de seguridad, papelera de reciclaje y servicios) quedan indexados cronológicamente en SQLite WAL.

---

## 5. Asistente Visual de Consola y Comandos CLI `nas`

Para administradores que prefieren la terminal o gestionan el servidor por SSH:

- **Comando global `nas`:**
  - `sudo nas`: Inicia el asistente interactivo de 9 módulos.
  - `sudo nas status`: Muestra el estado integral de servicios, discos, recursos Samba y tareas de backup.
  - `sudo nas update`: Actualiza el código del servidor desde GitHub con verificación de integridad y rollback automático.
  - `sudo nas version`: Imprime la versión y el commit de Git instalado.
  - `sudo nas uninstall`: Limpia y desinstala por completo el software.
- **Asistente Whiptail con 9 módulos guiados:**
  - Despliegue en 5 pasos con detección automática de discos, IP y usuario.
  - Gestión de grupos departamentales (`grp_*`).
  - Creación y edición de carpetas compartidas Samba.
  - Programación y monitoreo de tareas de respaldo.
  - Gestión de usuarios y contraseñas de red (`smbpasswd`).
  - Diagnóstico de salud en vivo con informe de estado.
  - Reinicio seguro de servicios.
  - Búsqueda de actualizaciones.
  - Desinstalación total del servidor.

---

## 6. Mantenimiento Proactivo y Resiliencia Física

- **Prevención de pérdida por apagones:** En discos mecánicos, ext4 sincroniza el journal cada 2 segundos (`commit=2`), protegiendo los datos ante desconexiones repentinas de energía.
- **Prevención de corrupción silenciosa (*Bit Rot*):** En instalaciones con Btrfs, una tarea mensual programada (`btrfs scrub`) audita la integridad de cada bloque mediante sumas criptográficas SHA256.
- **Optimización continua de unidades SSD:** El servicio `fstrim.timer` ejecuta periódicamente el descarte de bloques para alargar la vida útil de memorias flash y NVMe.
- **Rotación de bitácoras (`logrotate`):** Comprime y archiva semanalmente los registros en `/srv/nas/LOGS_BACKUP/` con la directiva `copytruncate`, impidiendo que los logs consuman espacio en disco de forma invisible.
