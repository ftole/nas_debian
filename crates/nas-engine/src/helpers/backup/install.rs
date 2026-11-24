//! Instalación atómica de runners, cron y credenciales de tareas de respaldo.

use std::path::{Path, PathBuf};
use std::time::Duration;

use nas_core::{NasError, NasResult};
use nas_exec::{CommandSpec, Runner};

use crate::backup_control::valid_id;
use crate::backup_spec::{BackupSpec, Proto};

use super::permanent::freeze_backup_task;

/// Instala una tarea de respaldo a partir de sus archivos en staging.
pub fn install_backup_task(runner: &Runner, id: &str) -> NasResult<String> {
    if !valid_id(id) {
        return Err(NasError::Invalid(format!(
            "identificador de tarea inválido: '{id}'"
        )));
    }

    let staging = Path::new("/var/lib/nas/staging");
    let runner_src = staging.join(format!("nas_backup_{id}.sh"));
    let cron_src = staging.join(format!("nas_backup_{id}.cron"));
    let cred_src = staging.join(format!("nas_backup_{id}.cred"));
    let key_src = staging.join(format!("nas_backup_{id}.id_ed25519"));

    if !runner_src.exists() {
        return Err(NasError::NotFound(format!(
            "falta runner preparado: {}",
            runner_src.display()
        )));
    }
    if !cron_src.exists() {
        return Err(NasError::NotFound(format!(
            "falta cron preparado: {}",
            cron_src.display()
        )));
    }

    // Validar sintaxis del runner con bash -n
    let check = runner.run(
        &CommandSpec::new("bash")
            .args(["-n", &runner_src.to_string_lossy()])
            .timeout(Duration::from_secs(15)),
    )?;
    if !check.is_success() {
        return Err(NasError::Invalid(format!(
            "runner con sintaxis inválida: {}",
            check.stderr
        )));
    }

    // Determinar destino
    let mut dest_root = "/srv/nas/BACKUPS_HISTORICOS".to_string();
    if let Ok(content) = std::fs::read_to_string(&runner_src) {
        for line in content.lines() {
            if let Some(rest) = line.strip_prefix("DEST_ROOT=") {
                let trimmed = rest.trim_matches(|c| c == '\'' || c == '"' || c == ' ');
                if trimmed.starts_with("/srv/nas/") && !trimmed.contains("..") {
                    dest_root = trimmed.to_string();
                    break;
                }
            }
        }
    }

    let _ = std::fs::create_dir_all("/usr/local/bin");
    let _ = std::fs::create_dir_all("/etc/cron.d");
    let _ = std::fs::create_dir_all("/etc/backup-credentials");
    let _ = std::fs::create_dir_all(&dest_root);
    let task_dest = format!("{dest_root}/{id}");
    let _ = std::fs::create_dir_all(&task_dest);

    let _ = runner.run(&CommandSpec::new("chown").args(["root:grp_samba", &dest_root, &task_dest]));
    let _ = runner.run(&CommandSpec::new("chmod").args(["2770", &dest_root, &task_dest]));

    let runner_dst = format!("/usr/local/bin/backup_{id}.sh");
    let cron_dst = format!("/etc/cron.d/backup_{id}");
    let cred_dst = format!("/etc/backup-credentials/{id}.cred");
    let key_dst = format!("/etc/backup-credentials/{id}.id_ed25519");

    std::fs::copy(&runner_src, &runner_dst).map_err(|e| NasError::Denied(e.to_string()))?;
    std::fs::copy(&cron_src, &cron_dst).map_err(|e| NasError::Denied(e.to_string()))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&runner_dst, std::fs::Permissions::from_mode(0o755));
        let _ = std::fs::set_permissions(&cron_dst, std::fs::Permissions::from_mode(0o644));
        let _ = std::fs::set_permissions(
            "/etc/backup-credentials",
            std::fs::Permissions::from_mode(0o700),
        );
    }

    if cred_src.exists() && cred_src.metadata().map(|m| m.len() > 0).unwrap_or(false) {
        std::fs::copy(&cred_src, &cred_dst).map_err(|e| NasError::Denied(e.to_string()))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&cred_dst, std::fs::Permissions::from_mode(0o600));
        }
    } else {
        let _ = std::fs::remove_file(&cred_dst);
    }

    if key_src.exists() && key_src.metadata().map(|m| m.len() > 0).unwrap_or(false) {
        std::fs::copy(&key_src, &key_dst).map_err(|e| NasError::Denied(e.to_string()))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&key_dst, std::fs::Permissions::from_mode(0o600));
        }
    } else {
        let _ = std::fs::remove_file(&key_dst);
    }

    let _ = std::fs::remove_file(&runner_src);
    let _ = std::fs::remove_file(&cron_src);
    let _ = std::fs::remove_file(&cred_src);
    let _ = std::fs::remove_file(&key_src);

    let perm_root = dest_root.replace("BACKUPS_HISTORICOS", "BACKUPS_PERMANENTES");
    let perm_task_dir = PathBuf::from(&perm_root).join(id);
    let _ = std::fs::create_dir_all(&perm_task_dir);
    let _ = runner
        .run(&CommandSpec::new("chown").args(["root:grp_samba", &perm_task_dir.to_string_lossy()]));
    let _ = runner.run(&CommandSpec::new("chmod").args(["2770", &perm_task_dir.to_string_lossy()]));

    let runner_content = std::fs::read_to_string(&runner_dst).unwrap_or_default();
    let auto_permanent = runner_content.contains("AUTO_PERMANENT=true");
    if auto_permanent {
        let has_perm = std::fs::read_dir(&perm_task_dir)
            .map(|entries| {
                entries.flatten().any(|e| {
                    e.path().is_dir()
                        && e.file_name()
                            .to_str()
                            .map(|name| name.starts_with("FULL_"))
                            .unwrap_or(false)
                })
            })
            .unwrap_or(false);

        if !has_perm {
            let has_snaps = std::fs::read_dir(&task_dest)
                .map(|entries| {
                    entries.flatten().any(|e| {
                        e.path().is_dir()
                            && e.file_name()
                                .to_str()
                                .map(|name| name.starts_with("snapshot_"))
                                .unwrap_or(false)
                    })
                })
                .unwrap_or(false);
            if has_snaps {
                let _ = freeze_backup_task(runner, id, Some("INICIAL"));
            }
        }
    }

    Ok(format!("tarea {id} instalada"))
}

