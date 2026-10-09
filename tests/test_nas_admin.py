"""
==============================================================================
Pruebas Unitarias para el Asistente de Administración Remota NAS (nas_admin.py)
==============================================================================
Verifica el parseo de `lsblk -J`, la exclusión del disco del sistema operativo,
la construcción del comando de despliegue y la abstracción de credenciales
(keyring con fallback a .env).
==============================================================================
"""

import pathlib
import sys
import unittest.mock as mock
import pytest

# Asegurar importación desde la raíz del proyecto
PROJECT_ROOT = pathlib.Path(__file__).resolve().parent.parent
if str(PROJECT_ROOT) not in sys.path:
    sys.path.insert(0, str(PROJECT_ROOT))

import nas_admin  # noqa: E402


def _sample_lsblk() -> str:
    return """
    {
      "blockdevices": [
        {"name":"sda","size":"238.5G","type":"disk","mountpoint":"/","model":"SSD","rota":"0"},
        {"name":"sdb","size":"3.6T","type":"disk","mountpoint":null,"model":"ST4000","rota":"1"},
        {"name":"sda1","size":"237G","type":"part","mountpoint":"/","model":null,"rota":"0"},
        {"name":"sdb1","size":"3.6T","type":"part","mountpoint":"/srv/nas","model":null,"rota":"1"}
      ]
    }
    """


def test_parse_lsblk_json_muestra_solo_discos() -> None:
    disks = nas_admin.parse_lsblk_json(_sample_lsblk())
    assert [d["name"] for d in disks] == ["sda", "sdb"]
    assert disks[1]["dev"] == "/dev/sdb"


def test_parse_lsblk_json_invalido_devuelve_lista_vacia() -> None:
    assert nas_admin.parse_lsblk_json("no-es-json") == []
    assert nas_admin.parse_lsblk_json("") == []


def test_disk_choices_incluye_local_y_excluye_disco_del_so() -> None:
    disks = nas_admin.parse_lsblk_json(_sample_lsblk())
    choices = nas_admin.disk_choices(disks, os_disk_name="sda")
    assert [c["name"] for c in choices] == ["LOCAL", "sdb"]


def test_build_deploy_command_archivos_con_keep_data() -> None:
    cmd = nas_admin.build_deploy_command(
        "/dev/sdb", "EADCOL", "SRV-NAS", "admin", "ARCHIVOS", keep_data=True)
    assert "deploy.sh /dev/sdb EADCOL SRV-NAS admin - ARCHIVOS --force --confirm --keep-data" in cmd
    assert nas_admin.PASS_TOKEN in cmd


def test_build_deploy_command_backup_sin_keep_data() -> None:
    cmd = nas_admin.build_deploy_command("LOCAL", "GRUPO", "SRV-BKP", "admin", "BACKUP")
    assert "deploy.sh LOCAL GRUPO SRV-BKP admin - BACKUP --force --confirm" in cmd
    assert "--keep-data" not in cmd


def test_build_deploy_command_archivos_backup() -> None:
    cmd = nas_admin.build_deploy_command("LOCAL", "GRUPO", "SRV-NAS", "admin", "ARCHIVOS_BACKUP")
    assert "deploy.sh LOCAL GRUPO SRV-NAS admin - ARCHIVOS_BACKUP --force --confirm" in cmd


def test_build_deploy_command_hibrido_normaliza_a_archivos_backup() -> None:
    cmd = nas_admin.build_deploy_command("LOCAL", "GRUPO", "SRV-NAS", "admin", "HIBRIDO")
    assert "deploy.sh LOCAL GRUPO SRV-NAS admin - ARCHIVOS_BACKUP --force --confirm" in cmd


def test_build_deploy_command_rol_desconocido_queda_en_archivos() -> None:
    cmd = nas_admin.build_deploy_command("LOCAL", "WG", "NB", "adm", "OTRO")
    assert " - ARCHIVOS --force" in cmd


def test_check_dependencies_detecta_paramiko_ausente(capsys: pytest.CaptureFixture) -> None:
    with mock.patch.object(nas_admin.tr, "paramiko", None):
        assert nas_admin.check_dependencies() is False
        captured = capsys.readouterr()
        assert "paramiko" in captured.out


