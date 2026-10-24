# Despliegue del panel NAS en Rust

> Guía de instalación, cutover y rollback del panel Rust (`nas-web`) frente al
> panel PHP histórico. El stack anterior se conserva intacto hasta completar F7.

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
sudo bash packaging/install.sh              # instala todo
sudo bash packaging/install.sh --no-nginx   # solo servicio y binarios
```

El instalador:

1. Copia los cinco binarios a `/usr/local/bin`.
2. Crea el usuario de servicio `nas-web` y `/var/lib/nas` (0750).
3. Genera `/etc/nas-web.env` (0600) con secreto aleatorio y hash SHA-256
   de la contraseña solicitada por consola.
4. Instala y habilita `nas-web.service` endurecido.
5. Instala `/etc/sudoers.d/nas-web` (validado con `visudo -c`) limitado a
   `systemctl`, `smbstatus`, `testparm`, `realm` y operaciones de staging.
6. Configura el host virtual de Nginx con **`proxy_pass` a 127.0.0.1:8080**,
   TLS, CSP y límite de subida de 512 MB (respaldando la configuración previa).

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
/opt/nas_debian/
├── releases/
│   ├── 0.1.0/        ← panel PHP actual
│   └── 0.2.0/        ← panel Rust
└── current -> releases/0.2.0
```

1. Publicar la release Rust en `releases/<versión>` (tarball verificado con
   `nas update verify` y firma Ed25519).
2. `ln -sfn releases/<versión> current.new && mv -T current.new current`.
3. `nas-web.service` usa binarios de `/usr/local/bin` (independientes de la
   release), por lo que el cambio de panel se hace con Nginx:
   - **Rust**: `proxy_pass http://127.0.0.1:8080` (`packaging/nginx-nas-web.conf`).
   - **PHP**: `fastcgi_pass unix:/run/php/php-fpm-nas.sock` (configuración previa).
4. `nginx -t && systemctl reload nginx`.

## 6. Rollback al panel PHP

```bash
# 1. Restaurar el host virtual respaldado por el instalador
sudo cp -a /etc/nginx/sites-available/nas-web.bak.<fecha> /etc/nginx/sites-available/nas-web
sudo nginx -t && sudo systemctl reload nginx

# 2. Detener el backend Rust (opcional, libera 8080)
sudo systemctl disable --now nas-web.service

# 3. Verificar el panel PHP
curl -k -I https://127.0.0.1/login
```

Los datos son compatibles: `nas-web` usa el mismo `/var/lib/nas/nas.sqlite`
(tablas `web_users`, `web_sessions`, `web_audit`) y no borra nada del stack PHP.

## 7. Checklist de verificación tras el cutover

| Comprobación | Comando |
| :--- | :--- |
| Servicio activo | `systemctl is-active nas-web` |
| Salud del backend | `curl -fsS http://127.0.0.1:8080/healthz` |
| Login y sesión | `curl -k -i -X POST -d 'username=admin&password=…' https://<host>/login` |
| API | `curl -k -H 'Cookie: nas_session=…' https://<host>/api/metrics` |
| Vistas | `https://<host>/dashboard`, `/shares`, `/files`, `/backups`, `/terminal` |
| Respaldos | `nas backup list` y `nas backup run <tarea> --dry-run` |
| Auditoría | `nas audit verify` y `https://<host>/audit` |
| Rollback probado | restaurar sitio PHP y validar `/login` |

## 8. Endurecimiento pendiente

La unidad usa `NoNewPrivileges=no` porque el panel invoca `sudo -n` para
`systemctl`, `realm` y `smbstatus`. Para activar `NoNewPrivileges=yes` hay que
sustituir esas llamadas por un helper privilegiado (`nas-agent`) con acciones
fijas, igual que el patrón `nas-terminal` del stack anterior. Está planificado
como mejora posterior a F6.
