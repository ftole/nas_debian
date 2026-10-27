//! Vistas del panel: renderizan las plantillas portadas 1:1 desde `layout.php`.

use std::collections::HashMap;

use crate::template::Template;

/// Plantilla completa del panel (port de `layout.php`).
pub const LAYOUT: &str = include_str!("../templates/layout.html");
/// Pantalla de acceso (port de `login.php`).
pub const LOGIN: &str = include_str!("../templates/login.html");
/// Hoja de estilos original.
pub const APP_CSS: &str = include_str!("../assets/app.css");
/// Estilos del acceso.
pub const LOGIN_CSS: &str = include_str!("../assets/login.css");
/// Sprite de iconos (no se usa directamente: va embebido en layout.html).
pub const ICONS_SVG: &str = include_str!("../assets/icons.svg");
/// Favicon.
pub const FAVICON: &[u8] = include_bytes!("../assets/favicon.svg");

/// Contexto de sesión para renderizar el panel.
#[derive(Debug)]
pub struct Shell<'a> {
    /// Vista activa (`dashboard`, `files`, …).
    pub active: &'a str,
    /// Usuario autenticado.
    pub user: &'a str,
    /// Hostname del servidor.
    pub hostname: &'a str,
    /// Grupo de trabajo de Samba.
    pub workgroup: &'a str,
    /// Etiqueta del rol del usuario (`Administrador`, …).
    pub role_label: &'a str,
    /// Rol del servidor (`ARCHIVOS`, `BACKUP`, `ARCHIVOS_BACKUP`).
    pub server_role: &'a str,
    /// Indica si es administrador.
    pub is_admin: bool,
    /// Indica si es superadministrador.
    pub is_super: bool,
    /// IPv4 del servidor.
    pub ip: &'a str,
}

fn escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

fn token(map: &mut HashMap<&'static str, String>, key: &'static str, value: impl Into<String>) {
    map.insert(key, value.into());
}

/// Genera un token CSRF aleatorio (32 bytes hex).
pub fn csrf_token() -> String {
    let mut buffer = [0u8; 32];
    if getrandom::getrandom(&mut buffer).is_err() {
        return String::from("csrf-unavailable");
    }
    hex::encode(buffer)
}

/// Indica si el volumen de datos está en Btrfs (controla los botones de scrub).
fn data_root_is_btrfs() -> bool {
    std::fs::read_to_string("/proc/mounts")
        .map(|text| {
            text.lines()
                .filter(|line| line.split_whitespace().nth(1) == Some("/srv/nas"))
                .any(|line| line.split_whitespace().nth(2) == Some("btrfs"))
        })
        .unwrap_or(false)
}

