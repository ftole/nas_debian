//! Proyección JSON y progreso en tiempo real de tareas de respaldo.

use serde_json::{json, Value};

use super::{
    calculate_destination_disk, classify_backup_status, cron_description, human_bytes, task_cron,
    task_storage_with_cache, BackupDiskMetrics,
};

/// Proyección plana de una tarea de respaldo.
pub fn backup_row(task: &nas_engine::tasks::BackupTask) -> Value {
    let mut disk_cache = std::collections::HashMap::new();
    backup_row_with_metrics(task, &mut disk_cache)
}

/// Proyección plana de una tarea de respaldo reutilizando el cálculo de disco por destino.
pub fn backup_row_with_metrics(
    task: &nas_engine::tasks::BackupTask,
    disk_cache: &mut std::collections::HashMap<String, BackupDiskMetrics>,
) -> Value {
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

    let (percent, elapsed, remaining, speed) = if running {
        if let Some(prog) = read_backup_progress(id) {
            let spd = if prog.speed.is_empty() {
                "0.00kB/s".to_string()
            } else {
                prog.speed
            };
            let elap = if prog.elapsed.is_empty() {
                "00:00:00".to_string()
            } else {
                prog.elapsed
            };
            let rem = if prog.remaining.is_empty() {
                "--:--:--".to_string()
            } else {
                prog.remaining
            };
            (prog.percent.min(100), elap, rem, spd)
        } else {
            (
                0,
                "00:00:00".to_string(),
                "--:--:--".to_string(),
                "0.00kB/s".to_string(),
            )
        }
    } else if last_status == "OK" {
        (
            100,
            "00:00:00".to_string(),
            "00:00:00".to_string(),
            "0.00kB/s".to_string(),
        )
    } else {
        (
            0,
            "00:00:00".to_string(),
            "00:00:00".to_string(),
            "0.00kB/s".to_string(),
        )
    };

    let storage_summary = task_storage_with_cache(task);
    let full_size_bytes = storage_summary.full_size_bytes;
    let disk_usage_bytes = storage_summary.disk_usage_bytes;

    let full_size_formatted = if storage_summary.complete {
        human_bytes(full_size_bytes)
    } else {
        format!("≈ {}", human_bytes(full_size_bytes))
    };
    let disk_usage_formatted = if storage_summary.complete {
        human_bytes(disk_usage_bytes)
    } else {
        format!("≈ {}", human_bytes(disk_usage_bytes))
    };

    let size_formatted = full_size_formatted.clone();

    let disk_info = disk_cache
        .entry(task.destination_root.clone())
        .or_insert_with(|| calculate_destination_disk(&task.destination_root))
        .clone();

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
        "percent": percent,
        "elapsed": elapsed,
        "remaining": remaining,
        "speed": speed,
        "auto_permanent": task.auto_permanent,
        "size_bytes": full_size_bytes,
        "size_formatted": size_formatted.clone(),
        "task_size_bytes": full_size_bytes,
        "task_size_formatted": size_formatted,
        "full_size_bytes": full_size_bytes,
        "full_size_formatted": full_size_formatted,
        "disk_usage_bytes": disk_usage_bytes,
        "disk_usage_formatted": disk_usage_formatted,
        "disk_total_bytes": disk_info.total_bytes,
        "disk_used_bytes": disk_info.used_bytes,
        "disk_available_bytes": disk_info.available_bytes,
        "disk_used_percent": disk_info.used_percent,
        "disk_total_formatted": disk_info.total_formatted,
        "disk_used_formatted": disk_info.used_formatted,
        "disk_available_formatted": disk_info.available_formatted,
        "disk_summary": disk_info.summary,
    })
}

#[derive(Debug, Default, serde::Deserialize)]
struct BackupProgress {
    #[serde(default)]
    percent: u32,
    #[serde(default)]
    speed: String,
    #[serde(default)]
    elapsed: String,
    #[serde(default)]
    remaining: String,
}

fn read_backup_progress(id: &str) -> Option<BackupProgress> {
    for path in [
        format!("/run/nas-backup-{id}.progress"),
        format!("/tmp/nas-backup-{id}.progress"),
    ] {
        if let Ok(content) = std::fs::read_to_string(path) {
            if let Ok(prog) = serde_json::from_str::<BackupProgress>(&content) {
                return Some(prog);
            }
        }
    }
    None
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

#[cfg(test)]
mod tests {
    use super::backup_row;

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
            key_file: None,
            auto_permanent: true,
        };
        let row = backup_row(&task);
        assert_eq!(row["id"], "demo");
        assert_eq!(row["dest_path"], "/srv/nas/BACKUPS_HISTORICOS/demo");
        assert_eq!(row["source"], "//10.0.0.5/DATOS");
        assert_eq!(row["retention"], 7);
        assert_eq!(row["auto_permanent"], true);
        assert!(row["cron_desc"].is_string());
        assert_eq!(row["size_bytes"], 0);
        assert_eq!(row["size_formatted"], "0 B");
        assert_eq!(row["task_size_bytes"], 0);
        assert_eq!(row["task_size_formatted"], "0 B");
        assert!(row["disk_total_formatted"].is_string());
        assert!(row["disk_summary"].is_string());
    }
}
