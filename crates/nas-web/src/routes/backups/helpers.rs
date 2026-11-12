pub fn last_snapshot(task: &nas_engine::tasks::BackupTask) -> Option<String> {
    last_snapshot_path(task).map(|path| path.display().to_string())
}

pub fn last_snapshot_path(task: &nas_engine::tasks::BackupTask) -> Option<std::path::PathBuf> {
    let entries = std::fs::read_dir(task.task_dir()).ok()?;
    let mut snapshots: Vec<std::path::PathBuf> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("snapshot_"))
        })
        .collect();
    snapshots.sort();
    snapshots.pop()
}

pub fn run_backup_helper(args: &[&str], password: Option<&str>) -> Result<String, String> {
    crate::routes::common::run_nas_helper("backup-manage", args, password, 60)
        .map_err(|raw| clean_helper_error(&raw))
}

pub fn clean_helper_error(raw: &str) -> String {
    let trimmed = raw.trim();
    if let Ok(val) = serde_json::from_str::<serde_json::Value>(trimmed) {
        if let Some(msg) = val
            .get("message")
            .and_then(|v| v.as_str())
            .or_else(|| val.get("error").and_then(|v| v.as_str()))
        {
            return sanitize_error_message(msg);
        }
    }
    sanitize_error_message(trimmed)
}

pub fn sanitize_error_message(msg: &str) -> String {
    let clean = if let Some((head, _)) = msg.split_once("\nRefer to the mount.cifs") {
        head.trim()
    } else if let Some((head, _)) = msg.split_once("Refer to the mount.cifs") {
        head.trim()
    } else {
        msg.trim()
    };

    let clean = clean.strip_prefix("operación denegada: ").unwrap_or(clean);

    if clean.contains("mount error(13): Permission denied") {
        "Fallo de conexión CIFS: Acceso denegado (usuario o contraseña incorrectos, o permisos insuficientes en el recurso compartido).".to_string()
    } else if clean.contains("mount error(112): Host is down")
        || clean.contains("mount error(113): No route to host")
    {
        "Fallo de conexión CIFS: Host no disponible (servidor remoto apagado o fuera de red)."
            .to_string()
    } else if clean.contains("mount error(111): Connection refused") {
        "Fallo de conexión CIFS: Conexión rechazada (puerto CIFS/SMB 445 no disponible)."
            .to_string()
    } else if clean.contains("mount error(6): No such device or address")
        || clean.contains("mount error(2): No such file or directory")
    {
        "Fallo de conexión CIFS: Recurso compartido no encontrado en el servidor remoto."
            .to_string()
    } else if clean.contains("mount error(115): Operation now in progress") {
        "Fallo de conexión CIFS: Tiempo de espera agotado al conectar al servidor remoto."
            .to_string()
    } else {
        clean.to_string()
    }
}

pub fn stage_backup_files(
    spec: &nas_engine::backup_spec::BackupSpec,
    proto: nas_engine::backup_spec::Proto,
) -> Result<(), String> {
    let dir = std::path::Path::new("/var/lib/nas/staging");
    std::fs::create_dir_all(dir).map_err(|error| error.to_string())?;
    let id = spec.id.trim();

    std::fs::write(
        dir.join(format!("nas_backup_{id}.sh")),
        spec.render_runner(proto),
    )
    .map_err(|error| error.to_string())?;
    std::fs::write(
        dir.join(format!("nas_backup_{id}.cron")),
        spec.render_cron(),
    )
    .map_err(|error| error.to_string())?;
    let cred = spec.render_cred(proto);
    if !cred.is_empty() {
        std::fs::write(dir.join(format!("nas_backup_{id}.cred")), cred)
            .map_err(|error| error.to_string())?;
    } else {
        let _ = std::fs::remove_file(dir.join(format!("nas_backup_{id}.cred")));
    }
    if let Some(key_content) = spec.render_key(proto) {
        std::fs::write(dir.join(format!("nas_backup_{id}.id_ed25519")), key_content)
            .map_err(|error| error.to_string())?;
    } else {
        let _ = std::fs::remove_file(dir.join(format!("nas_backup_{id}.id_ed25519")));
    }
    Ok(())
}

pub fn backup_action_id(id: &str) -> Option<String> {
    nas_engine::backup_control::valid_id(id).then(|| id.trim().to_string())
}
