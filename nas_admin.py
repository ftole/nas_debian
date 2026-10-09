#!/usr/bin/env python3
"""
==============================================================================
Asistente de Administración Remota NAS (Windows) - Producción
==============================================================================
Asistente TUI para administrar de forma remota un Servidor NAS & Central de
Respaldos EAD-COL (Debian 13) desde Windows, reutilizando el motor de
`test_remote.py` (SSH/paramiko, host keys accept-new, sudo por stdin).

Modo ADITIVO: no modifica ni duplica la lógica existente. Importa `test_remote`
como núcleo y añade un front-end de producción para el día a día:
  - Despliegue parametrizado (rol, disco, red, admin, --keep-data)
  - Estado y diagnóstico
  - Actualización de software (updater.sh --yes)
  - Reinicio/reload de servicios y consulta de logs
  - Consola SSH interactiva
  - Desinstalación con confirmación reforzada
  - Credenciales seguras (Windows Credential Manager vía keyring + .env)

Uso:
  python nas_admin.py                  -> Menú interactivo
  python nas_admin.py deploy|status|update|services|logs|console|uninstall|config
==============================================================================
"""

import argparse
import getpass
import json
import pathlib
import re
import shlex
import sys

try:
    import keyring
except ImportError:  # pragma: no cover - dependencia opcional
    keyring = None

try:
    import colorama
    colorama.just_fix_windows_console()
except Exception:  # pragma: no cover - no disponible en CI/consolas modernas
    colorama = None

# Forzar UTF-8 en consolas Windows para evitar mojibake en la salida.
for _stream in (sys.stdout, sys.stderr):
    if _stream is not None and hasattr(_stream, "reconfigure"):
        try:
            _stream.reconfigure(encoding="utf-8", errors="replace")
        except Exception:  # pragma: no cover
            pass

import test_remote as tr  # noqa: E402

# -----------------------------------------------------------------------------
# Constantes y re-exportación del núcleo
# -----------------------------------------------------------------------------
APP_NAME = "Asistente NAS"
KEYRING_SERVICE = "nas-assistant"
KEYRING_PASSWORD_KEY = "NAS_TEST_PASSWORD"
KEYRING_ROOT_KEY = "NAS_ROOT_PASSWORD"
GITHUB_REPO = "https://github.com/ftole/nas_debian.git"
REPO_DIR = "/opt/nas_debian"
PASS_TOKEN = "__NAS_ADMIN_PASS__"

Colors = tr.Colors


# -----------------------------------------------------------------------------
# Credenciales (keyring con fallback a .env)
# -----------------------------------------------------------------------------
def get_secret(key: str) -> str:
    """Obtiene un secreto del Credential Manager si keyring está disponible."""
    if keyring is None:
        return ""
    try:
        val = keyring.get_password(KEYRING_SERVICE, key)
        return val or ""
    except Exception:
        return ""


def set_secret(key: str, value: str) -> bool:
    """Guarda un secreto en el Credential Manager; True si se persistió."""
    if keyring is None:
        return False
    try:
        keyring.set_password(KEYRING_SERVICE, key, value)
        return True
    except Exception:
        return False


def delete_secret(key: str) -> None:
    if keyring is None:
        return
    try:
        keyring.delete_password(KEYRING_SERVICE, key)
    except Exception:
        pass


def load_config(env_path: pathlib.Path):
    """Carga .env y completa las contraseñas desde keyring."""
    raw = dict(tr.load_env_file(env_path))
    cfg = tr.normalize_env_config(raw)

    pw = get_secret(KEYRING_PASSWORD_KEY)
    rp = get_secret(KEYRING_ROOT_KEY)
    if pw:
        cfg["NAS_TEST_PASSWORD"] = pw
    if rp:
        cfg["NAS_ROOT_PASSWORD"] = rp
    cfg["NAS_ROOT_PASSWORD"] = cfg["NAS_ROOT_PASSWORD"] or cfg["NAS_TEST_PASSWORD"]
    return cfg


def config_is_usable(cfg) -> bool:
    required = ["NAS_TEST_IP", "NAS_TEST_USER", "NAS_TEST_PASSWORD"]
    return all(str(cfg.get(k, "")).strip() for k in required)


