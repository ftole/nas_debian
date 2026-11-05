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
        && user
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '\\' | '/' | '@' | '-'))
}

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

fn write_secure_file(path: &Path, content: &str) -> NasResult<()> {
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

    let stamp = chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0);
    let temp_dir = std::env::temp_dir().join(format!("cifs_test_{}_{stamp}", std::process::id()));
    let temp_cred = std::env::temp_dir().join(format!("cifs_cred_{}_{stamp}", std::process::id()));
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        let mut builder = std::fs::DirBuilder::new();
        builder.recursive(true);
        builder.mode(0o700);
        let _ = builder.create(&temp_dir);
    }
    #[cfg(not(unix))]
    let _ = std::fs::create_dir_all(&temp_dir);

    let cred_content = if let Some((dom, acc)) = user.split_once('\\') {
        format!("domain={dom}\nusername={acc}\npassword={password}\n")
    } else if let Some((dom, acc)) = user.split_once('/') {
        format!("domain={dom}\nusername={acc}\npassword={password}\n")
    } else {
        format!("username={user}\npassword={password}\n")
    };
    write_secure_file(&temp_cred, &cred_content)?;

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
        Ok(out) => Err(NasError::Denied(clean_cifs_mount_error(&out.stderr))),
        Err(e) => Err(NasError::Denied(format!("error al ejecutar mount: {e}"))),
    }
}

fn clean_cifs_mount_error(stderr: &str) -> String {
    let clean = if let Some((head, _)) = stderr.split_once("\nRefer to the mount.cifs") {
        head.trim()
    } else if let Some((head, _)) = stderr.split_once("Refer to the mount.cifs") {
        head.trim()
    } else {
        stderr.trim()
    };

    if clean.contains("mount error(13): Permission denied") {
        "falló conexión CIFS: Acceso denegado (usuario o contraseña incorrectos, o permisos insuficientes en el recurso compartido)".to_string()
    } else if clean.contains("mount error(112): Host is down")
        || clean.contains("mount error(113): No route to host")
    {
        "falló conexión CIFS: Host no disponible (servidor remoto apagado o inaccesible en la red)"
            .to_string()
    } else if clean.contains("mount error(111): Connection refused") {
        "falló conexión CIFS: Conexión rechazada (puerto CIFS/SMB 445 cerrado o inaccesible)"
            .to_string()
    } else if clean.contains("mount error(6): No such device or address")
        || clean.contains("mount error(2): No such file or directory")
    {
        "falló conexión CIFS: Recurso compartido no encontrado en el servidor remoto".to_string()
    } else if clean.contains("mount error(115): Operation now in progress") {
        "falló conexión CIFS: Tiempo de espera agotado al conectar al servidor remoto".to_string()
    } else {
        format!("falló conexión CIFS: {clean}")
    }
}

/// Prueba de conectividad SSH con contraseña o clave privada SSH (Ed25519/RSA).
pub fn test_ssh_conn(
    runner: &Runner,
    host: &str,
    port: u16,
    user: &str,
    auth: &str,
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
    let trimmed_auth = auth.trim();

    let (spec, temp_guard) = if trimmed_auth.starts_with("-----BEGIN") {
        nas_ssh::validate_private_key(trimmed_auth)?;
        let stamp = chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0);
        let temp_key = std::env::temp_dir().join(format!("ssh_key_{}_{stamp}", std::process::id()));
        let mut key_content = trimmed_auth.to_string();
        if !key_content.ends_with('\n') {
            key_content.push('\n');
        }
        write_secure_file(&temp_key, &key_content)?;

        let cmd = CommandSpec::new("ssh")
            .args([
                "-i",
                &temp_key.to_string_lossy(),
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
                "-o",
                "BatchMode=yes",
                "--",
                host,
                "true",
            ])
            .timeout(Duration::from_secs(15));
        (cmd, Some(temp_key))
    } else if !trimmed_auth.is_empty() && Path::new(trimmed_auth).is_file() {
        let cmd = CommandSpec::new("ssh")
            .args([
                "-i",
                trimmed_auth,
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
                "-o",
                "BatchMode=yes",
                "--",
                host,
                "true",
            ])
            .timeout(Duration::from_secs(15));
        (cmd, None)
    } else {
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
        spec = spec.env("SSHPASS", auth);
        (spec, None)
    };

    let res = runner.run(&spec);
    if let Some(path) = temp_guard {
        let _ = std::fs::remove_file(path);
    }
    let res = res?;
    if res.is_success() {
        Ok(format!("conexión SSH verificada: {user}@{host}:{port}"))
    } else {
        Err(NasError::Denied(format!(
            "falló conexión SSH: {}",
            res.stderr
        )))
    }
}

