//! Sincronización idempotente de permisos y ACLs POSIX para `nas-web` sin `CAP_DAC_OVERRIDE`.

use std::collections::BTreeSet;
#[cfg(unix)]
use std::os::unix::fs::MetadataExt;
use std::path::{Component, Path, PathBuf};
use std::time::Duration;

use nas_exec::{CommandSpec, Runner};

use super::spec_with_sudo;

const EXCLUDED_DATA_DIRS: &[&str] = &[
    "BACKUPS_HISTORICOS",
    "BACKUPS_PERMANENTES",
    "LOGS_BACKUP",
    ".trash",
    ".nas_staging",
    ".recycle",
    ".snapshots",
    "lost+found",
];

pub(super) fn has_acl_tools() -> bool {
    let has_setfacl =
        Path::new("/usr/bin/setfacl").is_file() || Path::new("/bin/setfacl").is_file();
    let has_getfacl =
        Path::new("/usr/bin/getfacl").is_file() || Path::new("/bin/getfacl").is_file();
    has_setfacl && has_getfacl
}

fn is_excluded_component(name: &str) -> bool {
    EXCLUDED_DATA_DIRS.contains(&name)
        || name.starts_with(".inprogress_")
        || name.starts_with(".upload-")
        || (name.starts_with("zip_") && name.ends_with(".tmp"))
}

fn is_excluded_relative_path(rel: &Path) -> bool {
    rel.components().any(|comp| match comp {
        Component::Normal(part) => is_excluded_component(&part.to_string_lossy()),
        _ => true,
    })
}

fn is_real_dir(path: &Path) -> bool {
    std::fs::symlink_metadata(path)
        .is_ok_and(|meta| !meta.file_type().is_symlink() && meta.is_dir())
}

/// Comprueba si la salida de `getfacl -p` otorga efectivamente los permisos `required`
/// (`r`, `w`, `x`) al usuario o grupo indicado, respetando la máscara (`#effective:`).
pub fn acl_has_effective(
    getfacl_output: &str,
    principal: &str,
    is_group: bool,
    required: &str,
    default_acl: bool,
) -> bool {
    let prefix = match (default_acl, is_group) {
        (true, true) => "default:group:",
        (false, true) => "group:",
        (true, false) => "default:user:",
        (false, false) => "user:",
    };
    getfacl_output.lines().any(|raw_line| {
        let line = raw_line.trim();
        let Some(rest) = line.strip_prefix(prefix) else {
            return false;
        };
        let Some((name, perms_and_comment)) = rest.split_once(':') else {
            return false;
        };
        if name != principal {
            return false;
        }
        let effective = if let Some((_, eff)) = perms_and_comment.split_once("#effective:") {
            eff.trim()
        } else {
            perms_and_comment.split('#').next().unwrap_or("").trim()
        };
        required.chars().all(|ch| effective.contains(ch))
    })
}

pub(super) fn dir_has_nas_acls(
    runner: &Runner,
    dir: &Path,
    user: &str,
    group: &str,
    sudo: bool,
) -> bool {
    let dir_str = dir.to_string_lossy();
    let out = runner.run(&spec_with_sudo(
        CommandSpec::new("getfacl")
            .args(["-p", "--", dir_str.as_ref()])
            .timeout(Duration::from_secs(15)),
        sudo,
    ));
    let Ok(out) = out else {
        return false;
    };
    if !out.is_success() {
        return false;
    }
    acl_has_effective(&out.stdout, user, false, "rwx", false)
        && acl_has_effective(&out.stdout, group, true, "rwx", false)
        && acl_has_effective(&out.stdout, user, false, "rwx", true)
        && acl_has_effective(&out.stdout, group, true, "rwx", true)
}

fn split_getfacl_blocks(stdout: &str) -> Vec<String> {
    let mut blocks = Vec::new();
    let mut current = String::new();
    for line in stdout.lines() {
        if line.starts_with("# file:") && !current.is_empty() {
            blocks.push(std::mem::take(&mut current));
        }
        if !line.trim().is_empty() {
            current.push_str(line);
            current.push('\n');
        }
    }
    if !current.is_empty() {
        blocks.push(current);
    }
    blocks
}

