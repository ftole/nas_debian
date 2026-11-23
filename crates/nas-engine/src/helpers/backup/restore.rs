//! Restauración de snapshots y respaldos permanentes hacia el origen o un destino local.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::Duration;

use nas_core::{NasError, NasResult};
use nas_exec::{CommandSpec, Runner};

use crate::backup_control::valid_id;
use crate::lock::TaskLock;

use super::permanent::{read_task_info, TaskScriptInfo};

const DEFAULT_RESTORE_TIMEOUT_SECS: u64 = 21_600;

fn restore_timeout() -> Duration {
    let secs = std::env::var("NAS_RESTORE_TIMEOUT_SECS")
        .ok()
        .and_then(|v| v.trim().parse::<u64>().ok())
        .map(|s| s.clamp(300, 86_400))
        .unwrap_or(DEFAULT_RESTORE_TIMEOUT_SECS);
    Duration::from_secs(secs)
}

fn is_protected_restore_subpath(rel: &Path) -> bool {
    let comps: Vec<String> = rel
        .components()
        .filter_map(|c| match c {
            std::path::Component::Normal(s) => Some(s.to_string_lossy().to_string()),
            _ => None,
        })
        .collect();
    let is_protected_name = |s: &str| {
        matches!(
            s,
            "BACKUPS_HISTORICOS"
                | "BACKUPS_PERMANENTES"
                | "LOGS_BACKUP"
                | ".trash"
                | ".nas_staging"
        )
    };
    if comps.first().is_some_and(|s| is_protected_name(s)) {
        return true;
    }
    if comps.first().map(String::as_str) == Some("unidades")
        && comps.get(2).is_some_and(|s| is_protected_name(s))
    {
        return true;
    }
    false
}

/// Valida un destino de restauración sin seguir symlinks fuera de `/srv/nas` o `/tmp`.
fn resolve_safe_restore_target(target: &str) -> NasResult<PathBuf> {
    let value = target.trim();
    if value.is_empty()
        || value.contains('\0')
        || value.contains("..")
        || value.chars().any(char::is_control)
    {
        return Err(NasError::Denied("destino de restauración inválido".into()));
    }
    let (allowed_root, rest) = if let Some(r) = value.strip_prefix("/srv/nas/") {
        (Path::new("/srv/nas"), r)
    } else if let Some(r) = value.strip_prefix("/tmp/") {
        (Path::new("/tmp"), r)
    } else {
        return Err(NasError::Denied(
            "el destino de restauración debe estar dentro de /srv/nas/ o /tmp/".into(),
        ));
    };

    let rest_trimmed = rest.trim_matches('/');
    if rest_trimmed.is_empty() {
        return Err(NasError::Denied(
            "el destino de restauración debe especificar un subdirectorio".into(),
        ));
    }
    if allowed_root == Path::new("/srv/nas")
        && is_protected_restore_subpath(Path::new(rest_trimmed))
    {
        return Err(NasError::Denied(
            "no se puede restaurar sobre directorios protegidos del sistema".into(),
        ));
    }

    let canon_root = std::fs::canonicalize(allowed_root)
        .map_err(|e| NasError::Denied(format!("raíz de destino inaccesible: {e}")))?;

    let candidate = allowed_root.join(rest_trimmed);
    let mut existing = candidate.clone();
    let mut remaining: Vec<OsString> = Vec::new();
    while std::fs::symlink_metadata(&existing).is_err() {
        let Some(name) = existing.file_name().map(|n| n.to_os_string()) else {
            break;
        };
        remaining.push(name);
        let Some(parent) = existing.parent() else {
            break;
        };
        existing = parent.to_path_buf();
    }

    // Verificar que ningún componente existente desde allowed_root hasta `existing` sea un symlink.
    if let Ok(rel_existing) = existing.strip_prefix(allowed_root) {
        let mut walk = allowed_root.to_path_buf();
        for comp in rel_existing.components() {
            if let std::path::Component::Normal(part) = comp {
                walk.push(part);
                if let Ok(meta) = std::fs::symlink_metadata(&walk) {
                    if meta.file_type().is_symlink() {
                        return Err(NasError::Denied(
                            "el destino de restauración contiene un enlace simbólico no permitido"
                                .into(),
                        ));
                    }
                }
            }
        }
    }

    let canon_existing = std::fs::canonicalize(&existing)
        .map_err(|e| NasError::Denied(format!("destino de restauración inaccesible: {e}")))?;
    if !canon_existing.starts_with(&canon_root) {
        return Err(NasError::Denied(
            "el destino de restauración escapa de /srv/nas o /tmp".into(),
        ));
    }

    let mut resolved = canon_existing;
    for name in remaining.iter().rev() {
        resolved.push(name);
    }
    if !resolved.starts_with(&canon_root) || resolved == canon_root {
        return Err(NasError::Denied(
            "el destino de restauración escapa de /srv/nas o /tmp".into(),
        ));
    }

    std::fs::create_dir_all(&resolved).map_err(|e| NasError::Denied(e.to_string()))?;
    let final_canon =
        std::fs::canonicalize(&resolved).map_err(|e| NasError::Denied(e.to_string()))?;
    if !final_canon.starts_with(&canon_root) || final_canon == canon_root {
        return Err(NasError::Denied(
            "el destino de restauración escapa de /srv/nas o /tmp".into(),
        ));
    }
    Ok(final_canon)
}