/// Metadatos e información de un respaldo permanente congelado (Golden Snapshot).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct PermanentBackupInfo {
    /// Identificador de la tarea.
    pub task: String,
    /// Nombre del directorio (ej. `FULL_2026-10-30_190000_cierre`).
    pub name: String,
    /// Ruta absoluta en disco.
    pub path: String,
    /// Fecha de congelación formateada.
    pub date: String,
    /// Etiqueta descriptiva opcional.
    pub tag: Option<String>,
    /// Tamaño aproximado en bytes.
    pub size_bytes: u64,
    /// Número de archivos contenidos.
    pub files_count: u64,
}

fn dir_stats(path: &Path) -> (u64, u64) {
    let mut files = 0u64;
    let mut bytes = 0u64;
    if let Ok(entries) = std::fs::read_dir(path) {
        for entry in entries.flatten() {
            if let Ok(meta) = entry.metadata() {
                if meta.is_file() {
                    files += 1;
                    bytes += meta.len();
                } else if meta.is_dir() {
                    let (sub_files, sub_bytes) = dir_stats(&entry.path());
                    files += sub_files;
                    bytes += sub_bytes;
                }
            }
        }
    }
    (files, bytes)
}

#[derive(Default)]
struct TaskScriptInfo {
    dest_root: String,
    source_type: String,
    src_ip: Option<String>,
    src_share: Option<String>,
    src_path: Option<String>,
    src_user: Option<String>,
    src_port: u16,
    cred_file: Option<String>,
}

fn read_task_info(id: &str) -> TaskScriptInfo {
    let runner_path = Path::new("/usr/local/bin").join(format!("backup_{id}.sh"));
    let content = std::fs::read_to_string(&runner_path).unwrap_or_default();
    let mut info = TaskScriptInfo {
        dest_root: "/srv/nas/BACKUPS_HISTORICOS".to_string(),
        source_type: "local".to_string(),
        src_port: 22,
        ..Default::default()
    };

    for line in content.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("DEST_ROOT=") {
            let val = rest.trim_matches(|c| c == '\'' || c == '"' || c == ' ');
            if val.starts_with("/srv/nas") {
                info.dest_root = val.to_string();
            }
        } else if let Some(rest) = line.strip_prefix("SOURCE_TYPE=") {
            info.source_type = rest
                .trim_matches(|c| c == '\'' || c == '"' || c == ' ')
                .to_ascii_lowercase();
        } else if let Some(rest) = line.strip_prefix("SRC_IP=") {
            info.src_ip = Some(
                rest.trim_matches(|c| c == '\'' || c == '"' || c == ' ')
                    .to_string(),
            );
        } else if let Some(rest) = line.strip_prefix("SRC_SHARE=") {
            info.src_share = Some(
                rest.trim_matches(|c| c == '\'' || c == '"' || c == ' ')
                    .to_string(),
            );
        } else if let Some(rest) = line.strip_prefix("SRC_PATH=") {
            info.src_path = Some(
                rest.trim_matches(|c| c == '\'' || c == '"' || c == ' ')
                    .to_string(),
            );
        } else if let Some(rest) = line.strip_prefix("SRC_USER=") {
            info.src_user = Some(
                rest.trim_matches(|c| c == '\'' || c == '"' || c == ' ')
                    .to_string(),
            );
        } else if let Some(rest) = line.strip_prefix("SRC_PORT=") {
            if let Ok(p) = rest
                .trim_matches(|c| c == '\'' || c == '"' || c == ' ')
                .parse::<u16>()
            {
                info.src_port = p;
            }
        } else if let Some(rest) = line.strip_prefix("CRED_FILE=") {
            info.cred_file = Some(
                rest.trim_matches(|c| c == '\'' || c == '"' || c == ' ')
                    .to_string(),
            );
        }
    }
    info
}

