# Migración a Rust — estado y guía

> Documento vivo del traspaso de la plataforma NAS Debian 13 a Rust.
> Cada fase deja el sistema anterior operativo: la migración es incremental y
> verificable, nunca un corte a ciegas.

## 1. Estado actual

| Fase | Alcance | Estado |
| :--- | :--- | :--- |
| **F0** | Contratos, workspace, CI de Rust, pruebas de la CLI | ✅ Completado |
| **F1** | `nas-core`, `nas-exec`, `nas-engine` (parsers, estado, red, respaldos), `nas-audit` | ✅ Completado |
| **F1b** | `nas-inventory` (multi-servidor), `nas-ssh` (OpenSSH), `nas-dev sync` real, `doctor`, `onboard`, `nasctl` | ✅ Completado |
| **F2** | Respaldos, actualizaciones firmadas, despliegue, almacenamiento y usuarios | ✅ CLI completa (deploy/update con guardas) |
| **F3** | `nas-web` (axum) con API completa y **vistas HTML** del panel | 🟡 Rebanada 8 |
| **F4** | TUI (`ratatui`) y paridad total de la CLI PHP | ⏳ Pendiente |
| **F5** | `nasctl` con ejecución remota sobre releases firmados | ⏳ Pendiente |
| **F6** | Empaquetado `.deb`, unidades systemd, cutover y rollback | ⏳ Pendiente |
| **F7** | Retirada de PHP/Bash/Python y limpieza del árbol | ⏳ Pendiente |

Mientras no se complete F7, **el sistema en producción sigue siendo el actual**
(PHP + Bash + Python). El workspace Rust convive en `crates/` sin interferir.

## 2. Estructura del workspace

```text
crates/
├── nas-core/       tipos, errores, contrato JSON, rol del servidor y rutas
├── nas-exec/       ejecución segura (argv, sudo, timeout) y redacción de secretos
├── nas-inventory/  inventario multi-servidor: validación y perfiles dev/prod
├── nas-ssh/        transporte OpenSSH, cotización de argumentos y host keys
├── nas-engine/     parsers del SO, métricas, red, respaldos y tareas
├── nas-audit/      bitácora JSONL con encadenamiento de hashes
├── nas-cli/        binario `nas`     (estado, red, servicios, diagnóstico, auditoría)
├── nas-dev/        binario `nas-dev` (preflight, plan, pack, sync sin push)
└── nasctl/         binario `nasctl`  (inventario y perfiles de producción)
```

## 3. Compilación y pruebas

Requisitos: Rust ≥ 1.80 (probado con 1.99) y un enlazador del sistema.

```bash
# Compilar todo el workspace
cargo build --workspace

# Ejecutar la batería de pruebas (unitarias + integración de la CLI)
cargo test --workspace

# Linted estricto (lo mismo que exige el CI)
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
```

Binarios de desarrollo:

```bash
cargo run -p nas-cli -- status
cargo run -p nas-cli -- --json network
cargo run -p nas-dev -- --source . preflight
cargo run -p nas-dev -- --inventory inventory.example.yaml doctor --host lab-dev
cargo run -p nas-dev -- --inventory inventory.example.yaml onboard --host lab-dev
cargo run -p nas-dev -- --inventory inventory.example.yaml sync --host lab-dev --dry-run
cargo run -p nasctl -- --inventory inventory.example.yaml check
cargo run -p nas-cli -- backup list --dir /usr/local/bin
cargo run -p nas-cli -- backup plan contabilidad --dir /usr/local/bin
cargo run -p nas-cli -- backup run contabilidad --dry-run --dir /usr/local/bin
cargo run -p nas-cli -- update verify manifest.json --pubkey <hex>
cargo run -p nas-cli -- update plan manifest.json --artifact nas.tar.gz --previous 1.2.2
cargo run -p nas-cli -- deploy plan --disk /dev/sdb --workgroup TEAM-JOFRATO   --netbios SRV-NAS --admin admin --role BACKUP --ssd
```

> **Nota para Windows:** el toolchain `x86_64-pc-windows-msvc` requiere Visual
> Studio Build Tools. Sin ellos, use el toolchain GNU
> (`rustup toolchain install stable-x86_64-pc-windows-gnu`) con un compilador
> mingw-w64 en el `PATH`, o compile dentro de WSL/Debian.

