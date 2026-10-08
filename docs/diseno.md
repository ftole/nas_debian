# Sistema de Diseño Slate UI y Experiencia de Usuario • NAS Debian 13

Este documento detalla las decisiones visuales, la arquitectura de interfaz (UI), la paleta de tokens y los principios de experiencia de usuario (UX) implementados tanto en el panel web administrativo como en el asistente de consola.

---

## 1. Principios de Diseño Visual

La interfaz web fue construida siguiendo una premisa fundamental: **un servidor de infraestructura debe ofrecer una administración serena, rápida y sin distracciones visuales**.

1. **Diseño Espacioso y No Saturado:**  
   - Jerarquía clara basada en la escala de tonos Slate y Zinc.
   - Espaciado generoso de respiración (24px en contenedores principales y 18px en celdas de tablas).
   - Esquinas redondeadas suaves y modernas (10px a 12px en tarjetas, 6px en botones y campos de entrada).
   - Ausencia deliberada de degradados estridentes, animaciones pesadas o elementos que distraigan la lectura técnica.

2. **Tipografía Nativa del Sistema (Zero Network Overhead):**  
   - Eliminación total de llamadas a servicios remotos de fuentes (como Google Fonts).
   - Pila tipográfica que aprovecha las fuentes optimizadas del sistema operativo del cliente:
     ```css
     --font-sans: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, "Helvetica Neue", Arial, sans-serif;
     --font-mono: ui-monospace, SFMono-Regular, "SF Mono", Menlo, Monaco, Consolas, "Liberation Mono", monospace;
     ```
   - Tamaños proporcionales: 14px en texto base, 20px en títulos de sección, 26px en tarjetas de métricas (KPIs) y 11.5px en etiquetas auxiliares.

3. **100% Offline (Cero Dependencias Externas):**  
   - Sin librerías pesadas externas (sin Bootstrap, sin Tailwind CDN, sin dependencias de Node.js en tiempo de ejecución).
   - CSS puro y compacto en `/css/app.css` con variables CSS dinámicas para soporte instantáneo de tema Claro y Oscuro.
   - JavaScript nativo en ES6+ (`/js/app.js`) para una interactividad ágil sin empaquetadores complejos.
   - 36 iconos SVG vectoriales empaquetados localmente en un bloque `<svg><defs>` inline en el layout.

4. **Desacoplamiento Absoluto de Cockpit:**  
   - Interfaz propietaria construida sobre PHP 8 MVC, eliminando cualquier residuo o conflicto con Cockpit (`.cockpit-*`, `cockpit.css`), garantizando una experiencia coherente y personalizada.

---

## 2. Tokens de Color y Temas Dinámicos

La interfaz cuenta con soporte nativo para **Tema Oscuro** (predeterminado) y **Tema Claro**, conmutables con un solo clic y persistencia automática en el `localStorage` del navegador:

| Token CSS | Tema Oscuro (Dark) | Tema Claro (Light) | Propósito / Ámbito |
| :--- | :--- | :--- | :--- |
| `--bg-body` | `#070b14` (Slate Profundo) | `#f8fafc` (Slate 50) | Fondo principal de la ventana |
| `--bg-card` | `#0f172a` (Slate 900) | `#ffffff` (Blanco) | Superficie de paneles y tarjetas |
| `--bg-card-header` | `#131d35` (Slate 850) | `#f8fafc` (Slate 50) | Cabecera de tablas y bloques |
| `--bg-surface` | `#1e293b` (Slate 800) | `#f1f5f9` (Slate 100) | Fondos de controles secundarios y chips |
| `--bg-input` | `#0b1120` (Slate 950) | `#ffffff` (Blanco) | Cajas de texto y menús desplegables |
| `--border-color` | `rgba(255,255,255,0.08)` | `#e2e8f0` (Slate 200) | Líneas divisorias y bordes estándar |
| `--text-main` | `#f8fafc` (Slate 50) | `#0f172a` (Slate 900) | Texto principal de alto contraste |
| `--text-secondary` | `#cbd5e1` (Slate 300) | `#475569` (Slate 600) | Subtítulos y descripciones |
| `--text-muted` | `#94a3b8` (Slate 400) | `#64748b` (Slate 500) | Metadatos y textos informativos |
| `--accent-primary` | `#3b82f6` (Azul 500) | `#2563eb` (Azul 600) | Botones de acción y elementos activos |
| `--accent-success` | `#10b981` (Esmeralda 500) | `#059669` (Esmeralda 600) | Estados exitosos y servicios activos |
| `--accent-warning` | `#f59e0b` (Ámbar 500) | `#d97706` (Ámbar 600) | Advertencias preventivas |
| `--accent-danger` | `#ef4444` (Rojo 500) | `#dc2626` (Rojo 600) | Errores y acciones destructivas |

