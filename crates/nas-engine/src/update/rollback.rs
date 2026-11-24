//! Inventario de releases instaladas y planificación de regresión (downgrade A/B).

use std::cmp::Ordering;
use std::path::Path;

use nas_core::{NasError, NasResult};
use serde::Serialize;

use super::manifest::is_valid_version;
use super::plan::{healthcheck_step, service_reload_steps, UpdatePlan};

/// Release instalada localmente bajo `<root>/releases/<version>`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct InstalledRelease {
    /// Versión semántica sin prefijo `v`.
    pub version: String,
    /// Commit asociado en `version.json`, si existe.
    pub commit: Option<String>,
    /// Fecha de publicación/compilación en `version.json`, si existe.
    pub date: Option<String>,
    /// Notas de la release en `version.json`, si existen.
    pub notes: Option<String>,
    /// `true` si `<root>/current` apunta actualmente a esta release.
    pub current: bool,
    /// Ruta absoluta al directorio de la release.
    pub path: String,
    /// `true` si la release conserva al menos los binarios principales (`nas` y `nas-web`).
    pub has_binaries: bool,
}

/// Extrae componentes numéricos de una versión SemVer (`v1.2.3` -> `[1, 2, 3]`).
pub fn parse_semver_key(version: &str) -> Vec<u64> {
    version
        .trim()
        .trim_start_matches('v')
        .split(&['.', '-', '+'][..])
        .map_while(|part| part.parse::<u64>().ok())
        .collect()
}

fn compare_versions_desc(a: &str, b: &str) -> Ordering {
    let ka = parse_semver_key(a);
    let kb = parse_semver_key(b);
    let max_len = ka.len().max(kb.len());
    for i in 0..max_len {
        let av = *ka.get(i).unwrap_or(&0);
        let bv = *kb.get(i).unwrap_or(&0);
        match bv.cmp(&av) {
            Ordering::Equal => continue,
            non_eq => return non_eq,
        }
    }
    b.cmp(a)
}

fn current_release_name(root: &Path) -> Option<String> {
    let target = std::fs::read_link(root.join("current")).ok()?;
    let name = target.file_name()?.to_str()?.trim();
    if is_valid_version(name) {
        Some(name.to_string())
    } else {
        None
    }
}

/// Lista las releases disponibles bajo `<root>/releases/`, ordenadas de más reciente a más antigua.
pub fn list_installed_releases(root: &str) -> Vec<InstalledRelease> {
    let root_path = Path::new(root.trim_end_matches('/'));
    let releases_dir = root_path.join("releases");
    let Ok(entries) = std::fs::read_dir(&releases_dir) else {
        return Vec::new();
    };
    let active_name = current_release_name(root_path);

    let mut releases = Vec::new();
    for entry in entries.flatten() {
        let dir = entry.path();
        if !dir.is_dir() {
            continue;
        }
        let Some(name) = entry.file_name().to_str().map(|s| s.trim().to_string()) else {
            continue;
        };
        if !is_valid_version(&name) {
            continue;
        }
        let meta = std::fs::read_to_string(dir.join("version.json"))
            .ok()
            .and_then(|raw| serde_json::from_str::<serde_json::Value>(&raw).ok());
        let commit = meta
            .as_ref()
            .and_then(|v| v.get("commit").and_then(|s| s.as_str()))
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(ToString::to_string);
        let date = meta
            .as_ref()
            .and_then(|v| v.get("date").and_then(|s| s.as_str()))
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(ToString::to_string);
        let notes = meta
            .as_ref()
            .and_then(|v| v.get("notes").and_then(|s| s.as_str()))
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(ToString::to_string);
        let has_binaries = dir.join("bin/nas").is_file() && dir.join("bin/nas-web").is_file();
        let current = active_name.as_deref() == Some(name.as_str());

        releases.push(InstalledRelease {
            version: name,
            commit,
            date,
            notes,
            current,
            path: dir.to_string_lossy().to_string(),
            has_binaries,
        });
    }

    releases.sort_by(|a, b| compare_versions_desc(&a.version, &b.version));
    releases
}

