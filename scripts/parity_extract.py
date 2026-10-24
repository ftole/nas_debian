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
WEB = ROOT / "web"
PARITY = ROOT / "tests" / "parity"

LAYOUT = WEB / "templates" / "layout.php"
LOGIN = WEB / "templates" / "login.php"
APP_JS = WEB / "public" / "js" / "app.js"

ASSET_FILES = [
    "web/public/css/app.css",
    "web/public/js/app.js",
    "web/public/js/vendor/xlsx.full.min.js",
    "web/public/js/vendor/jszip.min.js",
    "web/public/js/vendor/docx-preview.min.js",
    "web/public/favicon.svg",
    "web/templates/layout.php",
    "web/templates/login.php",
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
    handler_calls = unique_sorted(
        re.findall(r"on(?:click|change|submit)=\\?\"([A-Za-z0-9_]+)\(", text)
    )
    return {
        "ids": by_id,
        "selectors": selectors,
        "endpoints": endpoints,
        "inline_handlers": handler_calls,
    }


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
            "layout": "web/templates/layout.php",
            "login": "web/templates/login.php",
            "app_js": "web/public/js/app.js",
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
        path = ROOT / relative
        digest = hashlib.sha256(path.read_bytes()).hexdigest()
        size = path.stat().st_size
        lines.append(f"{digest}  {size:>8}  {relative}")
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
    if failures:
        print(f"[parity] {failures} artefacto(s) desactualizado(s)")
        return 1
    print("[parity] contrato de paridad actualizado")
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
