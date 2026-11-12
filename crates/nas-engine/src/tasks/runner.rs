//! Ejecución y simulación tipada de tareas de respaldo con bloqueo y retención.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

use chrono::{DateTime, Local};
use nas_core::{NasError, NasResult};
use nas_exec::{CommandOutput, CommandSpec, Runner, SudoMode};
use serde::Serialize;

use crate::backup::{
    evaluate_space, parse_df, retention_victims, snapshot_dir_name, DfUsage, SpaceDecision,
};
use crate::lock::TaskLock;

use super::{BackupTask, SourceKind};

impl BackupTask {
    /// Plan de ejecución compuesto por comandos con argumentos separados.
    pub fn plan(&self, last_snapshot: Option<&Path>, now: &DateTime<Local>) -> Vec<Vec<String>> {
        let staging = self.staging_path(now);
        let task_dir = self.task_dir_unix();
        let mut steps: Vec<Vec<String>> = Vec::new();

        steps.push(vec![
            "mkdir".into(),
            "-p".into(),
            task_dir.clone(),
            self.log_dir.clone(),
        ]);

        match self.kind {
            SourceKind::Cifs => {
                let ip = self.source_ip.clone().unwrap_or_default();
                let share = self.share.clone().unwrap_or_default();
                let mount_point = format!("/mnt/backup_sources/{}", self.id);
                let credentials = self.credentials_file.clone().unwrap_or_default();

                steps.push(vec!["mkdir".into(), "-p".into(), mount_point.clone()]);
                steps.push(vec![
                    "mount".into(),
                    "-t".into(),
                    "cifs".into(),
                    format!("//{ip}/{share}"),
                    mount_point.clone(),
                    "-o".into(),
                    format!(
                        "credentials={credentials},ro,iocharset=utf8,vers=3.1.1,noserverino,cache=none,soft,nobrl"
                    ),
                ]);
                steps.push(self.rsync_step(&format!("{mount_point}/"), &staging, last_snapshot));
                steps.push(vec!["umount".into(), mount_point]);
            }
            SourceKind::Ssh => {
                let ip = self.source_ip.clone().unwrap_or_default();
                let user = self.user.clone().unwrap_or_default();
                let path = self.path.clone().unwrap_or_default();
                let port = self.port.unwrap_or(22);
                let default_key = format!("/etc/backup-credentials/{}.id_ed25519", self.id);
                let key_opt = self
                    .key_file
                    .as_deref()
                    .filter(|k| !k.is_empty())
                    .or_else(|| {
                        if Path::new(&default_key).is_file() {
                            Some(default_key.as_str())
                        } else {
                            None
                        }
                    });
                let (use_key, ssh_cmd) = match key_opt {
                    Some(key) => (true, nas_ssh::rsync_ssh_command(port, Some(Path::new(key)))),
                    None => (false, nas_ssh::rsync_ssh_command(port, None)),
                };

                let mut step = Vec::new();
                if !use_key {
                    step.push("sshpass".into());
                    if let Some(cred) = &self.credentials_file {
                        step.push("-f".into());
                        step.push(cred.clone());
                    } else {
                        step.push("-e".into());
                    }
                }
                step.extend([
                    "rsync".into(),
                    "-aAXH".into(),
                    "--numeric-ids".into(),
                    "--safe-links".into(),
                    "--timeout=300".into(),
                ]);
                if let Some(last) = last_snapshot {
                    step.push(format!("--link-dest={}", to_unix(last)));
                }
                step.extend([
                    "-e".into(),
                    ssh_cmd,
                    format!("{user}@{ip}:{}/", path.trim_end_matches('/')),
                    staging.clone(),
                ]);
                steps.push(step);
            }
            SourceKind::Local => {
                let path = self.path.clone().unwrap_or_default();
                steps.push(self.rsync_step(&format!("{path}/"), &staging, last_snapshot));
            }
        }

        steps.push(vec!["mv".into(), staging, self.snapshot_path(now)]);

        steps
    }