def test_replace_placeholder_inyecta_clave_por_stdin() -> None:
    cmd = nas_admin.build_deploy_command("LOCAL", "WG", "NB", "adm", "ARCHIVOS")
    final = cmd.replace(nas_admin.PASS_TOKEN, "s3cret")
    assert "printf '%s\\n' s3cret | bash src/core/deploy.sh" in final


def test_get_secret_usa_keyring_cuando_disponible() -> None:
    if nas_admin.keyring is None:
        pytest.skip("keyring no instalado")
    with mock.patch.object(nas_admin.keyring, "get_password", return_value="clave123"):
        assert nas_admin.get_secret("NAS_TEST_PASSWORD") == "clave123"


def test_load_config_completa_contraseñas_desde_keyring() -> None:
    fake_env = mock.MagicMock()
    with mock.patch.object(nas_admin.tr, "load_env_file", return_value={
            "NAS_TEST_IP": "10.0.0.5",
            "NAS_TEST_PORT": "22",
            "NAS_TEST_USER": "sistemas",
            "NAS_TEST_PASSWORD": "",
            "NAS_ROOT_PASSWORD": ""}):
        with mock.patch.object(nas_admin, "get_secret", side_effect=["pass-ssh", "pass-root"]):
            cfg = nas_admin.load_config(fake_env)
    assert cfg["NAS_TEST_IP"] == "10.0.0.5"
    assert cfg["NAS_TEST_USER"] == "sistemas"
    assert cfg["NAS_TEST_PASSWORD"] == "pass-ssh"
    assert cfg["NAS_ROOT_PASSWORD"] == "pass-root"


def test_persist_credentials_sin_keyring_guarda_en_env() -> None:
    candidate = {"NAS_TEST_IP": "10.0.0.5", "NAS_TEST_PASSWORD": "p", "NAS_ROOT_PASSWORD": "r",
                 "NAS_TEST_USER": "sistemas", "NAS_TEST_PORT": 22}
    fake_env = mock.MagicMock()
    with mock.patch.object(nas_admin, "keyring", None):
        with mock.patch.object(nas_admin.tr, "save_env_file", return_value=True) as save:
            with mock.patch.object(nas_admin, "write_env_non_secret") as wns:
                nas_admin.persist_credentials(fake_env, candidate)
    save.assert_called_once()
    wns.assert_not_called()


def test_persist_credentials_con_keyring_no_escribe_env_con_claves() -> None:
    candidate = {"NAS_TEST_IP": "10.0.0.5", "NAS_TEST_PASSWORD": "p", "NAS_ROOT_PASSWORD": "r",
                 "NAS_TEST_USER": "sistemas", "NAS_TEST_PORT": 22}
    fake_env = mock.MagicMock()

    class FakeKeyring:  # noqa: D204
        pass

    with mock.patch.object(nas_admin, "keyring", FakeKeyring):
        with mock.patch.object(nas_admin, "write_env_non_secret") as wns:
            with mock.patch.object(nas_admin, "set_secret", return_value=True) as ss:
                with mock.patch.object(nas_admin.tr, "save_env_file") as save:
                    nas_admin.persist_credentials(fake_env, candidate)
    wns.assert_called_once()
    assert ss.call_count == 2
    save.assert_not_called()


def test_main_permite_help_sin_paramiko(capsys: pytest.CaptureFixture) -> None:
    with mock.patch.object(nas_admin.tr, "paramiko", None):
        with pytest.raises(SystemExit) as exc:
            nas_admin.main(["--help"])
        assert exc.value.code == 0
        captured = capsys.readouterr()
        assert "Asistente de Administración Remota" in captured.out


def test_main_exige_paramiko_para_accion(capsys: pytest.CaptureFixture) -> None:
    with mock.patch.object(nas_admin.tr, "paramiko", None):
        code = nas_admin.main(["status"])
        assert code == 1
        captured = capsys.readouterr()
        assert "paramiko" in captured.out
