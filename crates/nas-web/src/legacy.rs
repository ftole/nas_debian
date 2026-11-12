//! Proyecciones con la forma exacta que consume `app.js` (paridad con el panel PHP).
//!
//! El panel Rust puede añadir campos, pero nunca omitir los que el JavaScript
//! original lee. Estas funciones concentran esas conversiones para poder
//! probarlas sin sistema operativo.

mod backup;
mod status;
mod storage;

use nas_core::NasPaths;
use nas_exec::Runner;
use serde_json::{json, Value};

use crate::files::{Entry, FileRoot};

pub use backup::{backup_row, backup_row_with_metrics};
pub use status::classify_backup_status;
pub use storage::{
    calculate_destination_disk, calculate_dirs_combined_size_ex, calculate_single_dir_size_ex,
    calculate_task_size, calculate_task_size_ex, calculate_task_storage, task_storage_with_cache,
    BackupDiskMetrics, TaskStorageSummary, SIZE_CACHE_TTL_SECS,
};

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

/// Tamaño legible en unidades binarias.
pub fn human_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
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
    let mut crumbs = vec![json!({ "name": "Inicio", "path": "" })];
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
    use super::{breadcrumbs, cron_description, human_bytes, share_scheme};

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
    fn human_bytes_formatea_terabytes() {
        assert_eq!(human_bytes(2 * 1024 * 1024 * 1024 * 1024), "2.0 TB");
    }
}