def write_env_non_secret(env_path: pathlib.Path, cfg) -> None:
    """Escribe en .env solo los datos no secretos (las claves van a keyring)."""
    lines = [
        "# ==============================================================================",
        "# Asistente NAS - Credenciales de Conexión Remota",
        "# ==============================================================================",
        "# Generado por nas_admin.py. Las contraseñas se guardan en el Credential Manager",
        "# de Windows (keyring); en .env solo residen host, puerto y usuario.",
        "",
        f"NAS_TEST_IP={str(cfg.get('NAS_TEST_IP', '')).strip()}",
        f"NAS_TEST_PORT={str(cfg.get('NAS_TEST_PORT', '22')).strip()}",
        f"NAS_TEST_USER={str(cfg.get('NAS_TEST_USER', 'sistemas')).strip()}",
        "NAS_TEST_PASSWORD=",
        "NAS_ROOT_PASSWORD=",
        "",
    ]
    try:
        env_path.parent.mkdir(parents=True, exist_ok=True)
        env_path.write_text("\n".join(lines), encoding="utf-8")
    except OSError as e:
        tr.log_error(f"No se pudo guardar {env_path}: {e}")


def persist_credentials(env_path: pathlib.Path, candidate) -> None:
    """Persiste el perfil: con keyring las claves van al Credential Manager y
    `.env` queda sin secretos; sin keyring se conservan en `.env` con aviso."""
    if keyring is not None:
        write_env_non_secret(env_path, candidate)
        set_secret(KEYRING_PASSWORD_KEY, candidate["NAS_TEST_PASSWORD"])
        set_secret(KEYRING_ROOT_KEY, candidate["NAS_ROOT_PASSWORD"])
        tr.log_success("Perfil guardado (host en .env, claves en Credential Manager).")
    else:
        tr.save_env_file(env_path, {k: str(v) for k, v in candidate.items()})
        tr.log_warn("keyring no está instalado: las contraseñas se guardaron en .env.")
        tr.log_info("Para usar el Credential Manager de Windows: pip install -r requirements-assistant.txt")


# -----------------------------------------------------------------------------
# Funciones puras (parseo y construcción de comandos) - testeables
# -----------------------------------------------------------------------------
def parse_lsblk_json(raw: str):
    """Parsea la salida JSON de `lsblk -J` devolviendo solo los discos físicos."""
    blocks = []
    try:
        data = json.loads(raw)
        blocks = data.get("blockdevices") or []
    except (ValueError, TypeError):
        return []

    disks = []
    for b in blocks:
        if b.get("type") != "disk":
            continue
        name = str(b.get("name") or "")
        if not name:
            continue
        disks.append({
            "name": name,
            "dev": "/dev/" + name,
            "size": str(b.get("size") or ""),
            "model": str(b.get("model") or ""),
            "rotational": str(b.get("rota") or ""),
            "mountpoint": str(b.get("mountpoint") or ""),
        })
    return disks


def disk_choices(disks, os_disk_name: str = ""):
    """Devuelve LOCAL más los discos detectados cuyo nombre no es el del SO."""
    choices = [{
        "dev": "LOCAL",
        "name": "LOCAL",
        "size": "Partición raíz (almacenamiento compartido)",
        "model": "",
        "rotational": "",
        "mountpoint": "/",
    }]
    for d in disks:
        if os_disk_name and d["name"] == os_disk_name:
            continue
        choices.append(d)
    return choices


def build_deploy_command(disk: str, workgroup: str, netbios: str, admin_user: str,
                         role: str = "ARCHIVOS", keep_data: bool = False,
                         repo_dir: str = REPO_DIR) -> str:
    """Construye el comando remoto de deploy.sh con el placeholder de clave."""
    role = (role or "ARCHIVOS").upper()
    if role in ("HIBRIDO", "ARCHIVOSBACKUP"):
        role = "ARCHIVOS_BACKUP"
    if role not in ("ARCHIVOS", "BACKUP", "ARCHIVOS_BACKUP"):
        role = "ARCHIVOS"
    extra = " --keep-data" if keep_data else ""
    inner = (
        f"bash src/core/deploy.sh {shlex.quote(disk)} {shlex.quote(workgroup)} "
        f"{shlex.quote(netbios)} {shlex.quote(admin_user)} - {shlex.quote(role)} "
        f"--force --confirm{extra}"
    )
    return (
        f"cd {shlex.quote(repo_dir)} && "
        f"printf '%s\\n' {shlex.quote(PASS_TOKEN)} | {inner}"
    )


