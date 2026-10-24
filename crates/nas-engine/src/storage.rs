//! Inventario de almacenamiento: uso, discos e inspección.

use nas_core::{NasError, NasPaths, NasResult};
use nas_exec::{CommandSpec, Runner};
use serde::Serialize;

use crate::parsers::lsblk;

/// Resumen del volumen de datos.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct StorageOverview {
    /// Punto de montaje.
    pub path: String,
    /// Sistema de archivos.
    pub filesystem: String,
    /// Dispositivo de bloque subyacente.
    pub device: Option<String>,
    /// Capacidad total en GB.
    pub total_gb: f64,
    /// Espacio usado en GB.
    pub used_gb: f64,
    /// Espacio libre en GB.
    pub free_gb: f64,
    /// Porcentaje de uso.
    pub usage_percent: f64,
    /// Indica si el volumen es Btrfs.
    pub is_btrfs: bool,
    /// Indica si el dispositivo subyacente es flash.
    pub supports_trim: bool,
    /// Tipo de dispositivo legible.
    pub device_type: String,
}

/// Disco físico con protección del disco del sistema.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Disk {
    /// Nombre del dispositivo.
    pub name: String,
    /// Ruta completa.
    pub device: String,
    /// Tamaño legible.
    pub size: String,
    /// Modelo.
    pub model: Option<String>,
    /// Sistema de archivos.
    pub fstype: Option<String>,
    /// Punto de montaje principal.
    pub mountpoint: Option<String>,
    /// Indica si es mecánico.
    pub rotational: Option<bool>,
    /// Indica si aloja el sistema operativo.
    pub protected: bool,
    /// Indica si está montado en algún punto.
    pub in_use: bool,
}

/// Uso completo de un sistema de archivos según `df -Pk`.
#[derive(Debug, Clone, PartialEq)]
pub struct DfFull {
    /// Bloques totales en KiB.
    pub total_kb: u64,
    /// Bloques usados en KiB.
    pub used_kb: u64,
    /// Bloques disponibles en KiB.
    pub available_kb: u64,
    /// Porcentaje de uso.
    pub used_percent: f64,
}

/// Interpreta la segunda línea de `df -Pk`.
pub fn parse_df_full(output: &str) -> Option<DfFull> {
    let line = output.lines().nth(1)?;
    let fields: Vec<&str> = line.split_whitespace().collect();
    if fields.len() < 5 {
        return None;
    }

    Some(DfFull {
        total_kb: fields[1].parse().ok()?,
        used_kb: fields[2].parse().ok()?,
        available_kb: fields[3].parse().ok()?,
        used_percent: fields[4].trim_end_matches('%').parse().ok()?,
    })
}

/// Resumen del volumen indicado.
pub fn overview(runner: &Runner, path: &str) -> NasResult<StorageOverview> {
    let df = runner.run(&CommandSpec::new("df").args(["-Pk", path]))?;
    if !df.is_success() {
        return Err(NasError::Command {
            command: df.command,
            code: df.code,
            stderr: df.stderr,
        });
    }
    let usage = parse_df_full(&df.stdout)
        .ok_or_else(|| NasError::Invalid("salida de df no reconocida".into()))?;

    let filesystem = findmnt_value(runner, path, "FSTYPE").unwrap_or_else(|| "desconocido".into());
    let device = findmnt_value(runner, path, "SOURCE");

    let is_btrfs = filesystem.eq_ignore_ascii_case("btrfs");
    let mut supports_trim = false;
    let mut device_type = "Desconocido".to_string();

    if let Some(device) = &device {
        if let Some(disk) = parent_disk(device) {
            let rotational = runner
                .run(&CommandSpec::new("lsblk").args(["-ndo", "ROTA", &disk]))
                .ok()
                .and_then(|out| out.stdout.trim().parse::<u8>().ok());
            match rotational {
                Some(0) => {
                    supports_trim = true;
                    device_type = "SSD (Flash)".to_string();
                }
                Some(_) => device_type = "HDD (Mecánico)".to_string(),
                None => {}
            }
        }
    }

    Ok(StorageOverview {
        path: path.to_string(),
        filesystem,
        device,
        total_gb: round2(usage.total_kb as f64 / 1024.0 / 1024.0),
        used_gb: round2(usage.used_kb as f64 / 1024.0 / 1024.0),
        free_gb: round2(usage.available_kb as f64 / 1024.0 / 1024.0),
        usage_percent: usage.used_percent,
        is_btrfs,
        supports_trim,
        device_type,
    })
}

