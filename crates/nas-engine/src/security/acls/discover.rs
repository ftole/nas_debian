use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::time::Duration;

use nas_exec::{CommandSpec, Runner};

use super::probe::{is_excluded_component, is_excluded_relative_path, is_real_dir};

pub(in crate::security) fn resolve_unix_user_ids(
    runner: &Runner,
    user: &str,
) -> Option<(u32, u32, String)> {
    let uid_out = runner
        .run(
            &CommandSpec::new("id")
                .args(["-u", user])
                .timeout(Duration::from_secs(5)),
        )
        .ok()
        .filter(|o| o.is_success())?;
    let gid_out = runner
        .run(
            &CommandSpec::new("id")
                .args(["-g", user])
                .timeout(Duration::from_secs(5)),
        )
        .ok()
        .filter(|o| o.is_success())?;
    let gname_out = runner
        .run(
            &CommandSpec::new("id")
                .args(["-gn", user])
                .timeout(Duration::from_secs(5)),
        )
        .ok()
        .filter(|o| o.is_success())?;
    let uid = uid_out.stdout.trim().parse::<u32>().ok()?;
    let gid = gid_out.stdout.trim().parse::<u32>().ok()?;
    let gname = gname_out.stdout.trim().to_string();
    if gname.is_empty() {
        return None;
    }
    Some((uid, gid, gname))
}

pub(in crate::security) fn resolve_unix_group_gid(runner: &Runner, group: &str) -> Option<u32> {
    let out = runner
        .run(
            &CommandSpec::new("getent")
                .args(["group", group])
                .timeout(Duration::from_secs(5)),
        )
        .ok()
        .filter(|o| o.is_success())?;
    out.stdout.trim().split(':').nth(2)?.parse::<u32>().ok()
}

pub(in crate::security) fn user_in_group(runner: &Runner, user: &str, group: &str) -> bool {
    runner
        .run(
            &CommandSpec::new("id")
                .args(["-nG", user])
                .timeout(Duration::from_secs(5)),
        )
        .is_ok_and(|o| o.is_success() && o.stdout.split_whitespace().any(|g| g == group))
}

/// Descubre los directorios raíz de unidades adicionales montadas bajo `<data_root>/unidades`.
pub fn discover_unit_roots(data_root: &Path) -> Vec<PathBuf> {
    let unidades_dir = data_root.join("unidades");
    if !is_real_dir(&unidades_dir) {
        return Vec::new();
    }
    let mut roots = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&unidades_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();
            if name == "unidades" || is_excluded_component(&name) {
                continue;
            }
            if is_real_dir(&path) {
                roots.push(path);
            }
        }
    }
    roots.sort();
    roots
}

/// Descubre los shares activos bajo `data_root` y `smb_conf`, excluyendo respaldos, logs, `.trash`,
/// `.nas_staging`, `unidades` y enlaces simbólicos.
pub fn discover_active_share_dirs(
    data_root: &Path,
    smb_conf: &Path,
    unit_roots: &[PathBuf],
) -> Vec<PathBuf> {
    let Ok(canon_data_root) = std::fs::canonicalize(data_root) else {
        return Vec::new();
    };
    let unidades_dir = data_root.join("unidades");
    let canon_unidades = std::fs::canonicalize(&unidades_dir).ok();
    let canon_units: BTreeSet<PathBuf> = unit_roots
        .iter()
        .filter_map(|p| std::fs::canonicalize(p).ok())
        .collect();

    let mut candidates: Vec<PathBuf> = Vec::new();

    for parent in std::iter::once(data_root).chain(unit_roots.iter().map(PathBuf::as_path)) {
        if let Ok(entries) = std::fs::read_dir(parent) {
            for entry in entries.flatten() {
                let path = entry.path();
                let name = entry.file_name().to_string_lossy().to_string();
                if name == "unidades" || is_excluded_component(&name) {
                    continue;
                }
                if is_real_dir(&path) {
                    candidates.push(path);
                }
            }
        }
    }

    if let Ok(conf) = std::fs::read_to_string(smb_conf) {
        for share in crate::parsers::smb::parse_shares(&conf) {
            let Some(raw_path) = share.path else {
                continue;
            };
            let trimmed = raw_path.trim();
            if trimmed.is_empty() {
                continue;
            }
            let path = PathBuf::from(trimmed);
            if !is_real_dir(&path) {
                continue;
            }
            let Ok(canon_path) = std::fs::canonicalize(&path) else {
                continue;
            };
            let Ok(rel) = canon_path.strip_prefix(&canon_data_root) else {
                continue;
            };
            if rel.as_os_str().is_empty() {
                continue;
            }
            if canon_unidades.as_ref().is_some_and(|u| u == &canon_path)
                || canon_units.contains(&canon_path)
            {
                continue;
            }
            if is_excluded_relative_path(rel) {
                continue;
            }
            candidates.push(path);
        }
    }

    let mut seen_canon: Vec<(PathBuf, PathBuf)> = Vec::new();
    for path in candidates {
        if let Ok(canon) = std::fs::canonicalize(&path) {
            if !seen_canon.iter().any(|(c, _)| c == &canon) {
                seen_canon.push((canon, path));
            }
        }
    }
    seen_canon.sort_by(|a, b| {
        a.0.components()
            .count()
            .cmp(&b.0.components().count())
            .then(a.0.cmp(&b.0))
    });

    let mut filtered: Vec<(PathBuf, PathBuf)> = Vec::new();
    for (canon, orig) in seen_canon {
        if filtered
            .iter()
            .any(|(parent_canon, _)| canon.starts_with(parent_canon))
        {
            continue;
        }
        filtered.push((canon, orig));
    }

    filtered.into_iter().map(|(_, orig)| orig).collect()
}
