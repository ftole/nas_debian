//! Métricas del sistema (equivalente Rust de `getSystemMetrics` del panel PHP).

use std::path::Path;

use chrono::{Local, Offset};
use nas_exec::{CommandSpec, Runner};
use serde::Serialize;

use crate::parsers::ip;

/// Métricas de estado del servidor.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SystemMetrics {
    /// Nombre del host.
    pub hostname: String,
    /// Tiempo encendido en formato `Xd Xh Xm`.
    pub uptime: String,
    /// Versión del kernel.
    pub kernel: String,
    /// Distribución del sistema operativo.
    pub os: String,
    /// Modelo de CPU.
    pub cpu_model: String,
    /// Uso de CPU estimado (carga de 1 minuto normalizada).
    pub cpu_usage_pct: f64,
    /// Memoria total en GB.
    pub ram_total_gb: f64,
    /// Memoria usada en GB.
    pub ram_used_gb: f64,
    /// Porcentaje de memoria usada.
    pub ram_usage_pct: f64,
    /// Hora local del servidor (`YYYY-MM-DD HH:MM:SS`).
    pub server_time: String,
    /// Zona horaria IANA.
    pub timezone: String,
    /// Abreviatura de la zona horaria.
    pub timezone_abbr: String,
    /// Desplazamiento de la zona horaria en segundos.
    pub timezone_offset: i64,
    /// IPv4 primaria detectada.
    pub ip: Option<String>,
}

/// Recolecta las métricas del servidor; en plataformas no Linux devuelve datos simulados.
pub fn collect(runner: &Runner) -> SystemMetrics {
    if cfg!(target_os = "linux") {
        collect_linux(runner)
    } else {
        simulated()
    }
}

fn read_trimmed(path: &str) -> Option<String> {
    std::fs::read_to_string(path)
        .ok()
        .map(|text| text.trim().to_string())
        .filter(|text| !text.is_empty())
}

fn collect_linux(runner: &Runner) -> SystemMetrics {
    let now = Local::now();
    let offset = i64::from(now.offset().fix().local_minus_utc());

    let hostname = read_trimmed("/etc/hostname")
        .or_else(|| std::env::var("HOSTNAME").ok())
        .unwrap_or_else(|| "SRV-NAS".to_string());

    let uptime = std::fs::read_to_string("/proc/uptime")
        .ok()
        .and_then(|raw| raw.split_whitespace().next().map(str::to_string))
        .and_then(|raw| raw.parse::<f64>().ok())
        .map(format_uptime)
        .unwrap_or_else(|| "N/D".to_string());

    let kernel = read_trimmed("/proc/sys/kernel/osrelease").unwrap_or_else(|| {
        runner
            .run(&CommandSpec::new("uname").arg("-r"))
            .map(|out| out.stdout)
            .unwrap_or_else(|_| "N/D".to_string())
    });

    let cpu_model = std::fs::read_to_string("/proc/cpuinfo")
        .ok()
        .and_then(|raw| {
            raw.lines()
                .find(|line| line.starts_with("model name"))
                .and_then(|line| line.split_once(':'))
                .map(|(_, value)| value.trim().to_string())
        })
        .unwrap_or_else(|| "x86_64".to_string());

    let (ram_total_gb, ram_used_gb) = read_memory();
    let ram_usage_pct = if ram_total_gb > 0.0 {
        round2((ram_used_gb / ram_total_gb) * 100.0)
    } else {
        0.0
    };

    let cpu_usage_pct = std::fs::read_to_string("/proc/loadavg")
        .ok()
        .and_then(|raw| raw.split_whitespace().next().map(str::to_string))
        .and_then(|raw| raw.parse::<f64>().ok())
        .map_or(0.0, |load| round2((load * 10.0).min(100.0)));

    let timezone = read_trimmed("/etc/timezone").unwrap_or_else(|| "UTC".to_string());

    SystemMetrics {
        hostname,
        uptime,
        kernel,
        os: "Debian 13 (Trixie)".to_string(),
        cpu_model,
        cpu_usage_pct,
        ram_total_gb,
        ram_used_gb,
        ram_usage_pct,
        server_time: now.format("%Y-%m-%d %H:%M:%S").to_string(),
        timezone,
        timezone_abbr: now.format("%Z").to_string(),
        timezone_offset: offset,
        ip: primary_ip(runner),
    }
}

fn read_memory() -> (f64, f64) {
    let Ok(raw) = std::fs::read_to_string("/proc/meminfo") else {
        return (0.0, 0.0);
    };

    let parse = |key: &str| -> Option<f64> {
        raw.lines()
            .find(|line| line.starts_with(key))
            .and_then(|line| line.split_whitespace().nth(1))
            .and_then(|value| value.parse::<f64>().ok())
    };

    let total_kb = parse("MemTotal:").unwrap_or(0.0);
    let available_kb = parse("MemAvailable:").unwrap_or(0.0);
    let used_kb = (total_kb - available_kb).max(0.0);

    (
        round2(total_kb / 1024.0 / 1024.0),
        round2(used_kb / 1024.0 / 1024.0),
    )
}

fn primary_ip(runner: &Runner) -> Option<String> {
    let output = runner
        .run(&CommandSpec::new("ip").args(["-j", "-4", "addr", "show"]))
        .ok()?;
    if !output.is_success() {
        return None;
    }
    ip::parse_interfaces(&output.stdout)
        .ok()?
        .into_iter()
        .find_map(|iface| iface.ipv4)
}

fn format_uptime(seconds: f64) -> String {
    let total = seconds.max(0.0) as u64;
    let days = total / 86_400;
    let hours = (total % 86_400) / 3_600;
    let minutes = (total % 3_600) / 60;
    format!("{days}d {hours}h {minutes}m")
}

fn round2(value: f64) -> f64 {
    (value * 100.0).round() / 100.0
}

fn simulated() -> SystemMetrics {
    let now = Local::now();
    let offset = i64::from(now.offset().fix().local_minus_utc());
    SystemMetrics {
        hostname: "SRV-NAS".to_string(),
        uptime: "14d 6h 32m".to_string(),
        kernel: "6.12.0-amd64".to_string(),
        os: "Debian 13 (Trixie)".to_string(),
        cpu_model: "CPU simulada (entorno de desarrollo)".to_string(),
        cpu_usage_pct: 8.5,
        ram_total_gb: 16.0,
        ram_used_gb: 4.2,
        ram_usage_pct: 26.25,
        server_time: now.format("%Y-%m-%d %H:%M:%S").to_string(),
        timezone: "UTC".to_string(),
        timezone_abbr: now.format("%Z").to_string(),
        timezone_offset: offset,
        ip: Some("10.10.1.2".to_string()),
    }
}

/// Indica si existe la ruta (utilidad para diagnósticos).
pub fn path_exists(path: impl AsRef<Path>) -> bool {
    path.as_ref().exists()
}

#[cfg(test)]
mod tests {
    use super::{collect, format_uptime};
    use nas_exec::Runner;

    #[test]
    fn formatea_uptime_con_dias() {
        assert_eq!(format_uptime(254_691.8), "2d 22h 44m");
        assert_eq!(format_uptime(59.0), "0d 0h 0m");
    }

    #[test]
    fn metricas_tienen_campos_obligatorios() {
        let metrics = collect(&Runner::new());
        assert!(!metrics.hostname.is_empty());
        assert!(metrics.ram_total_gb >= 0.0);
        assert!((0.0..=100.0).contains(&metrics.cpu_usage_pct));
        assert!(!metrics.server_time.is_empty());
    }
}
