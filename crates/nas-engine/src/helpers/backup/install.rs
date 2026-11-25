//! Instalación atómica de runners, cron y credenciales de tareas de respaldo.

use std::path::{Path, PathBuf};
use std::time::Duration;

use nas_core::{NasError, NasResult};
use nas_exec::{CommandSpec, Runner};

use crate::backup_control::valid_id;
use crate::backup_spec::{BackupSpec, Proto};

use super::permanent::freeze_backup_task;

const MAX_RUNNER_BYTES: u64 = 256 * 1024;
const MAX_CRON_BYTES: u64 = 4 * 1024;
const MAX_CRED_BYTES: u64 = 16 * 1024;

/// Lee un archivo regular sin seguir enlaces simbólicos y exige que resida dentro
/// de alguno de los directorios permitidos.
fn read_confined_file(path: &Path, allowed_dirs: &[&Path], max_bytes: u64) -> NasResult<String> {
    let raw = path.to_string_lossy();
    if raw.is_empty()
        || raw.contains('\0')
        || path
            .components()
            .any(|c| matches!(c, std::path::Component::ParentDir))
    {
        return Err(NasError::Denied(format!(
            "ruta de origen inválida o con recorrido de directorios: {}",
            path.display()
        )));
    }

    let meta = std::fs::symlink_metadata(path).map_err(|_| {
        NasError::NotFound(format!(
            "archivo de origen no encontrado: {}",
            path.display()
        ))
    })?;
    if meta.file_type().is_symlink() || !meta.is_file() {
        return Err(NasError::Denied(format!(
            "el origen debe ser un archivo regular (sin enlaces simbólicos): {}",
            path.display()
        )));
    }
    if meta.len() > max_bytes {
        return Err(NasError::Invalid(format!(
            "archivo de origen demasiado grande (máximo {max_bytes} bytes): {}",
            path.display()
        )));
    }

    let canon = std::fs::canonicalize(path)
        .map_err(|e| NasError::Denied(format!("no se pudo resolver {}: {e}", path.display())))?;
    let confined = allowed_dirs.iter().any(|dir| {
        std::fs::canonicalize(dir)
            .ok()
            .is_some_and(|base| canon != base && canon.starts_with(&base))
    });
    if !confined {
        return Err(NasError::Denied(format!(
            "acceso denegado: {} está fuera de los directorios autorizados",
            path.display()
        )));
    }

    let content = std::fs::read_to_string(&canon).map_err(|e| NasError::Denied(e.to_string()))?;
    if content.contains('\0') {
        return Err(NasError::Invalid(
            "el archivo de origen contiene bytes nulos".into(),
        ));
    }
    Ok(content)
}

