//! Inventario de discos, volúmenes, pools Btrfs e inspección de almacenamiento.

use std::path::Path;

use nas_core::{NasError, NasPaths, NasResult};
use nas_exec::{CommandSpec, Runner};
use serde::Serialize;

use crate::parsers::lsblk;

use super::health::{check_block_health, detect_device_rotational};
use super::parsers::{
    btrfs_pools_from_lsblk, mount_source_from_proc_mounts, parse_btrfs_device_stats,
    parse_btrfs_df, parse_df_full, path_is_mounted_readonly, BtrfsPoolInfo,
};
use super::write_cache::inspect_write_cache;
use super::{clean_block_source, is_valid_device, parent_disk};

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
    /// Indica si la ruta analizada vive sobre el disco/partición del sistema operativo (`/`).
    pub data_root_on_os_disk: bool,
    /// Indica si el volumen está montado actualmente en solo lectura (`ro`).
    pub is_readonly: bool,
    /// Dispositivos físicos/particiones miembros del pool (p. ej. `["/dev/sda1", "/dev/sdb1"]`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pool_devices: Vec<String>,
    /// Perfil Btrfs de datos (`single`, `RAID1`, etc.), si aplica.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data_profile: Option<String>,
    /// Perfil Btrfs de metadatos (`RAID1`, `DUP`, etc.), si aplica.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata_profile: Option<String>,
    /// Total de errores de E/S o corrupción acumulados en `btrfs device stats`.
    #[serde(default)]
    pub io_errors_total: u64,
    /// Detalle legible por dispositivo/contador con errores > 0.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub io_errors_detail: Vec<String>,
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
    /// Indica si está montado o pertenece a un pool activo.
    pub in_use: bool,
    /// Número de particiones o volúmenes hijos directos.
    pub children_count: usize,
    /// UUID principal detectado en el disco o su partición.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uuid: Option<String>,
    /// UUID del pool Btrfs al que pertenece, si forma parte de un pool.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pool_uuid: Option<String>,
    /// Dispositivos miembros del pool compartido.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pool_members: Vec<String>,
    /// `true` cuando pertenece a un pool multi-dispositivo (>1 miembro).
    pub is_pool_member: bool,
}

/// Obtiene dispositivos `lsblk` incluyendo `UUID`.
pub fn lsblk_devices_with_uuid(runner: &Runner) -> NasResult<Vec<lsblk::BlockDevice>> {
    let output = runner.run(&CommandSpec::new("lsblk").args([
        "-J",
        "-o",
        "NAME,SIZE,TYPE,MOUNTPOINT,FSTYPE,MODEL,ROTA,UUID",
    ]))?;
    if !output.is_success() {
        return Err(NasError::Command {
            command: output.command,
            code: output.code,
            stderr: output.stderr,
        });
    }
    lsblk::parse(&output.stdout)
        .map_err(|error| NasError::Invalid(format!("lsblk inválido: {error}")))
}

/// Determina si `path` reside actualmente sobre el mismo disco o partición del sistema operativo.
pub fn is_path_on_os_disk(runner: &Runner, path: &Path) -> bool {
    let target_source = findmnt_value(runner, &path.to_string_lossy(), "SOURCE")
        .or_else(|| mount_source_from_proc_mounts(path));
    let Some(tgt_src) = target_source else {
        return findmnt_value(runner, "/", "SOURCE")
            .or_else(|| mount_source_from_proc_mounts(Path::new("/")))
            .is_some();
    };
    let tgt_clean = clean_block_source(&tgt_src);
    if crate::helpers::storage::is_os_disk(runner, tgt_clean).unwrap_or(false) {
        return true;
    }
    let os_source = findmnt_value(runner, "/", "SOURCE")
        .or_else(|| mount_source_from_proc_mounts(Path::new("/")));
    match os_source {
        Some(os_src) => {
            let os_clean = clean_block_source(&os_src);
            if os_clean == tgt_clean {
                return true;
            }
            let os_parent = parent_disk(os_clean);
            let tgt_parent = parent_disk(tgt_clean);
            os_parent.is_some() && os_parent == tgt_parent
        }
        None => false,
    }
}

