//! Inventario de almacenamiento: uso, discos e inspección.

use std::collections::{BTreeMap, HashSet};
use std::path::Path;

use nas_core::{NasError, NasPaths, NasResult};
use nas_exec::{CommandSpec, Runner};
use serde::Serialize;

use crate::parsers::lsblk;

/// Perfil de asignación de datos/metadatos en un volumen Btrfs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BtrfsProfileInfo {
    /// Perfil de bloques de datos (`single`, `RAID1`, `DUP`, etc.).
    pub data_profile: Option<String>,
    /// Perfil de metadatos (`RAID1`, `DUP`, `single`, etc.).
    pub metadata_profile: Option<String>,
    /// Perfil de sistema (`RAID1`, `DUP`, `single`, etc.).
    pub system_profile: Option<String>,
}

/// Pool o sistema de archivos Btrfs (puede agrupar 1 o N dispositivos físicos).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BtrfsPoolInfo {
    /// Etiqueta del filesystem, si tiene.
    pub label: Option<String>,
    /// UUID lógico del filesystem Btrfs (compartido por todos sus miembros).
    pub uuid: String,
    /// Cantidad total de dispositivos declarados en el filesystem.
    pub total_devices: usize,
    /// Rutas `/dev/...` de los miembros detectados.
    pub devices: Vec<String>,
    /// Punto de montaje activo, si está montado.
    pub mountpoint: Option<String>,
    /// Perfil de datos (`single`, `RAID1`, etc.).
    pub data_profile: Option<String>,
    /// Perfil de metadatos (`RAID1`, `DUP`, etc.).
    pub metadata_profile: Option<String>,
}

/// Anomalía detectada al auditar `/etc/fstab`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FstabIssue {
    /// Número de línea (1-based) dentro de `/etc/fstab`.
    pub line_number: usize,
    /// Severidad (`error` | `warning`).
    pub severity: String,
    /// Código máquina (`missing_device`, `missing_nofail`, `unsafe_mount_option`).
    pub code: String,
    /// Origen declarado en la primera columna.
    pub source: String,
    /// Punto de montaje declarado en la segunda columna.
    pub target: String,
    /// Descripción legible en español.
    pub detail: String,
    /// Línea original completa.
    pub line: String,
}

/// Resultado de auditoría de `/etc/fstab`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FstabAudit {
    /// `true` cuando no se detectaron anomalías.
    pub healthy: bool,
    /// Cantidad de anomalías corregibles automáticamente con `sanitize_fstab`.
    pub fixable_count: usize,
    /// Lista de anomalías encontradas.
    pub issues: Vec<FstabIssue>,
}

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

/// Interpreta `btrfs filesystem df <mountpoint>` para obtener perfiles de datos, metadatos y sistema.
pub fn parse_btrfs_df(output: &str) -> Option<BtrfsProfileInfo> {
    let mut data_profile = None;
    let mut metadata_profile = None;
    let mut system_profile = None;
    for line in output.lines() {
        let trimmed = line.trim();
        let Some((head, _)) = trimmed.split_once(':') else {
            continue;
        };
        let mut parts = head.split(',').map(str::trim);
        let kind = parts.next().unwrap_or_default();
        let profile = parts.next().filter(|s| !s.is_empty()).map(str::to_string);
        match kind.to_ascii_lowercase().as_str() {
            "data" => data_profile = profile,
            "metadata" => metadata_profile = profile,
            "system" => system_profile = profile,
            _ => {}
        }
    }
    if data_profile.is_some() || metadata_profile.is_some() || system_profile.is_some() {
        Some(BtrfsProfileInfo {
            data_profile,
            metadata_profile,
            system_profile,
        })
    } else {
        None
    }
}

