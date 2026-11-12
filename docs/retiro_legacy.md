# Retiro del stack anterior (F7)

> El stack PHP/Bash/Python se retira **después** del cutover validado. Este
> documento define el orden, las guardas y qué se conserva.

## 1. Principio

El panel Rust (`nas-web`) y la CLI (`nas`) ya cubren la operación diaria, pero
`deploy.sh` (1.400 líneas) y `uninstall.sh` siguen siendo el mecanismo de
aprovisionamiento del sistema operativo. Retirarlos antes de portarlos dejaría
el proyecto sin capacidad de instalar un servidor desde cero.

Por eso F7 se ejecuta en **dos olas**:

| Ola | Alcance | Requisito |
| :--- | :--- | :--- |
| **A** | Panel PHP, CLI PHP (`bin/nas-cli`), asistente whiptail y asistentes Python | `nas-web` activo + respaldo de Nginx + `/etc/nas-web.env` |
| **B** | `deploy.sh`, `uninstall.sh`, `updater.sh` | Paridad de `nas deploy`/`nas update` ejecutando nativamente |

## 2. Guardas (verificadas por el script)

1. `systemctl is-active nas-web` → el panel Rust debe estar operativo.
2. Existe un respaldo `/etc/nginx/sites-available/nas-web.bak.*` (rollback).
3. Existe `/etc/nas-web.env` (configuración del panel).
4. `cargo test --workspace` verde y `nas audit verify` sin errores.

## 3. Script

```bash
bash scripts/retire-legacy.sh            # simulación (por defecto)
bash scripts/retire-legacy.sh --apply    # aplica los git mv / git rm
```

Fases que ejecuta:

1. `web/` → `legacy/web/` (histórico; eliminado del árbol en F7.2, recuperable desde `legacy-php-final`).
2. `bin/nas-cli`, `nas_admin.py`, `nas_admin.bat`, `nas-assistant.spec`,
   `test_remote.py`, `test_remote.sh` → `git rm`.
3. `src/asistente.sh` y `src/modules/` → `legacy/` (sustituidos por `nas-tui`).
4. Pruebas del stack anterior → `legacy/tests/`.

## 4. Lo que se conserva

- `src/core/deploy.sh`, `src/core/uninstall.sh`, `src/core/updater.sh` y
  `install.sh` (bootstrap del sistema operativo).
- Material histórico retirado: disponible mediante la etiqueta Git `legacy-php-final` (purgado del árbol activo en F7.2).

## 5. Pasos manuales posteriores

1. Ajustar `.github/workflows/ci.yml`: retirar los jobs de PHP y del stack
   Python migrado; conservar ShellCheck/BATS solo para el bootstrap.
2. Revisar `install.sh` y cualquier referencia a `src/asistente.sh`.
3. Regenerar `AGENTS.md` y `README.md` con la nueva estructura.
4. Commit **por archivo** (regla del proyecto) con mensajes `refactor:`/`chore:`.

## 6. Verificación final

```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
pytest tests/ -q
bash packaging/verify.sh
nas migrate status
```

`nas migrate status` muestra qué componentes legacy siguen presentes y cuál es su
sustituto en Rust, para decidir cuándo ejecutar la ola B.

## 6. Ola A ejecutada (2026-10-26)

Árbol de trabajo:

```text
web/                → legacy/web/
src/asistente.sh    → legacy/asistente.sh
src/modules/        → legacy/modules/
tests del stack PHP/Python/BATS → legacy/tests/
bin/nas-cli, nas_admin.*, nas-assistant.spec, test_remote.* → retirados
```

Assets del panel antes de mover `web/` para que el binario no dependa del
árbol legado:

```text
web/public/js/app.js                     → crates/nas-web/assets/js/app.js
web/public/js/vendor/*.js                → crates/nas-web/assets/js/vendor/
```

Los hashes de `tests/parity/assets.sha256` no cambiaron: los archivos son
idénticos byte a byte.

Servidor `<host>`:

```text
respaldo: /root/nas-deploy-backup/retire-<fecha>/
  sudoers-nas-web, nas-web.conf, www-nas-web.tar.gz, opt-nas_debian-src.tar.gz
retirado: /etc/php/8.4/fpm/pool.d/nas-web.conf
retirado: /etc/sudoers.d/nas-web
retirado: /var/www/nas-web
conservado: /opt/nas_debian (fuente y actualizador)
```

Verificación posterior:

```text
nas-web: active · nginx: active · smbd: active
healthz 200 · panel 200 · testparm OK · 4 tareas de respaldo intactas
```

### Rollback de la ola A

