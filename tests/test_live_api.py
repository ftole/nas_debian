"""Prueba en vivo de endpoints API recién implementados."""
import ssl
import http.client
import json
import re
import test_remote


def test_live_api_endpoints():
    cfg = test_remote.normalize_env_config(test_remote.load_env_file(test_remote.get_default_env_path()))
    ip = cfg["NAS_TEST_IP"]
    ctx = ssl._create_unverified_context()
    conn = http.client.HTTPSConnection(ip, 443, context=ctx, timeout=10)

    # Limpiar rate limits residuales para pruebas
    mgr = test_remote.SSHManager(cfg)
    mgr.run_command("sqlite3 /var/lib/nas/nas.sqlite 'DELETE FROM login_attempts; DELETE FROM api_rate_limits;'", sudo=True)

    # Obtener token CSRF
    conn.request("GET", "/login")
    res = conn.getresponse()
    html = res.read().decode("utf-8")
    cookie = res.getheader("Set-Cookie").split(";")[0]
    m = re.search(r'name=["\']csrf_token["\'][^>]*?value=["\']([a-f0-9]{64})["\']', html, re.IGNORECASE)
    if not m:
        m = re.search(r'value=["\']([a-f0-9]{64})["\'][^>]*?name=["\']csrf_token["\']', html, re.IGNORECASE)
    assert m, f"No se encontró token CSRF en login:\n{html[:500]}"
    csrf = m.group(1)

    # Iniciar sesión
    login_body = json.dumps({
        "username": cfg["NAS_TEST_USER"],
        "password": cfg["NAS_TEST_PASSWORD"],
        "csrf_token": csrf,
    })
    headers = {
        "Content-Type": "application/json",
        "Cookie": cookie,
        "X-CSRF-Token": csrf,
    }
    conn.request("POST", "/api/auth/login", body=login_body, headers=headers)
    lres = conn.getresponse()
    assert lres.status == 200, f"Login falló con status {lres.status}"
    lcookie = lres.getheader("Set-Cookie", cookie).split(";")[0]
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

    # 5. Detener respaldo inexistente
    conn.request("POST", "/api/backups/tarea_test_inexistente/stop", body="{}", headers=headers)
    r = conn.getresponse()
    assert r.status == 200, f"Stop backup falló con status {r.status}"

    conn.close()
