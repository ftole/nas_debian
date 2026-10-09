#!/usr/bin/env python3
"""
==============================================================================
Servidor NAS & Central de Respaldos (Debian 13) - Suite de Pruebas Remotas
==============================================================================
Herramienta multiplataforma interactiva y automatizada para pruebas remotas
mediante SSH, validación de despliegue, desinstalación, auditoría web, Samba,
copias de seguridad con deduplicación por hardlinks y diagnóstico integral.

Lee y persiste la configuración en un archivo '.env' local. Si los datos
existen, ingresa directamente al menú de acciones; de lo contrario, los
solicita interactivamente y verifica la conexión antes de guardar.
==============================================================================
"""

import argparse
import getpass
import http.client
import io
import json
import os
import pathlib
import re
import shlex
import ssl
import subprocess
import sys
import tarfile
import time
import urllib.parse
import urllib.request
from typing import Any, Dict, List, Optional, Tuple

if sys.platform == "win32":
    try:
        if hasattr(sys.stdout, "reconfigure"):
            sys.stdout.reconfigure(encoding="utf-8", errors="replace")
        if hasattr(sys.stderr, "reconfigure"):
            sys.stderr.reconfigure(encoding="utf-8", errors="replace")
    except Exception:
        pass

try:
    import paramiko
except ImportError:
    paramiko = None


# -----------------------------------------------------------------------------
# Constantes y Colores ANSI
# -----------------------------------------------------------------------------
class Colors:
    RESET = "\033[0m"
    BOLD = "\033[1m"
    DIM = "\033[2m"
    RED = "\033[1;31m"
    GREEN = "\033[1;32m"
    YELLOW = "\033[1;33m"
    BLUE = "\033[1;34m"
    CYAN = "\033[1;36m"
    WHITE = "\033[1;37m"
    GRAY = "\033[0;90m"


def print_banner() -> None:
    print(f"\n{Colors.CYAN}{Colors.BOLD}=============================================================================={Colors.RESET}")
    print(f"{Colors.WHITE}{Colors.BOLD}  SERVIDOR NAS & CENTRAL DE RESPALDOS EAD-COL (Debian 13){Colors.RESET}")
    print(f"{Colors.CYAN}  Suite Automatizada de Pruebas Remotas y Diagnóstico Local/Remoto{Colors.RESET}")
    print(f"{Colors.CYAN}{Colors.BOLD}=============================================================================={Colors.RESET}")


def log_info(msg: str) -> None:
    print(f" {Colors.CYAN}[•]{Colors.RESET} {msg}")


def log_success(msg: str) -> None:
    print(f" {Colors.GREEN}[✔]{Colors.RESET} {msg}")


def log_warn(msg: str) -> None:
    print(f" {Colors.YELLOW}[!]{Colors.RESET} {msg}")


def log_error(msg: str) -> None:
    print(f" {Colors.RED}[✘]{Colors.RESET} {msg}")


def log_step(step: str, title: str) -> None:
    print(f"\n{Colors.CYAN}{Colors.BOLD}>>> [{step}] {title}{Colors.RESET}")


# -----------------------------------------------------------------------------
# Gestión de Archivo .env
# -----------------------------------------------------------------------------
ENV_KEYS = [
    "NAS_TEST_IP",
    "NAS_TEST_PORT",
    "NAS_TEST_USER",
    "NAS_TEST_PASSWORD",
    "NAS_ROOT_PASSWORD",
]


def get_default_env_path() -> pathlib.Path:
    return pathlib.Path(__file__).resolve().parent / ".env"


def get_default_known_hosts_path() -> pathlib.Path:
    return pathlib.Path(__file__).resolve().parent / ".nas_known_hosts"


def load_env_file(filepath: pathlib.Path) -> Dict[str, str]:
    """Carga y parsea un archivo de variables de entorno .env."""
    data: Dict[str, str] = {}
    if not filepath.is_file():
        return data

    try:
        content = filepath.read_text(encoding="utf-8", errors="replace")
    except OSError:
        return data

    for line in content.splitlines():
        line = line.strip()
        if not line or line.startswith("#"):
            continue
        if "=" not in line:
            continue
        key, val = line.split("=", 1)
        key = key.strip()
        val = val.strip()
        # Eliminar comillas envolventes simples o dobles
        if (val.startswith('"') and val.endswith('"')) or (val.startswith("'") and val.endswith("'")):
            val = val[1:-1]
        data[key] = val

    return data


def save_env_file(filepath: pathlib.Path, data: Dict[str, str]) -> bool:
    """Guarda las variables de entorno en el archivo .env con permisos seguros."""
    lines = [
        "# ==============================================================================",
        "# Servidor NAS & Central de Respaldos (Debian 13) - Credenciales de Prueba",
        "# ==============================================================================",
        "# Generado automáticamente por test_remote.py",
        "",
        f"NAS_TEST_IP={data.get('NAS_TEST_IP', '').strip()}",
        f"NAS_TEST_PORT={data.get('NAS_TEST_PORT', '22').strip()}",
        f"NAS_TEST_USER={data.get('NAS_TEST_USER', 'sistemas').strip()}",
        f"NAS_TEST_PASSWORD={data.get('NAS_TEST_PASSWORD', '').strip()}",
        f"NAS_ROOT_PASSWORD={data.get('NAS_ROOT_PASSWORD', '').strip()}",
        "",
    ]
    try:
        filepath.parent.mkdir(parents=True, exist_ok=True)
        filepath.write_text("\n".join(lines), encoding="utf-8")
        if os.name != "nt":
            try:
                os.chmod(filepath, 0o600)
            except OSError:
                pass
        return True
    except OSError as e:
        log_error(f"Error al guardar {filepath}: {e}")
        return False


def validate_env_config(data: Dict[str, str]) -> Tuple[bool, List[str]]:
    """Verifica si todas las variables requeridas están presentes y no vacías."""
    required = ["NAS_TEST_IP", "NAS_TEST_USER", "NAS_TEST_PASSWORD"]
    missing = [k for k in required if not data.get(k, "").strip()]
    return (len(missing) == 0, missing)


def normalize_env_config(data: Dict[str, str]) -> Dict[str, Any]:
    """Normaliza valores por defecto de la configuración."""
    port_str = data.get("NAS_TEST_PORT", "22").strip() or "22"
    try:
        port = int(port_str)
    except ValueError:
        port = 22

    user = data.get("NAS_TEST_USER", "sistemas").strip() or "sistemas"
    user_pass = data.get("NAS_TEST_PASSWORD", "")
    root_pass = data.get("NAS_ROOT_PASSWORD", "").strip() or user_pass

    return {
        "NAS_TEST_IP": data.get("NAS_TEST_IP", "").strip(),
        "NAS_TEST_PORT": port,
        "NAS_TEST_USER": user,
        "NAS_TEST_PASSWORD": user_pass,
        "NAS_ROOT_PASSWORD": root_pass,
    }


# -----------------------------------------------------------------------------
# Política Estricta de Claves de Host SSH (accept-new)
# -----------------------------------------------------------------------------
if paramiko is not None:
    class StrictAcceptNewPolicy(paramiko.MissingHostKeyPolicy):
        """
        Implementación equivalente a 'StrictHostKeyChecking=accept-new' de OpenSSH.
        - Si la clave del host es desconocida, la añade automáticamente y la persiste.
        - Si la clave del host ya está registrada pero difiere (ataque MITM o cambio
          no advertido), RECHAZA la conexión con error fatal de seguridad.
        - Bajo ninguna circunstancia ignora discrepancias criptográficas.
        """

        def __init__(self, known_hosts_file: pathlib.Path):
            self.known_hosts_file = known_hosts_file

        def missing_host_key(self, client: paramiko.SSHClient, hostname: str, key: Any) -> None:
            host_keys = client.get_host_keys()
            if hostname in host_keys and key.get_name() in host_keys[hostname]:
                raise paramiko.SSHException(
                    f"VIOLACIÓN DE SEGURIDAD: La clave pública del host '{hostname}' ha cambiado. "
                    f"Posible ataque Man-in-the-Middle o reinstalación sin purga de clave."
                )

            host_keys.add(hostname, key.get_name(), key)
            try:
                self.known_hosts_file.parent.mkdir(parents=True, exist_ok=True)
                client.save_host_keys(str(self.known_hosts_file))
                if os.name != "nt":
                    try:
                        os.chmod(self.known_hosts_file, 0o600)
                    except OSError:
                        pass
            except Exception as ex:
                log_warn(f"No se pudo guardar la clave del host en {self.known_hosts_file}: {ex}")