/// Recolecta errores acumulados por dispositivo en `btrfs device stats`.
pub fn collect_btrfs_device_errors(runner: &Runner, path: &str) -> (u64, Vec<String>) {
    let Ok(out) = runner.run(
        &CommandSpec::new("btrfs")
            .args(["device", "stats", path])
            .timeout(std::time::Duration::from_secs(5)),
    ) else {
        return (0, Vec::new());
    };
    if !out.is_success() {
        return (0, Vec::new());
    }
    parse_btrfs_device_stats(&out.stdout)
}

/// Obtiene el resumen de almacenamiento para una ruta dada.
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
        let clean_dev = clean_block_source(device);
        let target_path = resolve_target_device(clean_dev);
        let probe_dev = target_path.as_deref().unwrap_or(clean_dev);
        match detect_device_rotational(runner, probe_dev) {
            Some(false) => {
                supports_trim = true;
                device_type = "SSD (Flash)".to_string();
            }
            Some(true) => device_type = "HDD (Mecánico)".to_string(),
            None => {}
        }
    }

    let (data_profile, metadata_profile) = if is_btrfs {
        let df_out = runner
            .run(
                &CommandSpec::new("btrfs")
                    .args(["filesystem", "df", path])
                    .timeout(std::time::Duration::from_secs(10)),
            )
            .ok()
            .filter(|o| o.is_success())
            .and_then(|o| parse_btrfs_df(&o.stdout));
        match df_out {
            Some(info) => (info.data_profile, info.metadata_profile),
            None => (None, None),
        }
    } else {
        (None, None)
    };

    let mut pool_devices = Vec::new();
    if let Some(dev) = &device {
        let clean_dev = clean_block_source(dev);
        if is_btrfs {
            if let Ok(lsblk_devs) = lsblk_devices_with_uuid(runner) {
                let pools = btrfs_pools_from_lsblk(&lsblk_devs);
                if let Some(found) = pools.into_iter().find(|p| {
                    p.mountpoint.as_deref() == Some(path)
                        || p.devices.iter().any(|d| d == clean_dev)
                }) {
                    pool_devices = found.devices;
                }
            }
        }
        if pool_devices.is_empty() && clean_dev.starts_with("/dev/") {
            pool_devices.push(clean_dev.to_string());
        }
    }

    let is_readonly = path_is_mounted_readonly(Path::new(path));
    let data_root_on_os_disk = is_path_on_os_disk(runner, Path::new(path));
    let (io_errors_total, io_errors_detail) = if is_btrfs {
        collect_btrfs_device_errors(runner, path)
    } else {
        (0, Vec::new())
    };

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
        data_root_on_os_disk,
        is_readonly,
        pool_devices,
        data_profile,
        metadata_profile,
        io_errors_total,
        io_errors_detail,
    })
}

/// Lista los discos físicos, marcando el del sistema operativo y todos los miembros de pools Btrfs.
pub fn disks(runner: &Runner) -> NasResult<Vec<Disk>> {
    let devices = lsblk_devices_with_uuid(runner)?;
    let mut os_list = crate::helpers::storage::os_disks(runner);
    if os_list.is_empty() {
        if let Some(os_disk) = findmnt_value(runner, "/", "SOURCE")
            .as_deref()
            .and_then(parent_disk)
        {
            os_list.push(os_disk);
        }
    }

    Ok(compute_disks_from_lsblk(&devices, &os_list))
}

