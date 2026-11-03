//! Modelo de tareas de respaldo y generación de su plan de ejecución.
//!
//! Lee los runners existentes (`/usr/local/bin/backup_<tarea>.sh`) para mantener
//! compatibilidad total con las tareas ya creadas por el asistente Bash, y
//! deriva de ellos un plan de comandos tipado y verificable.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

use chrono::{DateTime, Local};
use nas_core::{NasError, NasResult};
use nas_exec::{CommandOutput, CommandSpec, Runner, SudoMode};
use serde::Serialize;

use crate::backup::{
    evaluate_space, parse_df, retention_victims, snapshot_dir_name, staging_dir_name, DfUsage,
    SpaceDecision,
};
use crate::lock::TaskLock;

/// Origen de una tarea de respaldo.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SourceKind {
    /// Recurso compartido Windows vía CIFS/SMB.
    Cifs,
    /// Servidor Linux remoto vía SSH.
    Ssh,
    /// Ruta local del propio servidor.
    Local,
}

impl SourceKind {
    /// Etiqueta legible.
    pub fn label(self) -> &'static str {
        match self {
            Self::Cifs => "CIFS/SMB",
            Self::Ssh => "SSH",
            Self::Local => "LOCAL",
        }
    }
}

/// Tarea de respaldo normalizada.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BackupTask {
    /// Identificador (nombre del runner sin prefijo `backup_` ni extensión).
    pub id: String,
    /// Tipo de origen.
    pub kind: SourceKind,
    /// IP o nombre del servidor origen.
    pub source_ip: Option<String>,
    /// Recurso compartido (CIFS).
    pub share: Option<String>,
    /// Ruta remota o local de origen.
    pub path: Option<String>,
    /// Usuario del origen.
    pub user: Option<String>,
    /// Puerto SSH.
    pub port: Option<u16>,
    /// Número de snapshots conservados.
    pub retention: u32,
    /// Raíz del repositorio histórico.
    pub destination_root: String,
    /// Directorio de bitácoras.
    pub log_dir: String,
    /// Archivo de credenciales CIFS (0600 root:root).
    pub credentials_file: Option<String>,
    /// Archivo de clave privada SSH (0600 root:root).
    pub key_file: Option<String>,
}

const DEFAULT_DESTINATION: &str = "/srv/nas/BACKUPS_HISTORICOS";
const DEFAULT_LOG_DIR: &str = "/srv/nas/LOGS_BACKUP";
const DEFAULT_RETENTION: u32 = 30;

/// Extrae `CLAVE=valor` tolerando comillas simples o dobles.
fn extract(text: &str, key: &str) -> Option<String> {
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('#') {
            continue;
        }
        let Some((name, raw_value)) = line.split_once('=') else {
            continue;
        };
        if name.trim() != key {
            continue;
        }

        let value = raw_value.trim();
        let unquoted = if let Some(inner) = value
            .strip_prefix('"')
            .and_then(|rest| rest.strip_suffix('"'))
        {
            inner.trim().to_string()
        } else if let Some(inner) = value
            .strip_prefix('\'')
            .and_then(|rest| rest.strip_suffix('\''))
        {
            inner.replace("'\\''", "'").trim().to_string()
        } else {
            value.to_string()
        };

        if unquoted.is_empty() || unquoted.contains("PLACEHOLDER") || unquoted.starts_with('$') {
            return None;
        }

        return Some(unquoted);
    }

    None
}

