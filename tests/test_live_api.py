"""Prueba en vivo de endpoints API recién implementados."""
import http.client
import json
import os
import pathlib
import re
import ssl
import sys
import pytest

# Asegurar importación de test_remote desde la raíz del proyecto
PROJECT_ROOT = pathlib.Path(__file__).resolve().parent.parent
if str(PROJECT_ROOT) not in sys.path:
    sys.path.insert(0, str(PROJECT_ROOT))

try:
    import test_remote
except ImportError:
    test_remote = None


def test_live_api_endpoints():
    if test_remote is None:
        pytest.skip("Módulo test_remote no disponible; omitiendo prueba en vivo.")

    env_path = test_remote.get_default_env_path()
    if not os.path.isfile(env_path):
        pytest.skip("Archivo .env no encontrado; omitiendo prueba en vivo.")

    panel_user = os.environ.get("NAS_WEB_PANEL_USER", "admin")
    panel_password = os.environ.get("NAS_WEB_PANEL_PASSWORD", "")
    if not panel_password:
        pytest.skip("NAS_WEB_PANEL_PASSWORD no definido; omitiendo prueba en vivo del panel Rust.")

    raw_cfg = test_remote.load_env_file(env_path)
    cfg = test_remote.normalize_env_config(raw_cfg)
    ip = cfg.get("NAS_TEST_IP", "")
    if not ip:
        pytest.skip("NAS_TEST_IP no configurado; omitiendo prueba en vivo.")

    try:
        ctx = ssl._create_unverified_context()
        conn = http.client.HTTPSConnection(ip, 443, context=ctx, timeout=3)
        conn.connect()
    except Exception as e:
        pytest.skip(f"No se pudo conectar al servidor NAS {ip}:443 ({e}); omitiendo prueba en vivo.")

    # Limpiar rate limits residuales para pruebas
    mgr = test_remote.SSHManager(cfg)
    mgr.run_command("sqlite3 /var/lib/nas/nas.sqlite 'DELETE FROM login_attempts; DELETE FROM api_rate_limits;' 2>/dev/null || true", sudo=True)
    mgr.run_command("sqlite3 /var/www/nas-web/data/nas.sqlite 'DELETE FROM login_attempts; DELETE FROM api_rate_limits;' 2>/dev/null || true", sudo=True)

    # Obtener token CSRF
    conn.request("GET", "/login")
    res = conn.getresponse()
    html = res.read().decode("utf-8")
    cookie = (res.getheader("Set-Cookie") or "nas_session=pending").split(";")[0]
    m = re.search(r'name=["\']csrf_token["\'][^>]*?value=["\']([a-f0-9]{64})["\']', html, re.IGNORECASE)
    if not m:
        m = re.search(r'value=["\']([a-f0-9]{64})["\'][^>]*?name=["\']csrf_token["\']', html, re.IGNORECASE)
    assert m, f"No se encontró token CSRF en login: {html[:500]}"
    csrf = m.group(1)

    # Iniciar sesión (contrato Rust: formulario y cookie posterior)
    from urllib.parse import urlencode

    login_body = urlencode({
        "username": panel_user,
        "password": panel_password,
        "csrf_token": csrf,
    })
    headers = {
        "Content-Type": "application/x-www-form-urlencoded",
        "Cookie": cookie,
        "X-CSRF-Token": csrf,
    }
    conn.request("POST", "/login", body=login_body, headers=headers)
    lres = conn.getresponse()
    assert lres.status in (303, 200), f"Login falló con status {lres.status}"
    lcookie = (lres.getheader("Set-Cookie") or cookie).split(";")[0]
    lres.read()
    headers["Cookie"] = lcookie

    # 1. Diagnóstico Ping
    conn.request("POST", "/api/diagnostics/run", body=json.dumps({"type": "ping", "target": "127.0.0.1"}), headers=headers)
    r = conn.getresponse()
    assert r.status == 200, f"Ping falló con status {r.status}"
    data = json.loads(r.read().decode("utf-8"))
    assert data.get("success") is True

    # 2. Diagnóstico Disk I/O
    conn.request("POST", "/api/diagnostics/run", body=json.dumps({"type": "disk_io"}), headers=headers)
    r = conn.getresponse()
    assert r.status == 200, f"Disk I/O falló con status {r.status}"
    data = json.loads(r.read().decode("utf-8"))
    assert data.get("success") is True

    # 3. Diagnóstico DNS
    conn.request("POST", "/api/diagnostics/run", body=json.dumps({"type": "dns", "target": "debian.org"}), headers=headers)
    r = conn.getresponse()
    assert r.status == 200, f"DNS falló con status {r.status}"
    data = json.loads(r.read().decode("utf-8"))
    assert data.get("success") is True

    # 4. Logs agrupados
    conn.request("GET", "/api/logs?grouped=1&limit=5", headers=headers)
    r = conn.getresponse()
    assert r.status == 200, f"Logs falló con status {r.status}"
    data = json.loads(r.read().decode("utf-8"))
    assert "data" in data

    # 5. Respaldos en ejecución
    conn.request("GET", "/api/backups/running", headers=headers)
    r = conn.getresponse()
    assert r.status == 200, f"Backups running falló con status {r.status}"
    running = json.loads(r.read().decode("utf-8")).get("data", {})
    assert "running" in running

    # 6. Consultar actualizaciones en caliente
    conn.request("GET", "/api/system/updates", headers=headers)
    r = conn.getresponse()
    assert r.status == 200, f"Updates falló con status {r.status}"
    upd = json.loads(r.read().decode("utf-8")).get("data", {})
    assert "version" in upd and "latest_version" in upd and "has_updates" in upd

    # 7. Consultar métricas de aplicaciones en vivo
    conn.request("GET", "/api/system/applications", headers=headers)
    r = conn.getresponse()
    assert r.status == 200, f"Applications falló con status {r.status}"
    apps = json.loads(r.read().decode("utf-8")).get("data", {})
    assert "samba" in apps and "web" in apps and "btrfs" in apps and "security" in apps

    # 8. Terminal PTY real: sesión, captura, envío y muerte
    conn.request("POST", "/api/terminal/session", body=json.dumps({"cols": 80, "rows": 24}), headers=headers)
    r = conn.getresponse()
    assert r.status == 200, f"Terminal session falló con status {r.status}"
    r.read()

    conn.request("GET", "/api/terminal/capture", headers=headers)
    r = conn.getresponse()
    assert r.status == 200, f"Terminal capture falló con status {r.status}"
    cap = json.loads(r.read().decode("utf-8")).get("data", {})
    assert "output" in cap and "session" in cap

    conn.request("POST", "/api/terminal/send", body=json.dumps({"data": "echo LIVE_TERMINAL_OK"}), headers=headers)
    r = conn.getresponse()
    assert r.status == 200, f"Terminal send falló con status {r.status}"
    r.read()

    conn.request("GET", "/api/terminal/history?limit=10", headers=headers)
    r = conn.getresponse()
    assert r.status == 200, f"Terminal history falló con status {r.status}"
    thist = json.loads(r.read().decode("utf-8")).get("data", [])
    assert isinstance(thist, list)

    conn.request("POST", "/api/terminal/kill", body="{}", headers=headers)
    r = conn.getresponse()
    assert r.status == 200, f"Terminal kill falló con status {r.status}"
    r.read()

    conn.close()
