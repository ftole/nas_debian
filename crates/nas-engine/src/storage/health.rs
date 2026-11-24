//! Detección rotacional (sysfs/lsblk) y salud preventiva S.M.A.R.T. / solo lectura.

use std::collections::HashSet;
use std::path::Path;

use nas_exec::{CommandSpec, Runner, SudoMode};
use serde::{Deserialize, Serialize};

use super::parsers::{mount_source_from_proc_mounts, path_is_mounted_readonly};
use super::pools::{findmnt_value, resolve_target_device};
use super::write_cache::is_euid_root;
use super::{clean_block_source, is_valid_device, parent_disk};

/// Interpreta una bandera booleana de rotación (`0`/`1`, `true`/`false`, `yes`/`no`).
pub fn parse_rotational_flag(raw: &str) -> Option<bool> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" => Some(true),
        "0" | "false" | "no" => Some(false),
        _ => None,
    }
}

fn resolve_sysfs_rotational(
    sys_root: &Path,
    dev_name: &str,
    visited: &mut HashSet<String>,
    depth: usize,
) -> Option<bool> {
    if depth >= 8 || dev_name.is_empty() || !visited.insert(dev_name.to_string()) {
        return None;
    }

    // 1. Dispositivos apilados (LVM, dm-crypt, MD RAID): recorrer `/sys/.../slaves/*`.
    for slaves_dir in [
        sys_root.join("class/block").join(dev_name).join("slaves"),
        sys_root.join("block").join(dev_name).join("slaves"),
    ] {
        if let Ok(entries) = std::fs::read_dir(&slaves_dir) {
            let mut saw_slave = false;
            let mut any_non_rotational = false;
            for entry in entries.flatten() {
                saw_slave = true;
                let slave_name = entry.file_name().to_string_lossy().to_string();
                match resolve_sysfs_rotational(sys_root, &slave_name, visited, depth + 1) {
                    Some(true) => return Some(true),
                    Some(false) => any_non_rotational = true,
                    None => {}
                }
            }
            if saw_slave && any_non_rotational {
                return Some(false);
            }
        }
    }

    // 2. Cola directa del dispositivo en sysfs (`queue/rotational`).
    for rota_path in [
        sys_root
            .join("class/block")
            .join(dev_name)
            .join("queue/rotational"),
        sys_root
            .join("block")
            .join(dev_name)
            .join("queue/rotational"),
    ] {
        if let Ok(raw) = std::fs::read_to_string(&rota_path) {
            if let Some(flag) = parse_rotational_flag(&raw) {
                return Some(flag);
            }
        }
    }

    // 3. Partición: resolver hacia el disco padre mediante enlace sysfs o nombre de partición.
    let class_entry = sys_root.join("class/block").join(dev_name);
    if let Ok(canon) = std::fs::canonicalize(&class_entry) {
        if let Some(parent_name) = canon
            .parent()
            .and_then(|p| p.file_name())
            .and_then(|n| n.to_str())
        {
            if parent_name != dev_name && parent_name != "block" {
                if let Some(flag) =
                    resolve_sysfs_rotational(sys_root, parent_name, visited, depth + 1)
                {
                    return Some(flag);
                }
            }
        }
    }

    if let Some(parent_name) = parent_disk(dev_name) {
        if parent_name != dev_name {
            if let Some(flag) = resolve_sysfs_rotational(sys_root, &parent_name, visited, depth + 1)
            {
                return Some(flag);
            }
        }
    }

    None
}

/// Detecta si un dispositivo de bloque es rotacional recorriendo `slaves/*` en un árbol sysfs.
pub fn detect_rotational_in_sysfs(sys_root: &Path, device_or_name: &str) -> Option<bool> {
    let trimmed = clean_block_source(device_or_name);
    if trimmed.is_empty() {
        return None;
    }

    let resolved = if sys_root == Path::new("/sys") && trimmed.starts_with("/dev/") {
        std::fs::canonicalize(trimmed)
            .ok()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|| trimmed.to_string())
    } else {
        trimmed.to_string()
    };

    let dev_name = resolved
        .trim_start_matches("/dev/")
        .trim_end_matches('/')
        .to_string();
    let mut visited = HashSet::new();
    if let Some(flag) = resolve_sysfs_rotational(sys_root, &dev_name, &mut visited, 0) {
        return Some(flag);
    }

    if let Some(last) = dev_name.rsplit('/').next().filter(|s| *s != dev_name) {
        let mut visited_last = HashSet::new();
        return resolve_sysfs_rotational(sys_root, last, &mut visited_last, 0);
    }

    None
}

/// Detecta de forma robusta si un dispositivo (`/dev/...`, LVM, MD RAID o VM) es rotacional.
pub fn detect_device_rotational(runner: &Runner, device: &str) -> Option<bool> {
    let device = clean_block_source(device);
    if device.is_empty() {
        return None;
    }
    if let Some(flag) = detect_rotational_in_sysfs(Path::new("/sys"), device) {
        return Some(flag);
    }

    // `lsblk -nso ROTA` recorre toda la pila inversa (`slaves`) hasta discos físicos.
    for args in [["-nso", "ROTA", device], ["-ndo", "ROTA", device]] {
        if let Ok(out) = runner.run(
            &CommandSpec::new("lsblk")
                .args(args)
                .timeout(std::time::Duration::from_secs(5)),
        ) {
            if out.is_success() {
                let mut saw_false = false;
                for line in out.stdout.lines() {
                    if let Some(flag) = parse_rotational_flag(line) {
                        if flag {
                            return Some(true);
                        }
                        saw_false = true;
                    }
                }
                if saw_false {
                    return Some(false);
                }
            }
        }
    }

    None
}