fn write_temp_file_mode(tmp: &Path, content: &[u8], mode: u32) -> NasResult<()> {
    let _ = std::fs::remove_file(tmp);

    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(mode)
            .open(tmp)
            .map_err(|e| NasError::Denied(e.to_string()))?;
        if let Err(e) = file.write_all(content) {
            let _ = std::fs::remove_file(tmp);
            return Err(NasError::Denied(e.to_string()));
        }
        let _ = std::fs::set_permissions(tmp, std::fs::Permissions::from_mode(mode));
    }
    #[cfg(not(unix))]
    {
        let _ = mode;
        if let Err(e) = std::fs::write(tmp, content) {
            let _ = std::fs::remove_file(tmp);
            return Err(NasError::Denied(e.to_string()));
        }
    }
    Ok(())
}

fn write_atomic_mode(path: &Path, content: &[u8], mode: u32) -> NasResult<()> {
    let parent = path
        .parent()
        .ok_or_else(|| NasError::Invalid("ruta destino sin directorio padre".into()))?;
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| NasError::Invalid("nombre de archivo destino inválido".into()))?;
    let tmp = parent.join(format!(".{name}.tmp.{}", std::process::id()));

    write_temp_file_mode(&tmp, content, mode)?;

    if let Err(e) = std::fs::rename(&tmp, path) {
        let _ = std::fs::remove_file(&tmp);
        return Err(NasError::Denied(e.to_string()));
    }
    Ok(())
}

fn preserved_cifs_password(cred_path: &Path) -> Option<String> {
    let text = std::fs::read_to_string(cred_path).ok()?;
    text.lines()
        .find_map(|line| line.strip_prefix("password="))
        .map(str::to_string)
}

