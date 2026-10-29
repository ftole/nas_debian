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

/// Detecta el sistema de archivos y el tipo de dispositivo para `/srv/nas`.
fn detect_storage_info() -> (String, String) {
    if let Ok(mounts) = std::fs::read_to_string("/proc/mounts") {
        let mut target = None;
        for line in mounts.lines() {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 3 && parts[1] == "/srv/nas" {
                target = Some((parts[0].to_string(), parts[2].to_string()));
                break;
            }
        }
        if target.is_none() {
            for line in mounts.lines() {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() >= 3 && parts[1] == "/" {
                    target = Some((parts[0].to_string(), parts[2].to_string()));
                    break;
                }
            }
        }

        if let Some((device_path, fs)) = target {
            let fstype_upper = fs.to_uppercase();
            let mut dev_type = "HDD / SSD".to_string();

            let dev_name = device_path.trim_start_matches("/dev/");
            let parent_disk = if dev_name.starts_with("nvme") {
                if let Some(pos) = dev_name.find('p') {
                    &dev_name[..pos]
                } else {
                    dev_name
                }
            } else {
                dev_name.trim_end_matches(|c: char| c.is_ascii_digit())
            };

            let rota_path = format!("/sys/block/{parent_disk}/queue/rotational");
            if let Ok(rota) = std::fs::read_to_string(&rota_path) {
                if rota.trim() == "0" {
                    dev_type = "SSD (Flash)".to_string();
                } else if rota.trim() == "1" {
                    dev_type = "HDD (Mecánico)".to_string();
                }
            } else if device_path.contains("nvme") {
                dev_type = "NVMe (Flash)".to_string();
            }

            return (fstype_upper, dev_type);
        }
    }

    (
        if data_root_is_btrfs() {
            "BTRFS"
        } else {
            "EXT4"
        }
        .to_string(),
        "NVMe / SSD".to_string(),
    )
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

    let runner = nas_exec::Runner::new();
    let metrics = nas_engine::collect_status(&runner);
    token(&mut tokens, "kernel", escape(&metrics.kernel));
    token(&mut tokens, "uptime", escape(&metrics.uptime));
    token(&mut tokens, "cpu_model", escape(&metrics.cpu_model));
    token(
        &mut tokens,
        "cpu_pct",
        format!("{:.1}", metrics.cpu_usage_pct),
    );
    token(
        &mut tokens,
        "ram_pct",
        format!("{:.1}", metrics.ram_usage_pct),
    );
    token(
        &mut tokens,
        "ramUsed",
        format!("{:.1}", metrics.ram_used_gb),
    );
    token(
        &mut tokens,
        "ramTotal",
        format!("{:.1}", metrics.ram_total_gb),
    );

    let storage = nas_engine::storage::overview_for(&nas_core::NasPaths::default(), &runner).ok();
    let (disk_pct, disk_used, disk_total) = if let Some(stg) = &storage {
        (stg.usage_percent, stg.used_gb, stg.total_gb)
    } else {
        (0.0, 0.0, 0.0)
    };
    token(&mut tokens, "disk_pct", format!("{:.1}", disk_pct));
    token(&mut tokens, "disk_used", format!("{:.1}", disk_used));
    token(&mut tokens, "disk_total", format!("{:.1}", disk_total));

    let (
        stg_color,
        stg_badge_display,
        stg_badge_class,
        stg_badge_text,
        stg_alert_display,
        stg_alert_class,
        stg_alert_text,
        stg_card_border,
    ) = if disk_pct >= 90.0 {
        (
            "var(--accent-danger)",
            "inline-block",
            "badge badge-err",
            format!("Crítico ({:.1}%)", disk_pct),
            "flex",
            "alert-box alert-box-danger",
            format!(
                "Capacidad crítica superada ({:.1}% en uso). A partir del 95% de uso las tareas de respaldo se cancelarán.",
                disk_pct
            ),
            "border-color:var(--accent-danger);",
        )
    } else if disk_pct >= 80.0 {
        (
            "var(--accent-warning)",
            "inline-block",
            "badge badge-warn",
            format!("Alerta preventiva ({:.1}%)", disk_pct),
            "flex",
            "alert-box alert-box-warning",
            format!(
                "Umbral preventivo superado ({:.1}% en /srv/nas). Al superar el 85%, el sistema generará alertas operativas.",
                disk_pct
            ),
            "border-color:var(--accent-warning);",
        )
    } else {
        (
            "var(--accent-primary)",
            "none",
            "badge",
            String::new(),
            "none",
            "alert-box",
            String::new(),
            "",
        )
    };
    token(&mut tokens, "stg_bar_color", stg_color.to_string());
    token(
        &mut tokens,
        "stg_badge_display",
        stg_badge_display.to_string(),
    );
    token(&mut tokens, "stg_badge_class", stg_badge_class.to_string());
    token(&mut tokens, "stg_badge_text", stg_badge_text);
    token(
        &mut tokens,
        "stg_alert_display",
        stg_alert_display.to_string(),
    );
    token(&mut tokens, "stg_alert_class", stg_alert_class.to_string());
    token(&mut tokens, "stg_alert_text", stg_alert_text);
    token(&mut tokens, "stg_card_border", stg_card_border.to_string());
    let (fstype, device_type) = detect_storage_info();
    token(&mut tokens, "strtoupper_fstype", fstype.clone());
    token(&mut tokens, "fs_type", fstype);
    token(&mut tokens, "device_type", device_type);
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
