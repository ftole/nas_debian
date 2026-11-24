//! Inspección de caché volátil de escritura (`hdparm -W`) y reglas udev persistentes.

use nas_exec::{CommandSpec, Runner, SudoMode};
use serde::{Deserialize, Serialize};

use super::pools::resolve_target_device;
use super::{clean_block_source, is_valid_device};

/// Interpreta la salida de `hdparm -W` (`Some(true)` activa, `Some(false)` desactivada).
pub fn parse_hdparm_write_cache(raw: &str) -> Option<bool> {
    for line in raw.lines() {
        let lower = line.to_ascii_lowercase();
        if !lower.contains("write-caching") {
            continue;
        }
        if lower.contains("not supported") {
            return None;
        }
        if lower.contains("(on)") || lower.contains("=  1") || lower.contains("= 1") {
            return Some(true);
        }
        if lower.contains("(off)") || lower.contains("=  0") || lower.contains("= 0") {
            return Some(false);
        }
    }
    None
}

/// Resultado estructurado de consulta de caché volátil de escritura (`hdparm -W`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WriteCacheProbe {
    /// `true` si el disco/controlador soporta consulta de write-cache.
    pub supported: bool,
    /// `Some(true)` activa, `Some(false)` desactivada, `None` desconocida.
    pub enabled: Option<bool>,
    /// Detalle legible del estado.
    pub detail: String,
}

#[cfg(unix)]
pub(super) fn is_euid_root() -> bool {
    let Ok(status) = std::fs::read_to_string("/proc/self/status") else {
        return false;
    };
    for line in status.lines() {
        if let Some(rest) = line.strip_prefix("Uid:") {
            let mut parts = rest.split_whitespace();
            let _real = parts.next();
            if let Some(euid) = parts.next() {
                return euid == "0";
            }
        }
    }
    false
}

#[cfg(not(unix))]
pub(super) fn is_euid_root() -> bool {
    false
}

/// Inspecciona directamente el estado de caché volátil vía `hdparm -W` (sin elevación `sudo`).
pub fn inspect_write_cache_direct(runner: &Runner, device: &str) -> WriteCacheProbe {
    let disk_path = resolve_target_device(clean_block_source(device))
        .unwrap_or_else(|| device.trim().to_string());
    if !is_valid_device(&disk_path) {
        return WriteCacheProbe {
            supported: false,
            enabled: None,
            detail: format!("dispositivo inválido para hdparm ({device})"),
        };
    }
    let Ok(out) = runner.run(
        &CommandSpec::new("hdparm")
            .args(["-W", &disk_path])
            .timeout(std::time::Duration::from_secs(5)),
    ) else {
        return WriteCacheProbe {
            supported: false,
            enabled: None,
            detail: "hdparm no disponible; ejecuta 'nas deploy ensure-deps' para instalarlo"
                .to_string(),
        };
    };
    if !out.is_success() {
        let err = out.stderr.trim();
        let msg = if err.is_empty() {
            "Caché volátil no consultable en este controlador".to_string()
        } else {
            format!("Sin acceso a caché vía hdparm ({err})")
        };
        return WriteCacheProbe {
            supported: false,
            enabled: None,
            detail: msg,
        };
    }
    match parse_hdparm_write_cache(&out.stdout) {
        Some(true) => WriteCacheProbe {
            supported: true,
            enabled: Some(true),
            detail: "Caché volátil ACTIVA (rendimiento estándar; riesgo en corte seco sin PLP)"
                .to_string(),
        },
        Some(false) => WriteCacheProbe {
            supported: true,
            enabled: Some(false),
            detail: "Caché volátil DESACTIVADA (máxima protección ante cortes eléctricos)"
                .to_string(),
        },
        None => WriteCacheProbe {
            supported: false,
            enabled: None,
            detail: "El controlador/disco no reporta control write-cache por hdparm".to_string(),
        },
    }
}

/// Inspecciona el estado de la caché volátil de escritura de un disco físico vía `hdparm -W`
/// (con respaldo `sudo` acotado vía `nas helper storage-apply write-cache-status` cuando corre sin root).
pub fn inspect_write_cache(runner: &Runner, device: &str) -> (bool, Option<bool>, String) {
    let direct = inspect_write_cache_direct(runner, device);
    if direct.supported || direct.enabled.is_some() || is_euid_root() {
        return (direct.supported, direct.enabled, direct.detail);
    }

    let disk_path = resolve_target_device(clean_block_source(device))
        .unwrap_or_else(|| device.trim().to_string());
    if is_valid_device(&disk_path) {
        let spec = CommandSpec::new("/usr/local/bin/nas")
            .args(["helper", "storage-apply", "write-cache-status", &disk_path])
            .sudo(SudoMode::Passwordless)
            .timeout(std::time::Duration::from_secs(8));
        if let Ok(out) = runner.run(&spec) {
            if out.is_success() {
                if let Ok(parsed) = serde_json::from_str::<WriteCacheProbe>(out.stdout.trim()) {
                    return (parsed.supported, parsed.enabled, parsed.detail);
                }
            }
        }
    }

    (direct.supported, direct.enabled, direct.detail)
}

/// Genera la regla udev persistente para aplicar `hdparm -W0` o `-W1` al conectar/arrancar el disco.
pub fn render_write_cache_udev_rule(
    dev_id_link: Option<&str>,
    kernel_name: &str,
    enabled: bool,
) -> String {
    let flag = if enabled { "-W1" } else { "-W0" };
    let clean_kernel = kernel_name.trim().trim_start_matches("/dev/");
    let matcher = match dev_id_link.map(str::trim).filter(|s| !s.is_empty()) {
        Some(id) => {
            format!(r#"SUBSYSTEM=="block", ENV{{DEVTYPE}}=="disk", ENV{{DEVLINKS}}=="*{id}*""#)
        }
        None => format!(r#"SUBSYSTEM=="block", ENV{{DEVTYPE}}=="disk", KERNEL=="{clean_kernel}""#),
    };
    format!(
        "# Regla generada por nas_debian para persistir protección write-cache\n\
         ACTION==\"add|change\", {matcher}, RUN+=\"/usr/sbin/hdparm {flag} /dev/%k\"\n"
    )
}
