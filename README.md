# Servidor NAS y Central de Respaldos Multiplataforma (Debian 13)

[![Debian 13](https://img.shields.io/badge/OS-Debian%2013%20(Trixie)-A81D33?style=for-the-badge&logo=debian&logoColor=white)](https://github.com/ftole/nas_debian) [![Rust](https://img.shields.io/badge/Platform-Rust-DEA584?style=for-the-badge&logo=rust&logoColor=white)](https://github.com/ftole/nas_debian) [![Samba](https://img.shields.io/badge/Service-Samba%20SMB-0066CC?style=for-the-badge)](https://github.com/ftole/nas_debian) [![Web UI](https://img.shields.io/badge/Web%20UI-Rust%20(axum)%20%2B%20Nginx-009688?style=for-the-badge)](https://github.com/ftole/nas_debian)

Este repositorio contiene las herramientas para desplegar y administrar sobre **Debian 13 (Trixie)** un servidor de almacenamiento en red (NAS departamental) y una central de copias de seguridad resistente a ransomware. 

Todo el sistema corre de forma **nativa** sobre Linux, sin capas de virtualización pesadas ni dependencias externas. Toda la plataforma está implementada en **Rust**: el CLI `nas`, el asistente de terminal `nas-tui` y el panel web `nas-web` (axum + Nginx) comparten el mismo motor (`nas-engine`), con los assets del panel embebidos en el binario y cero llamadas a CDNs.

El panel web portado conserva **las vistas y el JavaScript originales** (`app.js`, CSS y librerías de ofimática offline servidos byte a byte), verificado en CI mediante el contrato de paridad `tests/parity/` y comparado en vivo contra el panel PHP antes de retirarlo (fase F6/F7).

---

## 🏛️ Arquitectura General

El servidor combina el rendimiento del kernel de Debian 13 con interfaces ágiles pensadas tanto para la consola como para el navegador:

![Arquitectura General del Sistema](docs/assets/arquitectura_general.svg)

---

## 🧱 Estructura del Repositorio

| Ruta | Contenido |
| :--- | :--- |
| `crates/nas-core` | Configuración, rutas, errores y contrato de respuesta |
| `crates/nas-exec` | Ejecución de comandos con allowlist, timeouts y redacción |
| `crates/nas-engine` | Almacenamiento, Samba, usuarios, respaldos, dominio, diagnósticos |
| `crates/nas-cli` | Comando `nas` (ANSI y `--json`) |
| `crates/nas-web` | Panel web axum + assets embebidos + plantillas portadas |
| `crates/nas-tui` | Asistente de terminal (ratatui) |
| `crates/nasctl`, `crates/nas-dev` | Multi-host e inspección de entorno de desarrollo |
| `src/core`, `src/lib` | Motor Bash de despliegue/desinstalación (aún vigente) |
| `legacy/` | Panel PHP, asistente `whiptail`, CLI PHP y utilidades Python retiradas |

## 📚 Documentación Técnica Detallada

Para mantener este repositorio organizado y este documento directo al grano, el detalle técnico completo se encuentra estructurado en el directorio [`docs/`](docs/):

| Documento | ¿Qué encontrarás allí? |
| :--- | :--- |
| 🏗️ **[Arquitectura del Sistema](docs/arquitectura.md)** | Desglose profundo de capas, kernel sysctl, VFS Samba, SQLite WAL y concurrencia. |
| ⚡ **[Características y Capacidades](docs/caracteristicas.md)** | Roles del servidor, soporte para +100 puestos en Office/Excel, papelera web y terminal interactiva. |
| 🧰 **[Tecnologías Usadas y Cuadro Maestro](docs/tecnologias.md)** | Inventario técnico completo de cada componente, demonio y parámetro del kernel con sus beneficios. |
| 🛡️ **[Seguridad y Endurecimiento](docs/seguridad.md)** | Defensa en profundidad, UFW, SSH blindado, credenciales `0600`, proc_open seguro y rollback. |
| 🎨 **[Sistema de Diseño Slate UI y UX](docs/diseno.md)** | Tokens visuales, filosofía 100% offline, 36 iconos SVG nativos y diseño sereno. |
| 🔧 **[Operación Diaria y Mantenimiento](docs/operacion_mantenimiento.md)** | Guía de uso diario, restauración de archivos, checklist preventivo y tareas programadas. |
| 🚀 **[Publicación y Gestión de Releases](docs/releases.md)** | Manual completo para primer release (beta) y versiones continuas con firmas GPG. |
| 🚨 **[Posibles Problemas, Soluciones y FMEA](docs/problemas_soluciones.md)** | Diagnóstico paso a paso de fallos comunes y matriz de resiliencia con pruebas BATS. |

---

## 🎯 Tres Roles Especializados (`ARCHIVOS`, `BACKUP`, `ARCHIVOS_BACKUP`)

Durante el despliegue puedes configurar tu máquina para uno de tres perfiles según tus necesidades operativas:

- **Rol ARCHIVOS (NAS Departamental):** Diseñado para compartir carpetas en red a toda la oficina. Utiliza **ext4 optimizado** (`commit=2` en HDD para soportar cortes de energía), expone recursos visibles en el explorador de Windows y soporta más de 100 puestos simultáneos trabajando en hojas de cálculo de Excel sin bloqueos temporales (`~$`). En el panel web, restringe el módulo de respaldos y concentra la operativa en recursos compartidos (`/shares`).
- **Rol BACKUP (Central de Respaldos):** Diseñado como caja fuerte para recibir copias de servidores Windows, Linux y locales. Utiliza **Btrfs con compresión Zstandard** (`zstd:3`), deduplicación por enlaces duros (`rsync --link-dest` con >85% de ahorro), recursos ocultos terminados en `$` y revisiones mensuales automáticas contra corrupción silenciosa (*Bit Rot*). En el panel web, restringe el módulo de recursos abiertos y concentra la gestión en tareas de copias de seguridad (`/backups`).
- **Rol ARCHIVOS_BACKUP (Híbrido - Archivos y Respaldos):** La solución integral que combina ambas funciones en un único servidor. Permite desplegar recursos compartidos departamentales visibles para la red corporativa y, de manera paralela, programar respaldos automatizados multiplataforma con carpetas protegidas. En el panel web, **habilita simultáneamente tanto Recursos Compartidos (`/shares`) como Tareas de Respaldo (`/backups`)**, adaptando la navegación, métricas y permisos en tiempo real.

### Persistencia del Rol y Comportamiento en el Panel Web
El rol seleccionado se guarda de forma inmutable en `/etc/nas/role`. El backend web (`SystemService`) lee este archivo dinámicamente y el middleware de control de acceso (`AuthMiddleware`) adapta el acceso a rutas y la navegación lateral, mostrando además el badge correspondiente (`ARCHIVOS`, `BACKUP` o `ARCHIVOS & BACKUP`) en la cabecera. Todos los roles admiten el modo **`--keep-data`** para reutilizar discos sin formatear.

![Matriz de Roles y Selección de Almacenamiento](docs/assets/roles_almacenamiento.svg)

*(Conoce todos los detalles en la [Guía de Características](docs/caracteristicas.md)).*

---

## 💻 Resumen del Stack Tecnológico

El sistema aprovecha al máximo el rendimiento nativo del kernel de Debian 13 en lugar de envolver todo en contenedores lentos:

- **Almacenamiento Inteligente:** `ext4` transaccional anti-apagón para NAS y `Btrfs` con compresión `zstd:3` + scrub mensual para respaldos.
- **Samba 4 con VFS Avanzado:** Módulos `acl_xattr`, `streams_xattr` y `full_audit` para emular ADS de Windows, permitir concurrencia masiva en Office y registrar operaciones.
- **Descubrimiento Moderno WSDD2:** Los clientes Windows 10/11 ven el NAS al instante en el Explorador de Red sin protocolos antiguos inseguros.
- **Panel Web 100% Offline (Rust):** `nas-web.service` (axum) servido por Nginx con `proxy_pass` (~0 MB en reposo), assets embebidos y base SQLite WAL (`/var/lib/nas/nas.sqlite`).

*(Consulta el inventario componente por componente en [Tecnologías Usadas](docs/tecnologias.md)).*

---

## 🚀 Instalación y Puesta en Marcha

### Requisitos Previos
- Servidor o equipo con **Debian 13 (Trixie)** x86_64 recién instalado.
- Acceso con privilegios de `root` o usuario en el grupo `sudo`.
- Conexión a Internet durante la descarga inicial de paquetes.
- Disco secundario opcional para `/srv/nas` (puedes formatearlo o reutilizarlo con sus datos intactos usando `--keep-data`).

### Opción A: Instalador Remoto Oficial (Recomendado)

En la consola de tu servidor ejecuta:

```bash
curl -fsSL https://raw.githubusercontent.com/ftole/nas_debian/main/install.sh | sudo bash
```

*(Si utilizas `wget`: `wget -qO- https://raw.githubusercontent.com/ftole/nas_debian/main/install.sh | sudo bash`)*

> [!TIP]
> El instalador descarga los archivos en `/opt/nas_debian`, registra el comando global `nas` y abre el asistente visual en consola. **No tocará tus discos ni particiones hasta que lo confirmes expresamente.**

### Opción B: Preparación Manual Paso a Paso

Si por políticas corporativas de auditoría prefieres auditar y ejecutar cada paso manualmente (actualización de repositorios, cortafuegos UFW, endurecimiento SSH, fail2ban y despliegue), consulta nuestra [Guía de Preparación Manual Paso a Paso](docs/operacion_mantenimiento.md#7-preparación-y-despliegue-manual-paso-a-paso).

---

## 🛠️ Comandos Globales del CLI `nas`

Una vez instalado, tienes a tu disposición el binario unificado `nas` para administración en consola o scripts programáticos, con salida enriquecida ANSI y formato estructurado `--json`:

### Comandos de Diagnóstico y Estado
| Comando | Función |
| :--- | :--- |
| `sudo nas status` | Métricas generales del sistema (CPU, memoria, carga, uptime e IP). |
| `sudo nas network` | Inventario de red (interfaces, gateway, DNS, puertos y resolución SMB). |
| `sudo nas services` | Estado en tiempo real de los demonios clave (`smbd`, `nmbd`, `wsdd2`, `nginx`). |
| `sudo nas diag` | Diagnóstico integral con código de salida significativo y resolución de incidentes. |
| `sudo nas version` | Muestra la versión del workspace y commit de compilación. |

### Subcomandos de Operación y Administración
| Módulo / Subcomando | Descripción de Operaciones |
| :--- | :--- |
| `sudo nas backup <list>` | Lista e inspecciona las tareas de respaldo y runners instalados. |
| `sudo nas storage list` | Inventario de almacenamiento con jerarquía de particiones y protección de SO. |
| `sudo nas users list` | Listado estructurado de usuarios y grupos corporativos locales. |
| `sudo nas audit <verify\|show>` | Verificación criptográfica y visualización de la bitácora inmutable de auditoría. |
| `sudo nas deploy <plan\|apply>` | Planificación y aplicación nativa del despliegue en disco (con guardas anti-so y fstab). |
| `sudo nas update <status\|verify\|apply>` | Verificación y aplicación de actualizaciones firmadas con validación SHA-256 / Ed25519. |
| `sudo nas migrate status` | Verificación del estado de retiro del stack anterior y paridad activa. |
| `sudo nas uninstall [--scope os]` | Desinstalación granular y limpia del panel o ajustes del sistema operativo. |
| `sudo nas helper <helper>` | Invocación de helpers root acotados (`storage-apply`, `samba-apply`, `accounts`, etc.). |

> [!TIP]
> Todos los subcomandos aceptan la bandera opcional `--json` para emitir la respuesta en formato JSON estructurado, ideal para pipelines CI/CD, scripts de monitoreo y automatización.

---

## 🖥️ Asistente de Terminal Interactivo (`nas-tui`)

Para administración visual interactiva desde la consola o una sesión SSH, el sistema incluye **`nas-tui`**, desarrollado en Rust con la librería `ratatui`:

```bash
sudo nas-tui
```

El TUI ofrece:
- **Dashboard Principal:** Métricas de CPU, memoria, almacenamiento y estado de la red en tiempo real.
- **Gestión de Servicios:** Monitoreo y control directo de demonios (`smbd`, `nmbd`, `wsdd2`, `nginx`).
- **Tareas de Respaldo:** Supervisión de estado, últimas ejecuciones y acceso rápido a logs.
- **Herramientas de Diagnóstico:** Comprobaciones de salud automáticas con recomendaciones inmediatas.

---

## 💾 Despliegue Automatizado por Línea de Comandos

Si necesitas desplegar servidores de forma desatendida mediante scripts:

```bash
# Servidor NAS en disco secundario (/dev/sdb) formateando desde cero:
printf '%s\n' '<CLAVE_ADMIN>' | sudo bash src/core/deploy.sh /dev/sdb WORKGROUP SRV-NAS admin - ARCHIVOS

# Servidor NAS conservando datos preexistentes en /dev/sdb (--keep-data):
printf '%s\n' '<CLAVE_ADMIN>' | sudo bash src/core/deploy.sh /dev/sdb WORKGROUP SRV-NAS admin - ARCHIVOS --keep-data

# Central de Respaldos formateando disco secundario (/dev/sdb):
printf '%s\n' '<CLAVE_ADMIN>' | sudo bash src/core/deploy.sh /dev/sdb WORKGROUP SRV-BKP admin - BACKUP

# Servidor Híbrido (Archivos y Respaldos combinados) en partición local o disco secundario:
printf '%s\n' '<CLAVE_ADMIN>' | sudo bash src/core/deploy.sh LOCAL WORKGROUP SRV-NAS admin - ARCHIVOS_BACKUP
```

---

## 🪟 Administración Remota desde Windows

La plataforma activa se administra con `nasctl` (inventario multi-host) y `nas`
sobre OpenSSH, o con el panel web:

```powershell
# Estado remoto por inventario (inventory.example.yaml)
nasctl --inventory inventory.yaml status

# Ejecución de un subcomando en todos los hosts del perfil
nasctl --inventory inventory.yaml exec -- nas status

# Panel web
start https://10.10.1.222/
```

Los antiguos `nas_admin.bat` / `nas_admin.py` fueron **retirados en la fase F7** y
solo se conservan en el historial de Git; su funcionalidad queda cubierta por
`nasctl` + `nas` y por el panel web.


---

## 🧪 Pruebas y Verificación

```bash
# Workspace Rust
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings

# Contrato de paridad del panel (IDs, clases, endpoints y hashes de assets)
python3 scripts/parity_extract.py --check

# Auditoría de interfaz (estática; en vivo requiere credenciales)
python3 scripts/qa_ui_audit.py --static

# Empaquetado (systemd, nginx, instalador y sudoers)
bash packaging/verify.sh
```

Las suites remotas interactivas (`test_remote.py`, `test_remote.sh`) y el
asistente `nas_admin.*` fueron **retirados en F7**; su cobertura se trasladó a
los tests Rust, al contrato de paridad y a `nasctl`.

## 📂 Estructura del Repositorio

```text
nas_debian/
├── install.sh             -> Instalador oficial (toolchain Rust + build + packaging)
├── Cargo.toml             -> Workspace Rust
├── crates/
│   ├── nas-core/          -> Configuración, rutas, errores y contrato
│   ├── nas-exec/          -> Ejecución segura (argv, sudo, timeout, redacción)
│   ├── nas-engine/        -> Almacenamiento, Samba, respaldos, dominio, diagnósticos
│   ├── nas-cli/           -> Comando `nas` (ANSI y --json)
│   ├── nas-web/           -> Panel axum + plantillas portadas + assets embebidos
│   ├── nas-tui/           -> Asistente de terminal (ratatui)
│   └── nasctl/, nas-dev/  -> Multi-host e inspección de desarrollo
├── packaging/             -> Unidad systemd, nginx, helpers privilegiados e instalador
├── scripts/               -> Contrato de paridad, auditoría UI y retiro legacy
├── src/core/, src/lib/    -> Motor Bash de despliegue/desinstalación (ola B pendiente)
├── tests/parity/          -> Contrato congelado (IDs, clases, endpoints y hashes)
├── legacy/                -> Panel PHP, whiptail, CLI PHP y utilidades Python retiradas
└── docs/                  -> Documentación técnica y operativa
```

## ⚖️ Licencia

Este proyecto es propiedad exclusiva de su autor. Todos los derechos reservados. Consulta el archivo [LICENSE](LICENSE).