/// Interpreta `btrfs filesystem show` para agrupar dispositivos bajo su UUID lógico de filesystem.
pub fn parse_btrfs_filesystem_show(output: &str) -> Vec<BtrfsPoolInfo> {
    let mut pools = Vec::new();
    let mut current: Option<BtrfsPoolInfo> = None;

    for line in output.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("Label:") {
            if let Some(pool) = current.take() {
                pools.push(pool);
            }
            if let Some((label_part, uuid_part)) = rest.split_once("uuid:") {
                let raw_label = label_part.trim().trim_matches('\'');
                let label = if raw_label == "none" || raw_label.is_empty() {
                    None
                } else {
                    Some(raw_label.to_string())
                };
                let uuid = uuid_part.trim().to_string();
                if !uuid.is_empty() {
                    current = Some(BtrfsPoolInfo {
                        label,
                        uuid,
                        total_devices: 0,
                        devices: Vec::new(),
                        mountpoint: None,
                        data_profile: None,
                        metadata_profile: None,
                    });
                }
            }
        } else if let Some(pool) = current.as_mut() {
            if let Some(rest) = trimmed.strip_prefix("Total devices ") {
                if let Some(num_str) = rest.split_whitespace().next() {
                    pool.total_devices = num_str.parse().unwrap_or(0);
                }
            } else if trimmed.starts_with("devid ") {
                if let Some((_, dev_path)) = trimmed.rsplit_once(" path ") {
                    let dev = dev_path.trim().to_string();
                    if !dev.is_empty() && !pool.devices.contains(&dev) {
                        pool.devices.push(dev);
                    }
                }
            }
        }
    }
    if let Some(pool) = current.take() {
        pools.push(pool);
    }
    for pool in &mut pools {
        if pool.total_devices == 0 {
            pool.total_devices = pool.devices.len();
        }
    }
    pools
}

