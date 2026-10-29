#!/usr/bin/env python3
"""Extrae el contrato de paridad entre el panel PHP original y el panel Rust.

Genera (deterministas):

  tests/parity/contract.json     IDs, clases, data-*, globales y formularios
  tests/parity/selectors.txt     selectores usados por app.js (deben existir)
  tests/parity/endpoints.txt     endpoints /api consumidos por app.js
  tests/parity/assets.sha256     hashes de los assets originales intocables

Uso:
  python3 scripts/parity_extract.py           # regenera los artefactos
  python3 scripts/parity_extract.py --check   # falla si hay deriva
"""

from __future__ import annotations

import hashlib
import json
import pathlib
import re
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
PARITY = ROOT / "tests" / "parity"

# Fase 7.2: el contrato se congela con fixtures propias y los assets del crate.
FIXTURES = PARITY / "fixtures"
ASSETS = ROOT / "crates" / "nas-web" / "assets"

LAYOUT = FIXTURES / "layout.php"
LOGIN = FIXTURES / "login.php"
APP_JS = ASSETS / "js" / "app.js"

ASSET_FILES = [
    "app.css",
    "login.css",
    "icons.svg",
    "favicon.svg",
    "js/app.js",
    "js/vendor/xlsx.full.min.js",
    "js/vendor/jszip.min.js",
    "js/vendor/docx-preview.min.js",
]


def read(path: pathlib.Path) -> str:
    return path.read_text(encoding="utf-8", errors="replace")


def unique_sorted(values) -> list[str]:
    return sorted({value for value in values if value})


def extract_layout(text: str) -> dict:
    ids = unique_sorted(re.findall(r'\bid="([^"$]+)"', text))
    classes = unique_sorted(
        re.findall(r'\bclass="([^"$]+)"', text)
    )
    # las clases pueden venir con varias separadas por espacio
    split_classes = unique_sorted(
        token for group in classes for token in group.split()
    )
    data_attributes: dict[str, list[str]] = {}
    for name, value in re.findall(r'\b(data-[a-z0-9-]+)="([^"$]*)"', text):
        data_attributes.setdefault(name, [])
        data_attributes[name].append(value)
    data_attributes = {
        key: unique_sorted(values) for key, values in sorted(data_attributes.items())
    }

    globals_found = unique_sorted(re.findall(r"window\.(NAS_[A-Z0-9_]+)", text))
    forms = unique_sorted(
        action for action in re.findall(r'<form[^>]*action="([^"$]+)"', text)
    )
    return {
        "ids": ids,
        "classes": split_classes,
        "data_attributes": data_attributes,
        "window_globals": globals_found,
        "forms": forms,
        "has_csrf_field": "csrf_token" in text,
    }


def extract_app_js(text: str) -> dict:
    by_id = unique_sorted(
        re.findall(r"getElementById\(\s*['\"]([^'\"]+)['\"]", text)
    )
    selectors = unique_sorted(
        re.findall(r"querySelector(?:All)?\(\s*['\"]([^'\"]+)['\"]", text)
    )
    endpoints = unique_sorted(
        re.findall(r"(?:apiFetch|fetch)\(\s*[`'\"](/api/[^`'\"?${]+)", text)
    )
    # URLs construidas con plantillas (p. ej. `/api/backups/${id}/run`)
    for match in re.finditer(r"`([^`]*)`", text):
        raw = match.group(1)
        if "/api/" not in raw:
            continue
        normalized = re.sub(r"\$\{[^}]*\}", ":id", raw).split("?")[0]
        if normalized.startswith("/api/"):
            endpoints.append(normalized)
    for match in re.finditer(r"['\"](/api/[a-z0-9_/-]*)['\"]", text):
        endpoints.append(match.group(1))
    endpoints = unique_sorted(endpoints)
    handler_calls = unique_sorted(
        re.findall(r"on(?:click|change|submit)=\\?\"([A-Za-z0-9_]+)\(", text)
    )
    return {
        "ids": by_id,
        "selectors": selectors,
        "endpoints": endpoints,
        "inline_handlers": handler_calls,
    }


def extract_endpoints(text: str) -> list[str]:
    """Extrae rutas `/api/...` incluidas las construidas con plantillas JS."""
    found = set()
    # literales: '/api/x'  y  "api/x"
    for match in re.finditer(r"['\"](/api/[^'\"\s]*)['\"]", text):
        path = match.group(1)
        if "$" not in path:
            found.add(path)
    # plantillas: `/api/backups/${id}/run?x=${y}`
    for match in re.finditer(r"`([^`]*)`", text):
        raw = match.group(1)
        if "/api/" not in raw:
            continue
        normalized = re.sub(r"\$\{[^}]*\}", ":id", raw)
        normalized = normalized.split("?")[0]
        if normalized.startswith("/api/"):
            found.add(normalized)
    return sorted(found)