    fn rsync_step(
        &self,
        source: &str,
        destination: &str,
        last_snapshot: Option<&Path>,
    ) -> Vec<String> {
        let mut step = vec![
            "rsync".into(),
            "-aAXH".into(),
            "--numeric-ids".into(),
            "--safe-links".into(),
            "--timeout=300".into(),
        ];

        if let Some(last) = last_snapshot {
            step.push(format!("--link-dest={}", to_unix(last)));
        }

        step.push(source.to_string());
        step.push(destination.to_string());
        step
    }
}

/// Normaliza una ruta local a separador `/` para usarla en comandos remotos.
fn to_unix(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

/// Opciones de ejecución de una tarea de respaldo.
#[derive(Debug, Clone)]
pub struct RunOptions {
    /// Simula la ejecución sin tocar el sistema.
    pub dry_run: bool,
    /// Confirma la ejecución real.
    pub confirmed: bool,
    /// Eleva privilegios con `sudo -n`.
    pub sudo: bool,
    /// Directorio de bloqueos.
    pub lock_dir: PathBuf,
}

impl Default for RunOptions {
    fn default() -> Self {
        Self {
            dry_run: false,
            confirmed: false,
            sudo: false,
            lock_dir: PathBuf::from("/var/lock"),
        }
    }
}

/// Resultado de una ejecución (o simulación) de respaldo.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RunReport {
    /// Identificador de la tarea.
    pub task: String,
    /// Ruta del snapshot generado.
    pub snapshot: String,
    /// Snapshots eliminados por retención.
    pub removed: u32,
    /// Decisión de espacio aplicada.
    pub space: String,
    /// Pasos del plan (representación legible).
    pub steps: Vec<String>,
    /// Bitácora utilizada, si se ejecutó.
    pub log: Option<String>,
    /// Indica si fue una simulación.
    pub dry_run: bool,
}

/// Ejecuta (o simula) una tarea de respaldo con todas las guardas.
pub fn run(
    runner: &Runner,
    task: &BackupTask,
    options: &RunOptions,
    now: &DateTime<Local>,
) -> NasResult<RunReport> {
    let problems = task.validate();
    if !problems.is_empty() {
        return Err(NasError::Invalid(format!(
            "tarea '{}' inválida: {}",
            task.id,
            problems.join("; ")
        )));
    }

    let previous = last_snapshot(task);
    let plan = task.plan(previous.as_deref(), now);
    let snapshot = task.snapshot_path(now);

    let df_target = if task.task_dir().exists() {
        task.task_dir()
    } else {
        PathBuf::from(&task.destination_root)
    };
    let space = space_usage(runner, &df_target).map_or(SpaceDecision::Ok, |usage| {
        evaluate_space(usage.free_bytes, usage.used_percent)
    });

    if let SpaceDecision::Abort { reason } = &space {
        return Err(NasError::Denied(format!(
            "espacio insuficiente en {}: {reason}",
            df_target.display()
        )));
    }

    let mut projected = snapshot_names(task);
    projected.push(snapshot_dir_name(now));
    let victims = retention_victims(&projected, task.retention as usize);

    let steps: Vec<String> = plan
        .iter()
        .map(|step| nas_core::shell::quote_all(step))
        .collect();

    if options.dry_run {
        return Ok(RunReport {
            task: task.id.clone(),
            snapshot,
            removed: 0,
            space: format!("{space:?}"),
            steps,
            log: None,
            dry_run: true,
        });
    }

    if !options.confirmed {
        return Err(NasError::Denied(
            "la ejecución crea un snapshot real: se requiere confirmación explícita (--yes)".into(),
        ));
    }

    let lock_path = options.lock_dir.join(format!("backup_{}.lock", task.id));
    let _lock = TaskLock::acquire(&lock_path)?;

    let log_path = PathBuf::from(&task.log_dir).join(format!("backup_{}.log", task.id));
    append_log(
        &log_path,
        "INFO",
        &format!("INICIO ejecución de '{}'", task.id),
    );

    let mut failure: Option<NasError> = None;
    for step in &plan {
        let label = step.first().cloned().unwrap_or_default();
        append_log(
            &log_path,
            "INFO",
            &format!("ejecutando: {}", nas_core::shell::quote_all(step)),
        );

        let timeout = match label.as_str() {
            "mount" | "umount" | "mkdir" | "mv" => 120,
            _ => 3600,
        };

        if let Err(error) = run_step(runner, step, options.sudo, timeout) {
            failure = Some(error);
            break;
        }
    }

    if let Some(error) = failure {
        for step in plan
            .iter()
            .filter(|step| step.first().is_some_and(|arg| arg == "umount"))
        {
            let _ = run_step(runner, step, options.sudo, 120);
        }
        append_log(&log_path, "ERROR", &error.to_string());
        return Err(error);
    }

    let mut removed = 0_u32;
    for victim in &victims {
        let target = task.task_dir().join(victim);
        let mut spec = CommandSpec::new("rm")
            .args(["-rf".to_string(), target.to_string_lossy().to_string()])
            .timeout(Duration::from_secs(600));
        if options.sudo {
            spec = spec.sudo(SudoMode::Passwordless);
        }
        match runner.run(&spec) {
            Ok(output) if output.is_success() => {
                removed += 1;
                append_log(&log_path, "INFO", &format!("retención: eliminado {victim}"));
            }
            Ok(output) => append_log(
                &log_path,
                "WARN",
                &format!("retención: no se pudo eliminar {victim}: {}", output.stderr),
            ),
            Err(error) => append_log(&log_path, "WARN", &error.to_string()),
        }
    }

    append_log(
        &log_path,
        "INFO",
        &format!("FIN correcto: {snapshot} (retención aplicada: {removed})"),
    );

    Ok(RunReport {
        task: task.id.clone(),
        snapshot,
        removed,
        space: format!("{space:?}"),
        steps,
        log: Some(log_path.display().to_string()),
        dry_run: false,
    })
}

