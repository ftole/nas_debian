//! Sincronización idempotente de permisos y ACLs POSIX para `nas-web` sin `CAP_DAC_OVERRIDE`.

#[cfg(unix)]
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::time::Duration;

use nas_exec::{CommandSpec, Runner};

mod discover;
mod probe;

use super::spec_with_sudo;
pub use discover::{discover_active_share_dirs, discover_unit_roots};
pub(super) use discover::{resolve_unix_group_gid, resolve_unix_user_ids, user_in_group};
pub use probe::acl_has_effective;
pub(super) use probe::{dir_has_nas_acls, has_acl_tools, is_real_dir, share_tree_needs_nas_acls};

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
