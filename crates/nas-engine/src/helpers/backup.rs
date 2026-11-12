//! Helpers para instalación, control y diagnóstico de tareas de respaldo.

use std::path::{Path, PathBuf};
use std::time::Duration;

use nas_core::{NasError, NasResult};
use nas_exec::{CommandSpec, Runner};

use crate::backup_control::valid_id;

mod conn;
mod permanent;
mod restore;

pub use conn::{test_cifs_conn, test_ssh_conn};
pub use permanent::{
    delete_permanent_backup, freeze_backup_task, list_permanent_backups, PermanentBackupInfo,
};
pub use restore::restore_backup;

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
    let _ = runner.run(&CommandSpec::new("chown").args([
        "-R",
        "root:grp_samba",
        &perm_task_dir.to_string_lossy(),
    ]));
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

/// Ejecuta manualmente una tarea de respaldo en segundo plano.
pub fn run_backup_task(runner: &Runner, id: &str) -> NasResult<String> {
    if !valid_id(id) {
        return Err(NasError::Invalid(format!(
            "identificador de tarea inválido: '{id}'"
        )));
    }
    let runner_path = format!("/usr/local/bin/backup_{id}.sh");
    if !Path::new(&runner_path).exists() {
        return Err(NasError::NotFound(format!(
            "no existe el runner {runner_path}"
        )));
    }

    // Verificar si ya está en ejecución
    if let Ok(out) = runner.run(
        &CommandSpec::new("pgrep")
            .args(["-f", &runner_path])
            .timeout(Duration::from_secs(10)),
    ) {
        if out.is_success() && !out.stdout.trim().is_empty() {
            return Ok(format!("la tarea {id} ya está en ejecución"));
        }
    }

    let has_bus = Path::new("/run/systemd/private").exists();
    if has_bus {
        let stamp = chrono::Local::now().timestamp();
        let unit = format!("nas-backup-{id}-{stamp}");
        let _ = runner.run(
            &CommandSpec::new("systemd-run")
                .args([
                    "--quiet",
                    "--no-block",
                    "--collect",
                    &format!("--unit={unit}"),
                    "--setenv=PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin",
                    &runner_path,
                ])
                .timeout(Duration::from_secs(30)),
        );
    } else {
        let _ = runner.run(
            &CommandSpec::new("nohup")
                .args([&runner_path])
                .timeout(Duration::from_secs(10)),
        );
    }

    Ok(format!("tarea {id} iniciada"))
}

/// Detiene una tarea de respaldo en ejecución.
pub fn stop_backup_task(runner: &Runner, id: &str) -> NasResult<String> {
    if !valid_id(id) {
        return Err(NasError::Invalid("identificador inválido".into()));
    }
    let pattern = format!("/usr/local/bin/backup_{id}\\.sh");

    // Detener unidades de systemd
    if let Ok(out) = runner.run(
        &CommandSpec::new("systemctl")
            .args([
                "list-units",
                "--type=service",
                "--state=running",
                "--no-legend",
                &format!("nas-backup-{id}-*.service"),
            ])
            .timeout(Duration::from_secs(15)),
    ) {
        if out.is_success() {
            for line in out.stdout.lines() {
                if let Some(unit) = line.split_whitespace().next() {
                    let _ = runner.run(&CommandSpec::new("systemctl").args(["stop", unit]));
                }
            }
        }
    }

    // Detener procesos remanentes
    let _ = runner.run(&CommandSpec::new("pkill").args(["-TERM", "-f", &pattern]));
    std::thread::sleep(Duration::from_millis(500));
    let _ = runner.run(&CommandSpec::new("pkill").args(["-KILL", "-f", &pattern]));
    let _ = std::fs::remove_file(format!("/run/nas-backup-{id}.progress"));
    let _ = std::fs::remove_file(format!("/tmp/nas-backup-{id}.progress"));

    // Registrar explícitamente en la bitácora que la tarea fue cancelada manualmente
    let log_path = format!("/srv/nas/LOGS_BACKUP/backup_{id}.log");
    let now = chrono::Local::now().format("%F %T");
    let cancel_msg = format!("[{now}] [{id}] === RESPALDO CANCELADO: la ejecución fue detenida manualmente por el usuario ===\n");
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
    {
        use std::io::Write;
        let _ = f.write_all(cancel_msg.as_bytes());
    }

    Ok(format!("tarea {id} cancelada"))
}

/// Devuelve las últimas N líneas de la bitácora de una tarea.
pub fn backup_logs(id: &str, lines: Option<usize>) -> NasResult<String> {
    if !valid_id(id) {
        return Err(NasError::Invalid("identificador inválido".into()));
    }
    let count = lines.unwrap_or(200).clamp(1, 500);
    let log_path = PathBuf::from(format!("/srv/nas/LOGS_BACKUP/backup_{id}.log"));
    if !log_path.exists() {
        return Ok(format!("(sin bitácora todavía para {id})"));
    }

    let content = std::fs::read_to_string(&log_path).unwrap_or_default();
    let all_lines: Vec<&str> = content.lines().collect();
    let take_count = all_lines.len().min(count);
    let slice = &all_lines[all_lines.len() - take_count..];
    Ok(slice.join("\n"))
}

/// Elimina una tarea de respaldo (conservando los snapshots existentes).
pub fn delete_backup_task(id: &str) -> NasResult<String> {
    if !valid_id(id) {
        return Err(NasError::Invalid("identificador inválido".into()));
    }
    let _ = std::fs::remove_file(format!("/etc/cron.d/backup_{id}"));
    let _ = std::fs::remove_file(format!("/usr/local/bin/backup_{id}.sh"));
    let _ = std::fs::remove_file(format!("/etc/backup-credentials/{id}.cred"));
    let _ = std::fs::remove_file(format!("/etc/backup-credentials/{id}.id_ed25519"));
    let _ = std::fs::remove_file(format!("/var/lock/backup_{id}.lock"));
    let _ = std::fs::remove_file(format!("/run/nas-backup-{id}.progress"));
    let _ = std::fs::remove_file(format!("/tmp/nas-backup-{id}.progress"));

    Ok(format!("tarea {id} eliminada (snapshots conservados)"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rechaza_id_invalido() {
        assert!(!valid_id("id con espacios"));
        assert!(!valid_id(""));
        assert!(!valid_id("../escapes"));
    }

    #[test]
    fn valida_id_correcto() {
        assert!(valid_id("bkp_servidor1"));
        assert!(valid_id("tarea-diaria-1"));
    }
}
