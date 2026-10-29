//! Proyecciones con la forma exacta que consume `app.js` (paridad con el panel PHP).
//!
//! El panel Rust puede añadir campos, pero nunca omitir los que el JavaScript
//! original lee. Estas funciones concentran esas conversiones para poder
//! probarlas sin sistema operativo.

use nas_core::NasPaths;
use nas_exec::Runner;
use serde_json::{json, Value};

use crate::files::{Entry, FileRoot};

/// Etiquetas legibles de los servicios del panel.
pub const SERVICE_LABELS: [(&str, &str); 6] = [
    ("smbd", "Servidor de Archivos Samba"),
    ("nmbd", "Servicio de Nombres NetBIOS"),
    ("wsdd2", "Descubrimiento Web Services (WSD)"),
    ("nginx", "Servidor Web Nginx"),
    ("cron", "Planificador de Tareas Cron"),
    ("nas-web", "Panel Administrativo NAS (Rust/axum)"),
];

/// Estado de servicios con nombre legible y etiqueta.
pub fn service_rows(runner: &Runner) -> Vec<Value> {
    SERVICE_LABELS
        .iter()
        .map(|(unit, label)| {
            let active = runner
                .run(&nas_exec::CommandSpec::new("systemctl").args(["is-active", unit]))
                .map(|output| output.stdout == "active")
                .unwrap_or(false);
            json!({
                "service": unit,
                "name": label,
                "active": active,
                "status": if active { "activo" } else { "inactivo" },
            })
        })
        .collect()
}

/// Contadores de recursos, respaldos y usuarios.
pub fn dashboard_counts(paths: &NasPaths) -> Value {
    let shares = std::fs::read_to_string(&paths.samba_conf)
        .map(|text| nas_engine::parsers::smb::parse_shares(&text).len())
        .unwrap_or(0);
    let backups = nas_engine::tasks::BackupTask::discover("/usr/local/bin")
        .map(|tasks| tasks.len())
        .unwrap_or(0);
    let (users, _groups) = nas_engine::users::load_local();
    let users = nas_engine::users::managed_users(&users).len();
    json!({ "shares": shares, "backups": backups, "users": users })
}

/// Descripción legible de una expresión cron.
pub fn cron_description(expr: &str) -> String {
    let expr = expr.trim();
    match expr {
        "0 23 * * *" => "Diario a las 23:00".to_string(),
        "0 2 * * *" => "Diario a las 02:00".to_string(),
        "0 3 * * *" => "Diario a las 03:00".to_string(),
        "0 0 * * *" => "Diario a medianoche".to_string(),
        "0 2 * * 0" => "Semanal (domingo 02:00)".to_string(),
        "0 2 1 * *" => "Mensual (día 1, 02:00)".to_string(),
        other => other.to_string(),
    }
}

/// Entrada de cron instalada para una tarea (`/etc/cron.d/backup_<id>`).
pub fn task_cron(id: &str) -> String {
    std::fs::read_to_string(format!("/etc/cron.d/backup_{id}"))
        .ok()
        .and_then(|text| {
            text.lines()
                .map(str::trim)
                .find(|line| !line.is_empty() && !line.contains('=') && !line.starts_with('#'))
                .map(|line| {
                    line.split_whitespace()
                        .take(5)
                        .collect::<Vec<_>>()
                        .join(" ")
                })
        })
        .unwrap_or_else(|| "0 2 * * *".to_string())
}

/// Comprueba si existe físicamente al menos un snapshot en el disco de destino.
fn has_physical_snapshot(dest_root: &str, id: &str) -> bool {
    let dest_dir = std::path::Path::new(dest_root).join(id);
    let Ok(entries) = std::fs::read_dir(&dest_dir) else {
        return false;
    };
    entries.flatten().any(|e| {
        e.path().is_dir()
            && e.file_name()
                .to_str()
                .map(|name| name.starts_with("snapshot_"))
                .unwrap_or(false)
    })
}