/// Agrupa todos los nodos Btrfs detectados en `lsblk` por su UUID lógico de filesystem.
pub fn btrfs_pools_from_lsblk(devices: &[lsblk::BlockDevice]) -> Vec<BtrfsPoolInfo> {
    let mut by_uuid: BTreeMap<String, (Vec<String>, Option<String>)> = BTreeMap::new();
    for dev in devices {
        for node in dev.flatten() {
            if node
                .fstype
                .as_deref()
                .is_some_and(|fs| fs.eq_ignore_ascii_case("btrfs"))
            {
                if let Some(uuid) = node
                    .uuid
                    .as_deref()
                    .map(str::trim)
                    .filter(|u| !u.is_empty())
                {
                    let entry = by_uuid
                        .entry(uuid.to_string())
                        .or_insert_with(|| (Vec::new(), None));
                    let path = node.device_path();
                    if !entry.0.contains(&path) {
                        entry.0.push(path);
                    }
                    if entry.1.is_none() {
                        if let Some(mp) = node.mountpoint.as_ref() {
                            entry.1 = Some(mp.clone());
                        }
                    }
                }
            }
        }
    }
    by_uuid
        .into_iter()
        .map(|(uuid, (devs, mountpoint))| BtrfsPoolInfo {
            label: None,
            uuid,
            total_devices: devs.len(),
            devices: devs,
            mountpoint,
            data_profile: None,
            metadata_profile: None,
        })
        .collect()
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

/// Determina si `path` reside actualmente sobre el mismo disco o partición del sistema operativo (`/`).
pub fn is_path_on_os_disk(runner: &Runner, path: &Path) -> bool {
    let os_source = findmnt_value(runner, "/", "SOURCE")
        .or_else(|| mount_source_from_proc_mounts(Path::new("/")));
    let target_source = findmnt_value(runner, &path.to_string_lossy(), "SOURCE")
        .or_else(|| mount_source_from_proc_mounts(path));
    match (os_source, target_source) {
        (Some(os_src), Some(tgt_src)) => {
            let os_clean = clean_block_source(&os_src);
            let tgt_clean = clean_block_source(&tgt_src);
            if os_clean == tgt_clean {
                return true;
            }
            let os_parent = parent_disk(os_clean);
            let tgt_parent = parent_disk(tgt_clean);
            os_parent.is_some() && os_parent == tgt_parent
        }
        (Some(_), None) => true,
        _ => false,
    }
}

/// Comprueba en `/proc/mounts` si `target` es un punto de montaje exacto activo.
pub fn is_mountpoint_in_proc_mounts(raw: &str, target: &Path) -> bool {
    let target_str = target.to_string_lossy();
    let target_norm = if target_str == "/" {
        "/"
    } else {
        target_str.trim_end_matches('/')
    };
    raw.lines().any(|line| {
        let mut parts = line.split_whitespace();
        let _src = parts.next();
        let Some(mp) = parts.next() else {
            return false;
        };
        let mp_norm = if mp == "/" {
            "/"
        } else {
            mp.trim_end_matches('/')
        };
        mp_norm == target_norm
    })
}

/// Comprueba si `target` está montado actualmente leyendo `/proc/mounts` (inmune a `chmod 2770`).
pub fn is_mountpoint(target: &Path) -> bool {
    std::fs::read_to_string("/proc/mounts")
        .map(|raw| is_mountpoint_in_proc_mounts(&raw, target))
        .unwrap_or(false)
}

/// Devuelve los puntos de montaje hijos estrictos bajo `parent` en orden LIFO (más profundos primero).
pub fn proc_mounts_children_under(raw: &str, parent: &Path) -> Vec<String> {
    let parent_norm = parent.to_string_lossy().trim_end_matches('/').to_string();
    let prefix = format!("{parent_norm}/");
    let mut children: Vec<String> = raw
        .lines()
        .filter_map(|line| {
            let mp = line.split_whitespace().nth(1)?;
            let mp_norm = mp.trim_end_matches('/');
            if mp_norm.starts_with(&prefix) {
                Some(mp_norm.to_string())
            } else {
                None
            }
        })
        .collect();
    children.sort_by(|a, b| b.len().cmp(&a.len()).then_with(|| b.cmp(a)));
    children.dedup();
    children
}

/// Audita el contenido de `/etc/fstab` en busca de entradas huérfanas, falta de `nofail` u opciones dañinas.
pub fn audit_fstab_content(content: &str, existing_check: &dyn Fn(&str) -> bool) -> FstabAudit {
    let mut issues = Vec::new();
    for (idx, raw_line) in content.lines().enumerate() {
        let line_number = idx + 1;
        let trimmed = raw_line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let parts: Vec<&str> = trimmed.split_whitespace().collect();
        if parts.len() < 4 {
            continue;
        }
        let source = parts[0];
        let target = parts[1];
        let opts = parts[3];

        let is_nas_target = target == "/srv/nas" || target.starts_with("/srv/nas/");
        if source.starts_with("/dev/") && !existing_check(source) {
            issues.push(FstabIssue {
                line_number,
                severity: "error".into(),
                code: "missing_device".into(),
                source: source.into(),
                target: target.into(),
                detail: format!(
                    "Entrada huérfana en fstab: el dispositivo {source} ({target}) ya no existe."
                ),
                line: raw_line.to_string(),
            });
        } else if source.starts_with("UUID=") && is_nas_target && !existing_check(source) {
            issues.push(FstabIssue {
                line_number,
                severity: "warning".into(),
                code: "missing_uuid".into(),
                source: source.into(),
                target: target.into(),
                detail: format!(
                    "El UUID declarado para {target} ({source}) no se encuentra actualmente conectado."
                ),
                line: raw_line.to_string(),
            });
        }

        if is_nas_target {
            let opt_list: Vec<&str> = opts.split(',').map(str::trim).collect();
            if !opt_list.contains(&"nofail")
                || !opt_list
                    .iter()
                    .any(|o| o.starts_with("x-systemd.device-timeout="))
            {
                issues.push(FstabIssue {
                    line_number,
                    severity: "warning".into(),
                    code: "missing_nofail".into(),
                    source: source.into(),
                    target: target.into(),
                    detail: format!(
                        "{target} carece de 'nofail,x-systemd.device-timeout=15s' (riesgo de caída a modo emergencia en arranque)."
                    ),
                    line: raw_line.to_string(),
                });
            }
            for bad in ["autodefrag", "discard=async", "nobarrier", "data=writeback"] {
                if opt_list.contains(&bad) {
                    issues.push(FstabIssue {
                        line_number,
                        severity: "warning".into(),
                        code: "unsafe_mount_option".into(),
                        source: source.into(),
                        target: target.into(),
                        detail: format!(
                            "{target} usa opción no recomendada para discos de consumo: '{bad}'."
                        ),
                        line: raw_line.to_string(),
                    });
                }
            }
        }
    }
    let fixable_count = issues
        .iter()
        .filter(|issue| issue.code != "missing_uuid")
        .count();
    FstabAudit {
        healthy: issues.is_empty(),
        fixable_count,
        issues,
    }
}

/// Audita `/etc/fstab` real del host.
pub fn audit_system_fstab(runner: &Runner) -> FstabAudit {
    let Ok(content) = std::fs::read_to_string("/etc/fstab") else {
        return FstabAudit {
            healthy: true,
            fixable_count: 0,
            issues: Vec::new(),
        };
    };
    audit_fstab_content(&content, &|src| {
        if let Some(uuid) = src.strip_prefix("UUID=") {
            runner
                .run(
                    &CommandSpec::new("blkid")
                        .args(["-U", uuid])
                        .timeout(std::time::Duration::from_secs(5)),
                )
                .map(|o| o.is_success())
                .unwrap_or(false)
                || Path::new(&format!("/dev/disk/by-uuid/{uuid}")).exists()
        } else {
            Path::new(src).exists()
        }
    })
}

/// Parsea `btrfs device stats <ruta>` devolviendo `(suma_errores, líneas_con_errores)`.
pub fn parse_btrfs_device_stats(raw: &str) -> (u64, Vec<String>) {
    let mut total = 0u64;
    let mut details = Vec::new();
    for line in raw.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let mut parts = trimmed.split_whitespace();
        let Some(metric) = parts.next() else {
            continue;
        };
        let Some(count) = parts.next().and_then(|v| v.parse::<u64>().ok()) else {
            continue;
        };
        if count > 0 {
            total = total.saturating_add(count);
            details.push(format!("{metric}: {count}"));
        }
    }
    (total, details)
}

