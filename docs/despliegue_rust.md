# Despliegue del panel NAS en Rust

> Guía de instalación, verificación y rollback de la plataforma Rust
> (`nas`, `nas-web`, `nas-tui`). El panel PHP histórico ya fue retirado
> (fase F7) y se recupera desde la etiqueta `legacy-php-final` si hace falta comparar.

## 1. Requisitos

- Debian 13 (Trixie), `nginx`, `systemd`, `samba` y `openssl`.
- Binarios compilados con `cargo build --release --workspace` (musl o gnu).
- Puerto 8080 libre en `127.0.0.1` (el backend escucha solo en localhost).

## 2. Construcción

```bash
cargo build --release --workspace
# opcional: binarios estáticos
cargo build --release --workspace --target x86_64-unknown-linux-musl
```

Artefactos: `target/release/{nas,nas-web,nas-tui,nasctl,nas-dev}`.

## 3. Instalación

```bash
sudo bash install.sh                          # clona/actualiza, compila e instala
sudo bash packaging/install.sh                # reinstala servicio, nginx y helpers
sudo bash packaging/install.sh --no-nginx     # solo servicio, helpers y sudoers
```

`install.sh` (raíz del repo) es el punto de entrada oficial:

1. Instala dependencias de compilación y la toolchain estable de Rust si falta.
2. Clona o actualiza `/opt/nas_debian` (política opcional de tags firmadas).
3. Ejecuta `cargo build --release --locked --workspace`.
4. Copia `nas`, `nas-web`, `nas-tui`, `nasctl` y `nas-dev` a `/usr/local/bin`.
5. Delega en `packaging/install.sh`, que:
   - crea el usuario de servicio `nas-web` y `/var/lib/nas` (0750);
   - genera `/etc/nas-web.env` (0600) con secreto aleatorio y hash SHA-256;
   - instala y habilita `nas-web.service` endurecido;
   - instala los helpers `nas-samba-apply`, `nas-share-dir`, `nas-share-acl`,
     `nas-accounts` y `nas-backup-manage`;
   - escribe `/etc/sudoers.d/nas-web-rust` (validado con `visudo -cf`);
   - configura Nginx con **`proxy_pass` a 127.0.0.1:8080**, TLS, CSP y 512 MB
     de subida, respaldando la configuración previa.

## 4. Variables de entorno (`/etc/nas-web.env`)

| Variable | Uso |
| :--- | :--- |
| `NAS_WEB_BIND` | Dirección de escucha (por defecto `127.0.0.1:8080`). |
| `NAS_WEB_USER` | Usuario administrador inicial. |
| `NAS_WEB_SECRET` | Secreto de sesión (≥16 caracteres). |
| `NAS_WEB_PASSWORD_SHA256` | Hash SHA-256 de la contraseña. |
| `NAS_WEB_TTL_SECS` | Duración de la sesión. |
| `NAS_WEB_INSECURE_COOKIE=1` | Solo para HTTP local de pruebas. |
| `NAS_STATE_DIR` / `NAS_DATA_ROOT` | Estado SQLite y almacenamiento. |
| `NAS_SAMBA_CONF` / `NAS_ROLE_FILE` | Configuración de Samba y rol. |
| `NAS_BACKUP_DIR` / `NAS_INVENTORY` | Runners de respaldo e inventario. |

## 5. Cutover A/B

```text
/opt/nas_debian/          ← fuente clonada (usada por updater.sh)
/usr/local/bin/nas-web     ← binario activo
/etc/systemd/system/nas-web.service
/etc/nginx/sites-available/nas-web
```

1. Publicar la release Rust en `releases/<versión>` (tarball verificado con
   `nas update verify` y firma Ed25519).
2. `ln -sfn releases/<versión> current.new && mv -T current.new current`.
3. `nas-web.service` usa binarios de `/usr/local/bin` (independientes de la
   release), por lo que el cambio de panel se hace con Nginx:
   - **Rust**: `proxy_pass http://127.0.0.1:8080` (`packaging/nginx-nas-web.conf`).
   - **PHP (retirado)**: `fastcgi_pass` ya no se usa; solo existe en los respaldos de rollback.
4. `nginx -t && systemctl reload nginx`.

## 6. Rollback del retiro (solo emergencia)

El retiro de F7 conserva todo lo necesario en `/root/nas-deploy-backup/retire-<fecha>/`:

```bash
BK=/root/nas-deploy-backup/retire-<fecha>
sudo tar -xzf "$BK/www-nas-web.tar.gz" -C /var/www
sudo install -m 0440 "$BK/sudoers-nas-web" /etc/sudoers.d/nas-web
sudo install -m 0644 "$BK/nas-web.conf" /etc/php/8.4/fpm/pool.d/nas-web.conf
sudo systemctl reload php8.4-fpm
# restaurar el sitio nginx con fastcgi desde nginx-before-cutover-*.conf
```

Los datos son compatibles: `nas-web` usa el mismo `/var/lib/nas/nas.sqlite`
y no borra nada del stack anterior.

## 7. Checklist de verificación tras el cutover

| Comprobación | Comando |
| :--- | :--- |
| Servicio activo | `systemctl is-active nas-web` |
| Salud del backend | `curl -fsS http://127.0.0.1:8080/healthz` |
| Login y sesión | `curl -k -i -X POST -d 'username=admin&password=…' https://<host>/login` |
| API | `curl -k -H 'Cookie: nas_session=…' https://<host>/api/metrics` |
| Vistas (14) | `/dashboard`, `/files`, `/shares`, `/users`, `/backups`, `/storage`, `/terminal`, `/logs`, `/domain`, `/services`, `/networking`, `/applications`, `/updates`, `/diagnostics` |
| Respaldos | `nas backup list` y `nas backup run <tarea> --dry-run` |
| Auditoría | `nas audit verify` y `https://<host>/audit` |
| Rollback probado | restaurar sitio PHP y validar `/login` |

## 8. Endurecimiento aplicado y pendientes

`nas-web.service` corre con `ProtectSystem=yes`, `PrivateTmp`, `NoNewPrivileges=no`
(necesario para `sudo -n` de los helpers), `CapabilityBoundingSet` acotado y
`ReadWritePaths` restringido a estado, datos, helpers y configuración.

Mejoras pendientes (posteriores a F7):

- Sustituir las llamadas `systemctl` directas restantes por acciones del futuro
  `nas-agent`, lo que permitiría `NoNewPrivileges=yes`.
- Portar `deploy.sh`/`uninstall.sh` (ola B) para que el aprovisionamiento sea
  también nativo en Rust.