```bash
BK=/root/nas-deploy-backup/retire-<fecha>
tar -xzf "$BK/www-nas-web.tar.gz" -C /var/www
install -m 0440 "$BK/sudoers-nas-web" /etc/sudoers.d/nas-web
install -m 0644 "$BK/nas-web.conf" /etc/php/8.4/fpm/pool.d/nas-web.conf
systemctl reload php8.4-fpm
# restaurar el sitio nginx con fastcgi (respaldo nginx-before-cutover-*.conf)
```

### Integración continua tras el retiro

`.github/workflows/ci.yml` conserva ShellCheck y BATS del motor de despliegue,
valida la sintaxis del JavaScript empaquetado y ejecuta el contrato de paridad;
los jobs de PHP y pytest del stack retirado se eliminaron.

## 7. Criterios de aceptación de la ola B

Antes de retirar `src/core/deploy.sh`, `src/core/uninstall.sh`, `src/core/updater.sh`,
`src/lib` y los BATS asociados, deben cumplirse:

| Criterio | Evidencia esperada |
| :--- | :--- |
| `nas deploy` reproduce el despliegue base | Servidor limpio Debian 13 con `/srv/nas`, Samba, nginx, sysctl, auditd y AIDE |
| `nas deploy --keep-data` monta sin formatear | Disco con datos previos intacto y backup operativo |
| Protección del disco del SO y de discos en uso | Rechazo verificado con tests de inyección de fallos |
| `nas uninstall` deja el sistema en estado base | Ya implementado para la plataforma (servicio, binarios, helpers, sudoers, nginx, estado opcional). El desaprovisionamiento del SO sigue en `src/core/uninstall.sh` |
| `nas update` actualiza y reinicia sin downtime largo | Servicio activo y versión nueva verificada |
| Suite de paridad del motor Bash | BATS de `deploy`, `updater`, `failure_*` y `rotation` portados o sustituidos |
| Rollback documentado y probado | Restauración de configuración y datos sin pérdida |

Hasta entonces, el motor Bash permanece **vigente** y el retiro de la ola B no se
ejecuta.

## 8. Limpieza del árbol (fases 7.1 y 7.2)

- Etiqueta de referencia: `legacy-php-final`.
- Retirados: `legacy/tests/`, `requirements-*.txt`, `.env.example`,
  `docs/asistente_windows.md`, `.github/workflows/build-windows.yml` y el
  conversor `scripts/php2html.py` (archivado en `scripts/legacy/`).
- El contrato de paridad se desacopló del árbol PHP: las plantillas originales
  viven en `tests/parity/fixtures/` y los hashes se calculan sobre
  `crates/nas-web/assets/`.
- `legacy/web/` y el resto de `legacy/` eliminados del árbol (recuperables desde
  la etiqueta `legacy-php-final`).
- `scratch/`, `target/`, `.venv/`, `dist/` y `build/` quedan fuera de Git.

## 9. Avance de la ola B (fase 7.3)

Implementado y probado en Rust:

| Capacidad | Estado |
| :--- | :--- |
| Plan y ejecución de `nas deploy` (particionado, mkfs, montaje, grupos, `fstab` con UUID) | ✅ (secuenciación de artefactos previa a recargas del kernel) |
| Guarda estricta de protección del disco del SO (`is_os_disk`) | ✅ (bloquea formateo aun con flags forzados) |
| Artefactos del sistema: `/etc/nas/role`, sysctl, udev readahead, logrotate, cron de scrub, reglas auditd | ✅ (incluye regla auditd para `/usr/local/bin/nas`) |
| Bootstrap de instalación y certificados TLS para Nginx | ✅ (paquetes base Debian 13 y generación automática de certificado) |
| Hardening de privilegios en `/etc/sudoers.d/nas-web-rust` | ✅ (`systemd-run` restringido sin comodín libre) |
| `nas uninstall` del panel | ✅ |
| `nas uninstall --scope os` (ajustes de kernel/udev/rotación/auditoría) | ✅ plan y ejecución |
| `nas update` verify/apply/sign | ✅ |
| Samba (`smb.conf`, usuarios, dominio AD), nginx+TLS, AIDE y UFW integrados en un único flujo de primer arranque | ⏳ En consolidación |
| Validación en VM limpia extremo a extremo | ⏳ Pendiente (bloquea retirar `src/core`) |

Mientras esos dos últimos puntos no estén verdes, `src/core/{deploy,uninstall,updater}.sh`,
`src/lib/` y los BATS siguen vigentes: `nas deploy` complementa y aprovisiona con paridad
gradual hacia el reemplazo total del motor Bash.