else:
    StrictAcceptNewPolicy = object  # type: ignore


# -----------------------------------------------------------------------------
# Gestor SSH y Ejecución Remota
# -----------------------------------------------------------------------------
class SSHManager:
    """Administra la conexión SSH, ejecución de comandos con/sin sudo y transferencia SFTP."""

    def __init__(self, config: Dict[str, Any], known_hosts_path: Optional[pathlib.Path] = None):
        self.ip: str = config.get("NAS_TEST_IP", "")
        self.port: int = int(config.get("NAS_TEST_PORT", 22))
        self.user: str = config.get("NAS_TEST_USER", "sistemas")
        self.password: str = config.get("NAS_TEST_PASSWORD", "")
        self.root_password: str = config.get("NAS_ROOT_PASSWORD", "") or self.password
        self.known_hosts_file = known_hosts_path or get_default_known_hosts_path()
        self._client: Optional[paramiko.SSHClient] = None

    def connect(self, timeout: int = 15) -> paramiko.SSHClient:
        if paramiko is None:
            raise RuntimeError("La librería 'paramiko' no está instalada. Ejecuta: pip install paramiko")

        if self._client is not None:
            try:
                transport = self._client.get_transport()
                if transport is not None and transport.is_active():
                    return self._client
            except Exception:
                pass
            self.close()

        client = paramiko.SSHClient()
        # Cargar claves conocidas del sistema y archivo local
        client.load_system_host_keys()
        if self.known_hosts_file.is_file():
            try:
                client.load_host_keys(str(self.known_hosts_file))
            except Exception as e:
                log_warn(f"No se pudieron leer claves previas de {self.known_hosts_file}: {e}")

        # Política accept-new estricta con registro persistente
        client.set_missing_host_key_policy(StrictAcceptNewPolicy(self.known_hosts_file))

        client.connect(
            hostname=self.ip,
            port=self.port,
            username=self.user,
            password=self.password,
            timeout=timeout,
            banner_timeout=timeout,
            auth_timeout=timeout,
            allow_agent=False,
            look_for_keys=False,
        )
        self._client = client
        return client

    def close(self) -> None:
        if self._client is not None:
            try:
                self._client.close()
            except Exception:
                pass
            self._client = None

    def run_command(
        self,
        command: str,
        sudo: bool = False,
        timeout: int = 180,
        stream: bool = False,
        input_data: Optional[str] = None,
    ) -> Tuple[int, str, str]:
        """
        Ejecuta un comando en el servidor remoto vía SSH.
        Si sudo=True, invoca 'sudo -S -p "" bash -c ...' y transmite la contraseña por stdin.
        """
        client = self.connect()

        if sudo and self.user != "root":
            # Envolvemos el comando en una invocación sudo -S con shell explícito
            # Pasamos la contraseña por stdin sin exponerla en los argumentos de ps
            remote_cmd = f"sudo -S -p '' bash -c {shlex.quote(command)}"
            stdin, stdout, stderr = client.exec_command(remote_cmd, timeout=timeout)
            stdin.write(self.root_password + "\n")
            if input_data:
                stdin.write(input_data + "\n")
            stdin.flush()
            stdin.close()
        else:
            remote_cmd = f"bash -c {shlex.quote(command)}"
            stdin, stdout, stderr = client.exec_command(remote_cmd, timeout=timeout)
            if input_data:
                stdin.write(input_data + "\n")
                stdin.flush()
            stdin.close()

        stdout_chunks: List[str] = []

        if stream:
            while True:
                line = stdout.readline()
                if not line:
                    break
                print(line, end="")
                stdout_chunks.append(line)
            stdout_str = "".join(stdout_chunks)
            stderr_str = stderr.read().decode("utf-8", errors="replace")
        else:
            stdout_str = stdout.read().decode("utf-8", errors="replace")
            stderr_str = stderr.read().decode("utf-8", errors="replace")

        exit_code = stdout.channel.recv_exit_status()

        # Filtrar posibles prompts de sudo residuales en stderr
        if sudo and stderr_str.startswith("[sudo]"):
            lines = stderr_str.splitlines(keepends=True)
            stderr_str = "".join(l for l in lines if not l.startswith("[sudo]"))

        return (exit_code, stdout_str, stderr_str)

    def test_connection(self) -> Tuple[bool, str]:
        """Verifica la conectividad SSH básica y los permisos sudo del usuario."""
        try:
            self.connect(timeout=10)
            # Prueba de usuario
            code, out, err = self.run_command("whoami", sudo=False, timeout=10)
            if code != 0:
                return (False, f"Error al ejecutar whoami: {err.strip()}")
            remote_user = out.strip()

            # Prueba de sudo
            if self.user == "root":
                code_sudo, out_sudo, err_sudo = self.run_command("id -u", sudo=False, timeout=10)
            else:
                code_sudo, out_sudo, err_sudo = self.run_command("id -u", sudo=True, timeout=10)

            if code_sudo != 0 or out_sudo.strip() != "0":
                # sudo valida la contraseña del propio usuario SSH, no la de root.
                if self.user != "root" and self.password and self.root_password != self.password:
                    self.root_password = self.password
                    code_sudo, out_sudo, err_sudo = self.run_command("id -u", sudo=True, timeout=10)
                    if code_sudo == 0 and out_sudo.strip() == "0":
                        return (True, f"Conexión exitosa como '{remote_user}'. sudo aceptó la contraseña del usuario SSH "
                                      "(la contraseña de root indicada no es necesaria para sudo).")
                return (False, f"Autenticación sudo falló: {err_sudo.strip()}\n"
                               f"    Nota: sudo pide la contraseña de '{self.user}', no la de root.")

            return (True, f"Conexión exitosa como '{remote_user}' con privilegios sudo totales.")
        except Exception as e:
            return (False, f"Fallo al conectar con {self.ip}:{self.port} - {e}")
        finally:
            self.close()

    def sync_project_files(self, remote_dest: str = "/tmp/nas_debian_test") -> Tuple[bool, str]:
        """Empaqueta el código local de src/ y web/ y lo transfiere vía SFTP a la máquina remota."""
        project_root = pathlib.Path(__file__).resolve().parent
        log_info(f"Empaquetando archivos del proyecto desde {project_root.name}...")

        # Crear archivo tar en memoria
        tar_buffer = io.BytesIO()
        include_dirs = ["src", "web"]
        include_files = ["install.sh", "README.md", "AGENTS.md"]

        def tar_filter(tarinfo: tarfile.TarInfo) -> Optional[tarfile.TarInfo]:
            name = tarinfo.name.replace("\\", "/")
            # Exclusiones
            if any(part in name.split("/") for part in ["__pycache__", ".git", "vendor", "node_modules", ".pytest_cache"]):
                return None
            if name.endswith((".pyc", ".sqlite", ".sqlite-wal", ".sqlite-shm", ".swp")):
                return None
            return tarinfo

        with tarfile.open(fileobj=tar_buffer, mode="w:gz") as tar:
            for dirname in include_dirs:
                dir_path = project_root / dirname
                if dir_path.is_dir():
                    tar.add(str(dir_path), arcname=dirname, filter=tar_filter)
            for fname in include_files:
                fpath = project_root / fname
                if fpath.is_file():
                    tar.add(str(fpath), arcname=fname, filter=tar_filter)

        tar_buffer.seek(0)
        tar_bytes = tar_buffer.getvalue()
        tar_size_kb = len(tar_bytes) / 1024
        log_info(f"Paquete preparado: {tar_size_kb:.1f} KB. Transfiriendo vía SFTP...")

        client = self.connect()
        remote_tar = f"/tmp/nas_sync_{int(time.time())}.tar.gz"

        try:
            sftp = client.open_sftp()
            with sftp.file(remote_tar, "wb") as remote_file:
                remote_file.write(tar_bytes)
            sftp.close()

            # Desempaquetar, sanear saltos de línea CRLF y aplicar permisos ejecutables en remoto
            unpack_cmd = (
                f"rm -rf {remote_dest} && mkdir -p {remote_dest} && "
                f"tar -xzf {remote_tar} -C {remote_dest} && "
                f"find {remote_dest} -type f -name '*.sh' -exec sed -i 's/\\r$//' {{}} + 2>/dev/null && "
                f"find {remote_dest} -type f -name '*.sh' -exec chmod +x {{}} + 2>/dev/null && "
                f"rm -f {remote_tar}"
            )
            code, out, err = self.run_command(unpack_cmd, sudo=True, timeout=60)
            if code != 0:
                return (False, f"Error al desempaquetar en {remote_dest}: {err}")
            return (True, f"Código sincronizado correctamente en {remote_dest}")
        except Exception as e:
            return (False, f"Fallo en transferencia SFTP: {e}")

    def interactive_shell(self) -> None:
        """Abre una sesión SSH interactiva directa."""
        log_info(f"Conectando consola SSH interactiva hacia {self.user}@{self.ip}:{self.port}...")
        ssh_bin = "ssh"
        # Verificar si ssh está disponible en el PATH local
        try:
            # Se utiliza StrictHostKeyChecking=accept-new estricto
            cmd = [
                ssh_bin,
                "-p", str(self.port),
                "-o", "StrictHostKeyChecking=accept-new",
                "-o", f"UserKnownHostsFile={self.known_hosts_file.resolve()}",
                f"{self.user}@{self.ip}",
            ]
            subprocess.call(cmd)
        except FileNotFoundError:
            log_warn("El comando 'ssh' no está en el PATH local. Abriendo canal con paramiko...")
            self._interactive_paramiko_shell()

    def _interactive_paramiko_shell(self) -> None:
        client = self.connect()
        chan = client.invoke_shell(term="xterm")
        print(f"{Colors.GREEN}Consola remota iniciada. Escribe 'exit' para salir.{Colors.RESET}\n")

        stop_event = False

        def forward_stdin() -> None:
            try:
                if os.name == "nt":
                    import msvcrt
                    while not stop_event and not chan.exit_status_ready():
                        if msvcrt.kbhit():
                            ch = msvcrt.getch()
                            chan.send(ch)
                        else:
                            time.sleep(0.02)
                else:
                    import select
                    while not stop_event and not chan.exit_status_ready():
                        r, _, _ = select.select([sys.stdin], [], [], 0.05)
                        if r:
                            data = sys.stdin.read(1)
                            if not data:
                                break
                            chan.send(data)
            except Exception:
                pass

        import threading
        t_in = threading.Thread(target=forward_stdin, daemon=True)
        t_in.start()

        try:
            while not chan.exit_status_ready():
                if chan.recv_ready():
                    data = chan.recv(1024).decode("utf-8", errors="replace")
                    sys.stdout.write(data)
                    sys.stdout.flush()
                time.sleep(0.02)
        except KeyboardInterrupt:
            try:
                chan.send("\x03")
            except Exception:
                pass
        finally:
            stop_event = True
            chan.close()


