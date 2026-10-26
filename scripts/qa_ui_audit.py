#!/usr/bin/env python3
"""Auditoría automática de interacción del panel Rust (F4).

Comprueba contra el HTML servido por `nas-web`:

  * cada `getElementById('x')` de app.js tiene su `id="x"`;
  * cada `onclick="fn("` inline tiene la función definida en app.js;
  * cada `data-view` de la barra lateral apunta a una sección existente;
  * cada endpoint GET que carga una vista responde `success: true`.

Uso:
  NAS_WEB_PANEL_PASSWORD=... python3 scripts/qa_ui_audit.py [--host IP]
"""

from __future__ import annotations

import http.cookiejar
import json
import os
import re
import ssl
import sys
import urllib.error
import urllib.parse
import urllib.request
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
APP_JS = ROOT / "web" / "public" / "js" / "app.js"

CTX = ssl._create_unverified_context()


def fetch(opener, url, data=None, headers=None):
    request = urllib.request.Request(url, data=data, headers=headers or {})
    try:
        with opener.open(request, timeout=10) as response:
            return response.status, response.read().decode("utf-8", "replace")
    except urllib.error.HTTPError as error:
        return error.code, error.read().decode("utf-8", "replace")


def main() -> int:
    host = os.environ.get("NAS_TEST_IP", "10.10.1.222")
    password = os.environ.get("NAS_WEB_PANEL_PASSWORD", "")
    user = os.environ.get("NAS_WEB_PANEL_USER", "admin")
    if not password:
        print("Falta NAS_WEB_PANEL_PASSWORD")
        return 2

    base = f"https://{host}"
    jar = http.cookiejar.CookieJar()
    opener = urllib.request.build_opener(
        urllib.request.HTTPSHandler(context=CTX),
        urllib.request.HTTPCookieProcessor(jar),
    )

    status, login_html = fetch(opener, f"{base}/login")
    token = re.search(r'name="csrf_token"[^>]*value="([a-f0-9]+)"', login_html)
    form = urllib.parse.urlencode(
        {
            "username": user,
            "password": password,
            "csrf_token": token.group(1) if token else "",
        }
    ).encode()

    status, _ = fetch(
        opener,
        f"{base}/login",
        data=form,
        headers={"Content-Type": "application/x-www-form-urlencoded"},
    )
    if status not in (200, 303):
        print(f"Login falló: {status}")
        return 2

    status, html = fetch(opener, f"{base}/dashboard")
    if status != 200:
        print(f"Dashboard falló: {status}")
        return 2

    js = APP_JS.read_text(encoding="utf-8", errors="replace")

    failures = 0

    html_ids = set(re.findall(r'\bid="([^"]+)"', html))
    js_ids = set(re.findall(r"getElementById\(\s*['\"]([^'\"]+)['\"]", js))
    # IDs creados dinámicamente por app.js o resueltos por alias de modal
    dynamic_ids = {"users-no-results", "modal-format-disk", "notifications-badge"}
    missing_ids = sorted(js_ids - html_ids - dynamic_ids)
    print(f"[ids] app.js consulta {len(js_ids)} IDs; faltan {len(missing_ids)}")
    for missing in missing_ids:
        print(f"  FALTA id=\"{missing}\"")
        failures += 1

    handler_defs = set(
        re.findall(r"(?:async\s+)?function\s+([A-Za-z0-9_]+)", js)
    ) | set(re.findall(r"window\.([A-Za-z0-9_]+)\s*=", js))
    inline_calls = set(re.findall(r'on(?:click|change|submit|input)="([A-Za-z0-9_]+)\(', html))
    missing_handlers = sorted(inline_calls - handler_defs)
    print(f"[handlers] {len(inline_calls)} inline; sin definición: {len(missing_handlers)}")
    for missing in missing_handlers:
        print(f"  FALTA función {missing}()")
        failures += 1

    views = set(re.findall(r'id="(view-[a-z]+)"', html))
    nav_views = set(re.findall(r'data-view="([^"]+)"', html))
    missing_views = sorted(
        view for view in nav_views if f"view-{view}" not in views
    )
    print(f"[vistas] {len(views)} secciones; nav sin sección: {len(missing_views)}")
    for missing in missing_views:
        print(f"  FALTA sección view-{missing}")
        failures += 1

    endpoints = [
        "/api/metrics",
        "/api/network",
        "/api/shares",
        "/api/users",
        "/api/groups",
        "/api/shares/access",
        "/api/backups",
        "/api/backups/running",
        "/api/storage",
        "/api/storage/disks",
        "/api/storage/units",
        "/api/storage/subvolumes",
        "/api/storage/scrub",
        "/api/services",
        "/api/diagnostics",
        "/api/domain",
        "/api/domain/status",
        "/api/logs?limit=5",
        "/api/system/updates",
        "/api/system/applications",
        "/api/files?path=",
        "/api/files/trash",
        "/api/terminal/history?limit=5",
        "/api/audit?limit=5",
    ]
    print(f"[api] verificando {len(endpoints)} endpoints GET")
    for endpoint in endpoints:
        status, body = fetch(opener, f"{base}{endpoint}")
        ok = status == 200
        if ok:
            try:
                payload = json.loads(body)
                ok = bool(payload.get("success"))
            except json.JSONDecodeError:
                ok = False
        if not ok:
            print(f"  FALLO {endpoint}: HTTP {status}")
            failures += 1

    print()
    if failures:
        print(f"Auditoría con {failures} problema(s).")
        return 1
    print("Auditoría UI/API sin discrepancias.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