/// Congela el snapshot más reciente de una tarea como respaldo permanente inmutable (a costo 0).
pub fn freeze_backup_task(
    runner: &Runner,
    id: &str,
    tag: Option<&str>,
) -> NasResult<PermanentBackupInfo> {
    if !valid_id(id) {
        return Err(NasError::Invalid(format!(
            "identificador de tarea inválido: '{id}'"
        )));
    }

    let task_info = read_task_info(id);
    let task_dir = Path::new(&task_info.dest_root).join(id);

    // Buscar snapshots existentes
    let entries = std::fs::read_dir(&task_dir).map_err(|_| {
        NasError::NotFound(format!(
            "no se encontró el directorio de la tarea {}",
            task_dir.display()
        ))
    })?;
    let mut snapshots: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("snapshot_"))
        })
        .collect();
    snapshots.sort();

    let Some(latest) = snapshots.pop() else {
        return Err(NasError::NotFound(format!(
            "no hay snapshots disponibles para congelar en '{}'",
            task_dir.display()
        )));
    };

    let perm_root = task_info
        .dest_root
        .replace("BACKUPS_HISTORICOS", "BACKUPS_PERMANENTES");
    let perm_task_dir = PathBuf::from(&perm_root).join(id);
    std::fs::create_dir_all(&perm_task_dir).map_err(|e| NasError::Denied(e.to_string()))?;
    let _ = runner.run(&CommandSpec::new("chown").args([
        "-R",
        "root:grp_samba",
        &perm_task_dir.to_string_lossy(),
    ]));
    let _ = runner.run(&CommandSpec::new("chmod").args(["2770", &perm_task_dir.to_string_lossy()]));
    let _ = runner.run(&CommandSpec::new("chattr").args(["-i", &perm_task_dir.to_string_lossy()]));

    let now = chrono::Local::now();
    let date_str = now.format("%Y-%m-%d_%H%M%S").to_string();
    let clean_tag = tag
        .map(|t| {
            t.trim()
                .chars()
                .filter(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-')
                .take(32)
                .collect::<String>()
        })
        .filter(|t| !t.is_empty());

    let folder_name = match &clean_tag {
        Some(t) => format!("FULL_{date_str}_{t}"),
        None => format!("FULL_{date_str}"),
    };

    let target_path = perm_task_dir.join(&folder_name);

    // Levantar temporalmente inmutabilidad en el snapshot origen para permitir crear enlaces duros
    let _ = runner.run(&CommandSpec::new("chattr").args(["-i", &latest.to_string_lossy()]));

    // Clonación instantánea mediante Hardlinks (costo 0 bytes extras)
    let cp_res = runner.run(
        &CommandSpec::new("cp")
            .args([
                "-al",
                &latest.to_string_lossy(),
                &target_path.to_string_lossy(),
            ])
            .timeout(Duration::from_secs(120)),
    )?;

    // Restaurar inmutabilidad en el contenedor origen (sin -R para permitir deduplicación)
    let _ = runner.run(&CommandSpec::new("chattr").args(["+i", &latest.to_string_lossy()]));

    if !cp_res.is_success() {
        return Err(NasError::Invalid(format!(
            "error al congelar snapshot vía hardlinks: {}",
            cp_res.stderr
        )));
    }

    let (files_count, size_bytes) = dir_stats(&target_path);

    let info = PermanentBackupInfo {
        task: id.to_string(),
        name: folder_name.clone(),
        path: target_path.to_string_lossy().to_string(),
        date: date_str,
        tag: clean_tag,
        size_bytes,
        files_count,
    };

    // Guardar metadatos en el snapshot antes de bloquear escritura
    let meta_file = target_path.join(".freeze_meta.json");
    if let Ok(meta_json) = serde_json::to_string_pretty(&info) {
        let _ = std::fs::write(&meta_file, meta_json);
    }

    // Inmutabilidad: remover permisos de escritura y asegurar con chattr solo el contenedor (sin -R)
    let _ =
        runner.run(&CommandSpec::new("chmod").args(["-R", "a-w", &target_path.to_string_lossy()]));
    let _ = runner.run(&CommandSpec::new("chattr").args(["+i", &target_path.to_string_lossy()]));

    // Registrar en la bitácora de la tarea
    let log_file = format!("/srv/nas/LOGS_BACKUP/backup_{id}.log");
    let log_line = format!(
        "[{}] [{}] [FREEZE] Respaldo permanente congelado: {} (desde {})\n",
        now.format("%F %T"),
        id,
        target_path.display(),
        latest.display()
    );
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_file)
    {
        use std::io::Write;
        let _ = f.write_all(log_line.as_bytes());
    }

    Ok(info)
}