def build_contract() -> dict:
    layout_text = read(LAYOUT)
    login_text = read(LOGIN)
    app_text = read(APP_JS)

    layout = extract_layout(layout_text)
    login = extract_layout(login_text)
    app = extract_app_js(app_text)

    return {
        "generated_by": "scripts/parity_extract.py",
        "sources": {
            "layout": LAYOUT.relative_to(ROOT).as_posix(),
            "login": LOGIN.relative_to(ROOT).as_posix(),
            "app_js": APP_JS.relative_to(ROOT).as_posix(),
        },
        "layout": layout,
        "login": login,
        "app_js": app,
        "counts": {
            "layout_ids": len(layout["ids"]),
            "layout_classes": len(layout["classes"]),
            "app_ids": len(app["ids"]),
            "app_selectors": len(app["selectors"]),
            "app_endpoints": len(app["endpoints"]),
            "inline_handlers": len(app["inline_handlers"]),
        },
    }


def build_hashes() -> str:
    lines = []
    for relative in ASSET_FILES:
        path = ASSETS / relative
        digest = hashlib.sha256(path.read_bytes()).hexdigest()
        size = path.stat().st_size
        shown = path.relative_to(ROOT).as_posix()
        lines.append(f"{digest}  {size:>8}  {shown}")
    return "\n".join(lines) + "\n"


def render_outputs() -> dict[str, str]:
    contract = build_contract()
    return {
        "contract.json": json.dumps(contract, indent=2, ensure_ascii=False) + "\n",
        "selectors.txt": "\n".join(contract["app_js"]["selectors"] + contract["app_js"]["ids"]) + "\n",
        "endpoints.txt": "\n".join(contract["app_js"]["endpoints"]) + "\n",
        "assets.sha256": build_hashes(),
    }


def write_outputs(outputs: dict[str, str]) -> None:
    PARITY.mkdir(parents=True, exist_ok=True)
    for name, content in outputs.items():
        (PARITY / name).write_text(content, encoding="utf-8", newline="\n")
        print(f"[parity] escrito tests/parity/{name}")


def normalize_route(path: str) -> str:
    """Unifica `{id}` (axum) con `:id` (JavaScript)."""
    return re.sub(r"\{[a-z_]+\}", ":id", path)


def check_routes() -> list[str]:
    """Devuelve los endpoints de app.js sin ruta equivalente en lib.rs."""
    lib = (ROOT / "crates" / "nas-web" / "src" / "lib.rs").read_text(encoding="utf-8")
    registered = {
        re.sub(r"\{[^}]+\}", "{x}", route)
        for route in re.findall(r'\.route\(\s*"([^"]+)"', lib)
    }
    endpoints = [
        line.strip()
        for line in (PARITY / "endpoints.txt").read_text(encoding="utf-8").splitlines()
        if line.strip()
    ]

    def normalize(path: str) -> str:
        return re.sub(r"\{[^}]+\}", "{x}", path.replace(":id", "{x}"))

    return [
        endpoint
        for endpoint in endpoints
        if normalize(endpoint) not in registered
    ]


def check_outputs(outputs: dict[str, str]) -> int:
    failures = 0
    for name, content in outputs.items():
        path = PARITY / name
        if not path.is_file():
            print(f"[parity] FALTA tests/parity/{name}")
            failures += 1
            continue
        current = path.read_text(encoding="utf-8")
        if current != content:
            print(f"[parity] DERIVA en tests/parity/{name} (regenera el contrato)")
            failures += 1
    missing_routes = check_routes()
    for route in missing_routes:
        print(f"[parity] SIN RUTA en nas-web: {route}")
    failures += len(missing_routes)

    if failures:
        print(f"[parity] {failures} problema(s) de paridad")
        return 1
    print("[parity] contrato de paridad y rutas verificadas")
    return 0


def main(argv: list[str]) -> int:
    outputs = render_outputs()
    if "--check" in argv:
        return check_outputs(outputs)
    write_outputs(outputs)
    contract = json.loads(outputs["contract.json"])
    print("[parity] resumen:", json.dumps(contract["counts"], ensure_ascii=False))
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
