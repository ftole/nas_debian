//! Control de ejecución (inicio, detención, bitácoras y eliminación) de tareas de respaldo.

use std::path::{Path, PathBuf};
use std::time::Duration;

use nas_core::{NasError, NasResult};
use nas_exec::{CommandSpec, Runner};

use crate::backup_control::valid_id;

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
        let (mem_high_mb, mem_max_mb, cpu_quota_pct, tasks_max) =
            nas_core::ResourceBudget::detect().background_task_systemd_limits();
        let mem_high_arg = format!("MemoryHigh={mem_high_mb}M");
        let mem_max_arg = format!("MemoryMax={mem_max_mb}M");
        let cpu_arg = format!("CPUQuota={cpu_quota_pct}%");
        let tasks_arg = format!("TasksMax={tasks_max}");
        runner
            .run(
                &CommandSpec::new("systemd-run")
                    .args([
                        "--quiet",
                        "--no-block",
                        "--collect",
                        &format!("--unit={unit}"),
                        "-p",
                        "Nice=10",
                        "-p",
                        "IOSchedulingClass=best-effort",
                        "-p",
                        "IOSchedulingPriority=6",
                        "-p",
                        "IOWeight=20",
                        "-p",
                        &cpu_arg,
                        "-p",
                        &mem_high_arg,
                        "-p",
                        &mem_max_arg,
                        "-p",
                        &tasks_arg,
                        "--setenv=PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin",
                        &runner_path,
                    ])
                    .timeout(Duration::from_secs(30)),
            )?
            .ensure_success()?;
    } else {
        std::process::Command::new("nice")
            .args(["-n", "10", &runner_path])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .map_err(|e| NasError::Denied(format!("no se pudo iniciar {runner_path}: {e}")))?;
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
            let prefix = format!("nas-backup-{id}-");
            for line in out.stdout.lines() {
                if let Some(unit) = line.split_whitespace().next() {
                    let is_exact_task_unit = unit
                        .strip_prefix(&prefix)
                        .and_then(|rest| rest.strip_suffix(".service"))
                        .is_some_and(|stamp| {
                            !stamp.is_empty() && stamp.chars().all(|c| c.is_ascii_digit())
                        });
                    if is_exact_task_unit {
                        let _ = runner.run(&CommandSpec::new("systemctl").args(["stop", unit]));
                    }
                }
            }
        }
    }

    // Detener procesos remanentes esperando a que cleanup() desmonte y borre `.inprogress_*`
    let _ = runner.run(&CommandSpec::new("pkill").args(["-TERM", "-f", &pattern]));
    for _ in 0..60 {
        let alive = runner
            .run(
                &CommandSpec::new("pgrep")
                    .args(["-f", &pattern])
                    .timeout(Duration::from_secs(3)),
            )
            .is_ok_and(|out| out.is_success());
        if !alive {
            break;
        }
        std::thread::sleep(Duration::from_millis(250));
    }
    if runner
        .run(
            &CommandSpec::new("pgrep")
                .args(["-f", &pattern])
                .timeout(Duration::from_secs(3)),
        )
        .is_ok_and(|out| out.is_success())
    {
        let _ = runner.run(&CommandSpec::new("pkill").args(["-KILL", "-f", &pattern]));
    }

    // Limpiar posibles carpetas `.inprogress_*` huérfanas de esta tarea
    let mut dirs_to_clean = vec![PathBuf::from(format!("/srv/nas/BACKUPS_HISTORICOS/{id}"))];
    if let Ok(units) = std::fs::read_dir("/srv/nas/unidades") {
        for entry in units.flatten() {
            let candidate = entry.path().join("BACKUPS_HISTORICOS").join(id);
            if candidate.is_dir() {
                dirs_to_clean.push(candidate);
            }
        }
    }
    for dir in dirs_to_clean {
        crate::backup::clean_orphan_inprogress_dirs(&dir);
    }
    let _ = std::fs::remove_file(format!("/run/nas-backup-{id}.progress"));
    let _ = std::fs::remove_file(format!("/tmp/nas-backup-{id}.progress"));

    // Registrar explícitamente en la bitácora que la tarea fue cancelada manualmente
    let log_path = format!("/srv/nas/LOGS_BACKUP/backup_{id}.log");
    let now = chrono::Local::now().format("%F %T");
    let cancel_msg = format!(
        "[{now}] [{id}] === RESPALDO CANCELADO: la ejecución fue detenida manualmente por el usuario ===\n"
    );
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

    use std::io::{Read, Seek, SeekFrom};
    const MAX_TAIL_BYTES: u64 = 256 * 1024;
    let Ok(mut file) = std::fs::File::open(&log_path) else {
        return Ok(format!("(sin bitácora todavía para {id})"));
    };
    let len = file.metadata().map(|m| m.len()).unwrap_or(0);
    let mut start = len.saturating_sub(MAX_TAIL_BYTES);
    if start > 0 {
        let _ = file.seek(SeekFrom::Start(start));
    }
    let mut buf = Vec::new();
    let _ = file.read_to_end(&mut buf);
    if start > 0 {
        if let Some(pos) = buf.iter().position(|&b| b == b'\n') {
            start = pos as u64 + 1;
            buf.drain(..start as usize);
        }
    }
    let content = String::from_utf8_lossy(&buf);
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