/// Clasifica el estado de una tarea de respaldo evaluando hasta 50 líneas del log.
pub fn classify_backup_status(
    running: bool,
    log_content: Option<&str>,
    dest_root: &str,
    id: &str,
) -> &'static str {
    if running {
        return "En curso";
    }

    let Some(text) = log_content else {
        return "PENDIENTE";
    };

    let all_lines: Vec<&str> = text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    if all_lines.is_empty() {
        return "PENDIENTE";
    }

    let recent = if all_lines.len() > 50 {
        &all_lines[all_lines.len() - 50..]
    } else {
        &all_lines[..]
    };

    let mut last_error_idx: Option<usize> = None;
    let mut last_success_idx: Option<usize> = None;
    let mut last_start_idx: Option<usize> = None;

    for (idx, line) in recent.iter().enumerate() {
        let upper = line.to_uppercase();
        let mut line_is_error = false;

        if upper.contains("ERROR")
            || upper.contains("FALLO")
            || upper.contains("FAILED")
            || upper.contains("ABORTADO")
            || upper.contains("OMITIDO")
            || upper.contains("DESCARTA EL SNAPSHOT")
        {
            line_is_error = true;
        }

        for needle in [
            "CODIGO DE SALIDA ",
            "CÓDIGO DE SALIDA ",
            "CODIGO ",
            "CÓDIGO ",
        ] {
            if let Some(pos) = upper.find(needle) {
                let rest = &upper[pos + needle.len()..];
                let num_str: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
                if let Ok(code) = num_str.parse::<i32>() {
                    if code != 0 {
                        line_is_error = true;
                    }
                }
            }
        }

        if line_is_error {
            last_error_idx = Some(idx);
        }

        if upper.contains("INICIO (")
            || upper.contains("INICIANDO BACKUP")
            || upper.contains("=== BACKUP OMITIDO")
            || upper.contains("=== ABORTADO")
        {
            last_start_idx = Some(idx);
        }

        if upper.contains("SNAPSHOT COMPLETADO:")
            || upper.contains("BACKUP_COMPLETED")
            || upper.contains("BACKUP FINALIZADO CON ÉXITO")
            || upper.contains("BACKUP FINALIZADO CON EXITO")
        {
            last_success_idx = Some(idx);
        }
    }

    match last_success_idx {
        None => "Error",
        Some(succ_idx) => {
            if let Some(err_idx) = last_error_idx {
                if err_idx > succ_idx {
                    return "Error";
                }
            }
            if let Some(start_idx) = last_start_idx {
                if start_idx > succ_idx {
                    return "Error";
                }
            }
            if has_physical_snapshot(dest_root, id) {
                "OK"
            } else {
                "Error"
            }
        }
    }
}

/// Proyección plana de una tarea de respaldo.
pub fn backup_row(task: &nas_engine::tasks::BackupTask) -> Value {
    let id = &task.id;
    let cron = task_cron(id);
    let log_path = format!("{}/backup_{}.log", task.log_dir, id);
    let running = is_backup_task_running(id);
    let log_content = std::fs::read_to_string(&log_path).ok();

    let last_run = log_content
        .as_deref()
        .and_then(|text| {
            text.lines()
                .rev()
                .map(str::trim)
                .filter(|l| !l.is_empty())
                .find_map(|line| {
                    line.strip_prefix('[')
                        .and_then(|rest| rest.split(']').next())
                        .map(str::to_string)
                })
        })
        .unwrap_or_default();

    let last_status =
        classify_backup_status(running, log_content.as_deref(), &task.destination_root, id);

    let (protocol, source) = match task.kind {
        nas_engine::tasks::SourceKind::Cifs => (
            "CIFS (Windows)",
            format!(
                "//{}/{}",
                task.source_ip.clone().unwrap_or_default(),
                task.share.clone().unwrap_or_default()
            ),
        ),
        nas_engine::tasks::SourceKind::Ssh => (
            "SSH (Linux)",
            format!(
                "{}@{}:{}",
                task.user.clone().unwrap_or_default(),
                task.source_ip.clone().unwrap_or_default(),
                task.path.clone().unwrap_or_default()
            ),
        ),
        nas_engine::tasks::SourceKind::Local => ("LOCAL", task.path.clone().unwrap_or_default()),
    };

    let dest_unit = task
        .destination_root
        .strip_prefix("/srv/nas/unidades/")
        .and_then(|rest| rest.split('/').next())
        .filter(|unit| !unit.is_empty())
        .unwrap_or("principal");

    json!({
        "id": id,
        "protocol": protocol,
        "ip": task.source_ip.clone().unwrap_or_default(),
        "share": task.share.clone().unwrap_or_default(),
        "path": task.path.clone().unwrap_or_default(),
        "user": task.user.clone().unwrap_or_default(),
        "port": task.port.unwrap_or(22),
        "source": source,
        "destination": source,
        "dest_path": format!("{}/{}", task.destination_root, id),
        "dest_unit": dest_unit,
        "cron": cron,
        "cron_desc": cron_description(&cron),
        "retention": task.retention,
        "last_status": last_status,
        "last_run": last_run,
        "running": running,
        "percent": if running { 50 } else if last_status == "OK" { 100 } else { 0 },
        "elapsed": "00:00:00",
        "remaining": "00:00:00",
        "speed": "0.00kB/s",
    })
}