---

## 3. Experiencia de Usuario (UX) en Módulos Clave

### 3.1 Explorador de Archivos Drag-and-Drop
- **Navegación visual limpia:** Vista en lista con diferenciación clara de carpetas, documentos de oficina, archivos comprimidos, código fuente y medios multimedia.
- **Subida intuitiva:** Zona interactiva de arrastre desde cualquier explorador de Windows con barra de progreso fluida que soporta archivos de hasta 512 MB.
- **Descarga flexible:** Descarga individual de archivos y generación al vuelo de archivos comprimidos `.zip` para carpetas completas mediante `ZipArchive`.
- **Papelera de reciclaje visual (`.trash/`):**
  - Al pulsar "Eliminar", el archivo no se destruye de golpe; se mueve a la papelera registrando su ruta de origen.
  - La barra superior del explorador muestra un botón con icono de papelera y un contador numérico dinámico (*badge*).
  - La papelera permite inspeccionar la fecha de eliminación, restaurar el archivo a su carpeta original con un solo clic o vaciarla de forma definitiva tras confirmación modal.
- **Visor y editor integrado en caliente:**
  - Al hacer clic en cualquier archivo de texto, configuración, script o registro, se despliega un visor modal a pantalla completa.
  - Incluye numeración de líneas con desplazamiento sincronizado.
  - Un botón "Editar" transforma la vista en un editor de texto interactivo con guardado directo en el servidor mediante `/api/files/save`.
  - Permite visualizar imágenes (`png`, `jpg`, `svg`, `webp`) y documentos PDF directamente en el navegador.

---

### 3.2 Terminal PTY Real (tmux)
- **Pseudoterminal verdadero (PTY):** Cada sesión web abre su propia sesión `tmux` ejecutada *como el usuario autenticado*, no como `www-data`. Esto habilita programas interactivos de pantalla completa (`top`, `htop`, `nano`, `vi`, `less`, `man`) y el envío de señales como `Ctrl+C`.
- **Soporte real de `sudo`:** Al escribir `sudo <comando>`, el sistema solicita la contraseña del usuario tal como en SSH; la entrada se canaliza hacia el PTY de forma segura (el helper root valida que el usuario pertenezca a `grp_samba`/`grp_web`).
- **Aislamiento por usuario:** La sesión `tmux` vive en el servidor y persiste al cambiar de pestaña; arranca en el directorio de inicio del usuario y puede finalizarse con el botón de la barra o `exit`.
- **Historial y comandos rápidos:** Flechas `Arriba`/`Abajo` para el historial, botones de acceso rápido (`df -h`, `free -m`, `uptime`, `smbstatus`, `realm list`) y un botón `Ctrl+C` para interrumpir procesos en curso.
- **Helper de lista blanca:** La única operación privilegiada es `/usr/local/sbin/nas-terminal`, cuyo contrato se limita a `start`/`keys`/`capture`/`resize`/`kill` sobre la sesión del propio usuario.

---

### 3.3 Dashboard y Monitoreo en Tiempo Real
- **Tarjetas de estado comprensibles:** Métricas clave (Uso de Procesador, Memoria RAM libre, Ocupación de Discos y Tráfico de Red) acompañadas de colores contextuales (Verde para estado óptimo, Ámbar para precaución, Rojo para saturación).
- **Control de servicios:** Indicadores visuales en vivo del estado de los demonios (`smbd`, `wsdd2`, `nginx`, `php-fpm`, `sssd`), permitiendo reiniciar o detener servicios con un solo clic.

---

## 4. Asistente Visual de Consola (Whiptail TUI)

Para la administración directa desde la terminal o por SSH, el asistente `whiptail` implementa una experiencia cuidada:

- **Paleta ANSI nativa coherente:** Uso de colores que respetan el estándar del terminal de Debian, sin caracteres extraños ni dependencias de entornos gráficos.
- **Ciclo de validación y edición sin pérdida de datos:** Si una prueba de conexión remota (CIFS o SSH) falla por una contraseña incorrecta o un nombre de recurso erróneo, el asistente **no descarta el formulario**. Muestra el error exacto y reabre el cuadro de diálogo manteniendo intactos los datos introducidos para que el administrador solo corrija el campo necesario.
- **Detección inteligente de hardware:** Al configurar discos o el usuario administrador, el asistente analiza los dispositivos disponibles y resalta automáticamente las opciones recomendadas, protegiendo las unidades que contienen el sistema operativo.