## 4. Contrato de salida

Todas las salidas `--json` mantienen la envoltura del panel PHP:

```json
{ "success": true, "message": "Métricas del sistema", "data": { "...": "..." } }
```

```json
{ "success": false, "message": "…", "error": "…" }
```

Códigos de salida (`nas_core::NasError::exit_code`):

| Código | Significado |
| :--- | :--- |
| 0 | Éxito |
| 1 | Error de E/S o de ejecución |
| 2 | Valor inválido |
| 3 | Operación denegada por política |
| 4 | Recurso o dependencia ausente |
| 5 | Timeout de comando |

## 5. Sincronización de desarrollo (`nas-dev sync`)

Flujo implementado, **sin publicar en el repositorio remoto**:

1. **Guarda de entorno**: si el host no es `dev`, aborta con código 3.
2. **Preflight**: `git status`, `node --check` de los JS y `bash -n` de los scripts.
3. **Pack**: `tar.gz` con exclusiones (`.git`, `target`, `scratch`, `.venv`…) y
   **manifiesto SHA-256** que nombra el paquete.
4. **Upload**: `scp` con `BatchMode=yes`, `accept-new` y `known_hosts.d/<alias>`.
5. **Stage**: extracción en `/var/lib/nas-dev/dev-sync/stage/<sha>`.
6. **Snapshot**: respaldo del docroot actual en `.../backups/<timestamp>.tar.gz`.
7. **Switch**: `rsync -a --delete --exclude=data/` hacia el docroot de desarrollo.
8. **Reload**: `systemctl reload nginx` y `php8.4-fpm` (tolerante a fallos).
9. **Health**: `curl` a `https://127.0.0.1/login` (200/302).
10. **Rollback automático**: si el healthcheck falla, restaura el snapshot y
    devuelve error con auditoría.

Confirmaciones y seguridad:

- `--dry-run` imprime el plan completo (con prefijo `sudo -n --`) sin tocar nada.
- La ejecución real exige `--yes`.
- `--no-sudo` permite operar sin elevación (cuando el usuario es propietario).
- Cada ejecución se registra en `~/.local/state/nas-dev/audit.jsonl`
  (`%LOCALAPPDATA%\nas-dev\audit.jsonl` en Windows) con cadena de hashes.

### Alta y diagnóstico del host de desarrollo

- `nas-dev onboard --host <alias>` imprime el procedimiento completo (usuario,
  `authorized_keys`, `sudoers` acotado con `visudo -c`, docroot y estado
  aislados). **No ejecuta nada**: el operador revisa y aplica.
- `nas-dev doctor --host <alias>` comprueba por SSH los requisitos reales:
  usuario, `sudo -n`, `rsync`, `tar`, `curl`, docroot y su escritura, estado,
  `nginx -t` y `nginx` activo. Devuelve código 1 si algo falla y muestra la
  acción correctiva de cada fallo.
- Ambas órdenes rechazan hosts de producción con código 3.

> Endurecimiento pendiente (F2): sustituir el `sudoers` por un helper
> `/usr/local/sbin/nas-dev-apply` con acciones fijas, igual que el patrón
> `nas-terminal`, para no conceder `rm`/`mv` arbitrarios al usuario de desarrollo.

## 6. Actualizaciones firmadas (`nas update`)

- `nas update apply <manifiesto> --artifact <tgz> --pubkey <hex> --yes` aplica
  la release tras verificar firma y hash, con rollback automático.
- `nas update verify <manifiesto> --pubkey <hex>` comprueba **SHA-256 del
  artefacto** y **firma Ed25519** sobre la carga canónica
  `version|commit|artifact|sha256|published_at`. Devuelve código 3 si algo falla.
- `nas update plan <manifiesto> --artifact <tgz> [--previous <ver>]` genera el
  plan A/B: extracción en `releases/<version>`, `nginx -t`, cambio atómico del
  enlace `current` (`ln -sfn` + `mv -T`), recarga de servicios y healthcheck,
  con **rollback** a `releases/<anterior>` si se declara.
- La ejecución real sigue delegada en `updater.sh` (tags GPG) hasta F3; el
  verificador Rust permite validar releases antes de aplicarlas.

