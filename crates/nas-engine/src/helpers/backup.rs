//! Helpers para instalación, control y diagnóstico de tareas de respaldo.

use std::path::{Path, PathBuf};
use std::time::Duration;

use nas_core::{NasError, NasResult};
use nas_exec::{CommandSpec, Runner};

use crate::backup_control::valid_id;

fn is_valid_host(host: &str) -> bool {
    let host = host.trim();
    !host.is_empty()
        && host.len() <= 253
        && !host.starts_with('-')
        && !host.contains("..")
        && host
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | ':' | '-'))
}

fn is_valid_port(port: u16) -> bool {
    port >= 1
}

fn is_valid_ssh_user(user: &str) -> bool {
    let user = user.trim();
    !user.is_empty()
        && user.len() <= 64
        && !user.starts_with('-')
        && user
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
}

fn is_valid_cifs_user(user: &str) -> bool {
    let user = user.trim();
    !user.is_empty()
        && user.len() <= 64
        && !user.starts_with('-')
        && user.chars().all(|c| {
            c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '\\' | '/' | '@' | '-')
        })
}

/// Instala una tarea de respaldo a partir de sus archivos en staging.
pub fn install_backup_task(runner: &Runner, id: &str) -> NasResult<String> {
    if !valid_id(id) {
        return Err(NasError::Invalid(format!("identificador de tarea inválido: '{id}'")));
    }

    let staging = Path::new("/var/lib/nas/staging");
    let runner_src = staging.join(format!("nas_backup_{id}.sh"));
    let cron_src = staging.join(format!("nas_backup_{id}.cron"));
    let cred_src = staging.join(format!("nas_backup_{id}.cred"));

    if !runner_src.exists() {
        return Err(NasError::NotFound(format!("falta runner preparado: {}", runner_src.display())));
    }
    if !cron_src.exists() {
        return Err(NasError::NotFound(format!("falta cron preparado: {}", cron_src.display())));
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

    std::fs::copy(&runner_src, &runner_dst).map_err(|e| NasError::Denied(e.to_string()))?;
    std::fs::copy(&cron_src, &cron_dst).map_err(|e| NasError::Denied(e.to_string()))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&runner_dst, std::fs::Permissions::from_mode(0o755));
        let _ = std::fs::set_permissions(&cron_dst, std::fs::Permissions::from_mode(0o644));
        let _ = std::fs::set_permissions("/etc/backup-credentials", std::fs::Permissions::from_mode(0o700));
    }

    if cred_src.exists() && cred_src.metadata().map(|m| m.len() > 0).unwrap_or(false) {
        std::fs::copy(&cred_src, &cred_dst).map_err(|e| NasError::Denied(e.to_string()))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&cred_dst, std::fs::Permissions::from_mode(0o600));
        }
    }

    let _ = std::fs::remove_file(&runner_src);
    let _ = std::fs::remove_file(&cron_src);
    let _ = std::fs::remove_file(&cred_src);

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
        return Err(NasError::Invalid("se requiere archivo o contenido para el cron".into()));
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

/// Escribe o actualiza el archivo de credenciales cifrado/protegido de una tarea.
pub fn write_cred(id: &str, file: Option<&Path>, content: Option<&str>) -> NasResult<String> {
    if !valid_id(id) {
        return Err(NasError::Invalid("identificador inválido".into()));
    }
    let _ = std::fs::create_dir_all("/etc/backup-credentials");
    let cred_dst = format!("/etc/backup-credentials/{id}.cred");
    if let Some(src) = file {
        std::fs::copy(src, &cred_dst).map_err(|e| NasError::Denied(e.to_string()))?;
    } else if let Some(txt) = content {
        std::fs::write(&cred_dst, txt).map_err(|e| NasError::Denied(e.to_string()))?;
    } else {
        return Err(NasError::Invalid("se requiere archivo o contenido de credencial".into()));
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&cred_dst, std::fs::Permissions::from_mode(0o600));
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
        return Err(NasError::Invalid(format!("identificador de tarea inválido: '{id}'")));
    }
    let runner_path = format!("/usr/local/bin/backup_{id}.sh");
    if !Path::new(&runner_path).exists() {
        return Err(NasError::NotFound(format!("no existe el runner {runner_path}")));
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
            .args(["list-units", "--type=service", "--state=running", "--no-legend", &format!("nas-backup-{id}-*.service")])
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

    Ok(format!("tarea {id} detenida"))
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
    let _ = std::fs::remove_file(format!("/var/lock/backup_{id}.lock"));

    Ok(format!("tarea {id} eliminada (snapshots conservados)"))
}