/// Último snapshot de la tarea, si existe.
pub fn last_snapshot(task: &BackupTask) -> Option<PathBuf> {
    let entries = std::fs::read_dir(task.task_dir()).ok()?;
    let mut snapshots: Vec<PathBuf> = entries
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

fn snapshot_names(task: &BackupTask) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(task.task_dir()) else {
        return Vec::new();
    };
    let mut names: Vec<String> = entries
        .flatten()
        .filter_map(|entry| entry.file_name().to_str().map(str::to_string))
        .filter(|name| name.starts_with("snapshot_"))
        .collect();
    names.sort();
    names
}

fn space_usage(runner: &Runner, target: &Path) -> Option<DfUsage> {
    let output = runner
        .run(
            &CommandSpec::new("df")
                .args(["-Pk".to_string(), target.to_string_lossy().to_string()])
                .timeout(Duration::from_secs(15)),
        )
        .ok()?;
    if !output.is_success() {
        return None;
    }
    parse_df(&output.stdout)
}

fn run_step(
    runner: &Runner,
    argv: &[String],
    sudo: bool,
    timeout_secs: u64,
) -> NasResult<CommandOutput> {
    let program = argv
        .first()
        .cloned()
        .ok_or_else(|| NasError::Invalid("paso del plan vacío".into()))?;
    let is_rsync = matches!(program.as_str(), "rsync" | "sshpass");

    let mut spec = CommandSpec::new(program)
        .args(argv[1..].to_vec())
        .timeout(Duration::from_secs(timeout_secs));
    if sudo {
        spec = spec.sudo(SudoMode::Passwordless);
    }

    let output = runner.run(&spec)?;
    if is_rsync && (output.code == 24 || output.code == 23) {
        return Ok(output);
    }
    output.ensure_success()
}

fn append_log(path: &Path, level: &str, message: &str) {
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let line = format!(
        "[{}] [{}] {}\n",
        Local::now().format("%Y-%m-%d %H:%M:%S"),
        level,
        message
    );
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    {
        let _ = file.write_all(line.as_bytes());
    }
}