impl BackupTask {
    /// Interpreta un runner de respaldo y construye la tarea normalizada.
    pub fn parse(id: &str, script: &str) -> NasResult<Self> {
        if id.trim().is_empty() {
            return Err(NasError::Invalid("identificador de tarea vacío".into()));
        }

        let source_ip = extract(script, "SRC_IP");
        let share = extract(script, "SRC_SHARE");
        let path = extract(script, "SRC_PATH");
        let user = extract(script, "SRC_USER");
        let port = extract(script, "SRC_PORT").and_then(|value| value.parse::<u16>().ok());

        let declared = extract(script, "SOURCE_TYPE").or_else(|| extract(script, "SRC_TYPE"));
        let kind = match declared.as_deref().map(str::to_ascii_lowercase).as_deref() {
            Some("cifs" | "smb" | "windows") => SourceKind::Cifs,
            Some("ssh" | "linux" | "rsync") => SourceKind::Ssh,
            Some("local") => SourceKind::Local,
            _ if share.is_some() => SourceKind::Cifs,
            _ if source_ip.is_some() && path.is_some() => SourceKind::Ssh,
            _ => SourceKind::Local,
        };

        let retention = extract(script, "RETENTION")
            .and_then(|value| value.parse::<u32>().ok())
            .unwrap_or(DEFAULT_RETENTION)
            .max(1);

        Ok(Self {
            id: id.to_string(),
            kind,
            source_ip,
            share,
            path,
            user,
            port,
            retention,
            destination_root: extract(script, "DEST_ROOT")
                .unwrap_or_else(|| DEFAULT_DESTINATION.to_string()),
            log_dir: extract(script, "LOG_DIR").unwrap_or_else(|| DEFAULT_LOG_DIR.to_string()),
            credentials_file: extract(script, "CRED_FILE"),
            key_file: extract(script, "KEY_FILE"),
        })
    }

    /// Carga una tarea desde un archivo `backup_<id>.sh`.
    pub fn load(path: impl AsRef<Path>) -> NasResult<Self> {
        let path = path.as_ref();
        let text = std::fs::read_to_string(path).map_err(|e| NasError::io(path, e))?;
        let stem = path
            .file_stem()
            .map(|stem| stem.to_string_lossy().to_string())
            .unwrap_or_default();
        let id = stem
            .strip_prefix("backup_")
            .map_or_else(|| stem.clone(), str::to_string);
        Self::parse(&id, &text)
    }

