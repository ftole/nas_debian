"""Pruebas de validacion y fallo del backend web de backups (backup_api.py)."""

import importlib.util
import io
import json
import os
import sys

import pytest

MODULE_PATH = os.path.join(
    os.path.dirname(__file__), "..", "src", "web", "backups", "backup_api.py"
)


def _load_module():
    spec = importlib.util.spec_from_file_location("backup_api", MODULE_PATH)
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


api = _load_module()


@pytest.mark.parametrize("value", ["10.0.0.1", "srv-win", "host.example.com"])
def test_valid_host_ok(value):
    assert api._valid_host(value)


@pytest.mark.parametrize("value", ["1.1.1.1; rm -rf /", "a b", "", "10.0.0.1$(id)"])
def test_valid_host_reject(value):
    assert not api._valid_host(value)


@pytest.mark.parametrize("value", ["C$", "Contabilidad", "docs-2024", "a.b_c"])
def test_valid_share_ok(value):
    assert api._valid_share(value)


@pytest.mark.parametrize("value", ["../etc", "a/b", "", "a;b", "a b"])
def test_valid_share_reject(value):
    assert not api._valid_share(value)


@pytest.mark.parametrize("value", ["/srv/nas/x", "/var/www", "/etc"])
def test_valid_path_ok(value):
    assert api._valid_path(value)


@pytest.mark.parametrize("value", ["/", "/srv/nas/../etc", "relative/path", "", "/a b"])
def test_valid_path_reject(value):
    assert not api._valid_path(value)


def test_sanitize_name():
    assert api._sanitize_name("a b/c") == "a_b_c"
    assert api._sanitize_name("ok-name_1") == "ok-name_1"


@pytest.mark.parametrize("value", ["0 23 * * *", "*/6 * * * *", "0 2 * * 0"])
def test_valid_cron_ok(value):
    assert api._valid_cron(value)


@pytest.mark.parametrize("value", ["0 23 * *", "0 23 * * *; rm -rf /", "", "a b c d e"])
def test_valid_cron_reject(value):
    assert not api._valid_cron(value)


def _patch_dirs(monkeypatch, tmp_path):
    for d in ("bin", "cron", "cred", "bkp", "log"):
        os.makedirs(str(tmp_path / d), exist_ok=True)
    monkeypatch.setattr(api, "BIN_DIR", str(tmp_path / "bin"))
    monkeypatch.setattr(api, "CRON_DIR", str(tmp_path / "cron"))
    monkeypatch.setattr(api, "CRED_DIR", str(tmp_path / "cred"))
    monkeypatch.setattr(api, "BKP_ROOT", str(tmp_path / "bkp"))
    monkeypatch.setattr(api, "LOG_ROOT", str(tmp_path / "log"))
    monkeypatch.setattr(api, "KNOWN_HOSTS", str(tmp_path / "known_hosts"))


def test_create_task_rechaza_ruta_con_traversal(tmp_path, monkeypatch, capsys):
    _patch_dirs(monkeypatch, tmp_path)
    api.create_task({"id": "t", "proto": "local", "path": "/srv/nas/../etc"})
    data = json.loads(capsys.readouterr().out)
    assert data["status"] == "error"
    assert not os.path.exists(str(tmp_path / "bin" / "backup_t.sh"))


def test_create_task_rechaza_cron_invalido(tmp_path, monkeypatch, capsys):
    _patch_dirs(monkeypatch, tmp_path)
    api.create_task({"id": "t2", "proto": "local", "cron": "0 23 * * *; rm -rf /", "path": "/srv/nas/x"})
    data = json.loads(capsys.readouterr().out)
    assert data["status"] == "error"
    assert not os.path.exists(str(tmp_path / "bin" / "backup_t2.sh"))


def test_create_task_valido_escribe_runner(tmp_path, monkeypatch, capsys):
    _patch_dirs(monkeypatch, tmp_path)
    api.create_task({"id": "t_local", "proto": "local", "cron": "0 23 * * *", "retention": 30, "path": "/srv/nas/datos"})
    data = json.loads(capsys.readouterr().out)
    assert data["status"] == "ok"
    assert os.path.exists(str(tmp_path / "bin" / "backup_t_local.sh"))


def test_test_cifs_rechaza_datos_invalidos(capsys):
    api.test_cifs("1.1.1.1; rm", "docs", "usuario", "x")
    data = json.loads(capsys.readouterr().out)
    assert data["status"] == "error"


def test_test_ssh_rechaza_datos_invalidos(capsys):
    api.test_ssh("host malo", "22", "root", "x")
    data = json.loads(capsys.readouterr().out)
    assert data["status"] == "error"


def test_read_payload_desde_stdin(monkeypatch):
    monkeypatch.setattr(sys, "stdin", io.StringIO('{"a": 1}'))
    assert api._read_payload() == {"a": 1}


def test_redact_oculta_el_secreto():
    assert api._redact("Error con clave supersecreta", "supersecreta") == "Error con clave ***"
    assert api._redact("sin secreto", "") == "sin secreto"
    assert api._redact(None, "x") is None


def test_test_ssh_no_filtra_contrasena(monkeypatch, tmp_path, capsys):
    monkeypatch.setattr(api, "KNOWN_HOSTS", str(tmp_path / "known_hosts"))

    class FakeRes:
        returncode = 1
        stderr = "auth failed for password supersecreta"
        stdout = ""

    monkeypatch.setattr(api.subprocess, "run", lambda *a, **k: FakeRes())
    api.test_ssh("10.0.0.1", "22", "root", "supersecreta")
    out = capsys.readouterr().out
    assert "supersecreta" not in out


def test_test_cifs_no_filtra_contrasena(monkeypatch, capsys):
    class FakeRes:
        returncode = 1
        stderr = "NT_STATUS_LOGON_FAILURE supersecreta"
        stdout = ""

    monkeypatch.setattr(api.subprocess, "run", lambda *a, **k: FakeRes())
    api.test_cifs("10.0.0.1", "docs", "usuario", "supersecreta")
    out = capsys.readouterr().out
    assert "supersecreta" not in out


def test_runner_generado_tiene_contenido_esperado(tmp_path, monkeypatch, capsys):
    _patch_dirs(monkeypatch, tmp_path)
    api.create_task({
        "id": "t_local",
        "proto": "local",
        "cron": "0 23 * * *",
        "retention": 30,
        "path": "/srv/nas/datos",
    })
    capsys.readouterr()
    runner = tmp_path / "bin" / "backup_t_local.sh"
    contenido = runner.read_text()
    assert "StrictHostKeyChecking=no" not in contenido
    assert "RETENTION=30" in contenido
    assert (os.stat(runner).st_mode & 0o777) == 0o750


def test_runner_ssh_usa_known_hosts_sin_secretos(tmp_path, monkeypatch, capsys):
    _patch_dirs(monkeypatch, tmp_path)
    api.create_task({
        "id": "t_ssh",
        "proto": "ssh",
        "ip": "10.0.0.20",
        "port": 22,
        "path": "/var/www",
        "user": "backup",
        "password": "secreto",
        "cron": "0 2 * * *",
        "retention": 15,
    })
    capsys.readouterr()
    runner = tmp_path / "bin" / "backup_t_ssh.sh"
    contenido = runner.read_text()
    assert "StrictHostKeyChecking=accept-new" in contenido
    assert "UserKnownHostsFile=" in contenido
    assert "secreto" not in contenido
    assert (os.stat(runner).st_mode & 0o777) == 0o750