# -----------------------------------------------------------------------------
# Operaciones remotas de administración
# -----------------------------------------------------------------------------
def ensure_remote_repo(manager, repo_dir: str = REPO_DIR) -> bool:
    """Asegura el repositorio en el servidor (clonado o actualización desde GitHub)."""
    tr.log_info("Garantizando el repositorio del proyecto en el servidor...")
    cmd = (
        "command -v git >/dev/null 2>&1 || apt-get install -y -qq git >/dev/null 2>&1; "
        f"if [ -d {repo_dir}/.git ]; then "
        f"  git -C {repo_dir} config --system --add safe.directory {repo_dir} >/dev/null 2>&1 || true; "
        f"  git -C {repo_dir} fetch -q --prune origin main && git -C {repo_dir} reset --hard -q origin/main; "
        f"else "
        f"  rm -rf {repo_dir}; git clone -q {GITHUB_REPO} {repo_dir}; "
        f"fi"
    )
    code, out, err = manager.run_command(cmd, sudo=True, timeout=300, stream=True)
    if code != 0:
        tr.log_error(f"No se pudo preparar el repositorio: {err or out}")
        return False
    return True


def run_deploy(manager, deploy_cmd: str, password: str, timeout: int = 600) -> bool:
    cmd = deploy_cmd.replace(PASS_TOKEN, password)
    tr.log_info("Ejecutando deploy.sh en el servidor remoto...")
    code, out, err = manager.run_command(cmd, sudo=True, timeout=timeout, stream=True)
    if code != 0:
        tr.log_error(f"Fallo en deploy.sh (código {code})")
        if err:
            print(f"{Colors.RED}{err}{Colors.RESET}")
        return False
    return True


def verify_deployment(manager) -> bool:
    """Comprueba servicios, Base de Datos SQLite y almacenamiento tras desplegar."""
    tr.log_info("Comprobando estado del despliegue...")
    ok = True
    for svc in ("smbd", "wsdd2", "nginx"):
        code, out, _ = manager.run_command(f"systemctl is-active {svc}", sudo=False, timeout=10)
        if out.strip() == "active":
            tr.log_success(f"Servicio '{svc}': ACTIVO")
        else:
            tr.log_error(f"Servicio '{svc}': INACTIVO")
            ok = False
    c, o, _ = manager.run_command("test -f /var/lib/nas/nas.sqlite", sudo=True, timeout=10)
    tr.log_success("Base de datos SQLite: PRESENTE") if c == 0 else tr.log_error("SQLite ausente")
    ok = ok and (c == 0)
    c, o, _ = manager.run_command("test -d /srv/nas", sudo=True, timeout=10)
    tr.log_success("Almacenamiento /srv/nas: PRESENTE") if c == 0 else tr.log_error("/srv/nas ausente")
    ok = ok and (c == 0)
    return ok


def detect_os_disk_name(manager) -> str:
    _, out, _ = manager.run_command(
        "bash -c 'lsblk -no PKNAME \"$(findmnt -n -o SOURCE /)\"'", sudo=True, timeout=10)
    return out.strip()


def detect_netbios_default(manager) -> str:
    _, out, _ = manager.run_command("hostname -s 2>/dev/null || echo SRV-NAS", sudo=False, timeout=10)
    name = out.strip().upper() or "SRV-NAS"
    return re.sub(r"[^A-Z0-9_-]", "", name) or "SRV-NAS"


def detect_workgroup_default(manager) -> str:
    _, out, _ = manager.run_command(
        "grep -i '^\\s*workgroup\\s*=' /etc/samba/smb.conf 2>/dev/null | head -1 | awk -F= '{print $2}' | tr -d ' '",
        sudo=True, timeout=10)
    return out.strip().upper() or "WORKGROUP"


def get_php_fpm_service(manager) -> str:
    c, out, _ = manager.run_command(
        "systemctl list-units --type=service --state=active 'php*-fpm*' --no-pager", sudo=False, timeout=10)
    m = re.search(r"php[\d.]*-fpm", out)
    return m.group(0) if (c == 0 and m) else "php-fpm"