/// Prueba de conectividad CIFS.
pub fn test_cifs_conn(
    runner: &Runner,
    host: &str,
    share: &str,
    user: &str,
    password: &str,
) -> NasResult<String> {
    if !is_valid_host(host) {
        return Err(NasError::Invalid("host inválido".into()));
    }
    if share.is_empty() || share.contains('/') || share.contains('\\') {
        return Err(NasError::Invalid("nombre de recurso CIFS inválido".into()));
    }
    if !is_valid_cifs_user(user) {
        return Err(NasError::Invalid("usuario CIFS inválido".into()));
    }

    let temp_dir = std::env::temp_dir().join(format!("cifs_test_{}", std::process::id()));
    let temp_cred = std::env::temp_dir().join(format!("cifs_cred_{}", std::process::id()));
    let _ = std::fs::create_dir_all(&temp_dir);

    let cred_content = if let Some((dom, acc)) = user.split_once('\\') {
        format!("domain={dom}\nusername={acc}\npassword={password}\n")
    } else if let Some((dom, acc)) = user.split_once('/') {
        format!("domain={dom}\nusername={acc}\npassword={password}\n")
    } else {
        format!("username={user}\npassword={password}\n")
    };
    std::fs::write(&temp_cred, cred_content).map_err(|e| NasError::Denied(e.to_string()))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&temp_cred, std::fs::Permissions::from_mode(0o600));
    }

    let opts = format!(
        "credentials={},ro,iocharset=utf8,vers=3.1.1,noserverino,cache=none,soft,nobrl",
        temp_cred.display()
    );
    let target = format!("//{host}/{share}");
    let temp_dir_str = temp_dir.to_string_lossy().to_string();
    let mount_res = runner.run(
        &CommandSpec::new("mount")
            .args(["-t", "cifs", &target, &temp_dir_str, "-o", &opts])
            .timeout(Duration::from_secs(15)),
    );

    let _ = runner.run(&CommandSpec::new("umount").args([&temp_dir_str]));
    let _ = std::fs::remove_file(&temp_cred);
    let _ = std::fs::remove_dir(&temp_dir);

    match mount_res {
        Ok(out) if out.is_success() => Ok(format!("conexión CIFS verificada: {target}")),
        Ok(out) => Err(NasError::Denied(format!("falló conexión CIFS: {}", out.stderr))),
        Err(e) => Err(NasError::Denied(format!("error al ejecutar mount: {e}"))),
    }
}

/// Prueba de conectividad SSH.
pub fn test_ssh_conn(
    runner: &Runner,
    host: &str,
    port: u16,
    user: &str,
    password: &str,
) -> NasResult<String> {
    if !is_valid_host(host) {
        return Err(NasError::Invalid("host inválido".into()));
    }
    if !is_valid_port(port) {
        return Err(NasError::Invalid("puerto SSH inválido".into()));
    }
    if !is_valid_ssh_user(user) {
        return Err(NasError::Invalid("usuario SSH inválido".into()));
    }

    let _ = std::fs::create_dir_all("/root/.ssh");
    let mut spec = CommandSpec::new("sshpass")
        .args([
            "-e",
            "ssh",
            "-p",
            &port.to_string(),
            "-l",
            user,
            "-o",
            "StrictHostKeyChecking=accept-new",
            "-o",
            "UserKnownHostsFile=/root/.ssh/known_hosts_backup",
            "-o",
            "ConnectTimeout=10",
            "--",
            host,
            "true",
        ])
        .timeout(Duration::from_secs(15));
    spec = spec.env("SSHPASS", password);

    let res = runner.run(&spec)?;
    if res.is_success() {
        Ok(format!("conexión SSH verificada: {user}@{host}:{port}"))
    } else {
        Err(NasError::Denied(format!("falló conexión SSH: {}", res.stderr)))
    }
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
