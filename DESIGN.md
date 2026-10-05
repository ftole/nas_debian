# DESIGN.md • Sistema de Diseño Slate UI & Especificación Visual NAS Debian 13

> **Versión:** 2.0 (Modern Slate Architecture)  
> **Ámbito:** Plataforma de Gestión Web NAS & Central de Respaldos Multiplataforma (Debian 13 Trixie)  
> **Filosofía:** Cero dependencias externas (100% Offline), tipografía del sistema de alta legibilidad, espaciado generoso no saturado, base de datos SQLite con PDO (0 MB de consumo en reposo), terminal bash real y explorador de archivos con Drag-and-Drop nativo.

---

## 1. Principios de Diseño Visual

1. **Diseño Espacioso y No Saturado:**  
   - Jerarquía visual serena basada en la escala Slate/Zinc.
   - Márgenes de respiración aumentados (24px de espaciado estándar en contenedores y 18px en celdas).
   - Radio de esquinas moderno (10px a 12px en tarjetas, 6px en controles e inputs).
   - Eliminación de contrastes estridentes y degradados pesados.

2. **Tipografía Nativa del Sistema (Zero External Network Overhead):**  
   - Eliminación estricta de Google Fonts y llamadas a redes externas.
   - Pila tipográfica ultra-optimizada y legible en cualquier sistema operativo:
     ```css
     --font-sans: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, "Helvetica Neue", Arial, sans-serif, "Apple Color Emoji", "Segoe UI Emoji";
     --font-mono: ui-monospace, SFMono-Regular, "SF Mono", Menlo, Monaco, Consolas, "Liberation Mono", "Courier New", monospace;
     ```
   - Tamaños base cómodos: cuerpo de texto 13.5px / 14px, títulos de página 20px, KPIs 26px, etiquetas micro 11.5px.

3. **0% Dependencias Externas (100% Offline):**  
   - Cero CDNs (sin Bootstrap, FontAwesome, Google Fonts, Tailwind CDN, unpkg ni jsdelivr).
   - 36 iconos vectoriales SVG limpios empaquetados en un sprite `<svg><defs>` inline en el layout.
   - Hoja de estilos única unificada (`/css/app.css`) con variables CSS para soporte nativo de temas Claro y Oscuro.
   - JavaScript puro en ES6+ (`/js/app.js`) sin librerías externas ni empaquetadores pesados.

4. **Desacoplamiento Absoluto de Cockpit:**  
   - Purga de todas las clases y dependencias de Cockpit (`.cockpit-*`, `cockpit.css`).
   - Arquitectura nativa MVC en PHP 8 servida de forma ultraligera mediante Nginx-light y PHP-FPM en modo `pm = ondemand`.

---

## 2. Paleta de Colores y Tokens de Diseño

El sistema implementa variables CSS dinámicas que se alternan fluidamente mediante el atributo `data-theme="dark"` o `data-theme="light"` en la etiqueta `<html>`:

| Token CSS | Tema Oscuro (Dark) | Tema Claro (Light) | Función / Ámbito |
| :--- | :--- | :--- | :--- |
| `--bg-body` | `#0f172a` (Slate 900) | `#f8fafc` (Slate 50) | Fondo principal de la aplicación |
| `--bg-card` | `#1e293b` (Slate 800) | `#ffffff` (White) | Superficie de paneles y tarjetas |
| `--bg-card-header` | `#172033` (Slate 850) | `#f1f5f9` (Slate 100) | Cabeceras de paneles y tablas |
| `--bg-surface` | `#334155` (Slate 700) | `#f1f5f9` (Slate 100) | Fondos de controles secundarios y chips |
| `--bg-input` | `#0f172a` (Slate 900) | `#ffffff` (White) | Fondos de campos de formulario y selects |
| `--border-color` | `#334155` (Slate 700) | `#e2e8f0` (Slate 200) | Líneas divisorias y bordes estándar |
| `--text-main` | `#f8fafc` (Slate 50) | `#0f172a` (Slate 900) | Texto principal de alto contraste |
| `--text-secondary` | `#cbd5e1` (Slate 300) | `#475569` (Slate 600) | Subtítulos y descripciones |
| `--text-muted` | `#94a3b8` (Slate 400) | `#64748b` (Slate 500) | Metadatos y textos auxiliares |
| `--accent-primary` | `#3b82f6` (Blue 500) | `#2563eb` (Blue 600) | Color de acción y acento corporativo |
| `--accent-success` | `#10b981` (Emerald 500) | `#059669` (Emerald 600) | Indicadores de éxito y servicios activos |
| `--accent-warning` | `#f59e0b` (Amber 500) | `#d97706` (Amber 600) | Alertas y advertencias preventivas |
| `--accent-danger` | `#ef4444` (Red 500) | `#dc2626` (Red 600) | Errores y acciones destructivas |