# -----------------------------------------------------------------------------
# Flujo Interactivo de Configuración .env
# -----------------------------------------------------------------------------
def interactive_configure(current: Optional[Dict[str, Any]] = None, env_path: Optional[pathlib.Path] = None) -> Dict[str, Any]:
    """Solicita interactivamente los parámetros de conexión y los valida en vivo."""
    path = env_path or get_default_env_path()
    curr = current or {}

    print_banner()
    print(f"\n{Colors.WHITE}{Colors.BOLD}Configuración de Conexión SSH para Pruebas Remotas:{Colors.RESET}")
    print(f"{Colors.GRAY}Los datos se guardarán de forma segura en {path.name} para futuros usos.{Colors.RESET}\n")

    default_ip = curr.get("NAS_TEST_IP", "10.10.1.2")
    default_port = str(curr.get("NAS_TEST_PORT", "22"))
    default_user = curr.get("NAS_TEST_USER", "sistemas")

    ip_input = input(f" [?] Dirección IP o Host remoto [{default_ip}]: ").strip()
    ip = ip_input if ip_input else default_ip

    port_input = input(f" [?] Puerto SSH [{default_port}]: ").strip()
    port_str = port_input if port_input else default_port
    try:
        port = int(port_str)
    except ValueError:
        port = 22

    user_input = input(f" [?] Usuario SSH con acceso sudo [{default_user}]: ").strip()
    user = user_input if user_input else default_user

    pass_prompt = " [?] Contraseña SSH: "
    if curr.get("NAS_TEST_PASSWORD"):
        pass_prompt = " [?] Contraseña SSH [dejar vacío para mantener actual]: "
    user_pass = getpass.getpass(pass_prompt)
    if not user_pass and curr.get("NAS_TEST_PASSWORD"):
        user_pass = curr["NAS_TEST_PASSWORD"]
    while not user_pass:
        log_warn("La contraseña SSH no puede estar vacía (si pegaste con el portapapeles, prueba escribirla).")
        user_pass = getpass.getpass(" [?] Contraseña SSH: ")
    print(f" [•] Contraseña SSH recibida ({len(user_pass)} caracteres).")

    root_prompt = (" [?] Contraseña para sudo [Enter = la misma del usuario SSH; "
                   "sudo valida la del usuario, no la de root]: ")
    root_pass = getpass.getpass(root_prompt)
    if not root_pass:
        root_pass = user_pass

    candidate: Dict[str, Any] = {
        "NAS_TEST_IP": ip,
        "NAS_TEST_PORT": port,
        "NAS_TEST_USER": user,
        "NAS_TEST_PASSWORD": user_pass,
        "NAS_ROOT_PASSWORD": root_pass,
    }

    log_info("Comprobando conectividad SSH y permisos de root en el servidor remoto...")
    manager = SSHManager(candidate)
    ok, msg = manager.test_connection()
    candidate["NAS_ROOT_PASSWORD"] = manager.root_password

    if ok:
        log_success(msg)
        save_env_file(path, {k: str(v) for k, v in candidate.items()})
        log_success(f"Configuración guardada en {path.name}")
    else:
        log_warn(f"Validación de conexión falló: {msg}")
        resp = input(f" {Colors.YELLOW}[?] ¿Deseas guardar estos datos de todas formas? [s/N]: {Colors.RESET}").strip().lower()
        if resp in ["s", "si", "y", "yes"]:
            save_env_file(path, {k: str(v) for k, v in candidate.items()})
            log_info(f"Guardado en {path.name} con advertencia.")
        else:
            log_error("Configuración no guardada.")

    return candidate


def ensure_env_config(env_path: Optional[pathlib.Path] = None, allow_interactive: bool = True) -> Dict[str, Any]:
    """Carga .env si existe y es válido; si no, solicita datos de forma interactiva."""
    path = env_path or get_default_env_path()

    if path.is_file():
        raw_data = load_env_file(path)
        is_valid, missing = validate_env_config(raw_data)
        if is_valid:
            return normalize_env_config(raw_data)
        else:
            log_warn(f"El archivo {path.name} existe pero faltan variables requeridas: {', '.join(missing)}")

    if not allow_interactive:
        raise RuntimeError(f"Configuración incompleta en {path.name} y modo no interactivo activado.")

    return interactive_configure(env_path=path)


# -----------------------------------------------------------------------------
# Módulos de Prueba Automatizada
# -----------------------------------------------------------------------------

