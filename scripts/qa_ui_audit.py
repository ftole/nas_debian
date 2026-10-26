#!/usr/bin/env python3
"""Auditoría automática de interacción del panel (F4).

Modo estático (apto para CI, sin servidor):
  python3 scripts/qa_ui_audit.py --static

Modo en vivo (además valida los endpoints GET de cada vista):
  NAS_WEB_PANEL_PASSWORD=... python3 scripts/qa_ui_audit.py [--host IP]

En ambos modos comprueba:
  * cada `getElementById('x')` de app.js tiene su `id="x"`;
  * cada `onclick="fn("` inline tiene la función definida en app.js;
  * cada `data-view` de la barra lateral apunta a una sección `view-*`.
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
TEMPLATE = ROOT / "crates" / "nas-web" / "templates" / "layout.html"

CTX = ssl._create_unverified_context()

# IDs que app.js crea en tiempo de ejecución o resuelve por alias de modal.
DYNAMIC_IDS = {"users-no-results", "modal-format-disk", "notifications-badge"}

ENDPOINTS = [
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


def fetch(opener, url, data=None, headers=None):
    request = urllib.request.Request(url, data=data, headers=headers or {})
    try:
        with opener.open(request, timeout=15) as response:
            return response.status, response.read().decode("utf-8", "replace")
    except urllib.error.HTTPError as error:
        return error.code, error.read().decode("utf-8", "replace")


def audit_dom(html: str, js: str) -> int:
    """Comprueba IDs, manejadores inline y secciones de vista."""
    failures = 0

    html_ids = set(re.findall(r'\bid="([^"]+)"', html))
    js_ids = set(re.findall(r"getElementById\(\s*['\"]([^'\"]+)['\"]", js))
    missing_ids = sorted(js_ids - html_ids - DYNAMIC_IDS)
    print(f"[ids] app.js consulta {len(js_ids)} IDs; faltan {len(missing_ids)}")
    for missing in missing_ids:
        print(f'  FALTA id="{missing}"')
        failures += 1

    handler_defs = set(re.findall(r"(?:async\s+)?function\s+([A-Za-z0-9_]+)", js)) | set(
        re.findall(r"window\.([A-Za-z0-9_]+)\s*=", js)
    )
    inline_calls = set(
        re.findall(r'on(?:click|change|submit|input|keyup)="([A-Za-z0-9_]+)\(', html)
    )
    missing_handlers = sorted(inline_calls - handler_defs)
    print(f"[handlers] {len(inline_calls)} inline; sin definición: {len(missing_handlers)}")
    for missing in missing_handlers:
        print(f"  FALTA función {missing}()")
        failures += 1

    views = set(re.findall(r'id="(view-[a-z]+)"', html))
    nav_views = set(re.findall(r'data-view="([^"]+)"', html))
    missing_views = sorted(view for view in nav_views if f"view-{view}" not in views)
    print(f"[vistas] {len(views)} secciones; nav sin sección: {len(missing_views)}")
    for missing in missing_views:
        print(f"  FALTA sección view-{missing}")
        failures += 1

    return failures


def audit_live(html: str, js: str, opener, base: str) -> int:
    failures = audit_dom(html, js)

    print(f"[api] verificando {len(ENDPOINTS)} endpoints GET")
    for endpoint in ENDPOINTS:
        status, body = fetch(opener, f"{base}{endpoint}")
        ok = status == 200
        if ok:
            try:
                ok = bool(json.loads(body).get("success"))
            except json.JSONDecodeError:
                ok = False
        if not ok:
            print(f"  FALLO {endpoint}: HTTP {status}")
            failures += 1
    return failures


def login(host: str) -> tuple[urllib.request.OpenerDirector, str]:
    user = os.environ.get("NAS_WEB_PANEL_USER", "admin")
    password = os.environ.get("NAS_WEB_PANEL_PASSWORD", "")
    base = f"https://{host}"
    jar = http.cookiejar.CookieJar()
    opener = urllib.request.build_opener(
        urllib.request.HTTPSHandler(context=CTX),
        urllib.request.HTTPCookieProcessor(jar),
    )

    _, login_html = fetch(opener, f"{base}/login")
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
        raise SystemExit(f"Login falló: {status}")
    return opener, base


def main() -> int:
    js = APP_JS.read_text(encoding="utf-8", errors="replace")

    if "--static" in sys.argv:
        html = TEMPLATE.read_text(encoding="utf-8")
        failures = audit_dom(html, js)
        label = "estática"
    else:
        password = os.environ.get("NAS_WEB_PANEL_PASSWORD", "")
        if not password:
            print("Falta NAS_WEB_PANEL_PASSWORD (o use --static)")
            return 2
        host = os.environ.get("NAS_TEST_IP", "10.10.1.222")
        opener, base = login(host)
        status, html = fetch(opener, f"{base}/dashboard")
        if status != 200:
            print(f"Dashboard falló: {status}")
            return 2
        failures = audit_live(html, js, opener, base)
        label = "UI/API"

    print()
    if failures:
        print(f"Auditoría {label} con {failures} problema(s).")
        return 1
    print(f"Auditoría {label} sin discrepancias.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