# -----------------------------------------------------------------------------
# Asistentes interactivos (menús y wizards)
# -----------------------------------------------------------------------------
def pause() -> None:
    try:
        input(f"\n{Colors.GRAY}Presiona Enter para continuar...{Colors.RESET}")
    except (KeyboardInterrupt, EOFError):
        print()


def ask_yes_no(question: str, default: bool = False) -> bool:
    d = "s" if default else "n"
    resp = input(f" {Colors.CYAN}[?] {question} [s/N]: {Colors.RESET}").strip().lower()
    if not resp:
        resp = d
    return resp in ("s", "si", "y", "yes")


def config_wizard(env_path: pathlib.Path, cfg=None):
    """Asistente de conexión: solicita datos y guarda secretos en keyring."""
    tr.print_banner()
    print(f"\n{Colors.WHITE}{Colors.BOLD}Configuración de Conexión SSH (Asistente NAS){Colors.RESET}")
    print(f"{Colors.GRAY}Las contraseñas se guardan en el Credential Manager de Windows.{Colors.RESET}\n")

    curr = cfg or load_config(env_path)
    ip = input(f" [?] Dirección IP o Host remoto [{curr.get('NAS_TEST_IP', '10.10.1.2')}]: ").strip() or curr.get("NAS_TEST_IP", "10.10.1.2")
    port_raw = input(f" [?] Puerto SSH [{curr.get('NAS_TEST_PORT', '22')}]: ").strip() or str(curr.get("NAS_TEST_PORT", "22"))
    try:
        port = int(port_raw)
    except ValueError:
        port = 22
    user = input(f" [?] Usuario SSH con acceso sudo [{curr.get('NAS_TEST_USER', 'sistemas')}]: ").strip() or curr.get("NAS_TEST_USER", "sistemas")

    prompt_pw = " [?] Contraseña SSH: "
    if get_secret(KEYRING_PASSWORD_KEY):
        prompt_pw = " [?] Contraseña SSH [Enter para mantener la guardada]: "
    pw = getpass.getpass(prompt_pw)
    if not pw:
        pw = get_secret(KEYRING_PASSWORD_KEY)
    while not pw:
        tr.log_warn("La contraseña SSH no puede estar vacía.")
        pw = getpass.getpass(" [?] Contraseña SSH: ")

    rp = getpass.getpass(" [?] Contraseña para sudo [Enter = la misma del usuario]: ")
    root_pw = rp if rp else pw

    candidate = {
        "NAS_TEST_IP": ip,
        "NAS_TEST_PORT": port,
        "NAS_TEST_USER": user,
        "NAS_TEST_PASSWORD": pw,
        "NAS_ROOT_PASSWORD": root_pw,
    }

    tr.log_info("Comprobando conectividad SSH y permisos de root...")
    manager = tr.SSHManager(candidate)
    ok, msg = manager.test_connection()
    if ok:
        candidate["NAS_ROOT_PASSWORD"] = manager.root_password
        tr.log_success(msg)
        persist_credentials(env_path, candidate)
    else:
        tr.log_warn(f"Validación de conexión falló: {msg}")
        if ask_yes_no("¿Deseas guardar estos datos de todas formas?"):
            persist_credentials(env_path, candidate)
    return candidate


def ensure_config(env_path: pathlib.Path, allow_interactive: bool = True):
    cfg = load_config(env_path)
    if config_is_usable(cfg):
        return cfg
    if not allow_interactive:
        raise RuntimeError(f"Configuración incompleta en {env_path.name}. Ejecuta 'config'.")
    return config_wizard(env_path, cfg)


def disk_detect_menu(manager) -> str:
    tr.log_info("Detectando discos físicos disponibles en el servidor...")
    code, out, _ = manager.run_command(
        "lsblk -J -o NAME,SIZE,TYPE,MOUNTPOINT,MODEL,ROTA 2>/dev/null", sudo=True, timeout=15)
    disks = parse_lsblk_json(out if code == 0 else "")
    os_disk = detect_os_disk_name(manager)
    choices = disk_choices(disks, os_disk)

    print(f"\n{Colors.WHITE}{Colors.BOLD}Selecciona el almacenamiento para /srv/nas:{Colors.RESET}")
    for i, d in enumerate(choices, 1):
        rot = " | SSD" if d.get("rotational") == "0" else (" | HDD" if d.get("rotational") == "1" else "")
        print(f"  [{i}] {d['dev']:>14}  {d.get('size', ''):>8}  {d.get('model', '')} {rot}")
    print("  [0] Cancelar")

    sel = input(f" {Colors.CYAN}[?] Opción [1-{len(choices)}]: {Colors.RESET}").strip()
    if sel == "0":
        return ""
    try:
        idx = int(sel) - 1
        if 0 <= idx < len(choices):
            return choices[idx]["dev"]
    except ValueError:
        pass
    tr.log_warn("Selección inválida.")
    return ""


