#!/usr/bin/env python3
"""Convierte layout.php y login.php en plantillas HTML fieles para nas-web.

No reescribe el diseño: transforma el PHP en marcadores que el renderizador
Rust resuelve (`<!--IF:flag-->`, `<!--ELSE-->`, `<!--ENDIF-->` y `%%token%%`).
"""

from __future__ import annotations

import pathlib
import re

ROOT = pathlib.Path(__file__).resolve().parent.parent
WEB = ROOT / "web" / "templates"
OUT = ROOT / "crates" / "nas-web" / "templates"

ACTIVE = re.compile(r"\$activeView\s*===\s*'([a-z]+)'\s*\?\s*'active'\s*:\s*''")
ACTIVE_GROUP = re.compile(
    r"in_array\(\$activeView,\s*\[([^\]]+)\],\s*true\)\s*\?\s*'active'\s*:\s*''"
)

CONDITIONS = {
    "$serverRole !== 'ARCHIVOS'": "role_has_backups",
    "$isAdmin": "is_admin",
    "$isSuperAdmin": "is_super",
    "$serverRole !== 'ARCHIVOS' && strtolower($fsType) !== 'ext4'": "btrfs_controls",
    "$netHealthOk": "net_health_ok",
    "empty($netInterfaces)": "net_no_interfaces",
    "($net['dhcp'] ?? null) !== null": "net_dhcp_known",
    "!empty($netWsdPorts)": "net_has_wsd_ports",
    "empty($netFirewall['rules_visible'])": "net_firewall_hidden",
    "empty($netListening)": "net_no_listening",
    "!empty($error)": "has_error",
}

TOKENS = {
    "$hostname": "hostname",
    "$sessionUser": "user",
    "$kernel": "kernel",
    "$uptime": "uptime",
    "$cpuModel": "cpu_model",
    "$cpuPct": "cpu_pct",
    "$ramPct": "ram_pct",
    "$diskPct": "disk_pct",
    "$diskUsed": "disk_used",
    "$diskTotal": "disk_total",
    "$fsType": "fs_type",
    "$deviceType": "device_type",
    "$serverRoleLabel": "role_label",
    "$roleLabel": "user_role_label",
}


def slug(text: str) -> str:
    text = re.sub(r"[^a-zA-Z0-9]+", "_", text).strip("_").lower()
    return text or "token"