pub(super) fn install_backup_spec_with_dirs(
    runner: &Runner,
    spec: &BackupSpec,
    runner_dir: &Path,
    cron_dir: &Path,
    cred_dir: &Path,
    dest_root_override: Option<&Path>,
) -> NasResult<String> {
    let proto = spec.validate()?;
    let id = spec.id.trim();

    let runner_content = spec.render_runner(proto);
    let cron_content = spec.render_cron();
    let key_content = spec.render_key(proto);

    let is_prod = dest_root_override.is_none();
    let dest_root = dest_root_override
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from(spec.destination_root()));
    let task_dest = dest_root.join(id);

    std::fs::create_dir_all(runner_dir).map_err(|e| NasError::Denied(e.to_string()))?;
    std::fs::create_dir_all(cron_dir).map_err(|e| NasError::Denied(e.to_string()))?;
    std::fs::create_dir_all(cred_dir).map_err(|e| NasError::Denied(e.to_string()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(cred_dir, std::fs::Permissions::from_mode(0o700));
    }
    std::fs::create_dir_all(&task_dest).map_err(|e| NasError::Denied(e.to_string()))?;

    let runner_tmp = runner_dir.join(format!(".backup_{id}.sh.tmp.{}", std::process::id()));
    write_temp_file_mode(&runner_tmp, runner_content.as_bytes(), 0o600)?;
    let check = runner.run(
        &CommandSpec::new("bash")
            .args(["-n", &runner_tmp.to_string_lossy()])
            .timeout(Duration::from_secs(15)),
    );
    match check {
        Ok(out) if out.is_success() => {}
        Ok(out) => {
            let _ = std::fs::remove_file(&runner_tmp);
            return Err(NasError::Invalid(format!(
                "runner con sintaxis inválida: {}",
                out.stderr
            )));
        }
        Err(err) => {
            let _ = std::fs::remove_file(&runner_tmp);
            return Err(err);
        }
    }

    let runner_dst = runner_dir.join(format!("backup_{id}.sh"));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&runner_tmp, std::fs::Permissions::from_mode(0o755));
    }
    if let Err(e) = std::fs::rename(&runner_tmp, &runner_dst) {
        let _ = std::fs::remove_file(&runner_tmp);
        return Err(NasError::Denied(e.to_string()));
    }

    let cron_dst = cron_dir.join(format!("backup_{id}"));
    write_atomic_mode(&cron_dst, cron_content.as_bytes(), 0o644)?;

    let cred_dst = cred_dir.join(format!("{id}.cred"));
    let keep_existing_cred =
        spec.preserve_existing_auth && spec.password.is_empty() && cred_dst.exists();
    match proto {
        Proto::Cifs => {
            let effective_cred = if keep_existing_cred {
                if let Some(prev_pass) = preserved_cifs_password(&cred_dst) {
                    let mut cloned = spec.clone();
                    cloned.password = prev_pass;
                    Some(cloned.render_cred(Proto::Cifs))
                } else {
                    None
                }
            } else {
                Some(spec.render_cred(Proto::Cifs))
            };
            if let Some(content) = effective_cred.filter(|c| !c.is_empty()) {
                write_atomic_mode(&cred_dst, content.as_bytes(), 0o600)?;
            }
        }
        Proto::Ssh => {
            let cred_content = spec.render_cred(Proto::Ssh);
            if !cred_content.is_empty() {
                write_atomic_mode(&cred_dst, cred_content.as_bytes(), 0o600)?;
            } else if !keep_existing_cred {
                let _ = std::fs::remove_file(&cred_dst);
            }
        }
        Proto::Local => {
            let _ = std::fs::remove_file(&cred_dst);
        }
    }

    let key_dst = cred_dir.join(format!("{id}.id_ed25519"));
    let keep_existing_key = proto == Proto::Ssh
        && spec.preserve_existing_auth
        && spec.password.is_empty()
        && key_dst.exists();
    if let Some(key) = key_content.filter(|k| !k.is_empty()) {
        write_atomic_mode(&key_dst, key.as_bytes(), 0o600)?;
    } else if !keep_existing_key {
        let _ = std::fs::remove_file(&key_dst);
    }

    if is_prod {
        let dest_root_str = dest_root.to_string_lossy().to_string();
        let task_dest_str = task_dest.to_string_lossy().to_string();
        let _ = runner.run(&CommandSpec::new("chown").args([
            "root:grp_samba",
            &dest_root_str,
            &task_dest_str,
        ]));
        let _ =
            runner.run(&CommandSpec::new("chmod").args(["2770", &dest_root_str, &task_dest_str]));

        let perm_root = dest_root_str.replace("BACKUPS_HISTORICOS", "BACKUPS_PERMANENTES");
        let perm_task_dir = PathBuf::from(&perm_root).join(id);
        let _ = std::fs::create_dir_all(&perm_task_dir);
        let perm_str = perm_task_dir.to_string_lossy().to_string();
        let _ = runner.run(&CommandSpec::new("chown").args(["root:grp_samba", &perm_str]));
        let _ = runner.run(&CommandSpec::new("chmod").args(["2770", &perm_str]));

        if spec.auto_permanent {
            let has_perm = std::fs::read_dir(&perm_task_dir)
                .map(|entries| {
                    entries.flatten().any(|e| {
                        e.path().is_dir()
                            && e.file_name()
                                .to_str()
                                .is_some_and(|name| name.starts_with("FULL_"))
                    })
                })
                .unwrap_or(false);

            if !has_perm {
                let has_snaps = std::fs::read_dir(&task_dest)
                    .map(|entries| {
                        entries.flatten().any(|e| {
                            e.path().is_dir()
                                && e.file_name()
                                    .to_str()
                                    .is_some_and(|name| name.starts_with("snapshot_"))
                        })
                    })
                    .unwrap_or(false);
                if has_snaps {
                    let _ = freeze_backup_task(runner, id, Some("INICIAL"));
                }
            }
        }
    }

    Ok(format!("tarea {id} instalada"))
}