fn append_switch_steps(steps: &mut Vec<Vec<String>>, root: &str, target_dir: &Path) {
    let link = format!("{root}/current");
    let link_next = format!("{root}/current.new");

    steps.push(vec![
        "ln".to_string(),
        "-sfn".to_string(),
        target_dir.to_string_lossy().to_string(),
        link_next.clone(),
    ]);
    steps.push(vec![
        "mv".to_string(),
        "-T".to_string(),
        link_next,
        link.clone(),
    ]);

    for bin in ["nas", "nas-web", "nas-tui", "nasctl", "nas-dev"] {
        if target_dir.join("bin").join(bin).is_file() {
            steps.push(vec![
                "install".to_string(),
                "-m".to_string(),
                "0755".to_string(),
                format!("{link}/bin/{bin}"),
                format!("/usr/local/bin/{bin}"),
            ]);
        }
    }
    for helper in crate::deploy::helpers::ROOT_HELPERS {
        if target_dir.join("packaging").join(helper).is_file() {
            steps.push(vec![
                "install".to_string(),
                "-m".to_string(),
                "0755".to_string(),
                "-o".to_string(),
                "root".to_string(),
                "-g".to_string(),
                "root".to_string(),
                format!("{link}/packaging/{helper}"),
                format!("/usr/local/sbin/{helper}"),
            ]);
        }
    }
    if target_dir.join("version.json").is_file() {
        steps.push(vec![
            "install".to_string(),
            "-m".to_string(),
            "0644".to_string(),
            format!("{link}/version.json"),
            "/etc/nas/version.json".to_string(),
        ]);
        steps.push(vec![
            "install".to_string(),
            "-m".to_string(),
            "0644".to_string(),
            format!("{link}/version.json"),
            format!("{root}/version.json"),
        ]);
    }
}

/// Construye un plan A/B de regresión (downgrade) hacia una release local ya instalada.
pub fn build_rollback_plan(
    root: &str,
    target_version: &str,
    current_version: Option<&str>,
) -> NasResult<UpdatePlan> {
    let target = target_version.trim();
    if !is_valid_version(target) {
        return Err(NasError::Invalid(format!(
            "versión objetivo inválida para regresión: {target_version}"
        )));
    }

    let root_clean = root.trim_end_matches('/');
    let root_path = Path::new(root_clean);
    let active = current_version
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToString::to_string)
        .or_else(|| current_release_name(root_path));

    if active.as_deref() == Some(target) {
        return Err(NasError::Invalid(format!(
            "la versión {target} ya se encuentra activa"
        )));
    }

    let target_dir = root_path.join("releases").join(target);
    if !target_dir.is_dir() {
        return Err(NasError::NotFound(format!(
            "no existe la release instalada {}",
            target_dir.display()
        )));
    }
    if !target_dir.join("bin/nas").is_file() || !target_dir.join("bin/nas-web").is_file() {
        return Err(NasError::Invalid(format!(
            "la release {target} no contiene binarios completos (bin/nas y bin/nas-web)"
        )));
    }

    let mut steps = Vec::new();
    append_switch_steps(&mut steps, root_clean, &target_dir);
    steps.push(vec!["nginx".to_string(), "-t".to_string()]);
    steps.extend(service_reload_steps());
    steps.push(healthcheck_step());

    let mut rollback = Vec::new();
    if let Some(prev) = active.as_deref().filter(|v| is_valid_version(v)) {
        let prev_dir = root_path.join("releases").join(prev);
        if prev_dir.join("bin/nas").is_file() && prev_dir.join("bin/nas-web").is_file() {
            append_switch_steps(&mut rollback, root_clean, &prev_dir);
            rollback.extend(service_reload_steps());
        }
    }

    Ok(UpdatePlan {
        version: target.to_string(),
        release_dir: target_dir.to_string_lossy().to_string(),
        steps,
        rollback,
    })
}