/// Lista los discos físicos, marcando el del sistema operativo.
pub fn disks(runner: &Runner) -> NasResult<Vec<Disk>> {
    let output = runner.run(&CommandSpec::new("lsblk").args([
        "-J",
        "-o",
        "NAME,SIZE,TYPE,MOUNTPOINT,FSTYPE,MODEL,ROTA",
    ]))?;
    if !output.is_success() {
        return Err(NasError::Command {
            command: output.command,
            code: output.code,
            stderr: output.stderr,
        });
    }

    let devices = lsblk::parse(&output.stdout)
        .map_err(|error| NasError::Invalid(format!("lsblk inválido: {error}")))?;

    let os_source = findmnt_value(runner, "/", "SOURCE");
    let os_disk = os_source.as_deref().and_then(parent_disk);

    Ok(devices
        .iter()
        .filter(|device| device.kind == "disk")
        .map(|device| {
            let flat = device.flatten();
            let mountpoint = flat.iter().find_map(|entry| entry.mountpoint.clone());
            let in_use = flat.iter().any(|entry| entry.mountpoint.is_some());
            let protected = os_disk
                .as_deref()
                .is_some_and(|name| name == device.device_path());

            Disk {
                name: device.name.clone(),
                device: device.device_path(),
                size: device.size.clone(),
                model: device.model.clone(),
                fstype: flat.iter().find_map(|entry| entry.fstype.clone()),
                mountpoint,
                rotational: device.rotational,
                protected,
                in_use,
            }
        })
        .collect())
}

/// Inspección detallada de un dispositivo `/dev/...`.
pub fn inspect(runner: &Runner, device: &str) -> NasResult<serde_json::Value> {
    if !is_valid_device(device) {
        return Err(NasError::Denied(format!("dispositivo inválido: {device}")));
    }

    let output = runner.run(&CommandSpec::new("lsblk").args([
        "-J",
        "-b",
        "-o",
        "NAME,SIZE,TYPE,MOUNTPOINT,FSTYPE,MODEL,ROTA",
        device,
    ]))?;
    if !output.is_success() {
        return Err(NasError::NotFound(format!(
            "no se pudo inspeccionar {device}: {}",
            output.stderr
        )));
    }

    let devices = lsblk::parse(&output.stdout)
        .map_err(|error| NasError::Invalid(format!("lsblk inválido: {error}")))?;
    let Some(root) = devices.first() else {
        return Err(NasError::NotFound(format!("sin datos para {device}")));
    };

    let in_use = root
        .flatten()
        .iter()
        .any(|entry| entry.mountpoint.is_some());

    Ok(serde_json::json!({
        "device": device,
        "name": root.name,
        "size": root.size,
        "model": root.model,
        "rotational": root.rotational,
        "in_use": in_use,
        "partitions": root.children.iter().map(|child| serde_json::json!({
            "name": child.name,
            "size": child.size,
            "fstype": child.fstype,
            "mountpoint": child.mountpoint,
        })).collect::<Vec<_>>(),
    }))
}

/// Valida que el dispositivo sea una ruta `/dev/...` simple.
pub fn is_valid_device(device: &str) -> bool {
    device.strip_prefix("/dev/").is_some_and(|rest| {
        !rest.is_empty()
            && rest
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '/' || c == '-' || c == '_')
    })
}

/// Resumen del volumen de datos configurado.
pub fn overview_for(paths: &NasPaths, runner: &Runner) -> NasResult<StorageOverview> {
    overview(runner, &paths.data_root.to_string_lossy())
}

fn findmnt_value(runner: &Runner, path: &str, column: &str) -> Option<String> {
    runner
        .run(&CommandSpec::new("findmnt").args(["-n", "-o", column, "--target", path]))
        .ok()
        .filter(|output| output.is_success())
        .map(|output| output.stdout.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn parent_disk(device: &str) -> Option<String> {
    let name = device.rsplit('/').next()?;
    if name.is_empty() {
        return None;
    }
    let disk = if let Some(index) = name.find("p").filter(|_| name.starts_with("nvme")) {
        &name[..index]
    } else {
        name.trim_end_matches(|c: char| c.is_ascii_digit())
    };
    if disk.is_empty() {
        None
    } else {
        Some(disk.to_string())
    }
}

fn round2(value: f64) -> f64 {
    (value * 100.0).round() / 100.0
}

#[cfg(test)]
mod tests {
    use super::{is_valid_device, parent_disk, parse_df_full};

    #[test]
    fn interpreta_df_completo() {
        let output = "F 1024-blocks Used Available Capacity Mounted\n/dev/sdb1 1921800000 900000000 1021800000 47% /srv/nas\n";
        let usage = parse_df_full(output).expect("df");
        assert_eq!(usage.total_kb, 1_921_800_000);
        assert_eq!(usage.used_kb, 900_000_000);
        assert_eq!(usage.available_kb, 1_021_800_000);
        assert!((usage.used_percent - 47.0).abs() < f64::EPSILON);
    }

    #[test]
    fn df_invalido_devuelve_none() {
        assert!(parse_df_full("").is_none());
        assert!(parse_df_full("solo cabecera\n").is_none());
    }

    #[test]
    fn valida_dispositivos() {
        assert!(is_valid_device("/dev/sdb"));
        assert!(is_valid_device("/dev/nvme0n1"));
        assert!(!is_valid_device("/dev/../etc"));
        assert!(!is_valid_device("sdb"));
        assert!(!is_valid_device("/dev/"));
        assert!(!is_valid_device("/dev/sdb;rm -rf /"));
    }

    #[test]
    fn deduce_el_disco_padre() {
        assert_eq!(parent_disk("/dev/sdb1").as_deref(), Some("sdb"));
        assert_eq!(parent_disk("/dev/nvme0n1p2").as_deref(), Some("nvme0n1"));
        assert_eq!(parent_disk("/dev/mmcblk0p1").as_deref(), Some("mmcblk0p"));
        assert_eq!(parent_disk(""), None);
    }
}