def deploy_wizard(manager, config):
    tr.log_step("DESPLEGAR", "Asistente de despliegue del servidor")
    print(f"{Colors.GRAY}Reutiliza el despliegue de 'install.sh'/'deploy.sh' con tus parámetros.{Colors.RESET}\n")

    disk = disk_detect_menu(manager)
    if not disk:
        tr.log_warn("Despliegue cancelado.")
        return

    role_in = input(f" {Colors.CYAN}[?] Rol [ARCHIVOS/BACKUP/ARCHIVOS_BACKUP] (ARCHIVOS): {Colors.RESET}").strip().upper() or "ARCHIVOS"
    if role_in in ("HIBRIDO", "ARCHIVOSBACKUP"):
        role = "ARCHIVOS_BACKUP"
    elif role_in in ("ARCHIVOS", "BACKUP", "ARCHIVOS_BACKUP"):
        role = role_in
    else:
        role = "ARCHIVOS"

    workgroup = input(f" {Colors.CYAN}[?] Workgroup [{detect_workgroup_default(manager)}]: {Colors.RESET}").strip().upper() \
        or detect_workgroup_default(manager)
    netbios = input(f" {Colors.CYAN}[?] Nombre NetBIOS [{detect_netbios_default(manager)}]: {Colors.RESET}").strip().upper() \
        or detect_netbios_default(manager)
    ssh_user = config.get("NAS_TEST_USER", "sistemas")
    admin_user = input(f" {Colors.CYAN}[?] Usuario administrador [{ssh_user}]: {Colors.RESET}").strip() \
        or ssh_user
    if admin_user != ssh_user:
        tr.log_warn(f"El usuario SSH actual '{ssh_user}' quedará BLOQUEADO tras el despliegue.")
        tr.log_warn(f"Continúa administrando con la cuenta '{admin_user}' (misma contraseña) "
                    f"o reanúbalo en consola con: sudo usermod -U {ssh_user}")
        if not ask_yes_no("¿Continuar de todas formas?", default=False):
            tr.log_warn("Despliegue cancelado.")
            return
    keep_data = ask_yes_no("¿Reutilizar datos preexistentes (--keep-data)?")

    print(f"\n{Colors.WHITE}Resumen del despliegue:{Colors.RESET}")
    print(f"  Rol             : {role}")
    print(f"  Almacenamiento  : {disk}")
    print(f"  Workgroup       : {workgroup}")
    print(f"  NetBIOS         : {netbios}")
    print(f"  Administrador   : {admin_user}")
    print(f"  Modo keep-data  : {'sí' if keep_data else 'no'}")

    if not ask_yes_no("¿Proceder con el despliegue? (formateará el disco elegido si no es --keep-data)", default=False):
        tr.log_warn("Despliegue cancelado.")
        return

    if not ensure_remote_repo(manager):
        return

    cmd = build_deploy_command(disk, workgroup, netbios, admin_user, role, keep_data)
    if not run_deploy(manager, cmd, config["NAS_TEST_PASSWORD"]):
        tr.log_error("El despliegue no finalizó correctamente. Revisa el log o la consola.")
        return

    # Verificar con la cuenta administrador del despliegue para evitar falsos
    # negativos causados por el bloqueo de las cuentas base no designadas.
    verify_cfg = dict(config)
    verify_cfg["NAS_TEST_USER"] = admin_user
    verify_cfg["NAS_TEST_PASSWORD"] = config["NAS_TEST_PASSWORD"]
    verify_cfg["NAS_ROOT_PASSWORD"] = config["NAS_TEST_PASSWORD"]
    verify_manager = tr.SSHManager(verify_cfg)
    verify_deployment(verify_manager)
    verify_manager.close()

    if admin_user != ssh_user:
        tr.log_warn(f"Para seguir administrando usa la cuenta '{admin_user}' "
                    f"(o reactiva '{ssh_user}' en consola con: sudo usermod -U {ssh_user}).")


