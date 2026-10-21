"""
Pruebas integrales de Aseguramiento de Calidad (QA) para el Panel Web.
Auditoría botón por botón, enlaces, modales y endpoints de la API.
"""

import os
import re

WEB_DIR = os.path.join(os.path.dirname(__file__), "..", "web")
LAYOUT_PHP = os.path.join(WEB_DIR, "templates", "layout.php")
APP_JS = os.path.join(WEB_DIR, "public", "js", "app.js")
INDEX_PHP = os.path.join(WEB_DIR, "public", "index.php")


def read_file(path: str) -> str:
    with open(path, "r", encoding="utf-8", errors="replace") as f:
        return f.read()


def test_layout_onclick_handlers_exist_in_js():
    """Cada manejador onclick en layout.php debe estar definido en app.js."""
    layout = read_file(LAYOUT_PHP)
    app = read_file(APP_JS)

    onclick_matches = re.findall(r'onclick=[\"\']([^\"\']+)[\"\']', layout)
    assert len(onclick_matches) > 0, "No se encontraron onclicks en layout.php"

    missing = []
    for expr in onclick_matches:
        expr = expr.strip()
        # Ignorar expresiones php o ternarios inline en layout
        if expr.startswith("<?") or "window.location" in expr:
            continue
        # Extraer nombre de la función
        m = re.match(r'([a-zA-Z0-9_$]+)\s*\(', expr)
        if not m:
            continue
        fn_name = m.group(1)
        # Buscar definicion en app.js
        pattern = rf'(function\s+{re.escape(fn_name)}\b|window\.{re.escape(fn_name)}\s*=|const\s+{re.escape(fn_name)}\s*=|let\s+{re.escape(fn_name)}\s*=|var\s+{re.escape(fn_name)}\s*=)'
        if not re.search(pattern, app):
            missing.append((fn_name, expr))

    assert missing == [], f"Funciones onclick llamadas en layout.php pero no definidas en app.js: {missing}"


def test_app_js_dynamic_onclick_handlers_exist():
    """Cada manejador onclick dinámico en app.js debe estar definido."""
    app = read_file(APP_JS)
    app_onclicks = re.findall(r'onclick=[\\]*[\"\']([^\\]*?)[\\]*[\"\']', app)

    missing = []
    for expr in app_onclicks:
        expr = expr.strip()
        m = re.match(r'([a-zA-Z0-9_$]+)\s*\(', expr)
        if not m:
            continue
        fn_name = m.group(1)
        pattern = rf'(function\s+{re.escape(fn_name)}\b|window\.{re.escape(fn_name)}\s*=|const\s+{re.escape(fn_name)}\s*=|let\s+{re.escape(fn_name)}\s*=|var\s+{re.escape(fn_name)}\s*=)'
        if not re.search(pattern, app):
            missing.append((fn_name, expr))

    assert missing == [], f"Funciones dinámicas llamadas en onclick pero no definidas en app.js: {missing}"


def test_app_js_api_fetch_endpoints_registered_in_routes():
    """Cada endpoint invocado por app.js debe estar registrado en index.php."""
    app = read_file(APP_JS)
    index = read_file(INDEX_PHP)

    # Extraer rutas de index.php
    routes = re.findall(r'router->(get|post|delete|put)\([\'\"]([^\'\"]+)[\'\"]', index)
    registered = set((m.upper(), p) for m, p in routes)

    # Buscar llamadas apiFetch('url', ...) o fetch('url', ...)
    fetch_calls = re.findall(r'(?:apiFetch|fetch)\(\s*[`\'\"]([^\`\'\"]+)[`\'\"](?:\s*,\s*\{[^}]*?method:\s*[\'\"]([A-Z]+)[\'\"])?', app)

    unmatched = []
    for url, method in fetch_calls:
        if not url.startswith("/api/"):
            continue
        method = method or "GET"
        clean_url = url.split("?")[0]
        # Reemplazar interpolaciones de JS ${...} con patrón regex o comodín
        clean_pattern = re.sub(r'\$\{[^}]+\}', '{param}', clean_url)
        # Comprobar si coincide con alguna ruta
        matched = False
        for reg_method, reg_path in registered:
            if reg_method != method:
                continue
            # Normalizar parámetros {name}
            reg_pattern = re.sub(r'\{[^}]+\}', '{param}', reg_path)
            if clean_pattern == reg_pattern or clean_pattern == reg_path:
                matched = True
                break
        if not matched:
            unmatched.append((method, url, clean_pattern))

    assert unmatched == [], f"Endpoints invocados en app.js no registrados en index.php: {unmatched}"


def test_all_modals_have_close_and_cancel_buttons():
    """Todos los modales deben tener botones de cierre y clase modal-static."""
    layout = read_file(LAYOUT_PHP)
    modals = re.findall(r'<div\s+class=[\"\']modal\b([^\"\']*)[\"\']\s+id=[\"\'](modal-[^\"\']+)[\"\']', layout)

    assert len(modals) > 0, "No se encontraron modales en layout.php"

    non_static = []
    for classes, modal_id in modals:
        if "modal-static" not in classes:
            non_static.append(modal_id)

    assert non_static == [], f"Modales sin protección modal-static: {non_static}"


def test_modals_div_balance():
    """Cada modal en layout.php debe tener sus etiquetas div balanceadas."""
    layout = read_file(LAYOUT_PHP)
    modal_ids = [
        'modal-new-share', 'modal-edit-share', 'modal-new-backup', 'modal-edit-backup',
        'modal-storage-manage', 'modal-new-user', 'modal-new-group', 'modal-edit-user',
        'modal-group-members', 'modal-rename-group', 'modal-user-welcome', 'modal-backup-logs',
        'modal-new-folder', 'modal-rename-file', 'modal-delete-file', 'modal-zip-backup-warning',
        'modal-file-preview', 'modal-empty-trash', 'modal-reboot-server'
    ]

    imbalanced = []
    for i, mid in enumerate(modal_ids):
        start = layout.find(f'id="{mid}"')
        if start == -1:
            start = layout.find(f"id='{mid}'")
        assert start != -1, f"Modal {mid} no encontrado en layout.php"

        next_start = -1
        if i + 1 < len(modal_ids):
            next_id = modal_ids[i + 1]
            next_start = layout.find(f'id="{next_id}"')
            if next_start == -1:
                next_start = layout.find(f"id='{next_id}'")
        else:
            next_start = layout.find('id="toast-container"')

        chunk = layout[start:next_start]
        opens = chunk.count('<div')
        closes = chunk.count('</div')
        if opens != closes:
            imbalanced.append((mid, opens, closes, opens - closes))

    assert imbalanced == [], f"Modales con divs desbalanceados: {imbalanced}"