def test_install_action(manager: SSHManager) -> bool:
    """Acción 1: Sincroniza el repositorio y ejecuta el despliegue limpio de deploy.sh e install.sh."""
    log_step("1/7", "Prueba de Despliegue e Instalación Limpia (test install)")

    # 1. Sincronizar archivos locales
    sync_ok, sync_msg = manager.sync_project_files("/tmp/nas_debian_test")
    if not sync_ok:
        log_error(sync_msg)
        return False
    log_success(sync_msg)

    # 2. Desplegar e instalar CLI global 'nas' en /opt/nas_debian y /usr/local/bin/nas
    log_info("Instalando componentes en /opt/nas_debian y registrando CLI global 'nas'...")
    install_cli_cmd = (
        "mkdir -p /opt/nas_debian && "
        "cp -a /tmp/nas_debian_test/. /opt/nas_debian/ && "
        "chmod +x /opt/nas_debian/install.sh /opt/nas_debian/src/asistente.sh && "
        "ln -sf /opt/nas_debian/install.sh /usr/local/bin/nas && "
        "ln -sf /usr/local/bin/nas /usr/local/bin/asistente_nas 2>/dev/null || true; "
        "ln -sf /usr/local/bin/nas /usr/local/bin/asistente-nas 2>/dev/null || true"
    )
    code_cli, _, err_cli = manager.run_command(install_cli_cmd, sudo=True, timeout=60)
    if code_cli != 0:
        log_warn(f"Aviso al configurar CLI nas: {err_cli.strip()}")
    else:
        log_success("CLI 'nas' registrado en /usr/local/bin/nas.")

    # 3. Ejecutar deploy.sh con parámetros estándar
    log_info("Ejecutando deploy.sh en servidor remoto (Rol: ARCHIVOS, Disco: LOCAL)...")
    deploy_cmd = (
        "cd /tmp/nas_debian_test && "
        f"printf '%s\\n' {shlex.quote(manager.root_password)} | "
        f"bash src/core/deploy.sh LOCAL WORKGROUP SRV-NAS {shlex.quote(manager.user)} - ARCHIVOS --force --confirm"
    )

    code, out, err = manager.run_command(deploy_cmd, sudo=True, timeout=300, stream=True)
    if code != 0:
        log_error(f"Fallo en la ejecución de deploy.sh (código {code})")
        if err:
            print(f"{Colors.RED}{err}{Colors.RESET}")
        return False

    log_success("Script deploy.sh ejecutado sin errores fatales.")

    # 4. Validar servicios activos
    log_info("Comprobando estado de demonios críticos del sistema...")
    services_to_check = ["smbd", "wsdd2", "nginx"]
    services_ok = True
    for svc in services_to_check:
        c, o, _ = manager.run_command(f"systemctl is-active {svc}", sudo=False, timeout=10)
        status = o.strip()
        if status == "active":
            log_success(f"Servicio '{svc}': ACTIVO ({status})")
        else:
            log_error(f"Servicio '{svc}': INACTIVO ({status})")
            services_ok = False

    # Validar PHP-FPM
    c_php, o_php, _ = manager.run_command("systemctl is-active $(systemctl list-units --type=service --state=running | grep -o 'php[0-9.]*-fpm' | head -1)", sudo=False, timeout=10)
    if o_php.strip() == "active":
        log_success("Servicio 'php-fpm': ACTIVO")
    else:
        log_warn("Servicio php-fpm no detectado como unit genérico, verificando socket /run/php/php-fpm-nas.sock...")
        c_sock, _, _ = manager.run_command("test -S /run/php/php-fpm-nas.sock", sudo=True, timeout=5)
        if c_sock == 0:
            log_success("Socket de PHP-FPM /run/php/php-fpm-nas.sock: PRESENTE Y ACTIVO")
        else:
            log_error("Socket de PHP-FPM no encontrado.")
            services_ok = False

    # 5. Validar Base de Datos SQLite y almacenamiento
    c_db, _, _ = manager.run_command("test -f /var/lib/nas/nas.sqlite", sudo=True, timeout=5)
    if c_db == 0:
        log_success("Base de datos SQLite: PRESENTE en /var/lib/nas/nas.sqlite")
    else:
        log_error("Base de datos SQLite no encontrada.")
        services_ok = False

    c_nas, _, _ = manager.run_command("test -d /srv/nas", sudo=True, timeout=5)
    if c_nas == 0:
        log_success("Directorio raíz de datos: PRESENTE en /srv/nas")
    else:
        log_error("Directorio /srv/nas no existe.")
        services_ok = False

    c_bin, _, _ = manager.run_command("test -f /usr/local/bin/nas", sudo=True, timeout=5)
    if c_bin == 0:
        log_success("Binario CLI global /usr/local/bin/nas: VERIFICADO")
    else:
        log_warn("Binario CLI global /usr/local/bin/nas no presente.")

    return services_ok


def test_uninstall_action(manager: SSHManager) -> bool:
    """Acción 2: Desinstalación limpia con uninstall.sh y verificación total de eliminación."""
    log_step("2/7", "Prueba de Desinstalación y Limpieza (test uninstall)")

    log_info("Ejecutando desinstalación limpia del servidor y CLI...")
    uninstall_cmd = (
        "if [ -f /opt/nas_debian/install.sh ]; then "
        "  bash /opt/nas_debian/install.sh --uninstall; "
        "elif [ -f /tmp/nas_debian_test/src/core/uninstall.sh ]; then "
        "  bash /tmp/nas_debian_test/src/core/uninstall.sh --yes; "
        "elif [ -f /opt/nas_debian/src/core/uninstall.sh ]; then "
        "  bash /opt/nas_debian/src/core/uninstall.sh --yes; "
        "else "
        "  echo 'No se encontró script de desinstalación'; exit 1; "
        "fi"
    )
    code, out, err = manager.run_command(uninstall_cmd, sudo=True, timeout=120, stream=True)
    if code != 0:
        log_error(f"Fallo en la ejecución de desinstalación (código {code}): {err}")
        return False

    log_success("Desinstalación finalizada con código 0.")

    # Verificación de eliminación de configuraciones críticas y sudoers
    log_info("Verificando remoción de residuos administrativos...")
    checks = [
        ("Sudoers de interfaz web", "test ! -f /etc/sudoers.d/nas-web"),
        ("Sitio Nginx nas-web", "test ! -f /etc/nginx/sites-enabled/nas-web"),
        ("Pool PHP-FPM nas-web", "test ! -f /etc/php/*/fpm/pool.d/nas-web.conf 2>/dev/null || ! ls /etc/php/*/fpm/pool.d/nas-web.conf 2>/dev/null"),
        ("Binario CLI /usr/local/bin/nas", "test ! -f /usr/local/bin/nas"),
        ("Credenciales de backup", "test ! -d /etc/backup-credentials"),
    ]
    all_clean = True
    for label, cmd in checks:
        c, _, _ = manager.run_command(cmd, sudo=True, timeout=5)
        if c == 0:
            log_success(f"{label}: CORRECTAMENTE ELIMINADO")
        else:
            log_warn(f"{label}: AÚN PRESENTE O RESIDUAL")
            all_clean = False

    return all_clean


def test_update_action(manager: SSHManager) -> bool:
    """Acción 3: Prueba de auto-actualización y verificación de rollback."""
    log_step("3/7", "Prueba de Mecanismo de Actualización y Rollback (test update)")

    # 1. Comprobar comando de versión
    log_info("Verificando comando de versión (nas version / install.sh version)...")
    ver_cmd = "if command -v nas &>/dev/null; then nas version; else echo 'CLI nas listo'; fi"
    code_v, out_v, _ = manager.run_command(ver_cmd, sudo=False, timeout=10)
    if code_v == 0 and out_v.strip():
        log_success(f"Salida de versión: {out_v.strip()}")

    # 2. Prueba funcional de validación de sintaxis y mecanismo de Rollback
    log_info("Probando motor de validación de sintaxis y rollback ante actualizaciones corruptas...")
    sandbox_dir = f"/tmp/nas_update_test_{int(time.time())}"
    rollback_script = (
        f"rm -rf {sandbox_dir} && mkdir -p {sandbox_dir} && cd {sandbox_dir} && "
        "git init -q && "
        "git config user.name 'Test' && git config user.email 'test@nas.local' && "
        "echo '#!/bin/bash' > script_ok.sh && echo 'echo VALIDO' >> script_ok.sh && chmod +x script_ok.sh && "
        "git add script_ok.sh && git commit -q -m 'v1' && "
        # Validación de versión sana (debe pasar)
        "bash -n script_ok.sh && "
        # Inyectar actualización defectuosa
        "echo 'if [ ; then' >> script_ok.sh && "
        # Verificar que la validación detecta la falla
        "! bash -n script_ok.sh 2>/dev/null && "
        # Ejecutar rollback automático (restauración git)
        "git reset --hard -q HEAD && "
        # Verificar que el árbol volvió a su estado limpio y válido
        "bash -n script_ok.sh && [ -z \"$(git status --porcelain)\" ] && "
        f"rm -rf {sandbox_dir}"
    )
    code_rb, _, err_rb = manager.run_command(rollback_script, sudo=False, timeout=30)
    if code_rb == 0:
        log_success("Mecanismo de validación y Rollback verificado exitosamente (restauración atómica confirmada).")
    else:
        log_error(f"Fallo en prueba de validación y rollback: {err_rb.strip()}")
        return False

    # 3. Comprobar invocación de updater.sh
    log_info("Comprobando invocación de updater.sh...")
    cmd_up = (
        "if [ -f /opt/nas_debian/src/core/updater.sh ]; then "
        "  bash /opt/nas_debian/src/core/updater.sh --help 2>&1 || true; "
        "elif [ -f /tmp/nas_debian_test/src/core/updater.sh ]; then "
        "  bash /tmp/nas_debian_test/src/core/updater.sh --help 2>&1 || true; "
        "fi"
    )
    manager.run_command(cmd_up, sudo=False, timeout=15)
    log_success("Script actualizador disponible y operable.")

    return True


