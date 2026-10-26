"""Pruebas automatizadas de la maqueta web Cockpit (100% offline y tema claro)."""

import os
import re

MOCKUP_DIR = os.path.join(
    os.path.dirname(__file__), "..", "src", "web", "mockup"
)


def _read_file(filename):
    path = os.path.join(MOCKUP_DIR, filename)
    with open(path, "r", encoding="utf-8") as f:
        return f.read()


def test_mockup_zero_external_dependencies():
    """Garantizar estrictamente 0% dependencias externas en la maqueta."""
    files = ["index.html", "style.css", "app.js"]
    cdn_patterns = [
        "cdnjs",
        "jsdelivr",
        "unpkg",
        "googleapis",
        "gstatic",
        "bootstrapcdn",
        "fontawesome",
    ]

    for fname in files:
        content = _read_file(fname)

        # Cero llamadas http:// o https://
        urls = re.findall(r"https?://[^\s\"\'\`<>]+", content, re.IGNORECASE)
        assert urls == [], f"Se detectaron URLs externas en {fname}: {urls}"

        # Cero referencias a CDNs
        for cdn in cdn_patterns:
            assert cdn not in content.lower(), (
                f"Referencia a CDN '{cdn}' encontrada en {fname}"
            )

    # Cero @import o url() en CSS
    css_content = _read_file("style.css")
    assert "@import" not in css_content, "Uso de @import detectado en style.css"
    assert "url(" not in css_content, "Uso de url(...) detectado en style.css"


def test_light_theme_tokens_complete():
    """Validar que el tema claro defina todos los tokens 100% claros."""
    css = _read_file("style.css")
    match = re.search(r'\[data-theme="light"\]\s*\{([^}]+)\}', css)
    assert match, "No se encontró el bloque [data-theme='light'] en style.css"
    light_block = match.group(1)

    expected_tokens = {
        "--bg-masthead": "#ffffff",
        "--border-masthead": "#d2d2d2",
        "--text-masthead": "#151515",
        "--text-masthead-hostname": "#151515",
        "--masthead-logo-color": "#151515",
        "--bg-sidebar": "#fafafa",
        "--border-sidebar": "#d2d2d2",
        "--text-sidebar-title": "#6a6e73",
        "--text-sidebar-nav": "#151515",
        "--bg-sidebar-nav-hover": "#eeeeee",
        "--bg-sidebar-nav-active": "#e7f1fa",
        "--border-sidebar-nav-active": "#0066cc",
        "--bg-terminal": "#ffffff",
        "--bg-terminal-bar": "#f0f0f0",
        "--border-terminal": "#d2d2d2",
        "--text-terminal": "#151515",
        "--text-terminal-cmd": "#0066cc",
        "--text-terminal-prompt": "#004080",
        "--bg-body": "#f0f0f0",
        "--bg-card": "#ffffff",
        "--border-color": "#d2d2d2",
        "--accent-teal": "#008284",
    }

    for token, val in expected_tokens.items():
        pattern = rf"{token}\s*:\s*{re.escape(val)}"
        assert re.search(pattern, light_block), (
            f"Token '{token}: {val}' ausente o incorrecto en [data-theme='light']"
        )


def test_dark_theme_tokens_intact():
    """Validar que el modo oscuro predeterminado permanezca intacto en :root."""
    css = _read_file("style.css")
    match = re.search(r":root\s*\{([^}]+)\}", css)
    assert match, "No se encontró el bloque :root en style.css"
    root_block = match.group(1)

    expected_tokens = {
        "--bg-masthead": "#151515",
        "--border-masthead": "#292e34",
        "--bg-sidebar": "#212427",
        "--border-sidebar": "#292e34",
        "--bg-terminal": "#0b0d0e",
        "--bg-terminal-bar": "#16191c",
        "--bg-body": "#0f1214",
        "--bg-card": "#1b1d21",
        "--border-color": "#3c3f42",
        "--accent-teal": "#009596",
    }

    for token, val in expected_tokens.items():
        pattern = rf"{token}\s*:\s*{re.escape(val)}"
        assert re.search(pattern, root_block), (
            f"Token '{token}: {val}' ausente o incorrecto en :root"
        )


def test_svg_symbols_scaling_and_viewbox():
    """Comprobar que los iconos SVG usen <symbol> con viewBox para evitar recortes."""
    html = _read_file("index.html")
    # No deben existir definiciones de iconos con etiqueta <g id="icon-">
    obsolete_g_icons = re.findall(r'<g\s+id="icon-[^"]+"', html)
    assert obsolete_g_icons == [], (
        f"Se encontraron iconos definidos con <g> que sufren recorte: {obsolete_g_icons}"
    )

    # Todos los simbolos deben definir viewBox="0 0 24 24"
    symbols = re.findall(r'<symbol\s+id="icon-[^"]+"[^>]*>', html)
    assert len(symbols) >= 35, f"Pocos simbolos encontrados ({len(symbols)})"
    for s in symbols:
        assert 'viewBox="0 0 24 24"' in s, (
            f"El símbolo {s} no define viewBox='0 0 24 24'"
        )


def test_app_js_localstorage_safety():
    """Garantizar que app.js maneje localStorage con try/catch ante politicas estrictas."""
    js = _read_file("app.js")
    # Debe existir manejo con try/catch en lectura inicial
    assert "try {" in js and "localStorage.getItem('nas_theme')" in js
    # La alternancia de tema debe estar protegida
    assert "localStorage.setItem('nas_theme'" in js


def test_web_app_js_no_confirm_shadowing():
    """Garantizar que web/public/js/app.js no tenga shadowing ni llamadas directas sin window."""
    app_js_path = os.path.join(
        os.path.dirname(__file__), "..", "web", "public", "js", "app.js"
    )
    with open(app_js_path, "r", encoding="utf-8") as f:
        content = f.read()

    # No debe declarar variables confirm, alert o prompt
    shadow_decl = re.findall(
        r"\b(?:const|let|var)\s+(?:confirm|alert|prompt)\b", content
    )
    assert shadow_decl == [], f"Variables que causan shadowing: {shadow_decl}"

    # Todas las llamadas a confirm, prompt o alert deben tener prefijo window.
    bare_dialogs = re.findall(
        r"(?<!\.)\b(?:confirm|prompt|alert)\s*\(", content
    )
    assert bare_dialogs == [], f"Llamadas sin prefijo window.: {bare_dialogs}"

    # submitStorageManage debe usar confirmText y window.confirm
    assert "confirmText" in content
    assert "window.confirm(" in content