fn resolve_snapshot_dir(
    id: &str,
    snap_str: &str,
    task_info: &TaskScriptInfo,
) -> NasResult<PathBuf> {
    if snap_str.is_empty() || snap_str.contains('\0') || snap_str.contains("..") {
        return Err(NasError::Invalid("ruta de snapshot inválida".into()));
    }

    let perm_repo = PathBuf::from(
        task_info
            .dest_root
            .replace("BACKUPS_HISTORICOS", "BACKUPS_PERMANENTES"),
    )
    .join(id);
    let hist_repo = PathBuf::from(&task_info.dest_root).join(id);

    let allowed_repos: Vec<PathBuf> = [perm_repo, hist_repo]
        .into_iter()
        .filter_map(|p| std::fs::canonicalize(&p).ok())
        .collect();

    if snap_str.starts_with('/') {
        let candidate = Path::new(snap_str);
        let meta = std::fs::symlink_metadata(candidate)
            .map_err(|_| NasError::NotFound(format!("snapshot '{snap_str}' no encontrado")))?;
        if meta.file_type().is_symlink() || !meta.is_dir() {
            return Err(NasError::Denied(
                "el origen de restauración debe ser un directorio real dentro de BACKUPS_*".into(),
            ));
        }
        let canon = std::fs::canonicalize(candidate)
            .map_err(|_| NasError::NotFound(format!("snapshot '{snap_str}' no encontrado")))?;
        let in_repo = allowed_repos
            .iter()
            .any(|repo| canon != *repo && canon.starts_with(repo));
        if !in_repo {
            return Err(NasError::Denied(
                "el origen de restauración debe residir dentro de BACKUPS_HISTORICOS o BACKUPS_PERMANENTES de la tarea"
                    .into(),
            ));
        }
        return Ok(canon);
    }

    if snap_str.contains('/') || snap_str.contains('\\') || snap_str.chars().any(char::is_control) {
        return Err(NasError::Invalid("nombre de snapshot inválido".into()));
    }

    for repo in &allowed_repos {
        let candidate = repo.join(snap_str);
        if let Ok(meta) = std::fs::symlink_metadata(&candidate) {
            if !meta.file_type().is_symlink() && meta.is_dir() {
                if let Ok(canon) = std::fs::canonicalize(&candidate) {
                    if canon != *repo && canon.starts_with(repo) {
                        return Ok(canon);
                    }
                }
            }
        }
    }

    Err(NasError::NotFound(format!(
        "snapshot o respaldo permanente '{snap_str}' no encontrado"
    )))
}

fn resolve_ssh_key_for_task(id: &str, task_info: &TaskScriptInfo) -> Option<PathBuf> {
    let cred_base = std::fs::canonicalize("/etc/backup-credentials").ok()?;
    let mut candidates = Vec::new();
    if let Some(k) = task_info
        .key_file
        .as_deref()
        .filter(|s| !s.trim().is_empty())
    {
        candidates.push(PathBuf::from(k.trim()));
    }
    candidates.push(PathBuf::from(format!(
        "/etc/backup-credentials/{id}.id_ed25519"
    )));
    for cand in candidates {
        if let Ok(meta) = std::fs::symlink_metadata(&cand) {
            if !meta.file_type().is_symlink() && meta.is_file() {
                if let Ok(canon) = std::fs::canonicalize(&cand) {
                    if canon.starts_with(&cred_base) {
                        return Some(canon);
                    }
                }
            }
        }
    }
    None
}