    /// Descubre todas las tareas de un directorio.
    pub fn discover(dir: impl AsRef<Path>) -> NasResult<Vec<Self>> {
        let dir = dir.as_ref();
        let entries = std::fs::read_dir(dir).map_err(|e| NasError::io(dir, e))?;

        let mut tasks = Vec::new();
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name.starts_with("backup_") && name.ends_with(".sh") {
                if let Ok(task) = Self::load(&path) {
                    tasks.push(task);
                }
            }
        }
        tasks.sort_by(|a, b| a.id.cmp(&b.id));
        Ok(tasks)
    }

    /// Valida la coherencia de la tarea.
    pub fn validate(&self) -> Vec<String> {
        let mut problems = Vec::new();

        if self.retention == 0 {
            problems.push("la retención debe ser al menos 1".to_string());
        }

        match self.kind {
            SourceKind::Cifs => {
                if self.source_ip.is_none() {
                    problems.push("CIFS requiere SRC_IP".to_string());
                }
                if self.share.is_none() {
                    problems.push("CIFS requiere SRC_SHARE".to_string());
                }
                if self.credentials_file.is_none() {
                    problems.push("CIFS requiere CRED_FILE (0600 root:root)".to_string());
                }
            }
            SourceKind::Ssh => {
                if self.source_ip.is_none() {
                    problems.push("SSH requiere SRC_IP".to_string());
                }
                if self.path.is_none() {
                    problems.push("SSH requiere SRC_PATH".to_string());
                }
                if self.user.is_none() {
                    problems.push("SSH requiere SRC_USER".to_string());
                }
                if self.port.is_none() {
                    problems.push("SSH requiere SRC_PORT".to_string());
                }
            }
            SourceKind::Local => {
                if self.path.is_none() {
                    problems.push("origen local requiere SRC_PATH".to_string());
                }
            }
        }

        problems
    }

    /// Directorio base de la tarea en el repositorio histórico (ruta local).
    pub fn task_dir(&self) -> PathBuf {
        PathBuf::from(self.task_dir_unix())
    }

    /// Directorio base con separador `/`, apto para planes remotos y JSON.
    pub fn task_dir_unix(&self) -> String {
        format!(
            "{}/{}",
            self.destination_root.trim_end_matches('/'),
            self.id
        )
    }

    /// Ruta lógica del snapshot definitivo para una marca de tiempo.
    pub fn snapshot_path(&self, now: &chrono::DateTime<chrono::Local>) -> String {
        format!("{}/{}", self.task_dir_unix(), snapshot_dir_name(now))
    }

    /// Ruta lógica del staging atómico para una marca de tiempo.
    pub fn staging_path(&self, now: &chrono::DateTime<chrono::Local>) -> String {
        format!("{}/{}", self.task_dir_unix(), staging_dir_name(now))
    }

    /// Plan de ejecución compuesto por comandos con argumentos separados.
    pub fn plan(
        &self,
        last_snapshot: Option<&Path>,
        now: &chrono::DateTime<chrono::Local>,
    ) -> Vec<Vec<String>> {
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
    if is_rsync && output.code == 24 {
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

#[cfg(test)]
mod tests {
    use super::{BackupTask, SourceKind};
    use chrono::TimeZone;

    const CIFS_RUNNER: &str = r#"#!/bin/bash
set -e
TASK="CONTABILIDAD"
SRC_IP="10.10.1.50"
SRC_SHARE="Contabilidad$"
CRED_FILE="/etc/backup-credentials/contabilidad.cred"
BKP_DIR="/srv/nas/BACKUPS_HISTORICOS/$TASK"
RETENTION=20
"#;

    const SSH_RUNNER: &str = r#"#!/bin/bash
set -e
TASK="SRV-LINUX"
SRC_IP="10.10.1.60"
SRC_PORT="2222"
SRC_PATH="/var/www"
SRC_USER="backup"
RETENTION=15
"#;

    fn now() -> chrono::DateTime<chrono::Local> {
        chrono::Local
            .with_ymd_and_hms(2026, 10, 23, 16, 0, 0)
            .single()
            .expect("fecha válida")
    }

    #[test]
    fn detecta_origen_cifs() {
        let task = BackupTask::parse("contabilidad", CIFS_RUNNER).expect("tarea");
        assert_eq!(task.kind, SourceKind::Cifs);
        assert_eq!(task.share.as_deref(), Some("Contabilidad$"));
        assert_eq!(task.retention, 20);
        assert!(task.validate().is_empty());
    }

    #[test]
    fn detecta_origen_ssh_y_puerto() {
        let task = BackupTask::parse("srv-linux", SSH_RUNNER).expect("tarea");
        assert_eq!(task.kind, SourceKind::Ssh);
        assert_eq!(task.port, Some(2222));
        assert_eq!(task.user.as_deref(), Some("backup"));
        assert!(task.validate().is_empty());
    }

    #[test]
    fn detecta_origen_local() {
        let script = "SRC_PATH=\"/datos\"\nRETENTION=7\n";
        let task = BackupTask::parse("local", script).expect("tarea");
        assert_eq!(task.kind, SourceKind::Local);
        assert_eq!(task.retention, 7);
        assert!(task.validate().is_empty());
    }

    #[test]
    fn ignora_placeholders_y_valores_vacios() {
        let script =
            "SRC_IP=\"WIN_IP_PLACEHOLDER\"\nSRC_SHARE=''\nRETENTION=RETENTION_PLACEHOLDER\n";
        let task = BackupTask::parse("nueva", script).expect("tarea");
        assert_eq!(task.source_ip, None);
        assert_eq!(task.share, None);
        assert_eq!(task.retention, super::DEFAULT_RETENTION);
    }

    #[test]
    fn valida_tarea_cifs_incompleta() {
        let script = "SOURCE_TYPE=\"cifs\"
SRC_IP=\"1.2.3.4\"
";
        let task = BackupTask::parse("rota", script).expect("tarea");
        let problems = task.validate();
        assert_eq!(task.kind, SourceKind::Cifs);
        assert!(problems.iter().any(|p| p.contains("SRC_SHARE")));
        assert!(problems.iter().any(|p| p.contains("CRED_FILE")));
    }

    #[test]
    fn respeta_el_tipo_declarado_explicitamente() {
        let script = "SOURCE_TYPE=\"ssh\"
SRC_IP=\"1.2.3.4\"
SRC_PATH=\"/data\"
SRC_USER=\"bk\"
SRC_PORT=22
";
        let task = BackupTask::parse("explicita", script).expect("tarea");
        assert_eq!(task.kind, SourceKind::Ssh);
    }

    #[test]
    fn plan_cifs_monta_rsynca_y_desmonta() {
        let task = BackupTask::parse("contabilidad", CIFS_RUNNER).expect("tarea");
        let plan = task.plan(None, &now());
        let labels: Vec<String> = plan.iter().map(|step| step[0].clone()).collect();

        assert_eq!(
            labels,
            vec!["mkdir", "mkdir", "mount", "rsync", "umount", "mv"]
        );
        let mount = &plan[2];
        assert!(mount.contains(&"cifs".to_string()));
        assert!(mount.iter().any(|arg| arg.contains("credentials=")));
        assert!(mount.iter().any(|arg| arg.contains("vers=3.1.1")));
        assert!(plan[3].iter().any(|arg| arg == "-aAXH"));
        assert!(plan[5]
            .iter()
            .any(|arg| arg.ends_with("snapshot_2026-10-23_160000")));
    }

    #[test]
    fn plan_ssh_usa_rsync_remoto_con_puerto() {
        let task = BackupTask::parse("srv-linux", SSH_RUNNER).expect("tarea");
        let plan = task.plan(None, &now());
        let rsync = plan
            .iter()
            .find(|step| step.first().is_some_and(|arg| arg == "sshpass"))
            .expect("paso sshpass");

        assert!(rsync.iter().any(|arg| arg.contains("ssh -p 2222")));
        assert!(rsync.iter().any(|arg| arg == "backup@10.10.1.60:/var/www/"));
    }

    #[test]
    fn plan_incluye_link_dest_cuando_existe_snapshot_previo() {
        let task = BackupTask::parse("local", "SRC_PATH=\"/datos\"\n").expect("tarea");
        let previous =
            std::path::Path::new("/srv/nas/BACKUPS_HISTORICOS/local/snapshot_2026-10-22_020000");
        let plan = task.plan(Some(previous), &now());
        let rsync = plan
            .iter()
            .find(|step| step.first().is_some_and(|arg| arg == "rsync"))
            .expect("paso rsync");

        assert!(rsync
            .iter()
            .any(|arg| arg
                == "--link-dest=/srv/nas/BACKUPS_HISTORICOS/local/snapshot_2026-10-22_020000"));
    }

    #[test]
    fn id_vacio_es_error() {
        assert!(BackupTask::parse("", CIFS_RUNNER).is_err());
    }

    #[test]
    fn plan_ssh_con_clave_privada_no_usa_sshpass() {
        let script = r#"#!/bin/bash
TASK="srv-key"
SOURCE_TYPE="ssh"
SRC_IP="10.10.1.70"
SRC_PORT="22"
SRC_PATH="/var/log"
SRC_USER="root"
KEY_FILE="/etc/backup-credentials/srv-key.id_ed25519"
RETENTION=10
"#;
        let task = BackupTask::parse("srv-key", script).expect("tarea");
        assert_eq!(
            task.key_file.as_deref(),
            Some("/etc/backup-credentials/srv-key.id_ed25519")
        );

        let plan = task.plan(None, &now());
        // No debe contener paso inicial sshpass
        assert!(!plan
            .iter()
            .any(|step| step.first().is_some_and(|arg| arg == "sshpass")));

        let rsync = plan
            .iter()
            .find(|step| step.first().is_some_and(|arg| arg == "rsync"))
            .expect("paso rsync");
        assert!(rsync
            .iter()
            .any(|arg| arg.contains("-i /etc/backup-credentials/srv-key.id_ed25519")));
    }
}