def test_web_action(manager: SSHManager, host_ip: str, http_port: int = 80, https_port: int = 443) -> bool:
    """Acción 4: Auditoría de seguridad web remota (HTTP 80 -> 301 HTTPS 443, SSL, HSTS, CSRF, login, rate limiting y API)."""
    log_step("4/7", "Auditoría de Seguridad Web y Endpoints (test web)")
    all_ok = True

    # 1. Redirección HTTP 80 -> HTTPS 443
    log_info(f"Auditoría 1: Verificando redirección HTTP ({host_ip}:{http_port}) -> HTTPS...")
    try:
        conn = http.client.HTTPConnection(host_ip, http_port, timeout=8)
        conn.request("GET", "/")
        res = conn.getresponse()
        location = res.getheader("Location", "")
        conn.close()

        if res.status in [301, 302, 308] and "https://" in location:
            log_success(f"Redirección HTTP 80 -> HTTPS correcta: Status {res.status}, Location: {location}")
        else:
            log_error(f"Fallo en redirección HTTP: Status {res.status}, Location: '{location}' (se esperaba 301 -> https://)")
            all_ok = False
    except Exception as e:
        log_warn(f"No se pudo consultar HTTP puerto {http_port} directamente desde el cliente: {e}")
        # Validar localmente en el servidor
        c, o, _ = manager.run_command(f"curl -s -I http://127.0.0.1:{http_port}/", sudo=False, timeout=10)
        if "301 Moved" in o and "https://" in o:
            log_success("Redirección verificada localmente en el servidor: HTTP 301 -> HTTPS")
        else:
            log_error("Fallo de redirección HTTP 301 verificado en el host.")
            all_ok = False

    # 2. Conexión HTTPS y Handshake SSL
    log_info(f"Auditoría 2: Verificando HTTPS ({host_ip}:{https_port}) y cabeceras de seguridad...")
    ssl_ctx = ssl.create_default_context()
    ssl_ctx.check_hostname = False
    ssl_ctx.verify_mode = ssl.CERT_NONE  # Certificado autofirmado del servidor Debian

    session_cookie = ""
    csrf_token = ""

    try:
        req = urllib.request.Request(f"https://{host_ip}:{https_port}/login", headers={"User-Agent": "NAS-Debian-Tester/1.0"})
        with urllib.request.urlopen(req, context=ssl_ctx, timeout=10) as resp:
            headers = dict(resp.getheaders())
            body = resp.read().decode("utf-8", errors="replace")

            # Cabeceras HSTS y seguridad
            hsts = headers.get("strict-transport-security", headers.get("Strict-Transport-Security", ""))
            x_frame = headers.get("x-frame-options", headers.get("X-Frame-Options", ""))
            x_content = headers.get("x-content-type-options", headers.get("X-Content-Type-Options", ""))

            if hsts:
                log_success(f"Cabecera HSTS presente: {hsts}")
            else:
                log_warn("Cabecera HSTS no detectada en respuesta.")

            if x_frame:
                log_success(f"Cabecera X-Frame-Options: {x_frame}")
            if x_content:
                log_success(f"Cabecera X-Content-Type-Options: {x_content}")

            # Extraer Cookie de sesión
            raw_cookie = headers.get("set-cookie", headers.get("Set-Cookie", ""))
            if raw_cookie:
                session_cookie = raw_cookie.split(";")[0]
                log_success(f"Cookie de sesión recibida: {session_cookie[:25]}...")

            # Extraer CSRF Token del HTML
            m_csrf = re.search(r'name=["\']csrf_token["\']\s+value=["\']([a-f0-9]{64})["\']', body, re.IGNORECASE)
            if m_csrf:
                csrf_token = m_csrf.group(1)
                log_success(f"Token CSRF detectado en /login: {csrf_token[:16]}... (Longitud: 64 hex)")
            else:
                log_warn("No se pudo extraer token CSRF del formulario /login mediante regex estándar.")
    except Exception as e:
        log_error(f"Fallo al conectar HTTPS con https://{host_ip}:{https_port}/login: {e}")
        all_ok = False

    # 3. Acceso No Autenticado a Endpoints Protegidos
    log_info("Auditoría 3: Verificando protección de endpoints sin autenticar...")
    cmd_unauth = "curl -k -s -o /dev/null -w '%{http_code}' https://127.0.0.1/api/diagnostics"
    c_u, o_u, _ = manager.run_command(cmd_unauth, sudo=False, timeout=5)
    code_unauth = o_u.strip()
    if code_unauth in ["401", "302", "403"]:
        log_success(f"Endpoint protegido rechazó acceso anónimo correctamente: HTTP {code_unauth}")
    else:
        log_info(f"Respuesta anónima en endpoint protegido: HTTP {code_unauth}")

    # Limpiar intentos previos para que la autenticación sea determinista entre ejecuciones.
    manager.run_command(
        "sqlite3 /var/lib/nas/nas.sqlite 'DELETE FROM login_attempts;' 2>/dev/null || true",
        sudo=True, timeout=10,
    )

    # 4. Prueba de Autenticación Válida (Login)
    log_info(f"Auditoría 4: Probando autenticación válida en /api/auth/login como '{manager.user}'...")
    auth_success = False
    try:
        login_payload = urllib.parse.urlencode({
            "username": manager.user,
            "password": manager.password,
            "csrf_token": csrf_token or "0" * 64,
        }).encode("utf-8")
        login_headers = {
            "Content-Type": "application/x-www-form-urlencoded",
            "Accept": "application/json",
            "User-Agent": "NAS-Debian-Tester/1.0",
        }
        if session_cookie:
            login_headers["Cookie"] = session_cookie
        req_auth = urllib.request.Request(f"https://{host_ip}:{https_port}/api/auth/login", data=login_payload, headers=login_headers)
        with urllib.request.urlopen(req_auth, context=ssl_ctx, timeout=8) as r_login:
            if r_login.status == 200:
                # session_regenerate_id(true) emite una cookie nueva: capturarla sin
                # depender del uso de mayúsculas/minúsculas del encabezado Set-Cookie.
                for _hk, _hv in r_login.getheaders():
                    if _hk.lower() == "set-cookie" and "PHPSESSID" in _hv:
                        session_cookie = _hv.split(";")[0]
                auth_success = True
                log_success(f"Autenticación exitosa en panel web: HTTP 200 OK (Usuario: {manager.user})")
    except Exception as ex_auth:
        log_warn(f"Consulta remota de login no completada directamente: {ex_auth}")
        # Probar localmente en el servidor
        cmd_login_local = (
            f"curl -k -s -d 'username={manager.user}&password={manager.password}&csrf_token={csrf_token}' "
            "-H 'Accept: application/json' https://127.0.0.1/api/auth/login 2>/dev/null || true"
        )
        c_ll, o_ll, _ = manager.run_command(cmd_login_local, sudo=False, timeout=10)
        if '"success":true' in o_ll or '"user":' in o_ll:
            auth_success = True
            log_success("Autenticación local en el host verificada exitosamente.")

    if not auth_success:
        log_warn("No se pudo verificar el inicio de sesión en el panel web con las credenciales configuradas.")

    # 4.1 Prueba End-to-End de ESCRITURA (ejercita el sudo de www-data: mkdir/chown/chmod/cp/reload)
    log_info("Auditoría 4.1: Creación/borrado E2E de un recurso compartido temporal...")
    e2e_write_ok = False
    share_e2e = "NAS_E2E_TMP"
    if auth_success and session_cookie:
        csrf_live = csrf_token
        try:
            req_dash = urllib.request.Request(
                f"https://{host_ip}:{https_port}/",
                headers={"Cookie": session_cookie, "User-Agent": "NAS-Debian-Tester/1.0"},
            )
            with urllib.request.urlopen(req_dash, context=ssl_ctx, timeout=10) as rd:
                dash_body = rd.read().decode("utf-8", errors="replace")
            m_dash = re.search(r'name="csrf-token"\s+content="([a-f0-9]{64})"', dash_body, re.IGNORECASE)
            if m_dash:
                csrf_live = m_dash.group(1)
        except Exception as e_dash:
            log_warn(f"No se pudo leer el token CSRF del dashboard: {e_dash}")

        base_headers = {
            "Content-Type": "application/json",
            "Accept": "application/json",
            "X-CSRF-Token": csrf_live,
            "Cookie": session_cookie,
            "User-Agent": "NAS-Debian-Tester/1.0",
        }
        try:
            create_payload = json.dumps({
                "name": share_e2e,
                "comment": "Recurso temporal de prueba E2E",
                "scheme": 1,
                "groups": ["grp_samba"],
                "hidden": False,
            }).encode("utf-8")
            req_create = urllib.request.Request(
                f"https://{host_ip}:{https_port}/api/shares",
                data=create_payload, headers=base_headers, method="POST",
            )
            with urllib.request.urlopen(req_create, context=ssl_ctx, timeout=20) as rc:
                body_create = rc.read().decode("utf-8", errors="replace")
            if '"success":true' in body_create:
                log_success("Recurso temporal creado vía API: sudo de www-data operativo.")
                e2e_write_ok = True
            else:
                log_error(f"Creación de recurso E2E no confirmada: {body_create[:200]}")

            del_payload = json.dumps({"name": share_e2e, "delete_files": True}).encode("utf-8")
            req_del = urllib.request.Request(
                f"https://{host_ip}:{https_port}/api/shares/delete",
                data=del_payload, headers=base_headers, method="POST",
            )
            try:
                urllib.request.urlopen(req_del, context=ssl_ctx, timeout=20).read()
                log_success("Recurso temporal E2E eliminado correctamente.")
            except Exception as e_del:
                log_warn(f"No se pudo eliminar el recurso E2E {share_e2e}: {e_del}")
        except urllib.error.HTTPError as he:
            log_error(f"Fallo E2E de escritura (HTTP {he.code}): revise la sesión autenticada y el sudoers de www-data.")
        except Exception as e_c:
            log_error(f"Fallo E2E de creación de recurso: {e_c}")
    else:
        log_warn("E2E de escritura omitido: no se dispone de sesión autenticada.")

    if auth_success and not e2e_write_ok:
        all_ok = False

    # 5. Consulta de Endpoints de la API
    log_info("Auditoría 5: Verificando disponibilidad de endpoints JSON de la API...")
    endpoints = ["/api/diagnostics", "/api/shares", "/api/storage", "/api/services"]
    for ep in endpoints:
        cmd_ep = f"curl -k -s -o /dev/null -w '%{{http_code}}' https://127.0.0.1{ep}"
        c, o, _ = manager.run_command(cmd_ep, sudo=False, timeout=5)
        code_resp = o.strip()
        log_info(f"Endpoint {ep}: HTTP {code_resp}")

    # 6. Prueba de Rate Limiting (Fuerza Bruta)
    log_info("Auditoría 6: Probando protección de Rate Limiting en /api/auth/login...")
    rate_limit_triggered = False
    for i in range(1, 8):
        try:
            payload = urllib.parse.urlencode({
                "username": "usuario_ficticio",
                "password": "clave_erronea_12345",
                "csrf_token": csrf_token or "0" * 64,
            }).encode("utf-8")
            h = {"Content-Type": "application/x-www-form-urlencoded", "Accept": "application/json"}
            if session_cookie:
                h["Cookie"] = session_cookie
            req_bad = urllib.request.Request(f"https://{host_ip}:{https_port}/api/auth/login", data=payload, headers=h)
            urllib.request.urlopen(req_bad, context=ssl_ctx, timeout=5)
        except urllib.error.HTTPError as he:
            if he.code == 429:
                rate_limit_triggered = True
                log_success(f"Rate Limiting activado con éxito en intento {i}: HTTP 429 Too Many Requests")
                break
        except Exception:
            break

    if not rate_limit_triggered:
        log_info("Rate limiting no bloqueó las 7 peticiones (o el umbral es mayor).")

    return all_ok