/// Consulta `btrfs device stats` en lectura sin bloquear ni despertar discos inactivos.
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

/// Parsea `hdparm -W <dev>` devolviendo `Some(true)` si está activa, `Some(false)` si está desactivada.
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

/// Inspecciona el estado de la caché volátil de escritura de un disco físico vía `hdparm -W`.
pub fn inspect_write_cache(runner: &Runner, device: &str) -> (bool, Option<bool>, String) {
    let disk_path = resolve_target_device(clean_block_source(device))
        .unwrap_or_else(|| device.trim().to_string());
    let Ok(out) = runner.run(
        &CommandSpec::new("hdparm")
            .args(["-W", &disk_path])
            .timeout(std::time::Duration::from_secs(5)),
    ) else {
        return (
            false,
            None,
            "hdparm no disponible; ejecuta 'nas deploy ensure-deps' para instalarlo".to_string(),
        );
    };
    if !out.is_success() {
        let err = out.stderr.trim();
        let msg = if err.is_empty() {
            "Caché volátil no consultable en este controlador".to_string()
        } else {
            format!("Sin acceso a caché vía hdparm ({err})")
        };
        return (false, None, msg);
    }
    match parse_hdparm_write_cache(&out.stdout) {
        Some(true) => (
            true,
            Some(true),
            "Caché volátil ACTIVA (rendimiento estándar; riesgo en corte seco sin PLP)".to_string(),
        ),
        Some(false) => (
            true,
            Some(false),
            "Caché volátil DESACTIVADA (máxima protección ante cortes eléctricos)".to_string(),
        ),
        None => (
            false,
            None,
            "El controlador/disco no reporta control write-cache por hdparm".to_string(),
        ),
    }
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
    let os_source = findmnt_value(runner, "/", "SOURCE");
    let os_disk = os_source.as_deref().and_then(parent_disk);

    Ok(compute_disks_from_lsblk(&devices, os_disk.as_deref()))
}

