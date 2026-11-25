#[cfg(unix)]
use std::os::unix::fs::MetadataExt;
use std::path::{Component, Path, PathBuf};
use std::time::Duration;

use nas_exec::{CommandSpec, Runner};

use super::super::spec_with_sudo;

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

pub(in crate::security) fn has_acl_tools() -> bool {
    let has_setfacl =
        Path::new("/usr/bin/setfacl").is_file() || Path::new("/bin/setfacl").is_file();
    let has_getfacl =
        Path::new("/usr/bin/getfacl").is_file() || Path::new("/bin/getfacl").is_file();
    has_setfacl && has_getfacl
}

pub(in crate::security) fn is_excluded_component(name: &str) -> bool {
    EXCLUDED_DATA_DIRS.contains(&name)
        || name.starts_with(".inprogress_")
        || name.starts_with(".upload-")
        || (name.starts_with("zip_") && name.ends_with(".tmp"))
}

pub(in crate::security) fn is_excluded_relative_path(rel: &Path) -> bool {
    rel.components().any(|comp| match comp {
        Component::Normal(part) => is_excluded_component(&part.to_string_lossy()),
        _ => true,
    })
}

pub(in crate::security) fn is_real_dir(path: &Path) -> bool {
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

pub(in crate::security) fn dir_has_nas_acls(
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

pub(in crate::security) fn share_tree_needs_nas_acls(
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
