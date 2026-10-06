"""
==============================================================================
Pruebas Unitarias para Suite de Pruebas Remotas (test_remote.py)
==============================================================================
Verifica la carga, guardado y validación de archivos .env, la política estricta
de verificación de claves de host (accept-new), el parseador CLI y las rutinas
de prueba y empaquetado seguro.
==============================================================================
"""

import pathlib
import sys
import unittest.mock as mock
import pytest

# Asegurar importación de test_remote desde la raíz del proyecto
PROJECT_ROOT = pathlib.Path(__file__).resolve().parent.parent
if str(PROJECT_ROOT) not in sys.path:
    sys.path.insert(0, str(PROJECT_ROOT))

import test_remote  # noqa: E402


class DummyKey:
    """Mock de clave pública para pruebas de host keys."""
    def __init__(self, key_type: str = "ssh-ed25519", data: bytes = b"key-sample-bytes-1"):
        self.key_type = key_type
        self.data = data

    def get_name(self) -> str:
        return self.key_type

    def asbytes(self) -> bytes:
        return self.data


class DummyHostKeys(dict):
    """Mock de contenedor de claves de host en paramiko."""
    def add(self, hostname: str, key_type: str, key: DummyKey) -> None:
        if hostname not in self:
            self[hostname] = {}
        self[hostname][key_type] = key


class DummySSHClient:
    """Mock de SSHClient para validar políticas de clave de host."""
    def __init__(self, host_keys: DummyHostKeys):
        self._host_keys = host_keys

    def get_host_keys(self) -> DummyHostKeys:
        return self._host_keys

    def save_host_keys(self, filename: str) -> None:
        pass


# -----------------------------------------------------------------------------
# 1. Pruebas de Gestión de Archivo .env
# -----------------------------------------------------------------------------
def test_load_env_file_parses_cleanly(tmp_path: pathlib.Path) -> None:
    env_file = tmp_path / ".env"
    env_file.write_text(
        "# Comentario de cabecera\n"
        "NAS_TEST_IP=192.168.1.100\n"
        "NAS_TEST_PORT=2222\n"
        "NAS_TEST_USER=\"admin_prueba\"\n"
        "NAS_TEST_PASSWORD='clave_secreta_123'\n"
        "NAS_ROOT_PASSWORD=clave_root_456\n"
        "\n"
        "# Línea vacía arriba\n"
        "OTRA_VAR=1\n",
        encoding="utf-8",
    )

    data = test_remote.load_env_file(env_file)
    assert data["NAS_TEST_IP"] == "192.168.1.100"
    assert data["NAS_TEST_PORT"] == "2222"
    assert data["NAS_TEST_USER"] == "admin_prueba"
    assert data["NAS_TEST_PASSWORD"] == "clave_secreta_123"
    assert data["NAS_ROOT_PASSWORD"] == "clave_root_456"
    assert data["OTRA_VAR"] == "1"


def test_load_env_file_missing_returns_empty(tmp_path: pathlib.Path) -> None:
    env_file = tmp_path / ".env.inexistente"
    data = test_remote.load_env_file(env_file)
    assert data == {}


def test_save_env_file_writes_and_reloads(tmp_path: pathlib.Path) -> None:
    env_file = tmp_path / ".env"
    payload = {
        "NAS_TEST_IP": "10.20.30.40",
        "NAS_TEST_PORT": "22",
        "NAS_TEST_USER": "sistemas",
        "NAS_TEST_PASSWORD": "password123",
        "NAS_ROOT_PASSWORD": "rootpassword456",
    }
    assert test_remote.save_env_file(env_file, payload) is True
    reloaded = test_remote.load_env_file(env_file)
    assert reloaded["NAS_TEST_IP"] == "10.20.30.40"
    assert reloaded["NAS_TEST_USER"] == "sistemas"
    assert reloaded["NAS_TEST_PASSWORD"] == "password123"
    assert reloaded["NAS_ROOT_PASSWORD"] == "rootpassword456"


def test_validate_env_config_detects_missing() -> None:
    incomplete = {"NAS_TEST_IP": "10.0.0.1"}
    valid, missing = test_remote.validate_env_config(incomplete)
    assert valid is False
    assert "NAS_TEST_USER" in missing
    assert "NAS_TEST_PASSWORD" in missing

    complete = {
        "NAS_TEST_IP": "10.0.0.1",
        "NAS_TEST_USER": "sistemas",
        "NAS_TEST_PASSWORD": "pass",
    }
    valid2, missing2 = test_remote.validate_env_config(complete)
    assert valid2 is True
    assert missing2 == []


def test_normalize_env_config_defaults() -> None:
    raw = {
        "NAS_TEST_IP": "10.10.1.2",
        "NAS_TEST_USER": "sistemas",
        "NAS_TEST_PASSWORD": "user_pass",
    }
    normalized = test_remote.normalize_env_config(raw)
    assert normalized["NAS_TEST_PORT"] == 22
    assert normalized["NAS_ROOT_PASSWORD"] == "user_pass"


# -----------------------------------------------------------------------------
# 2. Pruebas de Política Estricta de Claves de Host (accept-new)
# -----------------------------------------------------------------------------
def test_strict_accept_new_policy_accepts_new_key(tmp_path: pathlib.Path) -> None:
    known_hosts = tmp_path / "known_hosts"
    policy = test_remote.StrictAcceptNewPolicy(known_hosts)
    host_keys = DummyHostKeys()
    client = DummySSHClient(host_keys)

    new_key = DummyKey("ssh-ed25519", b"bytes-new-host")
    policy.missing_host_key(client, "10.10.1.2", new_key)

    assert "10.10.1.2" in host_keys
    assert host_keys["10.10.1.2"]["ssh-ed25519"] == new_key