fn compute_disks_from_lsblk(devices: &[lsblk::BlockDevice], os_disk: Option<&str>) -> Vec<Disk> {
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
            let matches_os_disk = os_disk.is_some_and(|name| {
                name == device.name || format!("/dev/{name}") == device.device_path()
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
    let name = clean_block_source(device).rsplit('/').next()?.trim();
    if name.is_empty() {
        return None;
    }
    if let Some((disk, part)) = name.rsplit_once('p') {
        if !part.is_empty()
            && part.chars().all(|c| c.is_ascii_digit())
            && disk.ends_with(|c: char| c.is_ascii_digit())
        {
            return Some(disk.to_string());
        }
    }
    if name.starts_with("nvme")
        || name.starts_with("mmcblk")
        || name.starts_with("loop")
        || name.starts_with("nbd")
        || name.starts_with("md")
        || name.starts_with("ram")
        || name.starts_with("pmem")
    {
        return Some(name.to_string());
    }
    let disk = name.trim_end_matches(|c: char| c.is_ascii_digit());
    if disk.is_empty() {
        None
    } else {
        Some(disk.to_string())
    }
}

fn round2(value: f64) -> f64 {
    (value * 100.0).round() / 100.0
}

fn resolve_target_device(device: &str) -> Option<String> {
    if device.starts_with("/dev/mapper/")
        || device.starts_with("/dev/dm-")
        || device.trim_start_matches("/dev/").contains('/')
    {
        Some(device.to_string())
    } else {
        parent_disk(device).map(|disk| format!("/dev/{disk}"))
    }
}

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

fn clean_block_source(device: &str) -> &str {
    let trimmed = device.trim();
    trimmed.split('[').next().unwrap_or(trimmed).trim()
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

fn mount_entry_in_proc_mounts(raw: &str, target: &Path) -> Option<(String, String)> {
    let target_str = target.to_string_lossy();
    let mut best: Option<(usize, String, String)> = None;
    for line in raw.lines() {
        let mut parts = line.split_whitespace();
        let Some(src) = parts.next() else {
            continue;
        };
        let Some(mp) = parts.next() else {
            continue;
        };
        let _fstype = parts.next();
        let opts = parts.next().unwrap_or("");
        if !src.starts_with("/dev/") {
            continue;
        }
        let matches = mp == "/"
            || target_str == mp
            || target_str
                .strip_prefix(mp)
                .is_some_and(|rest| rest.starts_with('/'));
        if matches && best.as_ref().map_or(true, |(len, _, _)| mp.len() > *len) {
            best = Some((mp.len(), src.to_string(), opts.to_string()));
        }
    }
    best.map(|(_, src, opts)| (src, opts))
}

fn mount_source_from_proc_mounts(target: &Path) -> Option<String> {
    let raw = std::fs::read_to_string("/proc/mounts").ok()?;
    mount_entry_in_proc_mounts(&raw, target).map(|(src, _)| src)
}

/// Indica si el sistema de archivos que aloja `path` está montado actualmente en solo lectura (`ro`).
pub fn path_is_mounted_readonly(path: &Path) -> bool {
    let Ok(raw) = std::fs::read_to_string("/proc/mounts") else {
        return false;
    };
    mount_entry_in_proc_mounts(&raw, path)
        .is_some_and(|(_, opts)| opts.split(',').any(|flag| flag == "ro"))
}

/// Resultado de comprobación preventiva S.M.A.R.T. y de montaje.
#[derive(Debug, Clone, PartialEq, Eq)]
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

/// Evalúa la salud S.M.A.R.T. de un dispositivo de bloque (degrada sin error si `smartctl` no existe).
pub fn check_block_health(runner: &Runner, device: &str) -> DiskHealthCheck {
    let cleaned = clean_block_source(device);
    let target_dev = resolve_target_device(cleaned).unwrap_or_else(|| cleaned.to_string());
    let probe = if target_dev.starts_with("/dev/mapper/") || target_dev.starts_with("/dev/dm-") {
        target_dev.clone()
    } else if let Some(parent) = parent_disk(&target_dev) {
        format!("/dev/{parent}")
    } else {
        target_dev.clone()
    };

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

#[cfg(test)]
mod tests {
    use super::{is_valid_device, parent_disk, parse_df_full, resolve_target_device};

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
        assert_eq!(parent_disk("/dev/sdb").as_deref(), Some("sdb"));
        assert_eq!(parent_disk("/dev/nvme0n1p2").as_deref(), Some("nvme0n1"));
        assert_eq!(parent_disk("/dev/nvme0n1").as_deref(), Some("nvme0n1"));
        assert_eq!(parent_disk("/dev/mmcblk0p1").as_deref(), Some("mmcblk0"));
        assert_eq!(parent_disk("/dev/mmcblk0").as_deref(), Some("mmcblk0"));
        assert_eq!(parent_disk(""), None);
    }

    #[test]
    fn resuelve_dispositivo_destino() {
        assert_eq!(
            resolve_target_device("/dev/mapper/nas_vg-nas_lv").as_deref(),
            Some("/dev/mapper/nas_vg-nas_lv")
        );
        assert_eq!(
            resolve_target_device("/dev/dm-0").as_deref(),
            Some("/dev/dm-0")
        );
        assert_eq!(
            resolve_target_device("/dev/nas_vg/nas_lv").as_deref(),
            Some("/dev/nas_vg/nas_lv")
        );
        assert_eq!(
            resolve_target_device("/dev/sdb1").as_deref(),
            Some("/dev/sdb")
        );
        assert_eq!(
            resolve_target_device("/dev/nvme0n1p2").as_deref(),
            Some("/dev/nvme0n1")
        );
    }

    #[test]
    fn marca_protegido_el_disco_del_sistema_y_cuenta_hijos() {
        let sample = r#"{
            "blockdevices": [
                {
                    "name": "nvme0n1",
                    "size": "476.9G",
                    "type": "disk",
                    "mountpoint": null,
                    "fstype": null,
                    "model": "NVMe OS",
                    "rota": false,
                    "children": [
                        {"name": "nvme0n1p1", "size": "512M", "type": "part", "mountpoint": "/boot/efi", "fstype": "vfat", "model": null, "rota": false},
                        {"name": "nvme0n1p2", "size": "476.4G", "type": "part", "mountpoint": "/", "fstype": "ext4", "model": null, "rota": false}
                    ]
                },
                {
                    "name": "sdb",
                    "size": "1.8T",
                    "type": "disk",
                    "mountpoint": null,
                    "fstype": null,
                    "model": "NAS HDD",
                    "rota": true,
                    "children": []
                }
            ]
        }"#;
        let parsed = crate::parsers::lsblk::parse(sample).expect("lsblk");
        let list = super::compute_disks_from_lsblk(&parsed, Some("nvme0n1"));
        assert_eq!(list.len(), 2);
        assert!(list[0].protected);
        assert_eq!(list[0].children_count, 2);
        assert!(!list[1].protected);
        assert_eq!(list[1].children_count, 0);
    }

    #[test]
    fn detecta_rotacional_en_lvm_md_particion_y_pool_mixto() {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let sys = std::env::temp_dir().join(format!("nas-sysfs-{}-{nanos}", std::process::id()));

        // Discos base: nvme0n1 (SSD=0), sda (HDD=1), sdb (SSD=0), vda (VM=0)
        for (disk, val) in [("nvme0n1", "0"), ("sda", "1"), ("sdb", "0"), ("vda", "0")] {
            let q = sys.join(format!("block/{disk}/queue"));
            std::fs::create_dir_all(&q).unwrap();
            std::fs::write(q.join("rotational"), format!("{val}\n")).unwrap();
        }

        // LVM dm-0 sobre nvme0n1p1 -> SSD (0)
        std::fs::create_dir_all(sys.join("block/dm-0/slaves/nvme0n1p1")).unwrap();
        std::fs::create_dir_all(sys.join("block/dm-0/queue")).unwrap();
        std::fs::write(sys.join("block/dm-0/queue/rotational"), "1\n").unwrap();
        assert_eq!(
            super::detect_rotational_in_sysfs(&sys, "/dev/dm-0"),
            Some(false)
        );

        // MD RAID md0 mixto (sdb1=0 + sda1=1) -> conservador true
        std::fs::create_dir_all(sys.join("block/md0/slaves/sdb1")).unwrap();
        std::fs::create_dir_all(sys.join("block/md0/slaves/sda1")).unwrap();
        assert_eq!(
            super::detect_rotational_in_sysfs(&sys, "/dev/md0"),
            Some(true)
        );

        // MD RAID md1 puro SSD (sdb1=0 + nvme0n1p1=0) -> false
        std::fs::create_dir_all(sys.join("block/md1/slaves/sdb1")).unwrap();
        std::fs::create_dir_all(sys.join("block/md1/slaves/nvme0n1p1")).unwrap();
        assert_eq!(
            super::detect_rotational_in_sysfs(&sys, "/dev/md1"),
            Some(false)
        );

        // VM virtio vda1 -> false
        assert_eq!(
            super::detect_rotational_in_sysfs(&sys, "/dev/vda1"),
            Some(false)
        );

        // Fuente de montaje Btrfs con subvolumen `[/@subvol]` -> false
        assert_eq!(
            super::detect_rotational_in_sysfs(&sys, "/dev/nvme0n1p1[/@subvol]"),
            Some(false)
        );
        assert_eq!(
            super::detect_rotational_in_sysfs(&sys, "/dev/sda1[/@]"),
            Some(true)
        );

        let _ = std::fs::remove_dir_all(&sys);
    }

    #[test]
    fn interpreta_estado_smartctl_y_montaje_ro() {
        assert_eq!(
            super::parse_smartctl_health(
                "SMART overall-health self-assessment test result: PASSED\n"
            ),
            Some(true)
        );
        assert_eq!(
            super::parse_smartctl_health(
                "SMART overall-health self-assessment test result: FAILED!\n"
            ),
            Some(false)
        );
        assert_eq!(
            super::parse_smartctl_health("SMART Health Status: OK\n"),
            Some(true)
        );
        assert_eq!(super::parse_smartctl_health("Device lacks SMART"), None);

        let mounts = "/dev/nvme0n1p2 / ext4 ro,relatime 0 0\n\
                      /dev/sda1 /srv/nas btrfs rw,noatime,compress=zstd:3 0 0\n";
        let root_entry = super::mount_entry_in_proc_mounts(mounts, std::path::Path::new("/etc"));
        assert!(root_entry.is_some_and(|(_, opts)| opts.split(',').any(|f| f == "ro")));
        let data_entry =
            super::mount_entry_in_proc_mounts(mounts, std::path::Path::new("/srv/nas/SISTEMAS"));
        assert!(data_entry.is_some_and(|(_, opts)| opts.split(',').any(|f| f == "rw")));
    }

    #[test]
    fn agrupa_pool_btrfs_multidisco_y_marca_todos_los_miembros_en_uso() {
        let sample = r#"{
            "blockdevices": [
                {
                    "name": "nvme0n1",
                    "size": "238.5G",
                    "type": "disk",
                    "mountpoint": null,
                    "fstype": null,
                    "model": "OS NVMe",
                    "rota": false,
                    "uuid": null,
                    "children": [
                        {"name": "nvme0n1p2", "size": "230G", "type": "part", "mountpoint": "/", "fstype": "ext4", "model": null, "rota": false, "uuid": "root-uuid"}
                    ]
                },
                {
                    "name": "sda",
                    "size": "1.8T",
                    "type": "disk",
                    "mountpoint": null,
                    "fstype": null,
                    "model": "HDD A",
                    "rota": true,
                    "uuid": null,
                    "children": [
                        {"name": "sda1", "size": "1.8T", "type": "part", "mountpoint": "/srv/nas", "fstype": "btrfs", "model": null, "rota": true, "uuid": "pool-uuid-123"}
                    ]
                },
                {
                    "name": "sdb",
                    "size": "1.8T",
                    "type": "disk",
                    "mountpoint": null,
                    "fstype": null,
                    "model": "HDD B",
                    "rota": true,
                    "uuid": null,
                    "children": [
                        {"name": "sdb1", "size": "1.8T", "type": "part", "mountpoint": null, "fstype": "btrfs", "model": null, "rota": true, "uuid": "pool-uuid-123"}
                    ]
                },
                {
                    "name": "sdc",
                    "size": "3.6T",
                    "type": "disk",
                    "mountpoint": null,
                    "fstype": null,
                    "model": "HDD Libre",
                    "rota": true,
                    "uuid": null,
                    "children": []
                }
            ]
        }"#;
        let parsed = crate::parsers::lsblk::parse(sample).expect("lsblk");
        let list = super::compute_disks_from_lsblk(&parsed, Some("nvme0n1"));
        assert_eq!(list.len(), 4);
        assert!(list[0].protected);
        // sda y sdb forman el mismo pool Btrfs montado en /srv/nas
        assert!(list[1].in_use);
        assert!(list[1].is_pool_member);
        assert_eq!(list[1].mountpoint.as_deref(), Some("/srv/nas"));
        assert!(
            list[2].in_use,
            "sdb debe marcarse en uso por compartir UUID Btrfs"
        );
        assert!(list[2].is_pool_member);
        assert_eq!(list[2].mountpoint.as_deref(), Some("/srv/nas"));
        assert_eq!(list[2].pool_uuid.as_deref(), Some("pool-uuid-123"));
        assert_eq!(list[2].pool_members, vec!["/dev/sda1", "/dev/sdb1"]);
        // sdc está libre
        assert!(!list[3].in_use);
        assert!(!list[3].is_pool_member);
    }

    #[test]
    fn parsea_btrfs_show_y_btrfs_df() {
        let show = "\
Label: 'NAS_DATA'  uuid: 07168427-c131-4b01-822d-8ebdcf220d4e
\tTotal devices 2 FS bytes used 123.17GiB
\tdevid    1 size 232.88GiB used 124.00GiB path /dev/sda1
\tdevid    2 size 465.76GiB used 124.00GiB path /dev/sdb1
";
        let pools = super::parse_btrfs_filesystem_show(show);
        assert_eq!(pools.len(), 1);
        assert_eq!(pools[0].label.as_deref(), Some("NAS_DATA"));
        assert_eq!(pools[0].uuid, "07168427-c131-4b01-822d-8ebdcf220d4e");
        assert_eq!(pools[0].total_devices, 2);
        assert_eq!(pools[0].devices, vec!["/dev/sda1", "/dev/sdb1"]);

        let df = "\
Data, single: total=215.73GiB, used=123.17GiB
System, RAID1: total=32.00MiB, used=48.00KiB
Metadata, RAID1: total=2.00GiB, used=1.04GiB
GlobalReserve, single: total=292.42MiB, used=0.00B
";
        let prof = super::parse_btrfs_df(df).expect("btrfs df");
        assert_eq!(prof.data_profile.as_deref(), Some("single"));
        assert_eq!(prof.metadata_profile.as_deref(), Some("RAID1"));
        assert_eq!(prof.system_profile.as_deref(), Some("RAID1"));
    }

    #[test]
    fn audita_fstab_detectando_huerfanos_falta_nofail_y_opciones_peligrosas() {
        let fstab = "\
# /etc/fstab
UUID=root-123 / ext4 errors=remount-ro 0 1
/dev/mapper/old_vg-old_lv /srv/nas ext4 defaults 0 2
UUID=btrfs-456 /srv/nas/unidades/pool2 btrfs rw,noatime,autodefrag,discard=async 0 0
UUID=btrfs-789 /srv/nas/unidades/sano btrfs rw,noatime,compress=zstd:3,nofail,x-systemd.device-timeout=15s 0 0
";
        let audit = super::audit_fstab_content(fstab, &|src| src != "/dev/mapper/old_vg-old_lv");
        assert!(!audit.healthy);
        assert!(audit
            .issues
            .iter()
            .any(|i| i.code == "missing_device" && i.source == "/dev/mapper/old_vg-old_lv"));
        assert!(audit
            .issues
            .iter()
            .any(|i| i.code == "missing_nofail" && i.target == "/srv/nas"));
        assert!(audit
            .issues
            .iter()
            .any(|i| i.code == "missing_nofail" && i.target == "/srv/nas/unidades/pool2"));
        assert!(audit
            .issues
            .iter()
            .any(|i| i.code == "unsafe_mount_option" && i.detail.contains("autodefrag")));
        assert!(audit
            .issues
            .iter()
            .any(|i| i.code == "unsafe_mount_option" && i.detail.contains("discard=async")));
        assert!(!audit
            .issues
            .iter()
            .any(|i| i.target == "/srv/nas/unidades/sano"));
    }

    #[test]
    fn detecta_montajes_exactos_e_hijos_lifo_en_proc_mounts() {
        let mounts = "\
/dev/sda1 /srv/nas btrfs rw,noatime 0 0
/dev/sdb1 /srv/nas/unidades/extra btrfs rw,noatime 0 0
/dev/sdc1 /srv/nas/unidades/extra/sub ext4 rw 0 0
";
        assert!(super::is_mountpoint_in_proc_mounts(
            mounts,
            std::path::Path::new("/srv/nas")
        ));
        assert!(super::is_mountpoint_in_proc_mounts(
            mounts,
            std::path::Path::new("/srv/nas/unidades/extra/")
        ));
        assert!(!super::is_mountpoint_in_proc_mounts(
            mounts,
            std::path::Path::new("/srv/nas/unidades/otro")
        ));
        let children = super::proc_mounts_children_under(mounts, std::path::Path::new("/srv/nas"));
        assert_eq!(
            children,
            vec!["/srv/nas/unidades/extra/sub", "/srv/nas/unidades/extra"]
        );
    }

    #[test]
    fn deduce_disco_padre_en_loop_nbd_y_subvolumen_btrfs() {
        assert_eq!(parent_disk("/dev/loop0p1").as_deref(), Some("loop0"));
        assert_eq!(parent_disk("/dev/loop0").as_deref(), Some("loop0"));
        assert_eq!(parent_disk("/dev/nbd0p2").as_deref(), Some("nbd0"));
        assert_eq!(parent_disk("/dev/sda1[/@subvol]").as_deref(), Some("sda"));
        assert_eq!(
            resolve_target_device(super::clean_block_source("/dev/sda1[/@]")).as_deref(),
            Some("/dev/sda")
        );
    }

    #[test]
    fn fstab_audit_cuenta_problemas_auto_corregibles() {
        let fstab = "UUID=missing-123 /srv/nas/unidades/ext btrfs rw,noatime,nofail,x-systemd.device-timeout=15s 0 0\n";
        let audit = super::audit_fstab_content(fstab, &|_| false);
        assert!(!audit.healthy);
        assert_eq!(audit.issues.len(), 1);
        assert_eq!(audit.issues[0].code, "missing_uuid");
        assert_eq!(audit.fixable_count, 0);
    }

    #[test]
    fn parsea_btrfs_device_stats_y_hdparm_write_cache() {
        let stats = "\
[/dev/sdc1].write_io_errs    0
[/dev/sdc1].read_io_errs     2
[/dev/sdc1].flush_io_errs    0
[/dev/sdc1].corruption_errs  1
[/dev/sdc1].generation_errs  0
";
        let (total, detail) = super::parse_btrfs_device_stats(stats);
        assert_eq!(total, 3);
        assert_eq!(detail.len(), 2);
        assert!(detail[0].contains("read_io_errs: 2"));

        assert_eq!(
            super::parse_hdparm_write_cache("/dev/sdb:\n write-caching =  1 (on)\n"),
            Some(true)
        );
        assert_eq!(
            super::parse_hdparm_write_cache("/dev/sdb:\n write-caching =  0 (off)\n"),
            Some(false)
        );
        assert_eq!(
            super::parse_hdparm_write_cache("/dev/sdb:\n write-caching = not supported\n"),
            None
        );

        let rule = super::render_write_cache_udev_rule(Some("ata-ST2000_ABC"), "sdb", false);
        assert!(rule.contains("ata-ST2000_ABC"));
        assert!(rule.contains("/usr/sbin/hdparm -W0 /dev/%k"));
    }
}
