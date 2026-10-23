# Migración a Rust — estado y guía

> Documento vivo del traspaso de la plataforma NAS Debian 13 a Rust.
> Cada fase deja el sistema anterior operativo: la migración es incremental y
> verificable, nunca un corte a ciegas.

## 1. Estado actual

| Fase | Alcance | Estado |
| :--- | :--- | :--- |
| **F0** | Contratos, workspace, CI de Rust, golden tests de la CLI | ✅ Completado |
| **F1** | `nas-core`, `nas-exec`, `nas-engine` (parsers, estado, red, respaldos), `nas-audit` | ✅ Completado |
| **F1b** | `nas` (CLI), `nas-dev` (preflight/plan/pack), `nasctl` (inventario multi-servidor) | ✅ Base funcional |
| **F2** | Motor completo: deploy, update firmado, backups, storage/LVM/Btrfs, usuarios/ACL, dominio | ⏳ Pendiente |
| **F3** | `nas-web` (axum + askama + sqlx) con paridad de rutas y plantillas | ⏳ Pendiente |
| **F4** | TUI (`ratatui`) y paridad total de la CLI PHP | ⏳ Pendiente |
| **F5** | `nas-dev sync` real y `nasctl` sobre releases firmados con SSH | ⏳ Pendiente |
| **F6** | Empaquetado `.deb`, unidades systemd, cutover y rollback | ⏳ Pendiente |
| **F7** | Retirada de PHP/Bash/Python y limpieza del árbol | ⏳ Pendiente |

Mientras no se complete F7, **el sistema en producción sigue siendo el actual**
(PHP + Bash + Python). El workspace Rust convive en `crates/` sin interferir.

## 2. Estructura del workspace

```text
crates/
├── nas-core/     tipos, errores, contrato JSON, rol del servidor y rutas
├── nas-exec/     ejecución segura (argv, sudo, timeout) y redacción de secretos
├── nas-engine/   parsers del SO, métricas, red, políticas de respaldo
├── nas-audit/    bitácora JSONL con encadenamiento de hashes
├── nas-cli/      binario `nas`   (estado, red, servicios, diagnóstico, auditoría)
├── nas-dev/      binario `nas-dev` (preflight, plan, pack, sync --dry-run)
└── nasctl/       binario `nasctl`  (inventario multi-servidor y perfil resuelto)
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
cargo run -p nas-dev -- --source . plan
cargo run -p nasctl -- --inventory inventory.example.yaml check
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

## 5. Mapa de migración

| Origen actual | Destino Rust | Estado |
| :--- | :--- | :--- |
| `bin/nas-cli` (PHP) | `crates/nas-cli` | 🟡 `status`, `network`, `services`, `diag`, `audit verify` |
| `src/lib/helpers.sh` (detección de entorno) | `nas-core::config` | ✅ rol, workgroup y rutas |
| Parsers repartidos en PHP/Bash/Python | `nas-engine::parsers` | ✅ `smb.conf`, `testparm`, `lsblk -J`, `ip -j`, `ss`, `btrfs scrub`, `resolv.conf` |
| `getSystemMetrics` (PHP) | `nas-engine::status` | ✅ |
| Vista de red (PHP + JS) | `nas-engine::network` | ✅ |
| Política de snapshots/retención/espacio (Bash) | `nas-engine::backup` | ✅ |
| `nas_admin.py` (keyring, perfiles) | `nasctl` | 🟡 inventario y perfiles; SSH en F5 |
| `test_remote.py` (SSH/SFTP) | `nas-ssh` (pendiente) | ⏳ F5 |
| `web/` (Nginx + PHP-FPM) | `nas-web` (axum) | ⏳ F3 |
| `src/asistente.sh` (whiptail) | `nas-tui` (ratatui) | ⏳ F4 |
| `src/core/deploy.sh`, `updater.sh`, `uninstall.sh` | `nas-engine::{deploy,update,uninstall}` | ⏳ F2 |

## 6. Seguridad implementada (F1)

- **Ejecución sin shell:** `nas-exec` construye `Command` con vectores de
  argumentos; nunca interpola cadenas.
- **Secretos fuera de `argv`:** viajan por `stdin` (`SecretInput`) y nunca se
  serializan (`Debug` enmascarado).
- **Redacción:** todo `stdout`/`stderr` capturado pasa por `Redactor`
  (secretos registrados + patrones `password=`, `token:`…).
- **Timeouts con muerte del proceso** y `env_clear()` con `PATH` y `LC_ALL`
  fijos para evitar influencias del entorno.
- **Auditoría encadenada:** `nas-audit` sella cada entrada con SHA-256 del
  registro anterior y `verify` detecta cualquier manipulación del histórico.
- **Rutas separadas dev/prod:** `NasPaths::development()` jamás apunta al
  docroot ni al repositorio de producción.
- **Lints:** `#![forbid(unsafe_code)]`, `clippy -D warnings` en CI.

## 7. Siguientes pasos (F2)

1. `nas-engine::deploy`: puerto de `deploy.sh` (roles, sistema de archivos,
   Samba, Nginx, hardening) con pruebas de paridad contra el script actual.
2. `nas-engine::update`: verificación de firma GPG (sequoia), swap A/B y
   rollback automático.
3. `nas-engine::backup`: pipeline `rsync --link-dest` con staging atómico,
   `flock` y retención (las funciones puras ya están probadas).
4. `nas-ssh` (russh): transporte, `known_hosts` por host y ejecución remota.
5. `nas-dev sync`: upload → stage → verify → snapshot → switch → health →
   rollback, reutilizando `nas-audit` para cada paso.