## 7. Despliegue (`nas deploy`)

- `nas deploy plan` valida disco, NetBIOS, workgroup, usuario y rol, deriva el
  sistema de archivos (ext4 para `ARCHIVOS`, Btrfs con zstd para `BACKUP` e
  híbrido), calcula la partición (`sdb1` vs `nvme0n1p1`) y las opciones de
  montaje según HDD/SSD.
- Emite el plan de referencia y la invocación equivalente a `deploy.sh`.
- `nas deploy apply` ejecuta el plan con guardas: `--format-token SI-FORMATEAR`
  para formatear, detección de disco en uso con `findmnt -R`, `--ignore-in-use`
  explícito, `--yes`, bloqueo `/var/lock/nas-deploy.lock` y auditoría.
- `nas update apply` verifica SHA-256 + firma antes de tocar nada, aplica el
  swap A/B, valida el healthcheck y **revierte automáticamente** si falla.

## 8. Tareas de respaldo (`nas backup`)

`nas-engine::tasks` lee los runners generados por el asistente Bash
(`/usr/local/bin/backup_<tarea>.sh`) sin modificarlos, detecta el tipo de origen
(CIFS, SSH o local), normaliza la retención y **genera el plan de comandos**
con argumentos separados:

- **CIFS**: `mkdir` → `mount -t cifs ... -o credentials=...,ro,vers=3.1.1,noserverino,cache=none,soft,nobrl` → `rsync -aAXH --numeric-ids` → `umount` → `mv` atómico.
- **SSH**: `sshpass -e rsync ... -e 'ssh -p <puerto> -o BatchMode=yes ...'` (la contraseña viaja por variable de entorno, nunca en `argv`).
- **Local**: `rsync` con `--link-dest` cuando existe snapshot previo.

El plan es puramente determinista y está cubierto por pruebas.

### Ejecución (`nas backup run`)

`nas backup run <tarea>` aplica las mismas guardas que los runners Bash, pero
desde un binario tipado:

1. **Validación** de la tarea (origen, credenciales, retención).
2. **Control de espacio** con `df -Pk` y `evaluate_space`: aborta si queda menos
   de 2 GiB o la ocupación supera el 95 % (82–95 % sólo advierte).
3. **Bloqueo exclusivo** por tarea (`/var/lock/backup_<id>.lock`) con detección
   de bloqueos huérfanos; se libera automáticamente al salir del proceso.
4. **Ejecución** de los pasos con timeouts (120 s montajes, 3600 s rsync) y
   limpieza de montaje (`umount`) si algo falla.
5. **Retención** calculada sobre los snapshots existentes + el nuevo, eliminando
   únicamente los más antiguos.
6. **Bitácora** en `<log_dir>/backup_<id>.log` y **auditoría encadenada** con los
   eventos `START`, `SUCCESS` o `FAILED`.

`--dry-run` muestra plan, decisión de espacio y retención sin tocar el sistema;
la ejecución real exige `--yes` y devuelve código 3 si falta la confirmación.

## 9. Panel web (`nas-web`)

Primera rebanada vertical del sustituto de PHP-FPM:

- **Rutas**: `GET /healthz`, `GET/POST /login`, `POST /logout`, `GET /`,
  `GET /api/metrics`, `/api/network`, `/api/shares`, `/api/users`,
  `/api/services` (+ `manage`), `/api/diagnostics`, `/api/backups` (+ `plan`),
  `/api/audit`, el explorador `/api/files` (`download`, `mkdir`, `rename`,
  `delete`, `upload`), `/api/storage` (+ `disks`, `inspect`), `/api/domain`
  (+ `join`, `leave`) y `/api/terminal` (`session`, `send`, `capture`,
  `resize`, `kill`).
- **Sesiones persistentes en SQLite** (`web_sessions`): la cookie transporta un
  identificador aleatorio de 256 bits; expiración en base de datos con purga,
  `HttpOnly`, `SameSite=Strict` y `Secure` por defecto.
- **Usuarios web en SQLite** (`web_users`) con rol; alta inicial del
  administrador desde el entorno y comparación de SHA-256 en tiempo constante.
- **Recursos** (`/api/shares`) parseados de `smb.conf` y filtrados por rol
  (`BACKUP` recibe 403). **Usuarios** (`/api/users`) desde `/etc/passwd` y
  `/etc/group`.