fn is_backup_task_running(id: &str) -> bool {
    let Ok(entries) = std::fs::read_dir("/proc") else {
        return false;
    };
    let needle = format!("backup_{id}.sh");
    entries.flatten().any(|entry| {
        let name = entry.file_name();
        let Some(pid) = name.to_str() else {
            return false;
        };
        if !pid.bytes().all(|b| b.is_ascii_digit()) {
            return false;
        }
        std::fs::read(entry.path().join("cmdline"))
            .map(|bytes| String::from_utf8_lossy(&bytes).contains(&needle))
            .unwrap_or(false)
    })
}

/// Tamaño legible en unidades binarias.
pub fn human_bytes(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["B", "KB", "MB", "GB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

/// Estructura de metadatos POSIX de un elemento (permisos, dueño y grupo).
#[cfg(unix)]
fn posix_info(path: &std::path::Path) -> (String, String, String) {
    use std::os::unix::fs::MetadataExt;
    match std::fs::metadata(path) {
        Ok(meta) => {
            let mode = format!("{:o}", meta.mode() & 0o7777);
            let owner = user_name(meta.uid());
            let group = group_name(meta.gid());
            (mode, owner, group)
        }
        Err(_) => ("644".to_string(), "root".to_string(), "root".to_string()),
    }
}

#[cfg(not(unix))]
fn posix_info(_path: &std::path::Path) -> (String, String, String) {
    ("644".to_string(), "root".to_string(), "root".to_string())
}

#[cfg(unix)]
fn user_name(uid: u32) -> String {
    nas_engine::users::load_local()
        .0
        .into_iter()
        .find(|user| user.uid == uid)
        .map(|user| user.name)
        .unwrap_or_else(|| uid.to_string())
}

#[cfg(unix)]
fn group_name(gid: u32) -> String {
    nas_engine::users::load_local()
        .1
        .into_iter()
        .find(|group| group.gid == gid)
        .map(|group| group.name)
        .unwrap_or_else(|| gid.to_string())
}

/// Elemento del explorador con los campos que espera `app.js`.
pub fn file_item(root: &FileRoot, relative: &str, entry: &Entry) -> Value {
    let absolute = root.root().join(&entry.path);
    let (permissions, owner, group) = posix_info(&absolute);
    let modified = chrono::DateTime::from_timestamp(entry.modified, 0)
        .map(|dt| dt.format("%Y-%m-%d %H:%M:%S").to_string())
        .unwrap_or_default();
    let _ = relative;
    json!({
        "name": entry.name,
        "path": entry.path,
        "relative_path": entry.path,
        "is_dir": entry.is_dir,
        "type": if entry.is_dir { "folder" } else { "file" },
        "size_bytes": entry.size,
        "size_formatted": human_bytes(entry.size),
        "mtime": modified,
        "modified_at": modified,
        "permissions": permissions,
        "owner": owner,
        "group": group,
        "is_snapshot": entry.name.starts_with("snapshot_"),
    })
}

/// Migas de pan acumulativas para la ruta actual.
pub fn breadcrumbs(relative: &str) -> Vec<Value> {
    let mut crumbs = vec![json!({ "name": "Raíz (/srv/nas)", "path": "" })];
    let mut accumulated = String::new();
    for part in relative.split('/').filter(|part| !part.is_empty()) {
        if accumulated.is_empty() {
            accumulated.push_str(part);
        } else {
            accumulated.push('/');
            accumulated.push_str(part);
        }
        crumbs.push(json!({ "name": part, "path": accumulated }));
    }
    crumbs
}

/// Payload de listado de archivos.
pub fn file_listing(root: &FileRoot, relative: &str, entries: &[Entry]) -> Value {
    json!({
        "current_path": relative,
        "breadcrumbs": breadcrumbs(relative),
        "items": entries
            .iter()
            .map(|entry| file_item(root, relative, entry))
            .collect::<Vec<_>>(),
        "total_items": entries.len(),
    })
}

/// Payload de la papelera.
pub fn trash_listing(entries: &[crate::files::TrashEntry]) -> Value {
    let total_size: u64 = entries.iter().map(|entry| entry.size).sum();
    json!({
        "items": entries,
        "total": entries.len(),
        "count": entries.len(),
        "total_size_bytes": total_size,
        "total_size_formatted": human_bytes(total_size),
        "total_size": total_size,
    })
}

/// Esquema de permisos (1..4) derivado de la configuración del recurso.
pub fn share_scheme(share: &Value) -> u8 {
    let guest = share
        .get("guest_ok")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let read_only = share
        .get("read_only")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let write_list = share
        .get("write_list")
        .and_then(Value::as_array)
        .map(|items| items.len())
        .unwrap_or(0);
    let valid_users = share
        .get("valid_users")
        .and_then(Value::as_array)
        .map(|items| items.len())
        .unwrap_or(0);

    if guest {
        4
    } else if read_only {
        3
    } else if write_list > 0 && valid_users > write_list {
        2
    } else {
        1
    }
}

#[cfg(test)]
mod tests {
    use super::{backup_row, breadcrumbs, cron_description, human_bytes, share_scheme};

    #[test]
    fn describe_cron_conocido() {
        assert_eq!(cron_description("0 23 * * *"), "Diario a las 23:00");
        assert_eq!(cron_description("5 4 * * *"), "5 4 * * *");
    }

    #[test]
    fn tamano_legible() {
        assert_eq!(human_bytes(0), "0 B");
        assert_eq!(human_bytes(2048), "2.0 KB");
    }

    #[test]
    fn migas_acumulativas() {
        let crumbs = breadcrumbs("A/B");
        assert_eq!(crumbs.len(), 3);
        assert_eq!(crumbs[2]["path"], "A/B");
    }

    #[test]
    fn esquema_derivado() {
        let guest = serde_json::json!({"guest_ok": true});
        assert_eq!(share_scheme(&guest), 4);
        let ro = serde_json::json!({"read_only": true});
        assert_eq!(share_scheme(&ro), 3);
        let rw = serde_json::json!({"read_only": false, "write_list": ["@a"], "valid_users": ["@a", "@b"]});
        assert_eq!(share_scheme(&rw), 2);
        let todo =
            serde_json::json!({"read_only": false, "write_list": ["@a"], "valid_users": ["@a"]});
        assert_eq!(share_scheme(&todo), 1);
    }

    #[test]
    fn fila_de_respaldo_plana() {
        let task = nas_engine::tasks::BackupTask {
            id: "demo".into(),
            kind: nas_engine::tasks::SourceKind::Cifs,
            source_ip: Some("10.0.0.5".into()),
            share: Some("DATOS".into()),
            path: None,
            user: Some("bk".into()),
            port: None,
            retention: 7,
            destination_root: "/srv/nas/BACKUPS_HISTORICOS".into(),
            log_dir: "/srv/nas/LOGS_BACKUP".into(),
            credentials_file: None,
        };
        let row = backup_row(&task);
        assert_eq!(row["id"], "demo");
        assert_eq!(row["dest_path"], "/srv/nas/BACKUPS_HISTORICOS/demo");
        assert_eq!(row["source"], "//10.0.0.5/DATOS");
        assert_eq!(row["retention"], 7);
        assert!(row["cron_desc"].is_string());
    }

    #[test]
    fn clasifica_estados_de_respaldo() {
        use super::classify_backup_status;
        assert_eq!(classify_backup_status(true, None, "/tmp", "t1"), "En curso");
        assert_eq!(
            classify_backup_status(false, None, "/tmp", "t1"),
            "PENDIENTE"
        );
        assert_eq!(
            classify_backup_status(false, Some(""), "/tmp", "t1"),
            "PENDIENTE"
        );
        assert_eq!(
            classify_backup_status(false, Some("   \n\n  "), "/tmp", "t1"),
            "PENDIENTE"
        );

        // Error explícito en log
        let err_log = "[2026-10-29 10:00:00] [t1] ERROR: se descarta el snapshot parcial (codigo de salida 1)";
        assert_eq!(
            classify_backup_status(false, Some(err_log), "/tmp", "t1"),
            "Error"
        );

        // Abortado
        let abort_log = "[2026-10-29 10:00:00] [t1] === ABORTADO: espacio libre insuficiente ===";
        assert_eq!(
            classify_backup_status(false, Some(abort_log), "/tmp", "t1"),
            "Error"
        );

        // Fallo de replicación rsync
        let rsync_err = "[2026-10-29 10:00:00] [t1] ERROR: en replicación rsync (código 12)";
        assert_eq!(
            classify_backup_status(false, Some(rsync_err), "/tmp", "t1"),
            "Error"
        );

        // Éxito afirmado pero sin directorio snapshot_* en disco -> Error
        let ok_log = "[2026-10-29 10:00:00] [t1] snapshot completado: /tmp/snapshot_2026";
        assert_eq!(
            classify_backup_status(false, Some(ok_log), "/tmp/nonexistent_backup_dir", "t1"),
            "Error"
        );

        // Éxito afirmado con snapshot físico en disco -> OK
        let tmp_root = std::env::temp_dir().join(format!("test_bkp_ok_{}", std::process::id()));
        let task_dir = tmp_root.join("t1");
        let snap_dir = task_dir.join("snapshot_2026-10-29_100000");
        std::fs::create_dir_all(&snap_dir).expect("snap_dir");
        assert_eq!(
            classify_backup_status(false, Some(ok_log), tmp_root.to_str().unwrap(), "t1"),
            "OK"
        );

        // Corrida previa con error seguida de corrida exitosa -> OK
        let multi_log = "[2026-10-28 10:00:00] [t1] ERROR: se descarta el snapshot parcial (codigo de salida 1)\n\
                         [2026-10-29 10:00:00] [t1] inicio (cifs): //server/share\n\
                         [2026-10-29 10:00:05] [t1] snapshot completado: /tmp/snapshot_2026";
        assert_eq!(
            classify_backup_status(false, Some(multi_log), tmp_root.to_str().unwrap(), "t1"),
            "OK"
        );

        // Corrida previa exitosa seguida de corrida con fallo -> Error
        let fail_after_ok = "[2026-10-28 10:00:05] [t1] snapshot completado: /tmp/snapshot_2026\n\
                             [2026-10-29 10:00:00] [t1] inicio (cifs): //server/share\n\
                             [2026-10-29 10:00:01] [t1] ERROR: se descarta el snapshot parcial (codigo de salida 32)";
        assert_eq!(
            classify_backup_status(false, Some(fail_after_ok), tmp_root.to_str().unwrap(), "t1"),
            "Error"
        );

        // Corrida previa exitosa seguida de nueva corrida interrumpida -> Error
        let interrupted_after_ok =
            "[2026-10-28 10:00:05] [t1] snapshot completado: /tmp/snapshot_2026\n\
                                    [2026-10-29 10:00:00] [t1] inicio (cifs): //server/share";
        assert_eq!(
            classify_backup_status(
                false,
                Some(interrupted_after_ok),
                tmp_root.to_str().unwrap(),
                "t1"
            ),
            "Error"
        );

        // Formato legado del servidor con BACKUP_COMPLETED
        let server_fmt_ok =
            "[2026-10-28 23:00:11] [t1] [BACKUP_COMPLETED] [notice] Respaldo finalizado con exito";
        assert_eq!(
            classify_backup_status(false, Some(server_fmt_ok), tmp_root.to_str().unwrap(), "t1"),
            "OK"
        );

        // Formato legado del servidor con MOUNT_FAILED / BACKUP_FAILED posterior
        let server_fmt_err = "[2026-10-27 23:00:15] [t1] [BACKUP_COMPLETED] [notice] Respaldo finalizado con exito\n\
                              mount error(115): could not connect to 10.2.0.10\n\
                              [2026-10-28 23:00:21] [t1] [MOUNT_FAILED] [err] Error al montar recurso CIFS\n\
                              === se descarta el snapshot parcial ===\n\
                              [2026-10-28 23:00:21] [t1] [BACKUP_FAILED] [err] Respaldo fallido";
        assert_eq!(
            classify_backup_status(
                false,
                Some(server_fmt_err),
                tmp_root.to_str().unwrap(),
                "t1"
            ),
            "Error"
        );

        let _ = std::fs::remove_dir_all(&tmp_root);
    }
}