/// Valida e instala una tarea de respaldo generando runner, cron y credenciales como root.
pub fn install_backup_spec(runner: &Runner, spec: &BackupSpec) -> NasResult<String> {
    install_backup_spec_with_dirs(
        runner,
        spec,
        Path::new("/usr/local/bin"),
        Path::new("/etc/cron.d"),
        Path::new("/etc/backup-credentials"),
        None,
    )
}

/// Escribe o actualiza el cron de una tarea de respaldo.
pub fn write_cron(id: &str, file: Option<&Path>, content: Option<&str>) -> NasResult<String> {
    if !valid_id(id) {
        return Err(NasError::Invalid("identificador inválido".into()));
    }
    let cron_dst = format!("/etc/cron.d/backup_{id}");
    if let Some(src) = file {
        std::fs::copy(src, &cron_dst).map_err(|e| NasError::Denied(e.to_string()))?;
    } else if let Some(txt) = content {
        std::fs::write(&cron_dst, txt).map_err(|e| NasError::Denied(e.to_string()))?;
    } else {
        return Err(NasError::Invalid(
            "se requiere archivo o contenido para el cron".into(),
        ));
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&cron_dst, std::fs::Permissions::from_mode(0o644));
    }
    Ok(format!("cron {id} instalado"))
}

/// Elimina el cron de una tarea.
pub fn delete_cron(id: &str) -> NasResult<String> {
    if !valid_id(id) {
        return Err(NasError::Invalid("identificador inválido".into()));
    }
    let cron_path = format!("/etc/cron.d/backup_{id}");
    let _ = std::fs::remove_file(&cron_path);
    Ok(format!("cron {id} eliminado"))
}

pub(super) fn write_secure_file(path: &Path, content: &str) -> NasResult<()> {
    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(path)
            .map_err(|e| NasError::Denied(e.to_string()))?;
        f.write_all(content.as_bytes())
            .map_err(|e| NasError::Denied(e.to_string()))?;
        Ok(())
    }
    #[cfg(not(unix))]
    {
        std::fs::write(path, content).map_err(|e| NasError::Denied(e.to_string()))
    }
}

/// Escribe o actualiza el archivo de credenciales cifrado/protegido de una tarea.
pub fn write_cred(id: &str, file: Option<&Path>, content: Option<&str>) -> NasResult<String> {
    if !valid_id(id) {
        return Err(NasError::Invalid("identificador inválido".into()));
    }
    let _ = std::fs::create_dir_all("/etc/backup-credentials");
    let cred_dst = format!("/etc/backup-credentials/{id}.cred");
    if let Some(src) = file {
        let txt = std::fs::read_to_string(src).map_err(|e| NasError::Denied(e.to_string()))?;
        write_secure_file(Path::new(&cred_dst), &txt)?;
    } else if let Some(txt) = content {
        write_secure_file(Path::new(&cred_dst), txt)?;
    } else {
        return Err(NasError::Invalid(
            "se requiere archivo o contenido de credencial".into(),
        ));
    }
    Ok(format!("credencial {id} instalada"))
}

/// Elimina el archivo de credenciales de una tarea.
pub fn delete_cred(id: &str) -> NasResult<String> {
    if !valid_id(id) {
        return Err(NasError::Invalid("identificador inválido".into()));
    }
    let cred_path = format!("/etc/backup-credentials/{id}.cred");
    let _ = std::fs::remove_file(&cred_path);
    Ok(format!("credencial {id} eliminada"))
}