def token_for(expr: str) -> str:
    expr = re.sub(r"\s+", " ", expr.strip())

    if (match := ACTIVE.fullmatch(expr)) is not None:
        return f"%%active_{match.group(1)}%%"
    if (match := ACTIVE_GROUP.fullmatch(expr)) is not None:
        views = re.findall(r"'([a-z]+)'", match.group(1))
        return "%%active_group_%s%%" % "_".join(views)

    special = {
        "$isSuper ? '' : 'disabled'": "%%super_disabled%%",
        "$isSuper ? '' : 'display:none;'": "%%super_hidden%%",
        "$isAdmin ? 'true' : 'false'": "%%js_is_admin%%",
        "$isSuper ? 'true' : 'false'": "%%js_is_super%%",
        "json_encode($role)": "%%role_json%%",
        "json_encode($serverRole)": "%%server_role_json%%",
        "time()": "%%now_ts%%",
        "(int) date('Z')": "%%tz_offset%%",
        "htmlspecialchars(date('H:i:s'), ENT_QUOTES, 'UTF-8')": "%%clock_time%%",
        "htmlspecialchars(date('T'), ENT_QUOTES, 'UTF-8')": "%%clock_tz%%",
        "htmlspecialchars(date('Y-m-d H:i:s'), ENT_QUOTES, 'UTF-8')": "%%clock_datetime%%",
        "htmlspecialchars(date('Y-m-d H:i:s T'), ENT_QUOTES, 'UTF-8')": "%%clock_datetime_tz%%",
        "htmlspecialchars(date_default_timezone_get(), ENT_QUOTES, 'UTF-8')": "%%clock_zone%%",
        "htmlspecialchars(\\App\\Core\\AuthMiddleware::getCsrfToken(), ENT_QUOTES, 'UTF-8')": "%%csrf_token%%",
        "htmlspecialchars(\\App\\Services\\SystemService::getWorkgroup(), ENT_QUOTES, 'UTF-8')": "%%workgroup%%",
        "htmlspecialchars($_SERVER['SERVER_ADDR'] ?? '10.10.1.2', ENT_QUOTES, 'UTF-8')": "%%server_ip%%",
        "file_exists(__DIR__ . '/../public/css/app.css') ? filemtime(__DIR__ . '/../public/css/app.css') : '2'": "%%asset_version%%",
        "!empty($error) ? 'visible' : ''": "%%error_visible%%",
        "htmlspecialchars((string) ($error ?? ''), ENT_QUOTES, 'UTF-8')": "%%error_text%%",
    }
    if expr in special:
        return special[expr]

    for prefix, template in (
        ("htmlspecialchars(", None),
        ("$esc(", None),
    ):
        if expr.startswith(prefix) and expr.endswith(")"):
            inner = expr[len(prefix):-1]
            if prefix == "htmlspecialchars(" and "," in inner:
                inner = inner.split(",", 1)[0]
            if inner in TOKENS:
                return f"%%{TOKENS[inner]}%%"
            if inner.startswith("$") and re.fullmatch(r"\$[A-Za-z_][A-Za-z0-9_]*", inner):
                return f"%%{inner[1:]}%%"
            return f"%%{slug(inner)}%%"

    if expr in TOKENS:
        return f"%%{TOKENS[expr]}%%"
    if re.fullmatch(r"\$[A-Za-z_][A-Za-z0-9_]*", expr):
        return f"%%{expr[1:]}%%"
    if re.fullmatch(r"\$[A-Za-z_][A-Za-z0-9_]*\[[^\]]+\]", expr):
        return f"%%{slug(expr)}%%"
    if expr.startswith("$netProtocolLabel("):
        return "%%net_protocol%%"
    if expr.startswith("$net"):
        return f"%%{slug(expr)}%%"

    return f"%%{slug(expr)}%%"


def convert(source: str) -> str:
    text = source

    text = re.sub(r"<\?php\s+if\s*\((.*?)\)\s*:\s*\?>", lambda m: f"<!--IF:{conditions_for(m.group(1))}-->", text, flags=re.S)
    text = re.sub(r"<\?php\s+else\s*:\s*\?>", "<!--ELSE-->", text)
    text = re.sub(r"<\?php\s+endif;\s*\?>", "<!--ENDIF-->", text)
    text = re.sub(r"<\?php\s+foreach\s*\(.*?\)\s*:\s*\?>", "", text, flags=re.S)
    text = re.sub(r"<\?php\s+endforeach;\s*\?>", "", text)

    # Eliminar cualquier bloque PHP restante (definiciones de variables, etc.)
    text = re.sub(r"<\?php.*?\?>", "", text, flags=re.S)

    # Sustituir ecos por tokens
    text = re.sub(r"<\?=\s*(.*?)\s*\?>", lambda m: token_for(m.group(1)), text, flags=re.S)

    return text


def conditions_for(expr: str) -> str:
    expr = re.sub(r"\s+", " ", expr.strip())
    if expr in CONDITIONS:
        return CONDITIONS[expr]
    if expr.startswith("$serverRole"):
        return "role_has_backups"
    return slug(expr)


def main() -> None:
    OUT.mkdir(parents=True, exist_ok=True)
    for name, source_name in (("layout.html", "layout.php"), ("login.html", "login.php")):
        source = (WEB / source_name).read_text(encoding="utf-8")
        converted = convert(source)
        (OUT / name).write_text(converted, encoding="utf-8", newline="\n")
        tokens = sorted(set(re.findall(r"%%([a-z0-9_]+)%%", converted)))
        flags = sorted(set(re.findall(r"<!--IF:([a-z0-9_]+)-->", converted)))
        print(f"[php2html] {name}: {len(tokens)} tokens, {len(flags)} flags")
        print(f"           tokens: {', '.join(tokens)}")
        print(f"           flags : {', '.join(flags)}")


if __name__ == "__main__":
    main()