/// Resultado de comprobación preventiva S.M.A.R.T. y de montaje.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiskHealthCheck {
    /// `false` únicamente cuando se detecta un fallo explícito (`ro` o SMART FAILED).
    pub healthy: bool,
    /// `true` cuando `smartctl` devolvió un estado evaluable del dispositivo.
    pub smart_available: bool,
    /// Detalle legible del diagnóstico.
    pub detail: String,
}

/// Interpreta la salida de `smartctl -H` (`Some(true)` = sano, `Some(false)` = fallo explícito).
pub fn parse_smartctl_health(output: &str) -> Option<bool> {
    for line in output.lines() {
        let trimmed = line.trim();
        let lower = trimmed.to_ascii_lowercase();
        if lower.contains("overall-health self-assessment test result:")
            || lower.contains("smart health status:")
        {
            if lower.contains("failed") || lower.contains("error") {
                return Some(false);
            }
            if lower.contains("passed") || lower.ends_with(": ok") || lower.contains(": ok ") {
                return Some(true);
            }
        }
    }
    None
}

fn resolve_smart_probe_device(device: &str) -> String {
    let cleaned = clean_block_source(device);
    let target_dev = resolve_target_device(cleaned).unwrap_or_else(|| cleaned.to_string());
    if target_dev.starts_with("/dev/mapper/") || target_dev.starts_with("/dev/dm-") {
        target_dev
    } else if let Some(parent) = parent_disk(&target_dev) {
        format!("/dev/{parent}")
    } else {
        target_dev
    }
}

/// Evalúa directamente la salud S.M.A.R.T. vía `smartctl -H` (sin elevación `sudo`).
pub fn check_block_health_direct(runner: &Runner, device: &str) -> DiskHealthCheck {
    let probe = resolve_smart_probe_device(device);
    if !is_valid_device(&probe) {
        return DiskHealthCheck {
            healthy: true,
            smart_available: false,
            detail: format!("dispositivo no evaluable por SMART ({device})"),
        };
    }

    let spec = CommandSpec::new("smartctl")
        .args(["-H", &probe])
        .timeout(std::time::Duration::from_secs(10));
    match runner.run(&spec) {
        Ok(out) => {
            let combined = format!("{}\n{}", out.stdout, out.stderr);
            match parse_smartctl_health(&combined) {
                Some(true) => DiskHealthCheck {
                    healthy: true,
                    smart_available: true,
                    detail: format!("{probe}: SMART PASSED"),
                },
                Some(false) => DiskHealthCheck {
                    healthy: false,
                    smart_available: true,
                    detail: format!("{probe}: S.M.A.R.T. reporta fallo de hardware (FAILED)"),
                },
                None => DiskHealthCheck {
                    healthy: true,
                    smart_available: false,
                    detail: format!("{probe}: SMART no soportado o sin veredicto"),
                },
            }
        }
        Err(_) => DiskHealthCheck {
            healthy: true,
            smart_available: false,
            detail: "smartctl no disponible en el sistema".to_string(),
        },
    }
}

/// Evalúa la salud S.M.A.R.T. de un dispositivo de bloque (degrada sin error si `smartctl` no existe;
/// bajo proceso sin privilegios como `nas-web` usa respaldo `sudo` acotado vía `nas helper storage-apply`).
pub fn check_block_health(runner: &Runner, device: &str) -> DiskHealthCheck {
    let direct = check_block_health_direct(runner, device);
    if direct.smart_available || !direct.healthy || is_euid_root() {
        return direct;
    }

    let probe = resolve_smart_probe_device(device);
    if is_valid_device(&probe) {
        let spec = CommandSpec::new("/usr/local/bin/nas")
            .args(["helper", "storage-apply", "smart-health", &probe])
            .sudo(SudoMode::Passwordless)
            .timeout(std::time::Duration::from_secs(12));
        if let Ok(out) = runner.run(&spec) {
            if out.is_success() {
                if let Ok(parsed) = serde_json::from_str::<DiskHealthCheck>(out.stdout.trim()) {
                    return parsed;
                }
            }
        }
    }

    direct
}

/// Comprueba estado `ro` y salud S.M.A.R.T. del volumen que aloja `path`.
pub fn check_path_storage_health(runner: &Runner, path: &Path) -> DiskHealthCheck {
    if path_is_mounted_readonly(path) {
        return DiskHealthCheck {
            healthy: false,
            smart_available: false,
            detail: format!(
                "{}: sistema de archivos montado en solo lectura (ro)",
                path.display()
            ),
        };
    }
    let Some(source) = findmnt_value(runner, &path.to_string_lossy(), "SOURCE")
        .filter(|s| s.starts_with("/dev/"))
        .or_else(|| mount_source_from_proc_mounts(path))
    else {
        return DiskHealthCheck {
            healthy: true,
            smart_available: false,
            detail: format!("{}: sin dispositivo de bloque asociado", path.display()),
        };
    };
    check_block_health(runner, &source)
}

/// Detecta si el volumen que aloja `path` reside sobre almacenamiento rotacional.
pub fn detect_path_rotational(runner: &Runner, path: &Path) -> Option<bool> {
    let mut existing = path;
    while !existing.exists() {
        let Some(parent) = existing.parent() else {
            break;
        };
        if parent.as_os_str().is_empty() {
            existing = Path::new("/");
            break;
        }
        existing = parent;
    }

    let source = findmnt_value(runner, &existing.to_string_lossy(), "SOURCE")
        .filter(|s| s.starts_with("/dev/"))
        .or_else(|| mount_source_from_proc_mounts(existing))?;
    detect_device_rotational(runner, &source)
}