def services_menu(manager) -> None:
    while True:
        tr.print_banner()
        print(f"\n{Colors.WHITE}{Colors.BOLD}Reinicio / gestión de servicios{Colors.RESET}")
        print(f"  [1] smbd   [2] nmbd   [3] wsdd2  [4] nginx  [5] {get_php_fpm_service(manager)}  [6] cron")
        print("  [a] status   [r] restart   [s] reload   [0] volver")
        opc = input(f" {Colors.CYAN}[?] opción: {Colors.RESET}").strip().lower()
        if opc in ("0", "q", ""):
            return
        service_map = {"1": "smbd", "2": "nmbd", "3": "wsdd2", "4": "nginx",
                       "5": get_php_fpm_service(manager), "6": "cron"}
        action = None
        if opc.startswith("a"):
            action = "status"
        elif opc.startswith("r"):
            action = "restart"
        elif opc.startswith("s"):
            action = "reload"
        if action is None:
            tr.log_warn("Opción inválida.")
            continue
        svc = service_map.get(opc[-1], "")
        if not svc:
            tr.log_warn("Servicio inválido.")
            continue
        tr.log_info(f"{action} {svc}...")
        code, out, err = manager.run_command(f"systemctl {action} {svc}", sudo=True, timeout=30, stream=True)
        if code == 0:
            tr.log_success(f"{svc}: {action} ejecutado.")
        else:
            tr.log_error(f"{svc}: fallo ({err or out})")
        pause()


def logs_menu(manager) -> None:
    while True:
        tr.print_banner()
        print(f"\n{Colors.WHITE}{Colors.BOLD}Logs / bitácoras del servidor{Colors.RESET}")
        print("  [1] systemd (últimas líneas)")
        print("  [2] Servicios NAS (smbd/nginx/journalctl por unidad)")
        print("  [3] Bitácora administrativa /var/log/nas-admin.log")
        print("  [4] Respaldos /srv/nas/LOGS_BACKUP")
        print("  [0] volver")
        opc = input(f" {Colors.CYAN}[?] opción: {Colors.RESET}").strip()
        if opc in ("0", "q", ""):
            return
        if opc == "1":
            manager.run_command("journalctl -n 80 --no-pager", sudo=True, timeout=20, stream=True)
        elif opc == "2":
            svc = input(" [?] Unidad (smbd/nmbox/wsdd2/nginx/php-fpm): ").strip() or "smbd"
            manager.run_command(f"journalctl -u {svc} -n 80 --no-pager", sudo=True, timeout=20, stream=True)
        elif opc == "3":
            manager.run_command("tail -n 100 /var/log/nas-admin.log 2>/dev/null || echo 'Sin bitácora aún'",
                                sudo=True, timeout=20, stream=True)
        elif opc == "4":
            manager.run_command(
                "for f in /srv/nas/LOGS_BACKUP/*.log; do echo \"===== $f =====\"; tail -n 40 \"$f\" 2>/dev/null; done",
                sudo=True, timeout=20, stream=True)
        else:
            tr.log_warn("Opción inválida.")
        pause()


def action_status(manager) -> None:
    tr.log_step("ESTADO", "Diagnóstico en vivo del servidor")
    cmd = (
        "if [ -f /opt/nas_debian/src/asistente.sh ]; then "
        "  bash /opt/nas_debian/src/asistente.sh --status; "
        "elif command -v nas >/dev/null 2>&1; then nas status; "
        "else "
        "  echo '=== INFORMACIÓN DEL SISTEMA ==='; uname -a; uptime; "
        "  echo; echo '=== MEMORIA Y DISCO ==='; free -h; df -h / /srv/nas 2>/dev/null || df -h /; "
        "  echo; echo '=== ESTADO DE SERVICIOS ==='; systemctl status smbd wsdd2 nginx --no-pager 2>&1 || true; "
        "fi"
    )
    manager.run_command(cmd, sudo=True, timeout=60, stream=True)