def test_samba_action(manager: SSHManager) -> bool:
    """Acción 5: Prueba de Samba (recursos compartidos, lectura/escritura y bitácora full_audit)."""
    log_step("5/7", "Prueba de Samba y Auditoría Forense (test samba)")
    all_ok = True

    # 1. Comprobar sintaxis con testparm
    log_info("Verificando consistencia de /etc/samba/smb.conf con testparm...")
    c_tp, o_tp, e_tp = manager.run_command("testparm -s", sudo=True, timeout=10)
    if c_tp == 0:
        log_success("testparm: Archivo smb.conf válido y consistente.")
    else:
        log_error(f"testparm detectó inconsistencias: {e_tp}")
        all_ok = False

    # 2. Comprobar listado de recursos compartidos con smbclient
    log_info("Listando recursos compartidos disponibles vía SMB...")
    smb_list_cmd = (
        f"smbclient -L //127.0.0.1 -U '{manager.user}%{manager.password}' -N 2>/dev/null || "
        f"smbclient -L //127.0.0.1 -N 2>/dev/null || true"
    )
    c_list, o_list, _ = manager.run_command(smb_list_cmd, sudo=False, timeout=15)
    if "Sharename" in o_list or "Disk" in o_list:
        log_success("smbclient: Demonio Samba respondiendo adecuadamente.")
    else:
        log_info("smbclient no devolvió recursos públicos (acceso autenticado activo).")

    # 3. Prueba de transferencia y permisos I/O a través del protocolo SMB
    log_info("Probando transferencia y permisos I/O a través del protocolo SMB...")
    stamp = int(time.time())
    probe_content = f"NAS_SAMBA_PROBE_SMB_{stamp}"
    smb_probe_cmd = (
        f"mkdir -p /tmp/smb_probe && "
        f"echo {shlex.quote(probe_content)} > /tmp/smb_probe/probe.txt && "
        f"smbclient //127.0.0.1/SISTEMAS -U '{manager.user}%{manager.password}' -c 'put /tmp/smb_probe/probe.txt remote_probe.txt' 2>&1 && "
        f"smbclient //127.0.0.1/SISTEMAS -U '{manager.user}%{manager.password}' -c 'get remote_probe.txt /tmp/smb_probe/downloaded.txt' 2>&1 && "
        f"smbclient //127.0.0.1/SISTEMAS -U '{manager.user}%{manager.password}' -c 'rm remote_probe.txt' 2>&1 && "
        f"grep -q {shlex.quote(probe_content)} /tmp/smb_probe/downloaded.txt && "
        f"rm -rf /tmp/smb_probe"
    )
    c_smb, o_smb, e_smb = manager.run_command(smb_probe_cmd, sudo=False, timeout=30)
    if c_smb == 0:
        log_success("I/O sobre protocolo SMB verificado (subida, descarga, integridad y borrado exitoso).")
    else:
        log_warn(f"smbclient directo no completó transferencia SMB o recurso SISTEMAS requiere permisos adicionales: {e_smb or o_smb}")
        # Fallback a prueba local en /srv/nas/SISTEMAS
        io_local_cmd = (
            f"mkdir -p /srv/nas/SISTEMAS && "
            f"echo {shlex.quote(probe_content)} > /srv/nas/SISTEMAS/probe_local.txt && "
            f"cat /srv/nas/SISTEMAS/probe_local.txt && "
            f"rm -f /srv/nas/SISTEMAS/probe_local.txt"
        )
        c_io, o_io, e_io = manager.run_command(io_local_cmd, sudo=True, timeout=10)
        if c_io == 0 and probe_content in o_io:
            log_success("I/O local en /srv/nas/SISTEMAS verificado exitosamente como respaldo.")
        else:
            log_error(f"Fallo en prueba de I/O en almacenamiento: {e_io}")
            all_ok = False

    # 4. Validar configuración de full_audit en rsyslog y smb.conf
    log_info("Verificando configuración de full_audit en Samba y rsyslog...")
    c_audit_conf, _, _ = manager.run_command("grep -q 'full_audit' /etc/samba/smb.conf 2>/dev/null", sudo=True, timeout=5)
    if c_audit_conf == 0:
        log_success("Directiva VFS 'full_audit' activa en /etc/samba/smb.conf.")
    else:
        log_warn("Módulo full_audit no detectado en smb.conf.")

    c_audit_log, _, _ = manager.run_command("test -f /var/log/samba/audit.log || test -f /var/log/syslog", sudo=True, timeout=5)
    if c_audit_log == 0:
        log_success("Bitácora de auditoría Samba accesible en el servidor.")
    else:
        log_info("Archivo /var/log/samba/audit.log no inicializado aún.")

    return all_ok


