#!/usr/bin/env python3
"""Compara en vivo el panel PHP original con el panel Rust portado (F6).

Se ejecuta en el servidor, con el panel PHP publicado en un puerto local
(p. ej. 127.0.0.1:8081) y el panel Rust en 127.0.0.1:8080.

  python3 scripts/php_parity_diff.py \
      --php http://127.0.0.1:8081 --rust http://127.0.0.1:8080 \
      --user admin --password '...'

Compara por página: IDs (y su orden), clases, atributos `data-*`, número de
secciones/vistas y modales. Después compara la forma (rutas de claves) de las
respuestas JSON de los endpoints de lectura.

Salida: informe legible y código 1 si hay diferencias.
"""

from __future__ import annotations

import argparse
import http.cookiejar
import json
import re
import ssl
import sys
import urllib.error
import urllib.parse
import urllib.request

PAGES = [
    "dashboard",
    "files",
    "shares",
    "users",
    "backups",
    "storage",
    "terminal",
    "logs",
    "domain",
    "services",
    "networking",
    "applications",
]

ENDPOINTS = [
    "/api/metrics",
    "/api/network",
    "/api/shares",
    "/api/shares/access",
    "/api/users",
    "/api/groups",
    "/api/backups",
    "/api/storage",
    "/api/storage/units",
    "/api/storage/subvolumes",
    "/api/storage/scrub",
    "/api/services",
    "/api/diagnostics",
    "/api/domain/status",
    "/api/logs?limit=5",
    "/api/system/updates",
    "/api/system/applications",
    "/api/files?path=",
    "/api/files/trash",
    "/api/audit?limit=5",
    "/api/terminal/history?limit=5",
]

DYNAMIC_IDS = {"users-no-results", "modal-format-disk", "notifications-badge"}


def fetch(opener, url, data=None, headers=None):
    request = urllib.request.Request(url, data=data, headers=headers or {})
    try:
        with opener.open(request, timeout=30) as response:
            return response.status, response.read().decode("utf-8", "replace")
    except urllib.error.HTTPError as error:
        return error.code, error.read().decode("utf-8", "replace")


def login(base: str, user: str, password: str):
    jar = http.cookiejar.CookieJar()
    handlers = [urllib.request.HTTPCookieProcessor(jar)]
    if base.startswith("https"):
        handlers.append(urllib.request.HTTPSHandler(context=ssl._create_unverified_context()))
    opener = urllib.request.build_opener(*handlers)

    _, login_html = fetch(opener, f"{base}/login")
    token = re.search(r'name="csrf_token"[^>]*value="([^"]+)"', login_html)
    fields = {"username": user, "password": password}
    if token:
        fields["csrf_token"] = token.group(1)

    form = urllib.parse.urlencode(fields).encode()
    status, body = fetch(
        opener,
        f"{base}/login",
        data=form,
        headers={"Content-Type": "application/x-www-form-urlencoded"},
    )
    if status not in (200, 303):
        raise SystemExit(f"login falló en {base}: {status} {body[:200]}")
    _, check = fetch(opener, f"{base}/dashboard")
    if 'id="view-dashboard"' not in check:
        raise SystemExit(f"la sesión no se estableció en {base}")
    return opener


def ids_of(html: str) -> list[str]:
    seen: list[str] = []
    for value in re.findall(r'\bid="([^"]+)"', html):
        if value not in seen:
            seen.append(value)
    return seen


def classes_of(html: str) -> set[str]:
    tokens: set[str] = set()
    for chunk in html.split('class="')[1:]:
        value = chunk.split('"')[0]
        tokens.update(value.split())
    return tokens


def data_attrs_of(html: str) -> set[str]:
    return set(re.findall(r"\bdata-([a-z][a-z0-9-]*)", html))


def counts_of(html: str) -> dict[str, int]:
    return {
        "view-section": len(re.findall(r'class="view-section', html)),
        "nav-item": len(re.findall(r'data-view="', html)),
        "modal": len(re.findall(r'class="modal[\s"]', html)),
        "form": len(re.findall(r"<form\b", html)),
        "table": len(re.findall(r"<table\b", html)),
        "svg-use": len(re.findall(r"<use\b", html)),
    }


