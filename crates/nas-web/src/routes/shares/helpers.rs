use nas_core::NasError;
use nas_exec::Runner;

use super::ShareBody;

pub fn share_path(name: &str, unit: &str) -> String {
    if unit.is_empty() || unit == "principal" {
        format!("/srv/nas/{name}")
    } else {
        format!("/srv/nas/unidades/{unit}/{name}")
    }
}

pub fn normalize_group(group: &str) -> String {
    let trimmed = group.trim();
    if trimmed.starts_with('@') {
        trimmed.to_string()
    } else {
        format!("@{trimmed}")
    }
}

pub fn share_spec(body: &ShareBody, path: &str) -> nas_engine::samba::ShareSpec {
    let groups: Vec<String> = body.groups.iter().map(|g| normalize_group(g)).collect();
    let write = if body.write_group.trim().is_empty() {
        None
    } else {
        Some(normalize_group(&body.write_group))
    };

    let read_only = body.scheme == 3;
    let guest_ok = body.scheme == 4;
    let write_list = if body.scheme == 2 {
        write.into_iter().collect()
    } else if body.scheme == 1 {
        groups.clone()
    } else {
        Vec::new()
    };

    nas_engine::samba::ShareSpec {
        name: body.name.trim().to_string(),
        path: path.to_string(),
        read_only,
        browseable: !body.hidden,
        guest_ok,
        valid_users: if guest_ok { Vec::new() } else { groups },
        write_list: if read_only || guest_ok {
            Vec::new()
        } else {
            write_list
        },
        comment: body.comment.clone(),
    }
}

pub fn validate_share_spec(spec: &nas_engine::samba::ShareSpec) -> Result<(), NasError> {
    nas_engine::samba::validate_name(&spec.name)?;
    nas_engine::samba::validate_path(&spec.path)?;
    nas_engine::samba::validate_comment(&spec.comment)?;
    for entry in spec.valid_users.iter().chain(spec.write_list.iter()) {
        nas_engine::samba::validate_principal(entry)?;
    }
    Ok(())
}

pub fn stage_and_apply(conf: &str) -> Result<(), String> {
    let stamp = chrono::Local::now().format("%Y%m%d%H%M%S%3f");
    let dir = std::path::Path::new("/var/lib/nas/staging");
    std::fs::create_dir_all(dir).map_err(|error| error.to_string())?;
    let staged = dir.join(format!("smbconf_{stamp}"));
    std::fs::write(&staged, conf).map_err(|error| error.to_string())?;

    let staged_str = staged.to_string_lossy().to_string();
    let output = Runner::new()
        .run(
            &nas_exec::CommandSpec::new("/usr/local/bin/nas")
                .args(["helper", "samba-apply", &staged_str])
                .sudo(nas_exec::SudoMode::Passwordless)
                .timeout(std::time::Duration::from_secs(60)),
        )
        .map_err(|error| error.to_string())?;

    if output.is_success() {
        Ok(())
    } else {
        let _ = std::fs::remove_file(&staged);
        Err(if output.stderr.is_empty() {
            output.stdout
        } else {
            output.stderr
        })
    }
}

pub fn apply_acl(path: &str, principal: &str, level: &str) -> Result<(), String> {
    let output = Runner::new()
        .run(
            &nas_exec::CommandSpec::new("/usr/local/bin/nas")
                .args([
                    "helper",
                    "share-acl",
                    path,
                    principal.trim_start_matches('@'),
                    level,
                ])
                .sudo(nas_exec::SudoMode::Passwordless)
                .timeout(std::time::Duration::from_secs(120)),
        )
        .map_err(|error| error.to_string())?;

    if output.is_success() {
        Ok(())
    } else {
        Err(if output.stderr.is_empty() {
            output.stdout
        } else {
            output.stderr
        })
    }
}

pub fn create_share_dir(name: &str, unit: &str) -> Result<(), String> {
    let output = Runner::new()
        .run(
            &nas_exec::CommandSpec::new("/usr/local/bin/nas")
                .args([
                    "helper",
                    "share-dir",
                    name,
                    if unit.is_empty() { "principal" } else { unit },
                ])
                .sudo(nas_exec::SudoMode::Passwordless)
                .timeout(std::time::Duration::from_secs(30)),
        )
        .map_err(|error| error.to_string())?;

    if output.is_success() {
        Ok(())
    } else {
        Err(if output.stderr.is_empty() {
            output.stdout
        } else {
            output.stderr
        })
    }
}

pub fn apply_spec_acls(spec: &nas_engine::samba::ShareSpec) {
    for group in &spec.valid_users {
        let level = if spec.write_list.contains(group) {
            "write"
        } else {
            "read"
        };
        if apply_acl(&spec.path, group, level).is_err() {
            eprintln!("aviso: no se pudo aplicar ACL de {group} en {}", spec.path);
        }
    }
    for group in spec
        .write_list
        .iter()
        .filter(|entry| !spec.valid_users.contains(entry))
    {
        if apply_acl(&spec.path, group, "write").is_err() {
            eprintln!("aviso: no se pudo aplicar ACL de {group} en {}", spec.path);
        }
    }
}

/// Revoca las ACLs POSIX de los principais retirados de un recurso.
pub fn revoke_removed_acls(
    previous: &nas_engine::parsers::smb::Share,
    spec: &nas_engine::samba::ShareSpec,
) {
    let keep: std::collections::HashSet<&str> = spec
        .valid_users
        .iter()
        .chain(spec.write_list.iter())
        .map(|entry| entry.trim_start_matches('@'))
        .chain(std::iter::once("grp_samba"))
        .collect();

    let mut removed: Vec<String> = Vec::new();
    for entry in previous
        .valid_users
        .iter()
        .chain(previous.write_list.iter())
    {
        let name = entry.trim_start_matches('@').to_string();
        if !name.is_empty() && !keep.contains(name.as_str()) && !removed.contains(&name) {
            removed.push(name);
        }
    }

    for name in removed {
        let ok = apply_acl(&spec.path, &name, "none");
        if ok.is_err() {
            eprintln!(
                "aviso: no se pudo revocar la ACL de {name} en {}",
                spec.path
            );
        }
    }
}