/// Restaura un snapshot en el origen configurado o en un destino local permitido.
pub fn restore_backup(
    runner: &Runner,
    id: &str,
    snapshot: &str,
    target: Option<&str>,
) -> NasResult<String> {
    if !valid_id(id) {
        return Err(NasError::Invalid(format!(
            "identificador de tarea inválido: '{id}'"
        )));
    }

    let snap_str = snapshot.trim();
    if snap_str.is_empty() || snap_str.contains("..") {
        return Err(NasError::Invalid("ruta de snapshot inválida".into()));
    }

    let custom_target = target.map(str::trim).filter(|t| !t.is_empty());
    let resolved_custom_target = if let Some(tgt) = custom_target {
        Some(resolve_safe_restore_target(tgt)?)
    } else {
        None
    };

    let task_info = read_task_info(id);
    let snap_path = resolve_snapshot_dir(id, snap_str, &task_info)?;

    let lock_path = format!("/var/lock/backup_{id}.lock");
    let _task_lock = match TaskLock::acquire(&lock_path) {
        Ok(lock) => Some(lock),
        Err(NasError::Denied(msg)) => return Err(NasError::Denied(msg)),
        Err(_) => None,
    };

    let timeout = restore_timeout();

    // Si se especifica un destino local alternativo
    if let Some(canonical) = resolved_custom_target {
        let rsync_res = runner.run(
            &CommandSpec::new("rsync")
                .args([
                    "-aAXH",
                    "--numeric-ids",
                    "--safe-links",
                    "--timeout=300",
                    &format!("{}/", snap_path.display()),
                    &canonical.to_string_lossy(),
                ])
                .timeout(timeout),
        )?;
        if !rsync_res.is_success() {
            return Err(NasError::Invalid(format!(
                "fallo al restaurar en destino: {}",
                rsync_res.stderr
            )));
        }
        return Ok(format!(
            "Restauración completada en destino local: {}",
            canonical.display()
        ));
    }

    // Restauración hacia el origen original de red
    match task_info.source_type.as_str() {
        "cifs" => {
            let Some(ip) = task_info.src_ip.as_deref() else {
                return Err(NasError::Invalid(
                    "falta SRC_IP en la configuración de la tarea".into(),
                ));
            };
            let Some(share) = task_info.src_share.as_deref() else {
                return Err(NasError::Invalid(
                    "falta SRC_SHARE en la configuración de la tarea".into(),
                ));
            };
            let Some(cred) = task_info.cred_file.as_deref() else {
                return Err(NasError::Invalid(
                    "falta CRED_FILE en la configuración de la tarea".into(),
                ));
            };

            let mount_dir = format!("/mnt/restore_targets/{id}");
            std::fs::create_dir_all(&mount_dir).map_err(|e| NasError::Denied(e.to_string()))?;

            let mount_opt = format!("credentials={cred},rw,iocharset=utf8,vers=3.1.1,noserverino");
            let mount_res = runner.run(
                &CommandSpec::new("mount")
                    .args([
                        "-t",
                        "cifs",
                        &format!("//{ip}/{share}"),
                        &mount_dir,
                        "-o",
                        &mount_opt,
                    ])
                    .timeout(Duration::from_secs(30)),
            )?;
            if !mount_res.is_success() {
                return Err(NasError::Invalid(format!(
                    "no se pudo montar el recurso CIFS en modo escritura: {}",
                    mount_res.stderr
                )));
            }

            let rsync_res = runner.run(
                &CommandSpec::new("rsync")
                    .args([
                        "-aAXH",
                        "--numeric-ids",
                        "--safe-links",
                        "--timeout=300",
                        &format!("{}/", snap_path.display()),
                        &format!("{mount_dir}/"),
                    ])
                    .timeout(timeout),
            );

            let _ = runner.run(
                &CommandSpec::new("umount")
                    .args([&mount_dir])
                    .timeout(Duration::from_secs(15)),
            );

            match rsync_res {
                Ok(res) if res.is_success() || res.code == 24 || res.code == 23 => Ok(format!(
                    "Restauración completada exitosamente hacia el recurso CIFS //{ip}/{share}"
                )),
                Ok(res) => Err(NasError::Invalid(format!(
                    "fallo durante la sincronización rsync: {}",
                    res.stderr
                ))),
                Err(e) => Err(e),
            }
        }
        "ssh" => {
            let Some(ip) = task_info.src_ip.as_deref() else {
                return Err(NasError::Invalid("falta SRC_IP en la tarea".into()));
            };
            let Some(user) = task_info.src_user.as_deref() else {
                return Err(NasError::Invalid("falta SRC_USER en la tarea".into()));
            };
            let path = task_info.src_path.as_deref().unwrap_or("/");

            let res = if let Some(key_path) = resolve_ssh_key_for_task(id, &task_info) {
                let ssh_cmd = nas_ssh::rsync_ssh_command(task_info.src_port, Some(&key_path));
                runner.run(
                    &CommandSpec::new("rsync")
                        .args([
                            "-aAXH",
                            "--numeric-ids",
                            "--safe-links",
                            "--timeout=300",
                            "-e",
                            &ssh_cmd,
                            &format!("{}/", snap_path.display()),
                            &format!("{user}@{ip}:{path}/"),
                        ])
                        .timeout(timeout),
                )?
            } else {
                let cred = task_info.cred_file.as_deref().unwrap_or_default();
                let mut pass = String::new();
                if Path::new(cred).is_file() {
                    if let Ok(c) = std::fs::read_to_string(cred) {
                        pass = c.lines().next().unwrap_or("").trim().to_string();
                    }
                }
                let ssh_cmd = nas_ssh::rsync_ssh_command(task_info.src_port, None);
                let mut spec = CommandSpec::new("sshpass")
                    .args([
                        "-e",
                        "rsync",
                        "-aAXH",
                        "--numeric-ids",
                        "--safe-links",
                        "--timeout=300",
                        "-e",
                        &ssh_cmd,
                        &format!("{}/", snap_path.display()),
                        &format!("{user}@{ip}:{path}/"),
                    ])
                    .timeout(timeout);
                let safe_runner = if !pass.is_empty() {
                    spec = spec.env("SSHPASS", &pass);
                    runner.clone().with_secret(&pass)
                } else {
                    runner.clone()
                };
                safe_runner.run(&spec)?
            };

            if res.is_success() || res.code == 24 || res.code == 23 {
                Ok(format!("Restauración completada hacia {user}@{ip}:{path}"))
            } else {
                Err(NasError::Invalid(format!(
                    "fallo al restaurar por SSH: {}",
                    res.stderr
                )))
            }
        }
        _ => {
            let path = task_info.src_path.as_deref().unwrap_or("/");
            let res = runner.run(
                &CommandSpec::new("rsync")
                    .args([
                        "-aAXH",
                        "--numeric-ids",
                        "--safe-links",
                        "--timeout=300",
                        &format!("{}/", snap_path.display()),
                        &format!("{path}/"),
                    ])
                    .timeout(timeout),
            )?;
            if res.is_success() || res.code == 24 || res.code == 23 {
                Ok(format!("Restauración local completada en {path}"))
            } else {
                Err(NasError::Invalid(format!(
                    "fallo al restaurar localmente: {}",
                    res.stderr
                )))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn restore_backup_rechaza_rutas_peligrosas() {
        let runner = Runner::new();
        assert!(restore_backup(&runner, "invalido;tarea", "snap", None).is_err());
        assert!(restore_backup(&runner, "tarea1", "../escape", None).is_err());
        assert!(restore_backup(&runner, "tarea1", "/etc/shadow", None).is_err());
        assert!(restore_backup(&runner, "tarea1", "/srv/nas/SISTEMAS", None).is_err());
        assert!(restore_backup(
            &runner,
            "tarea1",
            "/srv/nas/BACKUPS_PERMANENTES/tarea1/FULL_1",
            Some("/etc")
        )
        .is_err());
        assert!(resolve_safe_restore_target("/tmp").is_err());
        assert!(resolve_safe_restore_target("/srv/nas/BACKUPS_HISTORICOS/otro").is_err());
    }

    #[cfg(unix)]
    #[test]
    fn restore_rechaza_origen_fuera_de_backups_y_symlinks_en_destino() {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let root =
            std::env::temp_dir().join(format!("nas-restore-sec-{}-{nanos}", std::process::id()));
        let repo = root.join("BACKUPS_HISTORICOS/demo");
        let snap = repo.join("snapshot_1");
        let outside = root.join("SISTEMAS");
        std::fs::create_dir_all(&snap).unwrap();
        std::fs::create_dir_all(&outside).unwrap();

        let info = TaskScriptInfo {
            dest_root: root
                .join("BACKUPS_HISTORICOS")
                .to_string_lossy()
                .to_string(),
            ..Default::default()
        };
        assert!(resolve_snapshot_dir("demo", "snapshot_1", &info).is_ok());
        assert!(resolve_snapshot_dir("demo", &outside.to_string_lossy(), &info).is_err());

        // Symlink en /tmp debe ser rechazado antes de crear subdirectorios.
        let link_name = format!("nas-restore-sym-{}-{nanos}", std::process::id());
        let link_path = std::env::temp_dir().join(&link_name);
        std::os::unix::fs::symlink(&outside, &link_path).unwrap();
        let target_attempt = format!("{}/sub", link_path.display());
        if target_attempt.starts_with("/tmp/") {
            assert!(resolve_safe_restore_target(&target_attempt).is_err());
        }
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_file(&link_path);
    }
}