/// Renderiza el panel completo con la vista activa.
pub fn shell(context: &Shell<'_>, csrf: &str) -> String {
    let mut flags: HashMap<&str, bool> = HashMap::new();
    for view in [
        "dashboard",
        "files",
        "shares",
        "users",
        "logs",
        "storage",
        "backups",
        "services",
        "terminal",
        "networking",
        "domain",
    ] {
        flags.insert(view_flag(view), context.active == view);
    }
    flags.insert(
        "active_group_services_diagnostics",
        matches!(context.active, "services" | "diagnostics"),
    );
    flags.insert(
        "active_group_applications_updates",
        matches!(context.active, "applications" | "updates"),
    );
    flags.insert("role_has_backups", context.server_role != "ARCHIVOS");
    flags.insert("is_admin", context.is_admin);
    flags.insert("is_super", context.is_super);
    flags.insert("btrfs_controls", data_root_is_btrfs());
    flags.insert("net_health_ok", true);
    flags.insert("net_no_interfaces", false);
    flags.insert("net_dhcp_known", true);
    flags.insert("net_has_wsd_ports", true);
    flags.insert("net_firewall_hidden", true);
    flags.insert("net_no_listening", false);

    let now = chrono::Local::now();
    let mut tokens: HashMap<&str, String> = HashMap::new();
    token(&mut tokens, "hostname", escape(context.hostname));
    token(&mut tokens, "user", escape(context.user));
    token(&mut tokens, "user_role_label", escape(context.role_label));
    token(&mut tokens, "role_label", escape(context.server_role));
    token(&mut tokens, "workgroup", escape(context.workgroup));
    token(&mut tokens, "server_ip", escape(context.ip));
    token(&mut tokens, "csrf_token", csrf.to_string());
    token(
        &mut tokens,
        "active_group_services_diagnostics",
        if matches!(context.active, "services" | "diagnostics") {
            "active".to_string()
        } else {
            String::new()
        },
    );
    token(
        &mut tokens,
        "active_group_applications_updates",
        if matches!(context.active, "applications" | "updates") {
            "active".to_string()
        } else {
            String::new()
        },
    );
    token(&mut tokens, "asset_version", "1");
    token(&mut tokens, "now_ts", now.timestamp().to_string());
    token(
        &mut tokens,
        "tz_offset",
        now.offset().local_minus_utc().to_string(),
    );
    token(
        &mut tokens,
        "clock_time",
        now.format("%H:%M:%S").to_string(),
    );
    token(&mut tokens, "clock_tz", now.format("%Z").to_string());
    token(
        &mut tokens,
        "clock_datetime",
        now.format("%Y-%m-%d %H:%M:%S").to_string(),
    );
    token(
        &mut tokens,
        "clock_datetime_tz",
        now.format("%Y-%m-%d %H:%M:%S %Z").to_string(),
    );
    token(&mut tokens, "clock_zone", now.format("%Z").to_string());
    token(
        &mut tokens,
        "js_is_admin",
        if context.is_admin { "true" } else { "false" },
    );
    token(
        &mut tokens,
        "js_is_super",
        if context.is_super { "true" } else { "false" },
    );
    token(
        &mut tokens,
        "role_json",
        format!("\"{}\"", context.role_label),
    );
    token(
        &mut tokens,
        "server_role_json",
        format!("\"{}\"", context.server_role),
    );

    // Valores iniciales: app.js los actualiza al cargar la vista.
    token(&mut tokens, "kernel", "");
    token(&mut tokens, "uptime", "");
    token(&mut tokens, "cpu_model", "");
    token(&mut tokens, "cpu_pct", "0.0");
    token(&mut tokens, "ram_pct", "0.0");
    token(&mut tokens, "disk_pct", "0.0");
    token(&mut tokens, "disk_used", "0");
    token(&mut tokens, "disk_total", "0");
    token(&mut tokens, "fs_type", "");
    token(&mut tokens, "device_type", "");
    token(&mut tokens, "count_netlistening", "0");

    Template::new(LAYOUT).render(&flags, &tokens)
}

/// Renderiza la pantalla de acceso con valores por defecto.
pub fn login(hostname: &str, error: Option<&str>) -> String {
    login_with_context(hostname, "127.0.0.1", &csrf_token(), error)
}

/// Renderiza la pantalla de acceso con IP y token CSRF firmado.
pub fn login_with_context(hostname: &str, ip: &str, csrf: &str, error: Option<&str>) -> String {
    let mut tokens: HashMap<&str, String> = HashMap::new();
    let safe_host = escape(hostname);
    let safe_ip = escape(ip);
    token(&mut tokens, "hostname", safe_host.clone());
    token(&mut tokens, "safeHostname", safe_host);
    token(&mut tokens, "safeServerIp", safe_ip);
    token(&mut tokens, "csrf_token", escape(csrf));
    token(&mut tokens, "asset_version", "1");
    token(
        &mut tokens,
        "error_visible",
        if error.is_some() { "visible" } else { "" },
    );
    token(&mut tokens, "error_text", escape(error.unwrap_or("")));

    Template::new(LOGIN).render(&HashMap::new(), &tokens)
}

fn view_flag(view: &str) -> &'static str {
    match view {
        "dashboard" => "active_dashboard",
        "files" => "active_files",
        "shares" => "active_shares",
        "users" => "active_users",
        "logs" => "active_logs",
        "storage" => "active_storage",
        "backups" => "active_backups",
        "services" => "active_services",
        "terminal" => "active_terminal",
        "networking" => "active_networking",
        "domain" => "active_domain",
        _ => "active_dashboard",
    }
}