/// Valida que el contenido de un archivo `/etc/cron.d/backup_<id>` solo programe
/// el runner legítimo `/usr/local/bin/backup_<id>.sh` como `root`.
pub fn validate_cron_content(id: &str, content: &str) -> NasResult<()> {
    if !valid_id(id) {
        return Err(NasError::Invalid("identificador inválido".into()));
    }
    if content.len() as u64 > MAX_CRON_BYTES || content.contains('\0') || content.contains('\r') {
        return Err(NasError::Invalid(
            "contenido de cron inválido o excede el tamaño permitido".into(),
        ));
    }

    let expected_cmd = format!("/usr/local/bin/backup_{id}.sh");
    let mut job_lines = 0usize;

    for raw_line in content.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line == "SHELL=/bin/bash"
            || line == "SHELL=/bin/sh"
            || line == "MAILTO=\"\""
            || line == "MAILTO="
        {
            continue;
        }
        if let Some(path_val) = line.strip_prefix("PATH=") {
            if !path_val.is_empty()
                && path_val
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '/' | ':' | '_' | '-' | '.'))
            {
                continue;
            }
            return Err(NasError::Denied(
                "variable PATH inválida en archivo cron".into(),
            ));
        }

        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 7 {
            return Err(NasError::Denied(format!(
                "línea cron inválida o no autorizada: '{line}'"
            )));
        }
        let schedule_ok = parts[..5].iter().all(|field| {
            !field.is_empty()
                && field
                    .chars()
                    .all(|c| c.is_ascii_digit() || matches!(c, '*' | '/' | ',' | '-'))
        });
        if !schedule_ok {
            return Err(NasError::Denied(format!(
                "expresión de horario cron inválida: '{}'",
                parts[..5].join(" ")
            )));
        }
        if parts[5] != "root" {
            return Err(NasError::Denied(
                "el usuario de la tarea cron debe ser root".into(),
            ));
        }

        let cmd = parts[6..].join(" ");
        let allowed_cmd = cmd == expected_cmd
            || cmd == format!("{expected_cmd} >/dev/null 2>&1")
            || cmd == format!("{expected_cmd} > /dev/null 2>&1");
        if !allowed_cmd {
            return Err(NasError::Denied(format!(
                "comando cron no autorizado: solo se permite ejecutar {expected_cmd}"
            )));
        }
        job_lines += 1;
    }

    if job_lines == 0 {
        return Err(NasError::Invalid(
            "el archivo cron debe contener al menos una programación válida".into(),
        ));
    }
    Ok(())
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

    let runner_text = read_confined_file(&runner_src, &[staging], MAX_RUNNER_BYTES)?;
    let cron_text = read_confined_file(&cron_src, &[staging], MAX_CRON_BYTES)?;
    validate_cron_content(id, &cron_text)?;

    // Verificar que el runner corresponda a una tarea de respaldo válida y parseable.
    let parsed_task = crate::tasks::BackupTask::parse(id, &runner_text).map_err(|e| {
        NasError::Invalid(format!(
            "runner preparado inválido o sin metadatos de tarea de respaldo: {e}"
        ))
    })?;
    let task_errors = parsed_task.validate();
    if !task_errors.is_empty() {
        return Err(NasError::Invalid(format!(
            "runner preparado con configuración inválida: {}",
            task_errors.join("; ")
        )));
    }

    let cred_text = if std::fs::symlink_metadata(&cred_src).is_ok() {
        let txt = read_confined_file(&cred_src, &[staging], MAX_CRED_BYTES)?;
        (!txt.is_empty()).then_some(txt)
    } else {
        None
    };

    let key_text = if std::fs::symlink_metadata(&key_src).is_ok() {
        let txt = read_confined_file(&key_src, &[staging], MAX_CRED_BYTES)?;
        if txt.is_empty() {
            None
        } else {
            nas_ssh::validate_private_key(&txt)?;
            Some(txt)
        }
    } else {
        None
    };

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
    for line in runner_text.lines() {
        if let Some(rest) = line.strip_prefix("DEST_ROOT=") {
            let trimmed = rest.trim_matches(|c| c == '\'' || c == '"' || c == ' ');
            if trimmed.starts_with("/srv/nas/") && !trimmed.contains("..") {
                dest_root = trimmed.to_string();
                break;
            }
        }
    }

    let _ = std::fs::create_dir_all("/usr/local/bin");
    let _ = std::fs::create_dir_all("/etc/cron.d");
    let _ = std::fs::create_dir_all("/etc/backup-credentials");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(
            "/etc/backup-credentials",
            std::fs::Permissions::from_mode(0o700),
        );
    }
    let _ = std::fs::create_dir_all(&dest_root);
    let task_dest = format!("{dest_root}/{id}");
    let _ = std::fs::create_dir_all(&task_dest);

    let _ = runner.run(&CommandSpec::new("chown").args(["root:grp_samba", &dest_root, &task_dest]));
    let _ = runner.run(&CommandSpec::new("chmod").args(["2770", &dest_root, &task_dest]));

    let runner_dst = PathBuf::from(format!("/usr/local/bin/backup_{id}.sh"));
    let cron_dst = PathBuf::from(format!("/etc/cron.d/backup_{id}"));
    let cred_dst = PathBuf::from(format!("/etc/backup-credentials/{id}.cred"));
    let key_dst = PathBuf::from(format!("/etc/backup-credentials/{id}.id_ed25519"));

    write_atomic_mode(&runner_dst, runner_text.as_bytes(), 0o755)?;
    write_atomic_mode(&cron_dst, cron_text.as_bytes(), 0o644)?;

    if let Some(txt) = cred_text {
        write_atomic_mode(&cred_dst, txt.as_bytes(), 0o600)?;
    } else {
        let _ = std::fs::remove_file(&cred_dst);
    }

    if let Some(txt) = key_text {
        write_atomic_mode(&key_dst, txt.as_bytes(), 0o600)?;
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

    let auto_permanent = runner_text.contains("AUTO_PERMANENT=true");
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

/// Escribe o actualiza el cron de una tarea de respaldo validando estrictamente
/// el origen (solo `/var/lib/nas/staging` sin symlinks) y que el comando invoque
/// únicamente `/usr/local/bin/backup_<id>.sh`.
pub fn write_cron(id: &str, file: Option<&Path>, content: Option<&str>) -> NasResult<String> {
    if !valid_id(id) {
        return Err(NasError::Invalid("identificador inválido".into()));
    }
    let txt = if let Some(src) = file {
        read_confined_file(src, &[Path::new("/var/lib/nas/staging")], MAX_CRON_BYTES)?
    } else if let Some(raw) = content {
        raw.to_string()
    } else {
        return Err(NasError::Invalid(
            "se requiere archivo o contenido para el cron".into(),
        ));
    };

    validate_cron_content(id, &txt)?;

    let cron_dst = PathBuf::from(format!("/etc/cron.d/backup_{id}"));
    write_atomic_mode(&cron_dst, txt.as_bytes(), 0o644)?;
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
    write_atomic_mode(path, content.as_bytes(), 0o600)
}

/// Escribe o actualiza el archivo de credenciales cifrado/protegido de una tarea.
/// Si recibe `--file`, solo admite archivos regulares (sin symlinks) dentro de
/// `/var/lib/nas/staging` o `/etc/backup-credentials`.
pub fn write_cred(id: &str, file: Option<&Path>, content: Option<&str>) -> NasResult<String> {
    if !valid_id(id) {
        return Err(NasError::Invalid("identificador inválido".into()));
    }
    let txt = if let Some(src) = file {
        read_confined_file(
            src,
            &[
                Path::new("/var/lib/nas/staging"),
                Path::new("/etc/backup-credentials"),
            ],
            MAX_CRED_BYTES,
        )?
    } else if let Some(raw) = content {
        if raw.len() as u64 > MAX_CRED_BYTES || raw.contains('\0') {
            return Err(NasError::Invalid(
                "contenido de credencial inválido o demasiado grande".into(),
            ));
        }
        raw.to_string()
    } else {
        return Err(NasError::Invalid(
            "se requiere archivo o contenido de credencial".into(),
        ));
    };

    let _ = std::fs::create_dir_all("/etc/backup-credentials");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(
            "/etc/backup-credentials",
            std::fs::Permissions::from_mode(0o700),
        );
    }
    let cred_dst = format!("/etc/backup-credentials/{id}.cred");
    write_secure_file(Path::new(&cred_dst), &txt)?;
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

#[cfg(test)]
mod security_tests {
    use super::*;

    #[test]
    fn validate_cron_content_acepta_plantilla_oficial_y_rechaza_comandos_arbitrarios() {
        let valid = "SHELL=/bin/bash\nPATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin\nMAILTO=\"\"\n0 2 * * * root /usr/local/bin/backup_demo.sh\n";
        assert!(validate_cron_content("demo", valid).is_ok());

        let valid_devnull = "30 3 * * 1-5 root /usr/local/bin/backup_demo.sh >/dev/null 2>&1\n";
        assert!(validate_cron_content("demo", valid_devnull).is_ok());

        for malicious in [
            "* * * * * root /bin/sh -c 'id > /tmp/pwn'\n",
            "0 2 * * * root /usr/local/bin/backup_demo.sh; id\n",
            "0 2 * * * root /usr/local/bin/backup_demo.sh && touch /tmp/x\n",
            "0 2 * * * root /usr/local/bin/backup_otra.sh\n",
            "0 2 * * * nobody /usr/local/bin/backup_demo.sh\n",
            "PATH=/tmp:$(id)\n0 2 * * * root /usr/local/bin/backup_demo.sh\n",
            "# solo comentarios sin tarea\n",
        ] {
            assert!(
                validate_cron_content("demo", malicious).is_err(),
                "debe rechazar cron malicioso: {malicious}"
            );
        }
    }

    #[test]
    fn write_cron_y_write_cred_rechazan_archivos_fuera_de_staging() {
        assert!(write_cron("demo", Some(Path::new("/etc/shadow")), None).is_err());
        assert!(write_cron("demo", Some(Path::new("/etc/passwd")), None).is_err());
        assert!(write_cred("demo", Some(Path::new("/etc/shadow")), None).is_err());
        assert!(write_cred(
            "demo",
            Some(Path::new("/var/lib/nas/staging/../etc/passwd")),
            None
        )
        .is_err());
        assert!(write_cron("demo", None, Some("* * * * * root /bin/bash -c id\n")).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn read_confined_file_rechaza_symlinks_y_rutas_fuera_del_directorio() {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let root =
            std::env::temp_dir().join(format!("nas-install-sec-{}-{nanos}", std::process::id()));
        let staging = root.join("staging");
        let outside = root.join("outside");
        std::fs::create_dir_all(&staging).unwrap();
        std::fs::create_dir_all(&outside).unwrap();

        let ok_file = staging.join("ok.cron");
        let secret = outside.join("secret.txt");
        let link = staging.join("link.cron");
        std::fs::write(&ok_file, "0 2 * * * root /usr/local/bin/backup_demo.sh\n").unwrap();
        std::fs::write(&secret, "secreto").unwrap();
        std::os::unix::fs::symlink(&secret, &link).unwrap();

        assert!(read_confined_file(&ok_file, &[&staging], MAX_CRON_BYTES).is_ok());
        assert!(read_confined_file(&secret, &[&staging], MAX_CRON_BYTES).is_err());
        assert!(read_confined_file(&link, &[&staging], MAX_CRON_BYTES).is_err());

        let _ = std::fs::remove_dir_all(&root);
    }
}