---

## 3. Arquitectura de Módulos e Interactividad

### 3.1 Explorador de Archivos y Recursos (File Explorer)
* **Navegación Confinada (Jail Traversal Defense):** Confinamiento estricto sobre `/srv/nas` y conmutación rápida entre Recursos Compartidos y Repositorio de Backups (`/srv/nas/BACKUPS_HISTORICOS`).
* **Subida Drag-and-Drop:** Zona interactiva de arrastre desde cualquier explorador Windows con soporte para transferencias directas de hasta 512 MB por archivo e indicador visual de progreso (`upload-progress-bar`).
* **Descarga Individual y en ZIP:** Descarga directa de archivos y compresión en streaming `.zip` al vuelo para carpetas completas mediante `ZipArchive`.
* **Operaciones de Archivo:** Creación guiada de carpetas (`mkdir`), renombrado seguro con sanitización de caracteres (`renameItem`) y eliminación permanente con confirmación visual modal (`deleteItem`).

### 3.2 Terminal Bash Real
* **Ejecución Real:** Conectado directamente a `/bin/bash` mediante `proc_open` con flujos no bloqueantes (`stream_set_blocking`) y límite de ejecución de 15 segundos.
* **Persistencia de Navegación (`cwd`):** Mantiene el directorio de trabajo activo entre comandos, soportando navegación nativa mediante `cd <directorio>`.
* **Historial Interactivo:** Navegación por comandos previos mediante flechas `Arriba` (↑) y `Abajo` (↓), persistencia en la tabla SQLite `terminal_history` y chips de acceso rápido (`uptime`, `df -h`, `free -m`, `smbstatus`, `realm list`).

### 3.3 Integración con Active Directory (AD)
* **Pila Nativa Debian 13:** Aprovisionamiento de `realmd`, `sssd`, `sssd-tools`, `adcli`, `libpam-sss`, `libnss-sss`, `krb5-user` y `packagekit`.
* **Panel de Membresía:** Detección en vivo del estado del dominio (`realm list`), servidor KDC, Realm Kerberos y estado del demonio SSSD.
* **Descubrimiento y Unión:** Herramienta interactiva para descubrir controladores de dominio en la red local y formulario seguro de unión con paso de contraseña vía `stdin` seguro (evitando fugas en tablas de procesos `ps`).

### 3.4 Base de Datos SQLite Nativa (0 MB RAM en reposo)
* **Almacenamiento:** `/var/lib/nas/nas.sqlite` con permisos `0770 www-data:www-data`.
* **Modo WAL (Write-Ahead Logging):** Concurrencia de lectura sin bloqueos con `PRAGMA journal_mode = WAL` y `PRAGMA busy_timeout = 5000`.
* **Esquemas:**
  - `audit_logs`: Registro cronológico e indexado de todas las acciones del sistema, Samba y panel web.
  - `terminal_history`: Comandos ejecutados, usuario, tiempo de respuesta y códigos de salida.
  - `backup_tasks` y `backup_history`: Catálogo de réplicas, snapshots y métricas de deduplicación.
  - `system_settings`: Claves de configuración persistentes.
  - `domain_config`: Membresía y metadatos del dominio corporativo.

---

## 4. Distribución del Menú de Navegación

La barra lateral se organiza en 2 bloques intuitivos:

```text
ALMACENAMIENTO Y RECURSOS
├── Archivos                  -> Explorador de archivos con Drag-and-Drop
├── Redes compartidas         -> Recursos Samba con 4 esquemas de permisos
├── Respaldos                 -> Central de copias CIFS / SSH y deduplicación
└── Almacenamiento            -> Discos, BTRFS scrub y fstrim

ADMINISTRACIÓN Y SISTEMA
├── Vista general             -> Dashboard con métricas de CPU, RAM y red
├── Terminal                  -> Consola interactiva de bash
├── Usuarios y grupos         -> Cuentas Linux, grupos grp_* y smbpasswd
├── Dominio AD                -> Integración con Active Directory / realmd
├── Servicios                 -> Control de demonios systemd (smbd, wsdd2, sssd)
├── Registros (Logs)          -> Auditoría multidimensional y filtros
├── Diagnóstico               -> Chequeo de salud del servidor en vivo
├── Redes                     -> Interfaces de red e IP
├── Actualizaciones           -> Motor de actualización remota
└── Componentes               -> Tecnologías y subsistemas nativos
```
