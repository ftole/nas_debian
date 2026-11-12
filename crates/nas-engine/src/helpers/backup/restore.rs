//! Restauración de snapshots y respaldos permanentes hacia el origen o un destino local.

use std::path::{Path, PathBuf};
use std::time::Duration;

use nas_core::{NasError, NasResult};
use nas_exec::{CommandSpec, Runner};

use crate::backup_control::valid_id;

use super::permanent::read_task_info;

/// Valida un destino de restauración: sin `..`, sin NUL y dentro de las raíces permitidas.
fn validate_restore_target(target: &str) -> NasResult<String> {
    let value = target.trim();
    if value.is_empty() || value.contains('\0') || value.contains("..") {
        return Err(NasError::Denied("destino de restauración inválido".into()));
    }
    if !value.starts_with("/srv/nas/") && !value.starts_with("/tmp/") {
        return Err(NasError::Denied(
            "el destino de restauración debe estar en /srv/nas o /tmp".into(),
        ));
    }
    Ok(value.to_string())
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
            let safe = validate_restore_target(custom_tgt)?;
            std::fs::create_dir_all(&safe).map_err(|e| NasError::Denied(e.to_string()))?;
            let canonical =
                std::fs::canonicalize(&safe).map_err(|e| NasError::Denied(e.to_string()))?;
            if !canonical.starts_with("/srv/nas") && !canonical.starts_with("/tmp") {
                return Err(NasError::Denied(
                    "el destino de restauración escapa de /srv/nas o /tmp".into(),
                ));
            }
            let rsync_res = runner.run(
                &CommandSpec::new("rsync")
                    .args([
                        "-aAXH",
                        "--numeric-ids",
                        "--safe-links",
                        &format!("{}/", snap_path.display()),
                        &canonical.to_string_lossy(),
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
                "Restauración completada en destino local: {}",
                canonical.display()
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
}