def action_update(manager) -> None:
    tr.log_info("Actualizando software desde GitHub (updater.sh --yes)...")
    code, out, err = manager.run_command(
        "if [ -f /opt/nas_debian/src/core/updater.sh ]; then "
        "bash /opt/nas_debian/src/core/updater.sh --yes; "
        "else echo 'Actualizador no instalado; despliega primero.'; fi",
        sudo=True, timeout=300, stream=True)
    if code == 0:
        tr.log_success("Actualización finalizada.")
    else:
        tr.log_warn("La actualización no encontró cambios o el servidor no está desplegado.")
    sync_web_panel(manager)


def sync_web_panel(manager) -> None:
    """Propaga el código `web/` del repo actualizado a /var/www/nas-web.

    updater.sh actualiza /opt/nas_debian pero no despliega la interfaz; este paso
    copia solo la web (excluyendo `data/`) y recarga Nginx/PHP-FPM.
    """
    tr.log_info("Sincronizando el panel web con el código actualizado...")
    cmd = (
        "if [ -d /opt/nas_debian/web ] && [ -d /var/www/nas-web ]; then "
        "  rsync -a --delete --exclude='data/' /opt/nas_debian/web/ /var/www/nas-web/ && "
        "  chown -R www-data:www-data /var/www/nas-web && "
        "  chmod -R 755 /var/www/nas-web; "
        "  systemctl reload nginx 2>/dev/null || true; "
        "  systemctl reload php*-fpm 2>/dev/null || true; "
        "else echo 'Sincronización de web omitida (código o panel ausente)'; fi"
    )
    code, out, err = manager.run_command(cmd, sudo=True, timeout=120, stream=True)
    if code == 0:
        tr.log_success("Panel web sincronizado. Recarga la página con Ctrl+F5.")
    else:
        tr.log_warn(f"No se pudo sincronizar el panel web: {err or out}")


def action_uninstall(manager) -> None:
    print(f"\n{Colors.RED}{Colors.BOLD}¡CUIDADO! Esto desinstalará el NAS, Samba, el panel web y borrará configuraciones.{Colors.RESET}")
    if not ask_yes_no("¿Confirmas la desinstalación total?"):
        tr.log_warn("Cancelado.")
        return
    token = input(" Escribe DESINSTALAR para confirmar: ").strip()
    if token != "DESINSTALAR":
        tr.log_warn("Confirmación incorrecta. Cancelado.")
        return
    cmd = (
        "if [ -f /opt/nas_debian/install.sh ]; then "
        "bash /opt/nas_debian/install.sh --uninstall; "
        "elif [ -f /opt/nas_debian/src/core/uninstall.sh ]; then "
        "bash /opt/nas_debian/src/core/uninstall.sh --yes; "
        "else echo 'No se encontró desinstalador.'; fi"
    )
    code, out, err = manager.run_command(cmd, sudo=True, timeout=180, stream=True)
    if code == 0:
        tr.log_success("Desinstalación completada.")
    else:
        tr.log_error(f"Fallo en desinstalación: {err or out}")


# -----------------------------------------------------------------------------
# Menú principal y CLI
# -----------------------------------------------------------------------------
def check_dependencies() -> bool:
    """Verifica si las librerías necesarias están disponibles antes de solicitar credenciales."""
    if tr.paramiko is None:
        print(f"\n{Colors.RED}{Colors.BOLD}[-] ERROR: La librería requerida 'paramiko' no está disponible.{Colors.RESET}")
        print(f"{Colors.YELLOW}Para utilizar el Asistente NAS en Windows, instala los paquetes requeridos:{Colors.RESET}")
        print("    pip install -r requirements-assistant.txt\n")
        print(f"{Colors.CYAN}O ejecuta directamente el lanzador automatizado en Windows:{Colors.RESET}")
        print("    nas_admin.bat\n")
        return False
    return True