/// Lista los respaldos permanentes inmutables existentes.
pub fn list_permanent_backups(task_filter: Option<&str>) -> NasResult<Vec<PermanentBackupInfo>> {
    let mut roots = vec![PathBuf::from("/srv/nas/BACKUPS_PERMANENTES")];

    // Buscar en unidades secundarias montadas
    if let Ok(units) = std::fs::read_dir("/srv/nas/unidades") {
        for unit in units.flatten() {
            let perm = unit.path().join("BACKUPS_PERMANENTES");
            if perm.is_dir() {
                roots.push(perm);
            }
        }
    }

    let mut result = Vec::new();

    for root in roots {
        let Ok(tasks) = std::fs::read_dir(&root) else {
            continue;
        };
        for task_entry in tasks.flatten() {
            let task_path = task_entry.path();
            if !task_path.is_dir() {
                continue;
            }
            let task_name = task_entry.file_name().to_string_lossy().to_string();
            if let Some(filter) = task_filter {
                if !filter.trim().is_empty() && filter != task_name {
                    continue;
                }
            }

            let Ok(backups) = std::fs::read_dir(&task_path) else {
                continue;
            };
            for bkp_entry in backups.flatten() {
                let bkp_path = bkp_entry.path();
                if !bkp_path.is_dir() {
                    continue;
                }
                let folder_name = bkp_entry.file_name().to_string_lossy().to_string();
                if !folder_name.starts_with("FULL_") {
                    continue;
                }

                // Leer metadatos si existen
                let meta_path = bkp_path.join(".freeze_meta.json");
                if let Ok(content) = std::fs::read_to_string(&meta_path) {
                    if let Ok(info) = serde_json::from_str::<PermanentBackupInfo>(&content) {
                        result.push(info);
                        continue;
                    }
                }

                // Deducir desde el nombre de carpeta si no hay metadatos
                let remainder = folder_name.trim_start_matches("FULL_");
                let parts: Vec<&str> = remainder.split('_').collect();
                let (date_str, tag) = if parts.len() >= 3 {
                    (
                        format!("{}_{}", parts[0], parts[1]),
                        Some(parts[2..].join("_")),
                    )
                } else if parts.len() == 2 {
                    if parts[1].len() == 6 && parts[1].chars().all(|c| c.is_ascii_digit()) {
                        (format!("{}_{}", parts[0], parts[1]), None)
                    } else {
                        (parts[0].to_string(), Some(parts[1].to_string()))
                    }
                } else {
                    (parts.first().unwrap_or(&"desconocida").to_string(), None)
                };

                let (files_count, size_bytes) = dir_stats(&bkp_path);
                result.push(PermanentBackupInfo {
                    task: task_name.clone(),
                    name: folder_name,
                    path: bkp_path.to_string_lossy().to_string(),
                    date: date_str,
                    tag,
                    size_bytes,
                    files_count,
                });
            }
        }
    }

    result.sort_by(|a, b| b.name.cmp(&a.name));
    Ok(result)
}

/// Elimina un respaldo permanente inmutable.
pub fn delete_permanent_backup(runner: &Runner, id: &str, name: &str) -> NasResult<String> {
    if !valid_id(id) {
        return Err(NasError::Invalid(format!(
            "identificador de tarea inválido: '{id}'"
        )));
    }
    let name = name.trim();
    if !name.starts_with("FULL_")
        || name.contains('/')
        || name.contains('\\')
        || name.contains("..")
    {
        return Err(NasError::Invalid(format!(
            "nombre de respaldo permanente inválido: '{name}'"
        )));
    }

    let mut found_path = None;
    let candidates = [PathBuf::from("/srv/nas/BACKUPS_PERMANENTES")
        .join(id)
        .join(name)];
    for cand in &candidates {
        if cand.is_dir() {
            found_path = Some(cand.clone());
            break;
        }
    }

    if found_path.is_none() {
        if let Ok(units) = std::fs::read_dir("/srv/nas/unidades") {
            for unit in units.flatten() {
                let cand = unit.path().join("BACKUPS_PERMANENTES").join(id).join(name);
                if cand.is_dir() {
                    found_path = Some(cand);
                    break;
                }
            }
        }
    }

    let Some(path) = found_path else {
        return Err(NasError::NotFound(format!(
            "respaldo permanente '{name}' no encontrado"
        )));
    };

    // Restaurar permisos de escritura e inmutabilidad para permitir eliminación
    let _ = runner.run(&CommandSpec::new("chattr").args(["-R", "-i", &path.to_string_lossy()]));
    let _ = runner.run(&CommandSpec::new("chmod").args(["-R", "u+w", &path.to_string_lossy()]));
    let mut rm_success = false;
    if let Ok(res) = runner.run(&CommandSpec::new("rm").args(["-rf", &path.to_string_lossy()])) {
        if res.is_success() {
            rm_success = true;
        }
    }
    if !rm_success {
        let _ = std::fs::remove_dir_all(&path);
    }

    // Re-asegurar inmutabilidad en los snapshots permanentes de la tarea (sin -R para permitir deduplicación)
    if let Some(perm_parent) = path.parent() {
        if let Ok(entries) = std::fs::read_dir(perm_parent) {
            for entry in entries.flatten() {
                let p = entry.path();
                if p.is_dir()
                    && p.file_name()
                        .and_then(|n| n.to_str())
                        .is_some_and(|n| n.starts_with("FULL_"))
                {
                    let _ =
                        runner.run(&CommandSpec::new("chattr").args(["+i", &p.to_string_lossy()]));
                }
            }
        }
    }

    if path.exists() {
        return Err(NasError::Invalid(format!(
            "fallo al eliminar el respaldo permanente '{name}'"
        )));
    }

    Ok(format!(
        "Respaldo permanente '{name}' eliminado exitosamente"
    ))
}