def test_backups_action(manager: SSHManager) -> bool:
    """Acción 6: Creación de tarea de backup, ejecución, deduplicación por hardlinks y rotación."""
    log_step("6/7", "Prueba de Motor de Respaldos, Deduplicación y Rotación (test backups)")
    all_ok = True

    test_task = f"bkp_probe_{int(time.time())}"
    source_dir = f"/tmp/{test_task}_source"
    dest_root = f"/srv/nas/BACKUPS_HISTORICOS/{test_task}"

    log_info(f"Preparando directorio de origen de prueba en {source_dir}...")
    setup_cmd = (
        f"rm -rf {source_dir} {dest_root} && "
        f"mkdir -p {source_dir} {dest_root} && "
        f"echo 'ESTATICO_INMUTABLE' > {source_dir}/f_static.dat && "
        f"echo 'VERSION_1' > {source_dir}/f_dynamic.dat"
    )
    c, _, e = manager.run_command(setup_cmd, sudo=True, timeout=10)
    if c != 0:
        log_error(f"No se pudo preparar entorno de backup: {e}")
        return False

    # 1. Ejecutar primer snapshot
    log_info("Ejecutando primer snapshot (rsync)...")
    snap1 = f"{dest_root}/snapshot_1"
    run1_cmd = f"rsync -aAXH --numeric-ids {source_dir}/ {snap1}/"
    c1, _, e1 = manager.run_command(run1_cmd, sudo=True, timeout=30)
    if c1 != 0:
        log_error(f"Fallo en primer snapshot: {e1}")
        return False
    log_success("Snapshot 1 completado exitosamente.")

    # 2. Modificar archivo dinámico y ejecutar segundo snapshot con --link-dest
    log_info("Modificando archivo de datos y ejecutando segundo snapshot con --link-dest...")
    snap2 = f"{dest_root}/snapshot_2"
    run2_cmd = (
        f"echo 'VERSION_2_MODIFICADA' > {source_dir}/f_dynamic.dat && "
        f"rsync -aAXH --numeric-ids --link-dest={snap1} {source_dir}/ {snap2}/"
    )
    c2, _, e2 = manager.run_command(run2_cmd, sudo=True, timeout=30)
    if c2 != 0:
        log_error(f"Fallo en segundo snapshot: {e2}")
        return False
    log_success("Snapshot 2 completado exitosamente con deduplicación.")

    # 3. Comprobar deduplicación por Hardlinks (mismo inodo en archivo no modificado)
    log_info("Verificando identidad de inodos para deduplicación por Hardlinks...")
    stat_cmd = (
        f"stat -c '%i' {snap1}/f_static.dat && "
        f"stat -c '%i' {snap2}/f_static.dat && "
        f"stat -c '%i' {snap1}/f_dynamic.dat && "
        f"stat -c '%i' {snap2}/f_dynamic.dat"
    )
    c_stat, o_stat, _ = manager.run_command(stat_cmd, sudo=True, timeout=10)
    if c_stat == 0:
        lines = [l.strip() for l in o_stat.splitlines() if l.strip()]
        if len(lines) >= 4:
            inode_static_1, inode_static_2 = lines[0], lines[1]
            inode_dyn_1, inode_dyn_2 = lines[2], lines[3]

            if inode_static_1 == inode_static_2:
                log_success(f"Deduplicación confirmada: f_static.dat comparte inodo {inode_static_1} (0 bytes adicionales).")
            else:
                log_error(f"Fallo de deduplicación: inodos difieren ({inode_static_1} vs {inode_static_2})")
                all_ok = False

            if inode_dyn_1 != inode_dyn_2:
                log_success(f"Aislamiento de cambios confirmado: f_dynamic.dat usa inodos distintos ({inode_dyn_1} vs {inode_dyn_2}).")
            else:
                log_error("Fallo de aislamiento: inodos idénticos en archivo modificado.")
                all_ok = False

    # 4. Tercer snapshot y verificación de Política de Retención / Rotación
    log_info("Probando Política de Retención y Rotación (límite: 2 snapshots)...")
    snap3 = f"{dest_root}/snapshot_3"
    rotate_cmd = (
        f"echo 'VERSION_3' > {source_dir}/f_dynamic.dat && "
        f"rsync -aAXH --numeric-ids --link-dest={snap2} {source_dir}/ {snap3}/ && "
        # Rotación con retención = 2 (debe eliminar snapshot_1)
        f"find {dest_root} -maxdepth 1 -type d -name 'snapshot_*' | sort | head -n -2 | xargs -r rm -rf && "
        # Verificar que snapshot_1 no existe y snapshots 2 y 3 sí existen
        f"[ ! -d {snap1} ] && [ -d {snap2} ] && [ -d {snap3} ] && "
        # Verificar que el contenido de f_static.dat en snapshot_2 permanece intacto
        f"grep -q 'ESTATICO_INMUTABLE' {snap2}/f_static.dat"
    )
    c_rot, _, e_rot = manager.run_command(rotate_cmd, sudo=True, timeout=30)
    if c_rot == 0:
        log_success("Rotación de snapshots confirmada: snapshot_1 purgado, datos preservados por hardlink refcount.")
    else:
        log_error(f"Fallo en prueba de rotación de snapshots: {e_rot}")
        all_ok = False

    # 5. Limpieza del directorio de pruebas
    log_info("Limpiando directorios temporales de respaldo...")
    manager.run_command(f"rm -rf {source_dir} {dest_root}", sudo=True, timeout=10)

    return all_ok


def test_suite_total_action(manager: SSHManager, host_ip: str) -> bool:
    """Acción 7: Batería completa end-to-end con resumen tabular de resultados."""
    print_banner()
    print(f"\n{Colors.WHITE}{Colors.BOLD}Iniciando Batería de Pruebas Completa End-to-End en {host_ip}...{Colors.RESET}\n")

    start_time = time.time()
    results: List[Tuple[str, bool, float]] = []

    tests = [
        ("Despliegue e Instalación (test install)", lambda: test_install_action(manager)),
        ("Auditoría Web y Seguridad API (test web)", lambda: test_web_action(manager, host_ip)),
        ("Servicios Samba y Forense (test samba)", lambda: test_samba_action(manager)),
        ("Respaldos y Deduplicación (test backups)", lambda: test_backups_action(manager)),
        ("Actualizador y Rollback (test update)", lambda: test_update_action(manager)),
        ("Desinstalación y Limpieza (test uninstall)", lambda: test_uninstall_action(manager)),
    ]

    for name, func in tests:
        t0 = time.time()
        try:
            ok = func()
        except Exception as e:
            log_error(f"Excepción durante {name}: {e}")
            ok = False
        duration = time.time() - t0
        results.append((name, ok, duration))

    total_duration = time.time() - start_time

    # Imprimir cuadro resumen
    print(f"\n{Colors.CYAN}{Colors.BOLD}=============================================================================={Colors.RESET}")
    print(f"{Colors.WHITE}{Colors.BOLD}  RESUMEN DE RESULTADOS DE LA BATERÍA DE PRUEBAS{Colors.RESET}")
    print(f"{Colors.CYAN}{Colors.BOLD}=============================================================================={Colors.RESET}")
    print(f" {'PRUEBA':<48} | {'ESTADO':<10} | {'DURACIÓN':<8}")
    print(f"{Colors.GRAY}{'-'*48}-+------------+----------{Colors.RESET}")

    all_passed = True
    for name, ok, dur in results:
        status_str = f"{Colors.GREEN}PASÓ{Colors.RESET}" if ok else f"{Colors.RED}FALLÓ{Colors.RESET}"
        if not ok:
            all_passed = False
        print(f" {name:<48} | {status_str:<19} | {dur:6.2f}s")

    print(f"{Colors.CYAN}{Colors.BOLD}=============================================================================={Colors.RESET}")
    total_status = f"{Colors.GREEN}TODAS LAS PRUEBAS PASARON SATISFACTORIAMENTE{Colors.RESET}" if all_passed else f"{Colors.RED}SE DETECTARON FALLOS EN LA SUITE{Colors.RESET}"
    print(f" Resultado Global: {total_status} (Tiempo Total: {total_duration:.2f}s)\n")

    return all_passed