- **Cabeceras**: CSP, `X-Frame-Options`, `X-Content-Type-Options`,
  `Referrer-Policy` y `Permissions-Policy` inyectadas en cada respuesta.
- **Contrato JSON**: idéntico al panel PHP (`success`/`message`/`data`).
- **Configuración**: `NAS_WEB_BIND`, `NAS_WEB_USER`, `NAS_WEB_SECRET` (≥16),
  `NAS_WEB_PASSWORD_SHA256` (o `NAS_WEB_DEV_PASSWORD` en desarrollo),
  `NAS_WEB_INSECURE_COOKIE=1` para HTTP local, `NAS_WEB_TTL_SECS`.

```bash
cargo run -p nas-web --bin nas-web
# o con credenciales de desarrollo:
NAS_WEB_SECRET=0123456789abcdef NAS_WEB_DEV_PASSWORD=Secreta123 NAS_WEB_INSECURE_COOKIE=1 NAS_WEB_BIND=127.0.0.1:8080 cargo run -p nas-web
```

### Explorador de archivos (`nas-web::files`)

- Confinamiento estricto a `NasPaths::data_root`: se rechazan rutas absolutas,
  `..` y cualquier resolución que escape de la raíz (incluidos symlinks).
- `list` ordena carpetas primero; `mkdir`, `rename` y `delete` (papelera
  interna `.trash/<fecha>_<nombre>`).
- `upload` por **streaming** con límite de 512 MB, nombre saneado, escritura en
  temporal y promoción atómica; 409 si el archivo ya existe.
- `download` con `Content-Disposition: attachment` y CSP `default-src 'none'; sandbox`.

### Servicios, diagnóstico, respaldos y auditoría

- `GET /api/services` consulta `systemctl is-active` de la lista blanca;
  `POST /api/services/manage` exige rol `admin`/`superadmin`, valida
  servicio/acción contra lista blanca y registra el intento en auditoría.
- `GET /api/diagnostics` devuelve comprobaciones tipadas (rol, Samba,
  almacenamiento, estado, SQLite, demonios y red).
- `GET /api/backups` descubre los runners `backup_*.sh`; `POST /api/backups/plan`
  devuelve el plan con `--link-dest` cuando hay snapshot previo y
  `POST /api/backups/run` ejecuta con las **mismas guardas que la CLI**
  (`nas_engine::tasks::run`: validación, espacio, bloqueo, retención, bitácora),
  exigiendo `confirm: true` y rol administrador.
- **Auditoría web en SQLite** (`web_audit`): logins (éxito/fallo), gestión de
  servicios, borrados y cargas; consulta en `/api/audit` (solo administradores).

### Almacenamiento y dominio

- `GET /api/storage` agrega `df -Pk` + `findmnt` + `lsblk` (uso, dispositivo,
  Btrfs y soporte TRIM según ROTA); `GET /api/storage/disks` marca el disco del
  SO y su estado de uso; `POST /api/storage/inspect` valida `/dev/...` antes de
  consultar `lsblk`.
- `GET /api/domain` interpreta `realm list` y el estado de `sssd`;
  `POST /api/domain/join` y `/leave` exigen administrador, validan dominio y
  usuario, envían la contraseña por `stdin` y registran auditoría.

### Terminal web (`nas-web::terminal`)

- Sesiones **tmux** por usuario (`nas-web-<usuario>`, socket `-L nas`) ejecutadas
  como el usuario autenticado mediante `sudo -n -u`, igual que el panel PHP.
- Validación estricta: nombre de sesión, geometría (20-500 × 5-300), entrada
  ≤ 4096 caracteres sin NUL y terminador `--` en `send-keys`.
- Endpoints `POST /api/terminal/{session,send,resize,kill}` y
  `GET /api/terminal/capture`, **reservados al superadministrador** y con
  auditoría de apertura y cierre.

### Vistas HTML (`nas-web::views`)

- Layout Slate UI propio (CSS embebido, cero dependencias) con barra lateral de
  8 secciones, cabecera con rol y cierre de sesión.