def test_strict_accept_new_policy_rejects_altered_key(tmp_path: pathlib.Path) -> None:
    known_hosts = tmp_path / "known_hosts"
    policy = test_remote.StrictAcceptNewPolicy(known_hosts)
    host_keys = DummyHostKeys()
    original_key = DummyKey("ssh-ed25519", b"bytes-original")
    host_keys.add("10.10.1.2", "ssh-ed25519", original_key)

    client = DummySSHClient(host_keys)
    altered_key = DummyKey("ssh-ed25519", b"bytes-altered-mitm")

    # Debe lanzar paramiko.SSHException y NO permitir la conexión
    if test_remote.paramiko is not None:
        with pytest.raises(test_remote.paramiko.SSHException) as exc_info:
            policy.missing_host_key(client, "10.10.1.2", altered_key)
        assert "VIOLACIÓN DE SEGURIDAD" in str(exc_info.value)


# -----------------------------------------------------------------------------
# 3. Pruebas de Parser CLI y Argumentos
# -----------------------------------------------------------------------------
def test_cli_parser_subcommands() -> None:
    parser = test_remote.build_cli_parser()

    for cmd in ["install", "uninstall", "update", "web", "samba", "backups", "suite", "console", "status", "config"]:
        args = parser.parse_args([cmd])
        assert args.action == cmd

    args_custom = parser.parse_args(["web", "--env-file", "custom.env", "--non-interactive"])
    assert args_custom.action == "web"
    assert args_custom.env_file == "custom.env"
    assert args_custom.non_interactive is True


def test_cli_parser_defaults_to_none_action() -> None:
    parser = test_remote.build_cli_parser()
    args = parser.parse_args([])
    assert args.action is None


# -----------------------------------------------------------------------------
# 4. Pruebas de Ejecución Remota Mockeada y Resiliencia
# -----------------------------------------------------------------------------
def test_ssh_manager_test_connection_success() -> None:
    config = {
        "NAS_TEST_IP": "10.10.1.2",
        "NAS_TEST_PORT": 22,
        "NAS_TEST_USER": "sistemas",
        "NAS_TEST_PASSWORD": "pass",
        "NAS_ROOT_PASSWORD": "pass",
    }
    manager = test_remote.SSHManager(config)

    with mock.patch.object(manager, "connect"):
        with mock.patch.object(manager, "run_command") as mock_run:
            # whoami -> sistemas, id -u -> 0
            mock_run.side_effect = [
                (0, "sistemas\n", ""),
                (0, "0\n", ""),
            ]
            ok, msg = manager.test_connection()
            assert ok is True
            assert "Conexión exitosa" in msg


def test_ssh_manager_test_connection_failure() -> None:
    config = {
        "NAS_TEST_IP": "10.10.1.2",
        "NAS_TEST_PORT": 22,
        "NAS_TEST_USER": "sistemas",
        "NAS_TEST_PASSWORD": "bad_pass",
        "NAS_ROOT_PASSWORD": "bad_pass",
    }
    manager = test_remote.SSHManager(config)

    with mock.patch.object(manager, "connect", side_effect=Exception("Conexión rechazada")):
        ok, msg = manager.test_connection()
        assert ok is False
        assert "Conexión rechazada" in msg


def test_test_install_action_success() -> None:
    config = {
        "NAS_TEST_IP": "10.10.1.2",
        "NAS_TEST_PORT": 22,
        "NAS_TEST_USER": "sistemas",
        "NAS_TEST_PASSWORD": "pass",
        "NAS_ROOT_PASSWORD": "pass",
    }
    manager = test_remote.SSHManager(config)

    with mock.patch.object(manager, "sync_project_files", return_value=(True, "OK")):
        with mock.patch.object(manager, "run_command") as mock_run:
            # deploy.sh (0), smbd (active), wsdd2 (active), nginx (active), php-fpm (active), sqlite (0), srv/nas (0)
            mock_run.side_effect = [
                (0, "Despliegue finalizado\n", ""),
                (0, "active\n", ""),
                (0, "active\n", ""),
                (0, "active\n", ""),
                (0, "active\n", ""),
                (0, "", ""),
                (0, "", ""),
            ]
            result = test_remote.test_install_action(manager)
            assert result is True


def test_test_backups_dedup_success() -> None:
    config = {
        "NAS_TEST_IP": "10.10.1.2",
        "NAS_TEST_PORT": 22,
        "NAS_TEST_USER": "sistemas",
        "NAS_TEST_PASSWORD": "pass",
        "NAS_ROOT_PASSWORD": "pass",
    }
    manager = test_remote.SSHManager(config)

    with mock.patch.object(manager, "run_command") as mock_run:
        # 1. setup (0)
        # 2. rsync snap1 (0)
        # 3. rsync snap2 (0)
        # 4. stat inodos: inodo 100, 100, 201, 202
        # 5. cleanup (0)
        mock_run.side_effect = [
            (0, "", ""),
            (0, "", ""),
            (0, "", ""),
            (0, "1001\n1001\n2001\n2002\n", ""),
            (0, "", ""),
        ]
        result = test_remote.test_backups_action(manager)
        assert result is True