fn batch_has_nas_acls(
    runner: &Runner,
    paths: &[PathBuf],
    user: &str,
    group: &str,
    is_dirs: bool,
    sudo: bool,
) -> bool {
    if paths.is_empty() {
        return true;
    }
    for chunk in paths.chunks(256) {
        let mut args = vec!["-p".to_string(), "--".to_string()];
        args.extend(chunk.iter().map(|p| p.to_string_lossy().to_string()));
        let Ok(out) = runner.run(&spec_with_sudo(
            CommandSpec::new("getfacl")
                .args(args)
                .timeout(Duration::from_secs(30)),
            sudo,
        )) else {
            return false;
        };
        if !out.is_success() {
            return false;
        }
        let blocks = split_getfacl_blocks(&out.stdout);
        if blocks.len() != chunk.len() {
            return false;
        }
        let all_ok = blocks.iter().all(|block| {
            if is_dirs {
                acl_has_effective(block, user, false, "rwx", false)
                    && acl_has_effective(block, group, true, "rwx", false)
                    && acl_has_effective(block, user, false, "rwx", true)
                    && acl_has_effective(block, group, true, "rwx", true)
            } else {
                acl_has_effective(block, user, false, "rw", false)
                    && acl_has_effective(block, group, true, "rw", false)
            }
        });
        if !all_ok {
            return false;
        }
    }
    true
}

fn share_tree_needs_nas_acls(
    runner: &Runner,
    share_dir: &Path,
    user: &str,
    group: &str,
    sudo: bool,
) -> bool {
    if !dir_has_nas_acls(runner, share_dir, user, group, sudo) {
        return true;
    }

    let mut stack = vec![share_dir.to_path_buf()];
    let mut dir_batch: Vec<PathBuf> = Vec::new();
    let mut file_batch: Vec<PathBuf> = Vec::new();

    while let Some(current) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&current) else {
            return true;
        };
        for entry in entries {
            let Ok(entry) = entry else {
                return true;
            };
            let path = entry.path();
            let Ok(meta) = std::fs::symlink_metadata(&path) else {
                return true;
            };
            if meta.file_type().is_symlink() {
                continue;
            }
            if meta.is_dir() {
                #[cfg(unix)]
                if (meta.mode() & 0o070) != 0o070 {
                    return true;
                }
                stack.push(path.clone());
                dir_batch.push(path);
                if dir_batch.len() >= 256 {
                    if !batch_has_nas_acls(runner, &dir_batch, user, group, true, sudo) {
                        return true;
                    }
                    dir_batch.clear();
                }
            } else if meta.is_file() {
                #[cfg(unix)]
                if (meta.mode() & 0o060) != 0o060 {
                    return true;
                }
                file_batch.push(path);
                if file_batch.len() >= 256 {
                    if !batch_has_nas_acls(runner, &file_batch, user, group, false, sudo) {
                        return true;
                    }
                    file_batch.clear();
                }
            }
        }
    }

    if !batch_has_nas_acls(runner, &dir_batch, user, group, true, sudo) {
        return true;
    }
    if !batch_has_nas_acls(runner, &file_batch, user, group, false, sudo) {
        return true;
    }

    false
}