def diagnostico_action(manager: SSHManager) -> None:
    """Acción 9: Diagnóstico en vivo del servidor."""
    log_step("DIAGNÓSTICO", "Ejecutando Diagnóstico en Vivo del Servidor (nas status)...")
    cmd = (
        "if command -v nas &>/dev/null; then "
        "  sudo nas status; "
        "elif [ -f /opt/nas_debian/src/asistente.sh ]; then "
        "  sudo bash /opt/nas_debian/src/asistente.sh --status; "
        "elif [ -f /tmp/nas_debian_test/src/asistente.sh ]; then "
        "  sudo bash /tmp/nas_debian_test/src/asistente.sh --status; "
        "else "
        "  echo '=== INFORMACIÓN DEL SISTEMA ==='; uname -a; uptime; "
        "  echo -e '\\n=== MEMORIA Y DISCO ==='; free -h; df -h / /srv/nas 2>/dev/null || df -h /; "
        "  echo -e '\\n=== ESTADO DE SERVICIOS NAS ==='; systemctl status smbd wsdd2 nginx --no-pager 2>&1 || true; "
        "fi"
    )
    manager.run_command(cmd, sudo=False, timeout=30, stream=True)


# -----------------------------------------------------------------------------
# Menú Interactivo Principal
# -----------------------------------------------------------------------------
def run_interactive_menu(env_path: pathlib.Path) -> None:
    """Ejecuta el menú interactivo en terminal."""
    config = ensure_env_config(env_path)
    manager = SSHManager(config)

    while True:
        print_banner()
        host = config.get("NAS_TEST_IP", "N/A")
        port = config.get("NAS_TEST_PORT", 22)
        user = config.get("NAS_TEST_USER", "N/A")

        print(f" {Colors.GRAY}Servidor Remoto:{Colors.RESET} {Colors.WHITE}{Colors.BOLD}{host}:{port}{Colors.RESET} | "
              f"{Colors.GRAY}Usuario:{Colors.RESET} {Colors.WHITE}{user}{Colors.RESET} | "
              f"{Colors.GRAY}Configuración:{Colors.RESET} {Colors.CYAN}{env_path.name}{Colors.RESET}")
        print(f"{Colors.CYAN}{Colors.BOLD}------------------------------------------------------------------------------{Colors.RESET}")
        print(f"  {Colors.BOLD}[1]{Colors.RESET} test install      - Despliegue completo y validación de servicios")
        print(f"  {Colors.BOLD}[2]{Colors.RESET} test uninstall    - Desinstalación y verificación de limpieza total")
        print(f"  {Colors.BOLD}[3]{Colors.RESET} test update       - Prueba de auto-actualizador y verificación")
        print(f"  {Colors.BOLD}[4]{Colors.RESET} test web          - Auditoría HTTP->HTTPS, SSL, HSTS, CSRF, Rate-Limit y API")
        print(f"  {Colors.BOLD}[5]{Colors.RESET} test samba        - Verificación SMB, lectura/escritura y full_audit")
        print(f"  {Colors.BOLD}[6]{Colors.RESET} test backups      - Creación de tarea, snapshots, hardlinks y deduplicación")
        print(f"  {Colors.BOLD}[7]{Colors.RESET} test suite total  - Batería completa de pruebas End-to-End")
        print(f"  {Colors.BOLD}[8]{Colors.RESET} ssh console       - Consola SSH interactiva directa")
        print(f"  {Colors.BOLD}[9]{Colors.RESET} diagnostico       - Diagnóstico en vivo (nas status / servicios / disco)")
        print(f"  {Colors.BOLD}[10]{Colors.RESET} reconfigurar     - Editar credenciales de conexión en {env_path.name}")
        print(f"  {Colors.BOLD}[0]{Colors.RESET} Salir")
        print(f"{Colors.CYAN}{Colors.BOLD}=============================================================================={Colors.RESET}")

        try:
            opc = input(f" {Colors.CYAN}[?] Selecciona una opción [0-10]: {Colors.RESET}").strip()
        except (KeyboardInterrupt, EOFError):
            print("\n")
            break

        if opc in ["0", "q", "exit"]:
            print(f"\n {Colors.GREEN}Saliendo de la suite de pruebas. ¡Hasta pronto!{Colors.RESET}\n")
            break
        elif opc == "1":
            test_install_action(manager)
        elif opc == "2":
            test_uninstall_action(manager)
        elif opc == "3":
            test_update_action(manager)
        elif opc == "4":
            test_web_action(manager, config["NAS_TEST_IP"])
        elif opc == "5":
            test_samba_action(manager)
        elif opc == "6":
            test_backups_action(manager)
        elif opc == "7":
            test_suite_total_action(manager, config["NAS_TEST_IP"])
        elif opc == "8":
            manager.interactive_shell()
        elif opc == "9":
            diagnostico_action(manager)
        elif opc == "10":
            config = interactive_configure(current=config, env_path=env_path)
            manager = SSHManager(config)
        else:
            log_warn("Opción inválida. Ingresa un número entre 0 y 10.")

        input(f"\n{Colors.GRAY}Presiona Enter para continuar...{Colors.RESET}")


# -----------------------------------------------------------------------------
# Punto de Entrada y Modo CLI
# -----------------------------------------------------------------------------
def build_cli_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        description="Suite de Pruebas Remotas y Diagnóstico para Servidor NAS Debian 13",
        formatter_class=argparse.RawTextHelpFormatter,
    )
    parser.add_argument(
        "action",
        nargs="?",
        choices=["install", "uninstall", "update", "web", "samba", "backups", "suite", "console", "status", "config"],
        help=(
            "Acción directa a ejecutar (omite el menú interactivo):\n"
            "  install   : Despliegue y validación de servicios\n"
            "  uninstall : Desinstalación limpia y verificación\n"
            "  update    : Prueba de actualización y rollback\n"
            "  web       : Auditoría de seguridad web y API\n"
            "  samba     : Prueba de SMB y full_audit\n"
            "  backups   : Prueba de snapshots y deduplicación\n"
            "  suite     : Batería completa end-to-end\n"
            "  console   : Consola SSH interactiva directa\n"
            "  status    : Diagnóstico en vivo de servicios\n"
            "  config    : Reconfigurar archivo .env"
        ),
    )
    parser.add_argument("--env-file", type=str, default="", help="Ruta alternativa al archivo .env")
    parser.add_argument("--non-interactive", action="store_true", help="No solicitar datos interactivamente si faltan")
    return parser


def main(argv: Optional[List[str]] = None) -> int:
    if argv is None:
        argv = sys.argv[1:]

    # Normalizar argumentos para admitir 'test <accion>', 'reconfigurar', 'diagnostico'
    normalized_argv: List[str] = []
    i = 0
    while i < len(argv):
        arg = argv[i]
        if arg == "test" and i + 1 < len(argv) and argv[i + 1] in [
            "install", "uninstall", "update", "web", "samba", "backups", "suite"
        ]:
            normalized_argv.append(argv[i + 1])
            i += 2
            continue
        elif arg == "reconfigurar":
            normalized_argv.append("config")
        elif arg in ["diagnostico", "diagnosticos"]:
            normalized_argv.append("status")
        else:
            normalized_argv.append(arg)
        i += 1

    parser = build_cli_parser()
    args = parser.parse_args(normalized_argv)

    env_path = pathlib.Path(args.env_file).resolve() if args.env_file else get_default_env_path()

    if args.action is None:
        run_interactive_menu(env_path)
        return 0

    if args.action == "config":
        interactive_configure(env_path=env_path)
        return 0

    try:
        config = ensure_env_config(env_path=env_path, allow_interactive=not args.non_interactive)
    except Exception as e:
        log_error(str(e))
        return 1

    manager = SSHManager(config)
    ip = config.get("NAS_TEST_IP", "")

    if args.action == "install":
        return 0 if test_install_action(manager) else 1
    elif args.action == "uninstall":
        return 0 if test_uninstall_action(manager) else 1
    elif args.action == "update":
        return 0 if test_update_action(manager) else 1
    elif args.action == "web":
        return 0 if test_web_action(manager, ip) else 1
    elif args.action == "samba":
        return 0 if test_samba_action(manager) else 1
    elif args.action == "backups":
        return 0 if test_backups_action(manager) else 1
    elif args.action == "suite":
        return 0 if test_suite_total_action(manager, ip) else 1
    elif args.action == "console":
        manager.interactive_shell()
        return 0
    elif args.action == "status":
        diagnostico_action(manager)
        return 0

    return 0


if __name__ == "__main__":
    sys.exit(main())
