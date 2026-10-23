# Migración a Rust — estado y guía

> Documento vivo del traspaso de la plataforma NAS Debian 13 a Rust.
> Cada fase deja el sistema anterior operativo: la migración es incremental y
> verificable, nunca un corte a ciegas.

## 1. Estado actual

| Fase | Alcance | Estado |
| :--- | :--- | :--- |
| **F0** | Contratos, workspace, CI de Rust, pruebas de la CLI | ✅ Completado |
| **F1** | `nas-core`, `nas-exec`, `nas-engine` (parsers, estado, red, respaldos), `nas-audit` | ✅ Completado |
| **F1b** | `nas-inventory` (multi-servidor), `nas-ssh` (OpenSSH), `nas-dev sync` real, `nasctl` | ✅ Completado |
| **F2** | Motor de respaldos (modelo, plan y ejecución con guardas), deploy/update/storage/usuarios | 🟡 Parcial: `nas backup list`, `plan` y `run` |
| **F3** | `nas-web` (axum + askama + sqlx) con paridad de rutas y plantillas | ⏳ Pendiente |
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
cargo run -p nas-dev -- --inventory inventory.example.yaml sync --host lab-dev --dry-run
cargo run -p nasctl -- --inventory inventory.example.yaml check
cargo run -p nas-cli -- backup list --dir /usr/local/bin
cargo run -p nas-cli -- backup plan contabilidad --dir /usr/local/bin
cargo run -p nas-cli -- backup run contabilidad --dry-run --dir /usr/local/bin
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

## 6. Tareas de respaldo (`nas backup`)

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

## 7. Mapa de migración

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

## 8. Seguridad implementada

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

## 9. Siguientes pasos (F2)

1. `nas-engine::deploy`: puerto de `deploy.sh` (roles, sistema de archivos,
   Samba, Nginx, hardening) con pruebas de paridad contra el script actual.
3. `nas-engine::update`: verificación de firma GPG (sequoia), swap A/B y
   rollback automático.
4. `nas-engine::backup`: pipeline `rsync --link-dest` con staging atómico,
   `flock` y retención (las funciones puras ya están probadas).
5. `nas-web` (F3): axum + askama + sqlx reutilizando `nas-engine` y el mismo
   contrato JSON, con paridad de HTML y cabeceras.
