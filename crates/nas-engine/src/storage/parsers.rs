//! Parsers de sistemas de archivos, montajes `/proc/mounts`, `/etc/fstab` y Btrfs.

use std::collections::BTreeMap;
use std::path::Path;

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

/// Salida parseada de `df -Pk`.
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

pub(super) fn mount_entry_in_proc_mounts(raw: &str, target: &Path) -> Option<(String, String)> {
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
        if matches && best.as_ref().map_or(true, |(len, _, _)| mp.len() >= *len) {
            best = Some((mp.len(), src.to_string(), opts.to_string()));
        }
    }
    best.map(|(_, src, opts)| (src, opts))
}

pub(super) fn mount_source_from_proc_mounts(target: &Path) -> Option<String> {
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