- Páginas: `/dashboard`, `/shares`, `/users`, `/backups`, `/services`,
  `/diagnostics`, `/storage`, `/files` y `/domain`, renderizadas en servidor con
  datos reales y acciones vía `fetch` (plan/ejecutar respaldo, reiniciar
  servicio, crear carpeta, subir y borrar archivos, unir al dominio).
- `/` redirige a `/dashboard`; sin sesión todo redirige a `/login`.

Pendiente de F3: vista de terminal (hoy sólo API), vista de auditoría y ZIP de
descargas.

## 10. Almacenamiento y usuarios (`nas storage` / `nas users`)

- `nas storage list` usa `lsblk -J` y el parser tipado (tipo, tamaño, fstype,
  montaje, HDD/SSD).
- `nas users list` lee `/etc/passwd` y `/etc/group` y separa cuentas de sistema
  de las administrables, listando además los grupos `grp_*`.

## 11. Mapa de migración

| Origen actual | Destino Rust | Estado |
| :--- | :--- | :--- |
| `bin/nas-cli` (PHP) | `crates/nas-cli` | 🟡 `status`, `network`, `services`, `diag`, `audit verify` |
| `src/lib/helpers.sh` (detección de entorno) | `nas-core::config` | ✅ rol, workgroup y rutas |
| Parsers repartidos en PHP/Bash/Python | `nas-engine::parsers` | ✅ `smb.conf`, `testparm`, `lsblk -J`, `ip -j`, `ss`, `btrfs scrub`, `resolv.conf` |
| `getSystemMetrics` (PHP) | `nas-engine::status` | ✅ |
| Vista de red (PHP + JS) | `nas-engine::network` | ✅ |
| Política de snapshots/retención/espacio (Bash) | `nas-engine::backup` | ✅ |
| Tareas de respaldo (`backup_*.sh` del asistente) | `nas-engine::tasks` | ✅ lectura, validación y plan |
| `test_remote.py` (SSH/SFTP) | `nas-ssh` + `nas-dev` | ✅ transporte OpenSSH, upload y sincronización |
| `nas_admin.py` (keyring, perfiles) | `nasctl` + `nas-inventory` | ✅ inventario y perfiles; claves vía `ssh-agent`/`-i` |
| `web/` (Nginx + PHP-FPM) | `nas-web` (axum) | ⏳ F3 |
| `src/asistente.sh` (whiptail) | `nas-tui` (ratatui) | ⏳ F4 |
| `src/core/deploy.sh`, `updater.sh`, `uninstall.sh` | `nas-engine::{deploy,update,uninstall}` | ⏳ F2 |

## 12. Seguridad implementada

- **Ejecución sin shell:** `nas-exec` construye `Command` con vectores de
  argumentos; nunca interpola cadenas.
- **SSH con cotización individual:** `nas-ssh::quote` aplica comillas POSIX a
  cada token antes de unirlos; el shell remoto no recibe texto libre.
- **Host keys dedicadas:** `~/.ssh/known_hosts.d/<alias>` con `accept-new` y
  rechazo ante cambio de clave por parte de OpenSSH.
- **Secretos fuera de `argv`:** viajan por `stdin` (`SecretInput`) y nunca se
  serializan (`Debug` enmascarado).
- **Redacción:** todo `stdout`/`stderr` capturado pasa por `Redactor`.
- **Timeouts con muerte del proceso**, `env_clear()` y `LC_ALL=C`.
- **Auditoría encadenada:** SHA-256 del registro anterior; `nas audit verify`
  detecta manipulación.
- **Separación dev/prod:** `NasPaths::development()` y la guarda de `nas-dev`
  impiden que una herramienta de desarrollo toque producción.
- **Lints:** `#![forbid(unsafe_code)]`, `clippy -D warnings` en CI.

## 13. Siguientes pasos

F2 (motor y CLI) queda cubierta. Lo pendiente es la capa de servicio:

1. **F3 — `nas-web` (resto)**: plantillas askama de las 14 vistas, SQLite de
   sesiones/auditoría, uploads, explorador, terminal WS y las rutas restantes.
2. **F4 — `nas-tui`**: ratatui con los 13 módulos del asistente.
3. **F5 — `nasctl` remoto**: ejecución sobre releases firmados por SSH.
4. **F6 — empaquetado**: `.deb`, unidades systemd endurecidas y cutover.
5. **F7 — retirada** de PHP/Bash/Python.