pub(super) fn compute_disks_from_lsblk(
    devices: &[lsblk::BlockDevice],
    os_disks: &[String],
) -> Vec<Disk> {
    let btrfs_pools = btrfs_pools_from_lsblk(devices);
    devices
        .iter()
        .filter(|device| device.kind == "disk")
        .map(|device| {
            let flat = device.flatten();
            let direct_mountpoint = flat.iter().find_map(|entry| entry.mountpoint.clone());
            let direct_in_use = flat.iter().any(|entry| entry.mountpoint.is_some());
            let uuid = flat.iter().find_map(|entry| entry.uuid.clone());

            let matches_pool = |pool: &&BtrfsPoolInfo| {
                flat.iter().any(|entry| {
                    entry
                        .fstype
                        .as_deref()
                        .is_some_and(|fs| fs.eq_ignore_ascii_case("btrfs"))
                        && entry.uuid.as_deref() == Some(pool.uuid.as_str())
                })
            };
            let shared_pool = btrfs_pools
                .iter()
                .find(|pool| pool.mountpoint.is_some() && matches_pool(pool))
                .or_else(|| btrfs_pools.iter().find(matches_pool));

            let mountpoint = direct_mountpoint
                .clone()
                .or_else(|| shared_pool.and_then(|p| p.mountpoint.clone()));
            let in_use = direct_in_use || shared_pool.is_some_and(|p| p.mountpoint.is_some());
            let pool_uuid = shared_pool.map(|p| p.uuid.clone());
            let pool_members = shared_pool.map(|p| p.devices.clone()).unwrap_or_default();
            let is_pool_member = pool_members.len() > 1;

            let has_os_mount = flat.iter().any(|entry| {
                matches!(
                    entry.mountpoint.as_deref(),
                    Some("/" | "/boot" | "/boot/efi" | "/var" | "[SWAP]")
                )
            });
            let matches_os_disk = os_disks.iter().any(|name| {
                let clean = name.strip_prefix("/dev/").unwrap_or(name.as_str());
                clean == device.name
                    || name == &device.device_path()
                    || format!("/dev/{clean}") == device.device_path()
            });
            let protected = has_os_mount || matches_os_disk;

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
                children_count: device.children.len(),
                uuid,
                pool_uuid,
                pool_members,
                is_pool_member,
            }
        })
        .collect()
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
        "NAME,SIZE,TYPE,MOUNTPOINT,FSTYPE,MODEL,ROTA,UUID",
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

    let mut signatures: Vec<String> = Vec::new();
    if let Some(fs) = &root.fstype {
        signatures.push(fs.clone());
    }
    for child in &root.children {
        if let Some(fs) = &child.fstype {
            signatures.push(fs.clone());
        }
    }
    if let Ok(wipe_out) = runner.run(&CommandSpec::new("wipefs").args(["-n", device])) {
        if wipe_out.is_success() {
            for line in wipe_out.stdout.lines().skip(1) {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() >= 3
                    && !parts[2].is_empty()
                    && !signatures.iter().any(|s| s == parts[2])
                {
                    signatures.push(parts[2].to_string());
                }
            }
        }
    }
    signatures.sort();
    signatures.dedup();

    let has_data = !root.children.is_empty() || root.fstype.is_some() || !signatures.is_empty();
    let health = check_block_health(runner, device);
    let (write_cache_supported, write_cache_enabled, write_cache_detail) =
        inspect_write_cache(runner, device);

    Ok(serde_json::json!({
        "device": device,
        "name": root.name,
        "type": root.kind,
        "size": root.size,
        "model": root.model,
        "rotational": root.rotational,
        "uuid": root.uuid,
        "in_use": in_use,
        "has_data": has_data,
        "signatures": signatures,
        "smart_healthy": health.healthy,
        "smart_available": health.smart_available,
        "smart_detail": health.detail,
        "write_cache_supported": write_cache_supported,
        "write_cache_enabled": write_cache_enabled,
        "write_cache_detail": write_cache_detail,
        "partitions": root.children.iter().map(|child| serde_json::json!({
            "name": child.name,
            "size": child.size,
            "fstype": child.fstype,
            "mountpoint": child.mountpoint,
            "uuid": child.uuid,
        })).collect::<Vec<_>>(),
    }))
}

/// Resumen del volumen de datos configurado.
pub fn overview_for(paths: &NasPaths, runner: &Runner) -> NasResult<StorageOverview> {
    overview(runner, &paths.data_root.to_string_lossy())
}

pub(super) fn parse_findmnt_last_line(raw: &str) -> Option<String> {
    raw.lines()
        .rev()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(ToString::to_string)
}

pub(super) fn findmnt_value(runner: &Runner, path: &str, column: &str) -> Option<String> {
    runner
        .run(&CommandSpec::new("findmnt").args(["-n", "-o", column, "--target", path]))
        .ok()
        .filter(|output| output.is_success())
        .and_then(|output| parse_findmnt_last_line(&output.stdout))
}

fn round2(value: f64) -> f64 {
    (value * 100.0).round() / 100.0
}

pub(super) fn resolve_target_device(device: &str) -> Option<String> {
    if device.starts_with("/dev/mapper/")
        || device.starts_with("/dev/dm-")
        || device.trim_start_matches("/dev/").contains('/')
    {
        Some(device.to_string())
    } else {
        parent_disk(device).map(|disk| format!("/dev/{disk}"))
    }
}