def compare_sets(name: str, php: set[str], rust: set[str]) -> list[str]:
    """El panel Rust puede añadir elementos; solo se exige que no falte nada."""
    problems = []
    only_php = sorted(php - rust)
    if only_php:
        problems.append(f"{name}: falta en Rust ({len(only_php)}): {only_php[:12]}")
    return problems


def shape(value, prefix="") -> set[str]:
    found: set[str] = set()
    if isinstance(value, dict):
        for key, item in value.items():
            found.add(f"{prefix}.{key}")
            found |= shape(item, f"{prefix}.{key}")
    elif isinstance(value, list) and value:
        found |= shape(value[0], f"{prefix}[]")
    return found


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--php", default="http://127.0.0.1:8081")
    parser.add_argument("--rust", default="http://127.0.0.1:8080")
    parser.add_argument("--php-user", default="admin")
    parser.add_argument("--php-password", required=True)
    parser.add_argument("--rust-user", default="admin")
    parser.add_argument("--rust-password", required=True)
    args = parser.parse_args()

    php = login(args.php, args.php_user, args.php_password)
    rust = login(args.rust, args.rust_user, args.rust_password)

    problems: list[str] = []

    print("== Páginas ==")
    for page in PAGES:
        php_status, php_html = fetch(php, f"{args.php}/{page}")
        rust_status, rust_html = fetch(rust, f"{args.rust}/{page}")
        if php_status != 200 or rust_status != 200:
            problems.append(f"/{page}: PHP {php_status} vs Rust {rust_status}")
            continue

        php_ids, rust_ids = ids_of(php_html), ids_of(rust_html)
        php_set = set(php_ids) - DYNAMIC_IDS
        rust_set = set(rust_ids) - DYNAMIC_IDS

        page_problems = compare_sets("ids", php_set, rust_set)
        page_problems += compare_sets("clases", classes_of(php_html), classes_of(rust_html))
        page_problems += compare_sets(
            "data-*", data_attrs_of(php_html), data_attrs_of(rust_html)
        )

        php_counts, rust_counts = counts_of(php_html), counts_of(rust_html)
        for key, value in php_counts.items():
            if rust_counts[key] < value:
                page_problems.append(
                    f"{key}: PHP {value} vs Rust {rust_counts[key]}"
                )

        # Orden estructural: secciones de vista en el orden del DOM.
        def view_order(html: str) -> list[str]:
            return re.findall(r'id="(view-[a-z]+)"', html)

        if view_order(php_html) != view_order(rust_html):
            page_problems.append(
                f"orden de secciones: PHP {view_order(php_html)} vs Rust {view_order(rust_html)}"
            )

        if page_problems:
            print(f"  /{page}: {len(page_problems)} diferencia(s)")
            for detail in page_problems:
                print(f"    - {detail}")
            problems.extend(f"/{page} {detail}" for detail in page_problems)
        else:
            print(f"  /{page}: idéntico ({len(php_ids)} IDs)")

    print("\n== API (forma de las respuestas) ==")
    for endpoint in ENDPOINTS:
        php_status, php_body = fetch(php, f"{args.php}{endpoint}")
        rust_status, rust_body = fetch(rust, f"{args.rust}{endpoint}")
        if php_status == 404 and rust_status == 200:
            print(f"  {endpoint}: solo en Rust (mejora, no existe en PHP)")
            continue
        if php_status != 200 or rust_status != 200:
            problems.append(f"{endpoint}: HTTP PHP {php_status} vs Rust {rust_status}")
            print(f"  {endpoint}: HTTP PHP {php_status} vs Rust {rust_status}")
            continue
        try:
            php_json = json.loads(php_body)
            rust_json = json.loads(rust_body)
        except json.JSONDecodeError:
            problems.append(f"{endpoint}: JSON inválido")
            print(f"  {endpoint}: JSON inválido")
            continue

        php_shape = shape(php_json.get("data", php_json))
        rust_shape = shape(rust_json.get("data", rust_json))
        only_php = sorted(php_shape - rust_shape)
        if only_php:
            print(f"  {endpoint}: faltan claves de PHP")
            print(f"    faltan: {only_php[:12]}")
            problems.append(f"{endpoint}: forma distinta")
        else:
            print(f"  {endpoint}: misma forma")

    print()
    if problems:
        print(f"F6: {len(problems)} diferencia(s) detectada(s).")
        return 1
    print("F6: paridad en vivo verificada (páginas y API).")
    return 0


if __name__ == "__main__":
    sys.exit(main())