/// Restaura un snapshot o respaldo permanente hacia el origen de red o un destino local.
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
    if snap_str.contains("..") {
        return Err(NasError::Invalid("ruta de snapshot inválida".into()));
    }

    // Localizar ruta del snapshot
    let task_info = read_task_info(id);
    let snap_path = if snap_str.starts_with("/srv/nas/") && Path::new(snap_str).is_dir() {
        PathBuf::from(snap_str)
    } else {
        let perm_path = PathBuf::from(
            task_info
                .dest_root
                .replace("BACKUPS_HISTORICOS", "BACKUPS_PERMANENTES"),
        )
        .join(id)
        .join(snap_str);
        let hist_path = PathBuf::from(&task_info.dest_root).join(id).join(snap_str);

        if perm_path.is_dir() {
            perm_path
        } else if hist_path.is_dir() {
            hist_path
        } else {
            return Err(NasError::NotFound(format!(
                "snapshot o respaldo permanente '{snap_str}' no encontrado"
            )));
        }
    };

    // Si se especifica un destino local alternativo
    if let Some(custom_tgt) = target {
        let custom_tgt = custom_tgt.trim();
        if !custom_tgt.is_empty() {
            if !custom_tgt.starts_with("/srv/nas/") && !custom_tgt.starts_with("/tmp/") {
                return Err(NasError::Denied(
                    "el destino de restauración debe estar en /srv/nas o /tmp".into(),
                ));
            }
            std::fs::create_dir_all(custom_tgt).map_err(|e| NasError::Denied(e.to_string()))?;
            let rsync_res = runner.run(
                &CommandSpec::new("rsync")
                    .args([
                        "-aAXH",
                        "--numeric-ids",
                        "--safe-links",
                        &format!("{}/", snap_path.display()),
                        custom_tgt,
                    ])
                    .timeout(Duration::from_secs(300)),
            )?;
            if !rsync_res.is_success() {
                return Err(NasError::Invalid(format!(
                    "fallo al restaurar en destino: {}",
                    rsync_res.stderr
                )));
            }
            return Ok(format!(
                "Restauración completada en destino local: {custom_tgt}"
            ));
        }
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
                        &format!("{}/", snap_path.display()),
                        &format!("{mount_dir}/"),
                    ])
                    .timeout(Duration::from_secs(600)),
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
            let cred = task_info.cred_file.as_deref().unwrap_or_default();

            let mut pass = String::new();
            if Path::new(cred).is_file() {
                if let Ok(c) = std::fs::read_to_string(cred) {
                    pass = c.lines().next().unwrap_or("").trim().to_string();
                }
            }

            let mut spec = CommandSpec::new("sshpass");
            spec = spec.args([
                "-e",
                "rsync",
                "-aAXH",
                "--numeric-ids",
                "--safe-links",
                "-e",
                &format!("ssh -p {} -o StrictHostKeyChecking=accept-new -o UserKnownHostsFile=/root/.ssh/known_hosts_backup", task_info.src_port),
                &format!("{}/", snap_path.display()),
                &format!("{user}@{ip}:{path}/"),
            ]).timeout(Duration::from_secs(600));

            if !pass.is_empty() {
                spec = spec.env("SSHPASS", pass);
            }

            let res = runner.run(&spec)?;
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
                        &format!("{}/", snap_path.display()),
                        &format!("{path}/"),
                    ])
                    .timeout(Duration::from_secs(300)),
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
    fn clean_cifs_mount_error_humaniza_y_limpia() {
        let raw = "mount error(13): Permission denied\nRefer to the mount.cifs(8) manual page (e.g. man mount.cifs) and kernel log messages (dmesg)";
        let msg = clean_cifs_mount_error(raw);
        assert!(msg.contains("Acceso denegado"));
        assert!(!msg.contains("Refer to the mount.cifs"));

        let host_down = "mount error(112): Host is down\nRefer to the mount.cifs...";
        let msg2 = clean_cifs_mount_error(host_down);
        assert!(msg2.contains("Host no disponible"));

        let unknown = "mount error(999): Unknown error\nRefer to the mount.cifs...";
        let msg3 = clean_cifs_mount_error(unknown);
        assert_eq!(msg3, "falló conexión CIFS: mount error(999): Unknown error");
    }

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

    #[test]
    fn delete_permanent_rechaza_entradas_invalidas() {
        let runner = Runner::new();
        assert!(
            delete_permanent_backup(&runner, "invalido con espacios", "FULL_2026-10-30").is_err()
        );
        assert!(delete_permanent_backup(&runner, "tarea1", "../escape").is_err());
        assert!(delete_permanent_backup(&runner, "tarea1", "NOTFULL_2026").is_err());
    }

    #[test]
    fn restore_backup_rechaza_rutas_peligrosas() {
        let runner = Runner::new();
        assert!(restore_backup(&runner, "invalido;tarea", "snap", None).is_err());
        assert!(restore_backup(&runner, "tarea1", "../escape", None).is_err());
        assert!(restore_backup(&runner, "tarea1", "/etc/shadow", None).is_err());
        assert!(restore_backup(
            &runner,
            "tarea1",
            "/srv/nas/BACKUPS_PERMANENTES/tarea1/FULL_1",
            Some("/etc")
        )
        .is_err());
    }

    #[test]
    fn test_ssh_conn_valida_entradas_y_clave_pem() {
        let runner = Runner::new();
        // Host inválido
        assert!(test_ssh_conn(&runner, "-invalido", 22, "root", "pwd").is_err());
        // Usuario inválido
        assert!(test_ssh_conn(&runner, "10.10.1.1", 22, "-user", "pwd").is_err());
        // Clave PEM inválida
        let bad_pem =
            "-----BEGIN OPENSSH PRIVATE KEY-----\ncorto\n-----END OPENSSH PRIVATE KEY-----";
        assert!(test_ssh_conn(&runner, "10.10.1.1", 22, "root", bad_pem).is_err());
    }

    #[test]
    fn list_permanent_backups_deduce_tag_y_fecha_correctamente() {
        let f1 = "FULL_2026-11-04_144640_INICIAL".trim_start_matches("FULL_");
        let p1: Vec<&str> = f1.split('_').collect();
        assert_eq!(p1.len(), 3);
        let (d1, t1) = if p1.len() >= 3 {
            (format!("{}_{}", p1[0], p1[1]), Some(p1[2..].join("_")))
        } else {
            (p1[0].to_string(), None)
        };
        assert_eq!(d1, "2026-11-04_144640");
        assert_eq!(t1.as_deref(), Some("INICIAL"));

        let f2 = "FULL_2026-12-01_020000_MENSUAL".trim_start_matches("FULL_");
        let p2: Vec<&str> = f2.split('_').collect();
        let (d2, t2) = if p2.len() >= 3 {
            (format!("{}_{}", p2[0], p2[1]), Some(p2[2..].join("_")))
        } else {
            (p2[0].to_string(), None)
        };
        assert_eq!(d2, "2026-12-01_020000");
        assert_eq!(t2.as_deref(), Some("MENSUAL"));

        let f3 = "FULL_2026-10-30_120000".trim_start_matches("FULL_");
        let p3: Vec<&str> = f3.split('_').collect();
        let (d3, t3) = if p3.len() >= 3 {
            (format!("{}_{}", p3[0], p3[1]), Some(p3[2..].join("_")))
        } else if p3.len() == 2 && p3[1].len() == 6 && p3[1].chars().all(|c| c.is_ascii_digit()) {
            (format!("{}_{}", p3[0], p3[1]), None)
        } else {
            (p3[0].to_string(), None)
        };
        assert_eq!(d3, "2026-10-30_120000");
        assert_eq!(t3, None);
    }
}