fn resolve_unix_user_ids(runner: &Runner, user: &str) -> Option<(u32, u32, String)> {
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

fn resolve_unix_group_gid(runner: &Runner, group: &str) -> Option<u32> {
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

fn user_in_group(runner: &Runner, user: &str, group: &str) -> bool {
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

#[allow(clippy::too_many_arguments)]
fn ensure_root_dir_access(
    runner: &Runner,
    dir: &Path,
    service_user: &str,
    service_group: &str,
    expected_owner_uid: Option<u32>,
    grp_gid: u32,
    sudo: bool,
    warnings: &mut Vec<String>,
) -> bool {
    let Ok(meta) = std::fs::symlink_metadata(dir) else {
        return false;
    };
    if meta.file_type().is_symlink() || !meta.is_dir() {
        return false;
    }

    let dir_str = dir.to_string_lossy().to_string();
    let mut changed = false;

    #[cfg(unix)]
    let needs_chown = expected_owner_uid.is_some_and(|u| meta.uid() != u) || meta.gid() != grp_gid;
    #[cfg(not(unix))]
    let needs_chown = false;

    if needs_chown {
        let owner_spec = if expected_owner_uid.is_some() {
            format!("root:{service_group}")
        } else {
            format!(":{service_group}")
        };
        match runner.run(&spec_with_sudo(
            CommandSpec::new("chown")
                .args([&owner_spec, "--", &dir_str])
                .timeout(Duration::from_secs(20)),
            sudo,
        )) {
            Ok(out) if out.is_success() => changed = true,
            Ok(out) => warnings.push(format!(
                "no se pudo ajustar propietario/grupo en {dir_str}: {}",
                out.stderr.trim()
            )),
            Err(e) => warnings.push(format!(
                "no se pudo ajustar propietario/grupo en {dir_str}: {e}"
            )),
        }
    }

    #[cfg(unix)]
    let needs_chmod = (meta.mode() & 0o7777) != 0o2770;
    #[cfg(not(unix))]
    let needs_chmod = false;

    if needs_chmod {
        match runner.run(&spec_with_sudo(
            CommandSpec::new("chmod")
                .args(["2770", "--", &dir_str])
                .timeout(Duration::from_secs(20)),
            sudo,
        )) {
            Ok(out) if out.is_success() => changed = true,
            Ok(out) => warnings.push(format!(
                "no se pudo ajustar modo 2770 en {dir_str}: {}",
                out.stderr.trim()
            )),
            Err(e) => warnings.push(format!("no se pudo ajustar modo 2770 en {dir_str}: {e}")),
        }
    }

    if has_acl_tools() && !dir_has_nas_acls(runner, dir, service_user, service_group, sudo) {
        let acl_spec = format!(
            "u:{service_user}:rwx,g:{service_group}:rwx,m::rwx,d:u:{service_user}:rwx,d:g:{service_group}:rwx,d:m::rwx"
        );
        match runner.run(&spec_with_sudo(
            CommandSpec::new("setfacl")
                .args(["-m", &acl_spec, "--", &dir_str])
                .timeout(Duration::from_secs(30)),
            sudo,
        )) {
            Ok(out) if out.is_success() => changed = true,
            Ok(out) => warnings.push(format!(
                "no se pudieron aplicar ACLs en {dir_str}: {}",
                out.stderr.trim()
            )),
            Err(e) => warnings.push(format!("no se pudieron aplicar ACLs en {dir_str}: {e}")),
        }
    }

    changed
}

fn staging_has_foreign_owner(staging: &Path, uid: u32, gid: u32) -> bool {
    let Ok(entries) = std::fs::read_dir(staging) else {
        return false;
    };
    for entry in entries.flatten() {
        if let Ok(meta) = std::fs::symlink_metadata(entry.path()) {
            #[cfg(unix)]
            if meta.uid() != uid || meta.gid() != gid {
                return true;
            }
            if !meta.file_type().is_symlink()
                && meta.is_dir()
                && staging_has_foreign_owner(&entry.path(), uid, gid)
            {
                return true;
            }
        }
    }
    false
}

#[allow(clippy::too_many_arguments)]
pub(super) fn ensure_staging_path(
    runner: &Runner,
    staging: &Path,
    create_if_missing: bool,
    service_user: &str,
    service_user_group: &str,
    uid: u32,
    user_gid: u32,
    sudo: bool,
    warnings: &mut Vec<String>,
) -> bool {
    let staging_str = staging.to_string_lossy().to_string();

    match std::fs::symlink_metadata(staging) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            if !create_if_missing {
                return false;
            }
            match runner.run(&spec_with_sudo(
                CommandSpec::new("install")
                    .args([
                        "-d",
                        "-m",
                        "00700",
                        "-o",
                        service_user,
                        "-g",
                        service_user_group,
                        "--",
                        &staging_str,
                    ])
                    .timeout(Duration::from_secs(20)),
                sudo,
            )) {
                Ok(out) if out.is_success() => {
                    if has_acl_tools() {
                        let _ = runner.run(&spec_with_sudo(
                            CommandSpec::new("setfacl")
                                .args(["-b", "--", &staging_str])
                                .timeout(Duration::from_secs(20)),
                            sudo,
                        ));
                    }
                    true
                }
                Ok(out) => {
                    warnings.push(format!(
                        "no se pudo crear {staging_str}: {}",
                        out.stderr.trim()
                    ));
                    false
                }
                Err(err) => {
                    warnings.push(format!("no se pudo crear {staging_str}: {err}"));
                    false
                }
            }
        }
        Err(_) => false,
        Ok(meta) => {
            if meta.file_type().is_symlink() || !meta.is_dir() {
                warnings.push(format!(
                    "{staging_str} no es un directorio regular; omitido"
                ));
                return false;
            }

            #[cfg(unix)]
            let needs_fix = meta.uid() != uid
                || meta.gid() != user_gid
                || (meta.mode() & 0o7777) != 0o700
                || staging_has_foreign_owner(staging, uid, user_gid);
            #[cfg(not(unix))]
            let needs_fix = false;

            if !needs_fix {
                return false;
            }

            let owner_spec = format!("{service_user}:{service_user_group}");
            let mut changed = false;
            match runner.run(&spec_with_sudo(
                CommandSpec::new("chown")
                    .args(["-PR", &owner_spec, "--", &staging_str])
                    .timeout(Duration::from_secs(60)),
                sudo,
            )) {
                Ok(out) if out.is_success() => changed = true,
                Ok(out) => warnings.push(format!(
                    "no se pudo corregir propietario de {staging_str}: {}",
                    out.stderr.trim()
                )),
                Err(e) => warnings.push(format!(
                    "no se pudo corregir propietario de {staging_str}: {e}"
                )),
            }

            if has_acl_tools() {
                let _ = runner.run(&spec_with_sudo(
                    CommandSpec::new("setfacl")
                        .args(["-b", "--", &staging_str])
                        .timeout(Duration::from_secs(20)),
                    sudo,
                ));
            }

            match runner.run(&spec_with_sudo(
                CommandSpec::new("chmod")
                    .args(["00700", "--", &staging_str])
                    .timeout(Duration::from_secs(20)),
                sudo,
            )) {
                Ok(out) if out.is_success() => changed = true,
                Ok(out) => warnings.push(format!(
                    "no se pudo ajustar modo 0700 en {staging_str}: {}",
                    out.stderr.trim()
                )),
                Err(e) => warnings.push(format!(
                    "no se pudo ajustar modo 0700 en {staging_str}: {e}"
                )),
            }

            changed
        }
    }
}

pub(super) fn ensure_active_share_acls(
    runner: &Runner,
    share_dir: &Path,
    service_user: &str,
    service_group: &str,
    sudo: bool,
    warnings: &mut Vec<String>,
) -> bool {
    if !is_real_dir(share_dir) || !has_acl_tools() {
        return false;
    }
    if !share_tree_needs_nas_acls(runner, share_dir, service_user, service_group, sudo) {
        return false;
    }

    let share_str = share_dir.to_string_lossy().to_string();
    let acl_spec = format!(
        "u:{service_user}:rwX,g:{service_group}:rwX,d:u:{service_user}:rwx,d:g:{service_group}:rwx"
    );
    match runner.run(&spec_with_sudo(
        CommandSpec::new("setfacl")
            .args(["-RP", "-m", &acl_spec, "--", &share_str])
            .timeout(Duration::from_secs(300)),
        sudo,
    )) {
        Ok(out) if out.is_success() => true,
        Ok(out) => {
            warnings.push(format!(
                "algunos elementos de {share_str} no admitieron ACLs para {service_user}: {}",
                out.stderr.trim()
            ));
            false
        }
        Err(e) => {
            warnings.push(format!(
                "no se pudieron sincronizar ACLs en {share_str}: {e}"
            ));
            false
        }
    }
}

pub(super) fn resolve_state_dir(env_file: &Path) -> PathBuf {
    if let Ok(val) = std::env::var("NAS_STATE_DIR") {
        let trimmed = val.trim();
        if trimmed.starts_with('/') && !trimmed.contains("..") {
            return PathBuf::from(trimmed);
        }
    }
    if let Ok(raw) = std::fs::read_to_string(env_file) {
        for line in raw.lines() {
            let trimmed = line.trim();
            if let Some(rest) = trimmed.strip_prefix("NAS_STATE_DIR=") {
                let unquoted = rest.trim_matches(|c| c == '"' || c == '\'' || c == ' ');
                if unquoted.starts_with('/') && !unquoted.contains("..") {
                    return PathBuf::from(unquoted);
                }
            }
        }
    }
    PathBuf::from("/var/lib/nas")
}

/// Sincroniza de forma idempotente (y sin recursión destructiva sobre `BACKUPS_*` ni `LOGS_BACKUP`)
/// los permisos y ACLs de `service_user` y `service_group` en `data_root`, `unidades`, shares activos, `.nas_staging` y `<state_dir>/staging`.
#[allow(clippy::too_many_arguments)]
pub(crate) fn sync_data_root_permissions_with_principals(
    runner: &Runner,
    data_root: &Path,
    state_dir: Option<&Path>,
    smb_conf: &Path,
    service_user: &str,
    service_group: &str,
    expected_root_owner_uid: Option<u32>,
    sudo: bool,
    actions: &mut Vec<String>,
    warnings: &mut Vec<String>,
) -> bool {
    if !is_real_dir(data_root) {
        return false;
    }

    let Some((uid, user_gid, user_primary_group)) = resolve_unix_user_ids(runner, service_user)
    else {
        warnings.push(format!(
            "usuario '{service_user}' inexistente; omitida sincronización de ACLs en {}",
            data_root.display()
        ));
        return false;
    };
    let Some(grp_gid) = resolve_unix_group_gid(runner, service_group) else {
        warnings.push(format!(
            "grupo '{service_group}' inexistente; omitida sincronización de ACLs en {}",
            data_root.display()
        ));
        return false;
    };

    let mut restart_needed = false;
    if !user_in_group(runner, service_user, service_group) {
        match runner.run(&spec_with_sudo(
            CommandSpec::new("usermod")
                .args(["-aG", service_group, service_user])
                .timeout(Duration::from_secs(20)),
            sudo,
        )) {
            Ok(out) if out.is_success() => {
                actions.push(format!(
                    "agregado {service_user} al grupo suplementario {service_group}"
                ));
                restart_needed = true;
            }
            Ok(out) => warnings.push(format!(
                "no se pudo agregar {service_user} a {service_group}: {}",
                out.stderr.trim()
            )),
            Err(e) => warnings.push(format!(
                "no se pudo agregar {service_user} a {service_group}: {e}"
            )),
        }
    }

    let mut adjusted_roots = Vec::new();
    if ensure_root_dir_access(
        runner,
        data_root,
        service_user,
        service_group,
        expected_root_owner_uid,
        grp_gid,
        sudo,
        warnings,
    ) {
        adjusted_roots.push(data_root.display().to_string());
    }

    let unidades_dir = data_root.join("unidades");
    if is_real_dir(&unidades_dir)
        && ensure_root_dir_access(
            runner,
            &unidades_dir,
            service_user,
            service_group,
            expected_root_owner_uid,
            grp_gid,
            sudo,
            warnings,
        )
    {
        adjusted_roots.push(unidades_dir.display().to_string());
    }

    let unit_roots = discover_unit_roots(data_root);
    for unit_root in &unit_roots {
        if ensure_root_dir_access(
            runner,
            unit_root,
            service_user,
            service_group,
            expected_root_owner_uid,
            grp_gid,
            sudo,
            warnings,
        ) {
            adjusted_roots.push(unit_root.display().to_string());
        }
    }
    if !adjusted_roots.is_empty() {
        actions.push(format!(
            "permisos/ACLs de {service_user} y {service_group} sincronizados en raíces: {}",
            adjusted_roots.join(", ")
        ));
    }

    let mut adjusted_staging = Vec::new();
    let data_staging = data_root.join(".nas_staging");
    if ensure_staging_path(
        runner,
        &data_staging,
        true,
        service_user,
        &user_primary_group,
        uid,
        user_gid,
        sudo,
        warnings,
    ) {
        adjusted_staging.push(data_staging.display().to_string());
    }
    for unit_root in &unit_roots {
        let unit_staging = unit_root.join(".nas_staging");
        if ensure_staging_path(
            runner,
            &unit_staging,
            false,
            service_user,
            &user_primary_group,
            uid,
            user_gid,
            sudo,
            warnings,
        ) {
            adjusted_staging.push(unit_staging.display().to_string());
        }
    }
    if let Some(st_dir) = state_dir {
        let state_staging = st_dir.join("staging");
        if ensure_staging_path(
            runner,
            &state_staging,
            false,
            service_user,
            &user_primary_group,
            uid,
            user_gid,
            sudo,
            warnings,
        ) {
            adjusted_staging.push(state_staging.display().to_string());
        }
    }
    if !adjusted_staging.is_empty() {
        actions.push(format!(
            "propiedad/permisos de staging sincronizados para {service_user}: {}",
            adjusted_staging.join(", ")
        ));
    }

    let share_dirs = discover_active_share_dirs(data_root, smb_conf, &unit_roots);
    let mut adjusted_shares = Vec::new();
    for share_dir in share_dirs {
        if ensure_active_share_acls(
            runner,
            &share_dir,
            service_user,
            service_group,
            sudo,
            warnings,
        ) {
            adjusted_shares.push(share_dir.display().to_string());
        }
    }
    if !adjusted_shares.is_empty() {
        actions.push(format!(
            "ACLs de acceso para {service_user} y {service_group} sincronizadas en shares: {}",
            adjusted_shares.join(", ")
        ));
    }

    restart_needed
}