def run_interactive_menu(env_path: pathlib.Path) -> None:
    if not check_dependencies():
        return
    while True:
        try:
            config = ensure_config(env_path)
        except (RuntimeError, KeyboardInterrupt):
            return
        manager = tr.SSHManager(config)

        tr.print_banner()
        host = config.get("NAS_TEST_IP", "N/A")
        user = config.get("NAS_TEST_USER", "N/A")
        print(f" {Colors.GRAY}Servidor:{Colors.RESET} {Colors.WHITE}{host}{Colors.RESET} "
              f"| {Colors.GRAY}Usuario:{Colors.RESET} {Colors.WHITE}{user}{Colors.RESET} "
              f"| {Colors.GRAY}Config:{Colors.RESET} {Colors.CYAN}{env_path.name}{Colors.RESET}")
        print(f"{Colors.CYAN}{Colors.BOLD}------------------------------------------------------------------------------{Colors.RESET}")
        print(f"  {Colors.BOLD}[1]{Colors.RESET} Desplegar servidor (NAS / Backup)")
        print(f"  {Colors.BOLD}[2]{Colors.RESET} Estado y diagnóstico")
        print(f"  {Colors.BOLD}[3]{Colors.RESET} Actualizar software")
        print(f"  {Colors.BOLD}[4]{Colors.RESET} Reiniciar / reload de servicios")
        print(f"  {Colors.BOLD}[5]{Colors.RESET} Ver logs / bitácoras")
        print(f"  {Colors.BOLD}[6]{Colors.RESET} Consola SSH interactiva")
        print(f"  {Colors.BOLD}[7]{Colors.RESET} Desinstalar servidor")
        print(f"  {Colors.BOLD}[8]{Colors.RESET} Credenciales y conexión")
        print(f"  {Colors.BOLD}[0]{Colors.RESET} Salir")
        print(f"{Colors.CYAN}{Colors.BOLD}=============================================================================={Colors.RESET}")

        try:
            opc = input(f" {Colors.CYAN}[?] Selecciona una opción [0-8]: {Colors.RESET}").strip()
        except (KeyboardInterrupt, EOFError):
            print()
            break

        if opc in ("0", "q", "exit"):
            print(f"\n {Colors.GREEN}Saliendo del asistente. ¡Hasta pronto!{Colors.RESET}\n")
            break
        elif opc == "1":
            deploy_wizard(manager, config)
        elif opc == "2":
            action_status(manager)
        elif opc == "3":
            action_update(manager)
        elif opc == "4":
            services_menu(manager)
        elif opc == "5":
            logs_menu(manager)
        elif opc == "6":
            manager.interactive_shell()
        elif opc == "7":
            action_uninstall(manager)
        elif opc == "8":
            config = config_wizard(env_path, config)
            manager.close()
        else:
            tr.log_warn("Opción inválida. Ingresa un número entre 0 y 8.")
        pause()


def main(argv=None) -> int:
    if argv is None:
        argv = sys.argv[1:]

    parser = argparse.ArgumentParser(
        description="Asistente de Administración Remota NAS (Windows) - Producción",
        formatter_class=argparse.RawTextHelpFormatter,
    )
    parser.add_argument(
        "action", nargs="?", choices=["deploy", "status", "update", "services", "logs",
                                      "console", "uninstall", "config"],
        help=(
            "Acción directa (omite el menú):\n"
            "  deploy    : Asistente de despliegue\n"
            "  status    : Estado y diagnóstico\n"
            "  update    : Actualizar software\n"
            "  services  : Gestión de servicios\n"
            "  logs      : Ver bitácoras\n"
            "  console   : Consola SSH\n"
            "  uninstall : Desinstalar (con confirmación)\n"
            "  config    : Credenciales y conexión"
        ),
    )
    parser.add_argument("--env-file", type=str, default="", help="Ruta alternativa al archivo .env")
    parser.add_argument("--non-interactive", action="store_true", help="No solicitar datos interactivamente si faltan")
    args = parser.parse_args(argv)

    if not check_dependencies():
        return 1

    env_path = pathlib.Path(args.env_file).resolve() if args.env_file else tr.get_default_env_path()

    if args.action is None:
        run_interactive_menu(env_path)
        return 0

    if args.action == "config":
        config_wizard(env_path, load_config(env_path))
        return 0

    try:
        config = ensure_config(env_path, allow_interactive=not args.non_interactive)
    except Exception as e:
        tr.log_error(str(e))
        return 1

    manager = tr.SSHManager(config)
    if args.action == "deploy":
        deploy_wizard(manager, config)
    elif args.action == "status":
        action_status(manager)
    elif args.action == "update":
        action_update(manager)
    elif args.action == "services":
        services_menu(manager)
    elif args.action == "logs":
        logs_menu(manager)
    elif args.action == "console":
        manager.interactive_shell()
    elif args.action == "uninstall":
        action_uninstall(manager)
    return 0


if __name__ == "__main__":
    sys.exit(main())
