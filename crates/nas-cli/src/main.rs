//! CLI unificada del servidor NAS.
//!
//! Sustituye a `bin/nas-cli` (PHP) manteniendo el contrato `--json` con la
//! envoltura `{success, message, data}` que consumen el panel y los scripts.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Duration;

use clap::{Parser, Subcommand};
use nas_core::shell::quote_all;
use nas_core::{ApiResponse, NasError, NasPaths, NasResult, ServerRole};
use nas_engine::backup::{evaluate_space, retention_victims, snapshot_dir_name, SpaceDecision};
use nas_engine::deploy::{DeployParams, FileSystem};
use nas_engine::tasks::BackupTask;
use nas_engine::update::ReleaseManifest;
use nas_engine::{collect_network, collect_status};
use nas_exec::{CommandOutput, CommandSpec, Runner, SudoMode};
use serde::Serialize;

#[derive(Debug, Parser)]
#[command(
    name = "nas",
    version,
    about = "Administración del servidor NAS Debian 13",
    long_about = "CLI del servidor NAS: estado, red, servicios, diagnóstico y auditoría.\n\
                  Todas las salidas admiten --json con la envoltura {success, message, data}."
)]
struct Cli {
    /// Emite la respuesta en JSON (contrato estable para automatización).
    #[arg(long, global = true)]
    json: bool,

    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Métricas del sistema (CPU, memoria, uptime, IP).
    Status,
    /// Inventario de red (interfaces, ruta, DNS, Samba, puertos).
    Network,
    /// Estado de los demonios clave (smbd, nmbd, wsdd2, nginx).
    Services,
    /// Diagnóstico integral con código de salida significativo.
    Diag,
    /// Gestión de la bitácora de auditoría.
    Audit {
        #[command(subcommand)]
        action: AuditAction,
    },
    /// Tareas de respaldo instaladas en el servidor.
    Backup {
        #[command(subcommand)]
        action: BackupAction,
    },
    /// Verificación y planificación de actualizaciones firmadas.
    Update {
        #[command(subcommand)]
        action: UpdateAction,
    },
    /// Planificación del despliegue del servidor.
    Deploy {
        #[command(subcommand)]
        action: DeployAction,
    },
}

#[derive(Debug, Subcommand)]
enum UpdateAction {
    /// Verifica SHA-256 y firma Ed25519 del manifiesto.
    Verify {
        /// Manifiesto JSON de la release.
        manifest: PathBuf,
        /// Artefacto a verificar (por defecto, el declarado en el manifiesto).
        #[arg(long)]
        artifact: Option<PathBuf>,
        /// Clave pública Ed25519 en hexadecimal.
        #[arg(long)]
        pubkey: String,
    },
    /// Genera el plan A/B de la actualización.
    Plan {
        /// Manifiesto JSON de la release.
        manifest: PathBuf,
        /// Artefacto descargado.
        #[arg(long)]
        artifact: PathBuf,
        /// Raíz del despliegue.
        #[arg(long, default_value = "/opt/nas_debian")]
        root: String,
        /// Versión instalada actualmente (para el rollback).
        #[arg(long)]
        previous: Option<String>,
    },
}

#[derive(Debug, Subcommand)]
enum DeployAction {
    /// Muestra el plan de despliegue validado.
    Plan {
        /// Disco dedicado (`/dev/sdb`) o `LOCAL`.
        #[arg(long)]
        disk: String,
        /// Grupo de trabajo.
        #[arg(long)]
        workgroup: String,
        /// Nombre NetBIOS.
        #[arg(long)]
        netbios: String,
        /// Usuario administrador.
        #[arg(long)]
        admin: String,
        /// Rol del servidor.
        #[arg(long, default_value = "ARCHIVOS")]
        role: String,
        /// Reutiliza datos existentes sin formatear.
        #[arg(long)]
        keep_data: bool,
        /// Fuerza ext4 (por defecto se deriva del rol).
        #[arg(long)]
        ext4: bool,
        /// Fuerza Btrfs (por defecto se deriva del rol).
        #[arg(long)]
        btrfs: bool,
        /// El disco es SSD (afecta a las opciones de montaje).
        #[arg(long)]
        ssd: bool,
    },
}

#[derive(Debug, Subcommand)]
enum BackupAction {
    /// Lista las tareas de respaldo instaladas (`backup_*.sh`).
    List {
        /// Directorio de runners.
        #[arg(long, default_value = "/usr/local/bin")]
        dir: PathBuf,
    },
    /// Muestra el plan de ejecución de una tarea.
    Plan {
        /// Identificador de la tarea (sin prefijo `backup_`).
        id: String,
        /// Directorio de runners.
        #[arg(long, default_value = "/usr/local/bin")]
        dir: PathBuf,
    },
    /// Ejecuta una tarea de respaldo (requiere --yes).
    Run {
        /// Identificador de la tarea.
        id: String,
        /// Directorio de runners.
        #[arg(long, default_value = "/usr/local/bin")]
        dir: PathBuf,
        /// Confirma la ejecución real.
        #[arg(long)]
        yes: bool,
        /// Simula la ejecución mostrando el plan y las guardas.
        #[arg(long)]
        dry_run: bool,
        /// Elevar privilegios con `sudo -n`.
        #[arg(long)]
        sudo: bool,
        /// Directorio de bloqueos.
        #[arg(long, default_value = "/var/lock")]
        lock_dir: PathBuf,
    },
}

#[derive(Debug, Subcommand)]
enum AuditAction {
    /// Verifica la integridad de la cadena de auditoría.
    Verify,
}

#[derive(Debug, Serialize)]
struct ServiceStatus {
    service: String,
    active: bool,
}

#[derive(Debug, Serialize)]
struct Check {
    name: String,
    ok: bool,
    detail: String,
}

fn main() -> ExitCode {
    let cli = Cli::parse();

    match run(cli) {
        Ok(code) => code,
        Err(error) => {
            report_error(&error);
            ExitCode::from(u8::try_from(error.exit_code()).unwrap_or(1))
        }
    }
}

fn report_error(error: &NasError) {
    let payload = ApiResponse::<serde_json::Value>::error(error.to_string());
    match serde_json::to_string_pretty(&payload) {
        Ok(json) => eprintln!("{json}"),
        Err(_) => eprintln!("{{\"success\":false,\"error\":\"{error}\"}}"),
    }
}

fn run(cli: Cli) -> NasResult<ExitCode> {
    match cli.command {
        Command::Status => status(cli.json),
        Command::Network => network(cli.json),
        Command::Services => services(cli.json),
        Command::Diag => diag(cli.json),
        Command::Audit { action } => match action {
            AuditAction::Verify => audit_verify(cli.json),
        },
        Command::Backup { action } => match action {
            BackupAction::List { dir } => backup_list(&dir, cli.json),
            BackupAction::Plan { id, dir } => backup_plan(&dir, &id, cli.json),
            BackupAction::Run {
                id,
                dir,
                yes,
                dry_run,
                sudo,
                lock_dir,
            } => backup_run(
                &dir,
                &id,
                RunOptions {
                    dry_run,
                    confirmed: yes,
                    sudo,
                    lock_dir,
                },
                cli.json,
            ),
        },
        Command::Update { action } => match action {
            UpdateAction::Verify {
                manifest,
                artifact,
                pubkey,
            } => update_verify(&manifest, artifact.as_deref(), &pubkey, cli.json),
            UpdateAction::Plan {
                manifest,
                artifact,
                root,
                previous,
            } => update_plan(&manifest, &artifact, &root, previous.as_deref(), cli.json),
        },
        Command::Deploy { action } => match action {
            DeployAction::Plan {
                disk,
                workgroup,
                netbios,
                admin,
                role,
                keep_data,
                ext4,
                btrfs,
                ssd,
            } => deploy_plan(
                DeployRequest {
                    disk,
                    workgroup,
                    netbios,
                    admin,
                    role,
                    keep_data,
                    ext4,
                    btrfs,
                    ssd,
                },
                cli.json,
            ),
        },
    }
}

fn discover_tasks(dir: &Path) -> NasResult<Vec<BackupTask>> {
    if !dir.exists() {
        return Err(NasError::NotFound(format!(
            "directorio de tareas inexistente: {}",
            dir.display()
        )));
    }
    BackupTask::discover(dir)
}

fn backup_list(dir: &Path, json: bool) -> NasResult<ExitCode> {
    let tasks = discover_tasks(dir)?;

    if json {
        print_json(&ApiResponse::ok(&tasks, "Tareas de respaldo"))?;
        return Ok(ExitCode::SUCCESS);
    }

    println!("TAREAS DE RESPALDO ({})", tasks.len());
    if tasks.is_empty() {
        println!("No se encontraron runners backup_*.sh en {}", dir.display());
        return Ok(ExitCode::SUCCESS);
    }

    for task in &tasks {
        let origin = match task.kind {
            nas_engine::SourceKind::Cifs => format!(
                "//{}/{}",
                task.source_ip.as_deref().unwrap_or("?"),
                task.share.as_deref().unwrap_or("?")
            ),
            nas_engine::SourceKind::Ssh => format!(
                "{}@{}:{}",
                task.user.as_deref().unwrap_or("?"),
                task.source_ip.as_deref().unwrap_or("?"),
                task.path.as_deref().unwrap_or("?")
            ),
            nas_engine::SourceKind::Local => task.path.clone().unwrap_or_else(|| "?".to_string()),
        };
        println!(
            "{:<20} {:<10} {:<40} retención={}",
            task.id,
            task.kind.label(),
            origin,
            task.retention
        );
    }

    Ok(ExitCode::SUCCESS)
}

fn last_snapshot(task: &BackupTask) -> Option<PathBuf> {
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

/// Opciones de ejecución de una tarea de respaldo.
#[derive(Debug, Default)]
struct RunOptions {
    dry_run: bool,
    confirmed: bool,
    sudo: bool,
    lock_dir: PathBuf,
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

fn space_usage(target: &Path) -> Option<nas_engine::DfUsage> {
    let output = Runner::new()
        .run(
            &CommandSpec::new("df").args(["-Pk".to_string(), target.to_string_lossy().to_string()]),
        )
        .ok()?;
    if !output.is_success() {
        return None;
    }
    nas_engine::parse_df(&output.stdout)
}

fn append_log(path: &Path, level: &str, message: &str) {
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let line = format!(
        "[{}] [{}] {}\n",
        chrono::Local::now().format("%Y-%m-%d %H:%M:%S"),
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

    let mut spec = CommandSpec::new(program)
        .args(argv[1..].to_vec())
        .timeout(Duration::from_secs(timeout_secs));
    if sudo {
        spec = spec.sudo(SudoMode::Passwordless);
    }

    runner.run(&spec)?.ensure_success()
}

fn backup_run(dir: &Path, id: &str, options: RunOptions, json: bool) -> NasResult<ExitCode> {
    let tasks = discover_tasks(dir)?;
    let task = tasks.iter().find(|task| task.id == id).ok_or_else(|| {
        NasError::NotFound(format!("tarea '{id}' no encontrada en {}", dir.display()))
    })?;

    let problems = task.validate();
    if !problems.is_empty() {
        return Err(NasError::Invalid(format!(
            "tarea '{id}' inválida: {}",
            problems.join("; ")
        )));
    }

    let now = chrono::Local::now();
    let previous = last_snapshot(task);
    let plan = task.plan(previous.as_deref(), &now);
    let snapshot = task.snapshot_path(&now);

    let df_target = if task.task_dir().exists() {
        task.task_dir()
    } else {
        PathBuf::from(&task.destination_root)
    };
    let space = space_usage(&df_target).map_or(SpaceDecision::Ok, |usage| {
        evaluate_space(usage.free_bytes, usage.used_percent)
    });

    if let SpaceDecision::Abort { reason } = &space {
        return Err(NasError::Denied(format!(
            "espacio insuficiente en {}: {reason}",
            df_target.display()
        )));
    }

    let existing = snapshot_names(task);
    let mut projected = existing.clone();
    projected.push(snapshot_dir_name(&now));
    let victims = retention_victims(&projected, task.retention as usize);

    if options.dry_run {
        if json {
            print_json(&ApiResponse::ok(
                serde_json::json!({
                    "task": task.id,
                    "snapshot": snapshot,
                    "space": format!("{space:?}"),
                    "retention": { "keep": task.retention, "remove": victims },
                    "steps": plan.iter().map(|step| quote_all(step)).collect::<Vec<_>>(),
                }),
                "Simulación de respaldo",
            ))?;
        } else {
            println!("SIMULACIÓN DE RESPALDO: {}", task.id);
            println!("Snapshot destino : {snapshot}");
            println!("Espacio          : {space:?}");
            println!(
                "Retención        : conservar {}, eliminar {}",
                task.retention,
                victims.len()
            );
            for (index, step) in plan.iter().enumerate() {
                println!("{:>2}. {}", index + 1, quote_all(step));
            }
        }
        return Ok(ExitCode::SUCCESS);
    }

    if !options.confirmed {
        return Err(NasError::Denied(
            "la ejecución crea un snapshot real: repita con --yes para confirmar".into(),
        ));
    }

    let lock_path = options.lock_dir.join(format!("backup_{id}.lock"));
    let _lock = nas_engine::TaskLock::acquire(&lock_path)?;

    let log_path = PathBuf::from(&task.log_dir).join(format!("backup_{id}.log"));
    append_log(
        &log_path,
        "INFO",
        &format!("INICIO ejecución manual de '{id}'"),
    );

    let audit = nas_audit::AuditLog::new(NasPaths::from_env().audit_log());
    let actor = std::env::var("USER").unwrap_or_else(|_| "root".to_string());
    let _ = audit.append(nas_audit::AuditEntry::new(
        &actor,
        "local",
        "backup-run",
        &task.id,
        "START",
        format!("snapshot={snapshot}"),
    ));

    let runner = Runner::new();
    let mut failure: Option<NasError> = None;

    for step in &plan {
        let label = step.first().cloned().unwrap_or_default();
        append_log(
            &log_path,
            "INFO",
            &format!("ejecutando: {}", quote_all(step)),
        );

        let timeout = match label.as_str() {
            "mount" | "umount" | "mkdir" | "mv" => 120,
            _ => 3600,
        };

        if let Err(error) = run_step(&runner, step, options.sudo, timeout) {
            failure = Some(error);
            break;
        }
    }

    if let Some(error) = failure {
        for step in plan
            .iter()
            .filter(|step| step.first().is_some_and(|arg| arg == "umount"))
        {
            let _ = run_step(&runner, step, options.sudo, 120);
        }
        append_log(&log_path, "ERROR", &error.to_string());
        let _ = audit.append(nas_audit::AuditEntry::new(
            &actor,
            "local",
            "backup-run",
            &task.id,
            "FAILED",
            error.to_string(),
        ));
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
    let _ = audit.append(nas_audit::AuditEntry::new(
        &actor,
        "local",
        "backup-run",
        &task.id,
        "SUCCESS",
        format!("snapshot={snapshot} removed={removed}"),
    ));

    if json {
        print_json(&ApiResponse::ok(
            serde_json::json!({
                "task": task.id,
                "snapshot": snapshot,
                "retention_removed": removed,
                "log": log_path.display().to_string(),
            }),
            "Respaldo completado",
        ))?;
    } else {
        println!("Respaldo completado: {}", task.id);
        println!("Snapshot : {snapshot}");
        println!("Retención: {removed} snapshot(s) eliminados");
        println!("Bitácora : {}", log_path.display());
    }

    Ok(ExitCode::SUCCESS)
}

fn backup_plan(dir: &Path, id: &str, json: bool) -> NasResult<ExitCode> {
    let tasks = discover_tasks(dir)?;
    let task = tasks.iter().find(|task| task.id == id).ok_or_else(|| {
        NasError::NotFound(format!("tarea '{id}' no encontrada en {}", dir.display()))
    })?;

    let problems = task.validate();
    if !problems.is_empty() {
        return Err(NasError::Invalid(format!(
            "tarea '{id}' inválida: {}",
            problems.join("; ")
        )));
    }

    let previous = last_snapshot(task);
    let plan = task.plan(previous.as_deref(), &chrono::Local::now());
    let rendered: Vec<String> = plan.iter().map(|step| quote_all(step)).collect();

    if json {
        print_json(&ApiResponse::ok(
            serde_json::json!({
                "task": task,
                "last_snapshot": previous.as_ref().map(|p| p.display().to_string()),
                "steps": rendered,
            }),
            "Plan de respaldo",
        ))?;
        return Ok(ExitCode::SUCCESS);
    }

    println!("PLAN DE RESPALDO: {}", task.id);
    println!("Origen    : {}", task.kind.label());
    println!("Retención : {} snapshots", task.retention);
    println!(
        "Anterior  : {}",
        previous.as_ref().map_or_else(
            || "sin snapshots previos".to_string(),
            |p| p.display().to_string()
        )
    );
    println!();
    for (index, step) in rendered.iter().enumerate() {
        println!("{:>2}. {step}", index + 1);
    }

    Ok(ExitCode::SUCCESS)
}

fn print_json<T: Serialize>(payload: &ApiResponse<T>) -> NasResult<()> {
    println!("{}", serde_json::to_string_pretty(payload)?);
    Ok(())
}

fn status(json: bool) -> NasResult<ExitCode> {
    let runner = Runner::new();
    let metrics = collect_status(&runner);

    if json {
        print_json(&ApiResponse::ok(metrics, "Métricas del sistema"))?;
        return Ok(ExitCode::SUCCESS);
    }

    println!("SERVIDOR NAS — ESTADO DEL SISTEMA");
    println!("Hostname        : {}", metrics.hostname);
    println!("Sistema         : {}", metrics.os);
    println!("Kernel          : {}", metrics.kernel);
    println!("Uptime          : {}", metrics.uptime);
    println!(
        "CPU             : {} ({:.1}%)",
        metrics.cpu_model, metrics.cpu_usage_pct
    );
    println!(
        "Memoria         : {:.2} GiB / {:.2} GiB ({:.1}%)",
        metrics.ram_used_gb, metrics.ram_total_gb, metrics.ram_usage_pct
    );
    println!(
        "IP primaria     : {}",
        metrics.ip.as_deref().unwrap_or("N/D")
    );
    println!(
        "Hora            : {} {} (UTC{:+})",
        metrics.server_time,
        metrics.timezone_abbr,
        metrics.timezone_offset / 3600
    );

    Ok(ExitCode::SUCCESS)
}

fn network(json: bool) -> NasResult<ExitCode> {
    let paths = NasPaths::from_env();
    let runner = Runner::new();
    let overview = collect_network(&runner, &paths);

    if json {
        print_json(&ApiResponse::ok(overview, "Inventario de red"))?;
        return Ok(ExitCode::SUCCESS);
    }

    println!("CONFIGURACIÓN Y VISIBILIDAD DE RED");
    println!("Hostname        : {}", overview.hostname);
    println!("NetBIOS         : {}", overview.netbios);
    println!(
        "Workgroup       : {}",
        overview.workgroup.as_deref().unwrap_or("N/D")
    );
    for iface in &overview.interfaces {
        println!(
            "Interfaz        : {} {} {}{} (MTU {})",
            iface.name,
            iface.state,
            iface.ipv4.as_deref().unwrap_or("sin IPv4"),
            iface.prefix.map(|p| format!("/{p}")).unwrap_or_default(),
            iface
                .mtu
                .map(|m| m.to_string())
                .unwrap_or_else(|| "N/D".into())
        );
    }
    println!(
        "Puerta enlace   : {}",
        overview.gateway.as_deref().unwrap_or("N/D")
    );
    println!(
        "DNS             : {}",
        if overview.dns.is_empty() {
            "N/D".to_string()
        } else {
            overview.dns.join(", ")
        }
    );
    println!(
        "SMB             : {} → {} (firma: {}, cifrado: {})",
        overview.samba.min_protocol.as_deref().unwrap_or("N/D"),
        overview.samba.max_protocol.as_deref().unwrap_or("N/D"),
        overview.samba.signing.as_deref().unwrap_or("N/D"),
        overview.samba.encrypt.as_deref().unwrap_or("N/D")
    );
    let services = overview
        .services
        .iter()
        .map(|(name, active)| format!("{name}={}", if *active { "activo" } else { "inactivo" }))
        .collect::<Vec<_>>()
        .join(" ");
    println!("Demonios        : {services}");
    let ports = overview
        .listening
        .iter()
        .map(|port| format!("{}/{}", port.port, port.proto))
        .collect::<Vec<_>>()
        .join(" ");
    println!("Puertos         : {ports}");
    println!(
        "Cortafuegos     : {}",
        if overview.firewall.active {
            "activo"
        } else {
            "inactivo"
        }
    );
    println!(
        "Sesiones SMB    : {}",
        overview
            .smb_sessions
            .map(|n| n.to_string())
            .unwrap_or_else(|| "no verificable".to_string())
    );
    println!("Actualizado     : {}", overview.generated_at);

    Ok(ExitCode::SUCCESS)
}

fn services(json: bool) -> NasResult<ExitCode> {
    let runner = Runner::new();
    let list: Vec<ServiceStatus> = ["smbd", "nmbd", "wsdd2", "nginx"]
        .into_iter()
        .map(|service| ServiceStatus {
            service: service.to_string(),
            active: runner
                .run(&CommandSpec::new("systemctl").args(["is-active", service]))
                .map(|out| out.stdout == "active")
                .unwrap_or(false),
        })
        .collect();

    if json {
        print_json(&ApiResponse::ok(list, "Estado de servicios"))?;
        return Ok(ExitCode::SUCCESS);
    }

    println!("DEMONIOS DEL SISTEMA");
    for item in &list {
        let state = if item.active { "activo" } else { "inactivo" };
        println!("{:<10} {state}", item.service);
    }

    Ok(ExitCode::SUCCESS)
}

fn diag(json: bool) -> NasResult<ExitCode> {
    let paths = NasPaths::from_env();
    let runner = Runner::new();
    let role = ServerRole::from_file(&paths.role_file)?.unwrap_or(ServerRole::ArquivosBackup);

    let mut checks = Vec::new();

    checks.push(Check {
        name: "role".to_string(),
        ok: paths.role_file.exists(),
        detail: format!("{} ({})", role.as_str(), role.label()),
    });
    checks.push(Check {
        name: "samba_conf".to_string(),
        ok: paths.samba_conf.exists(),
        detail: paths.samba_conf.display().to_string(),
    });
    checks.push(Check {
        name: "data_root".to_string(),
        ok: paths.data_root.exists(),
        detail: paths.data_root.display().to_string(),
    });
    checks.push(Check {
        name: "state_dir".to_string(),
        ok: paths.state_dir.exists(),
        detail: paths.state_dir.display().to_string(),
    });

    for service in ["smbd", "nginx"] {
        let active = runner
            .run(&CommandSpec::new("systemctl").args(["is-active", service]))
            .map(|out| out.stdout == "active")
            .unwrap_or(false);
        checks.push(Check {
            name: format!("service:{service}"),
            ok: active,
            detail: if active { "activo" } else { "inactivo" }.to_string(),
        });
    }

    let all_ok = checks.iter().all(|check| check.ok);

    if json {
        print_json(&ApiResponse::ok(&checks, "Diagnóstico integral"))?;
    } else {
        println!("DIAGNÓSTICO INTEGRAL");
        for check in &checks {
            let mark = if check.ok { "[OK]" } else { "[FALLO]" };
            println!("{mark:<8} {:<18} {}", check.name, check.detail);
        }
    }

    if all_ok {
        Ok(ExitCode::SUCCESS)
    } else {
        Ok(ExitCode::from(1))
    }
}

fn audit_verify(json: bool) -> NasResult<ExitCode> {
    let paths = NasPaths::from_env();
    let log = nas_audit::AuditLog::new(paths.audit_log());

    let result = log.verify();
    let (ok, detail) = match &result {
        Ok(count) => (true, format!("{count} entrada(s) verificadas")),
        Err(error) => (false, error.to_string()),
    };

    if json {
        print_json(&ApiResponse::ok(
            serde_json::json!({ "ok": ok, "detail": detail, "path": log.path().display().to_string() }),
            "Verificación de auditoría",
        ))?;
    } else {
        println!("AUDITORÍA: {detail}");
    }

    result.map(|_| ExitCode::SUCCESS)
}

fn update_verify(
    manifest_path: &Path,
    artifact: Option<&Path>,
    public_key: &str,
    json: bool,
) -> NasResult<ExitCode> {
    let manifest = ReleaseManifest::load(manifest_path)?;
    let artifact = artifact
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from(&manifest.artifact));

    let artifact_check = manifest.verify_artifact(&artifact);
    let signature_check = manifest.verify_signature(public_key);
    let ok = artifact_check.is_ok() && signature_check.is_ok();

    let mut detail: Vec<String> = Vec::new();
    if let Err(error) = &artifact_check {
        detail.push(error.to_string());
    }
    if let Err(error) = &signature_check {
        detail.push(error.to_string());
    }

    if json {
        print_json(&ApiResponse::ok(
            serde_json::json!({
                "version": manifest.version,
                "artifact": artifact.display().to_string(),
                "sha256_ok": artifact_check.is_ok(),
                "signature_ok": signature_check.is_ok(),
                "detail": detail,
            }),
            "Verificación de release",
        ))?;
    } else {
        println!("VERIFICACIÓN DE RELEASE {}", manifest.version);
        println!("Artefacto : {}", artifact.display());
        println!(
            "SHA-256   : {}",
            if artifact_check.is_ok() {
                "OK"
            } else {
                "FALLO"
            }
        );
        println!(
            "Firma     : {}",
            if signature_check.is_ok() {
                "OK"
            } else {
                "FALLO"
            }
        );
        if let Err(error) = &artifact_check {
            println!("  ↳ {error}");
        }
        if let Err(error) = &signature_check {
            println!("  ↳ {error}");
        }
    }

    if ok {
        Ok(ExitCode::SUCCESS)
    } else {
        Ok(ExitCode::from(3))
    }
}

fn update_plan(
    manifest_path: &Path,
    artifact: &Path,
    root: &str,
    previous: Option<&str>,
    json: bool,
) -> NasResult<ExitCode> {
    let manifest = ReleaseManifest::load(manifest_path)?;
    let plan =
        nas_engine::build_update_plan(&manifest, root, &artifact.to_string_lossy(), previous);

    let steps: Vec<String> = plan.steps.iter().map(|step| quote_all(step)).collect();
    let rollback: Vec<String> = plan.rollback.iter().map(|step| quote_all(step)).collect();

    if json {
        print_json(&ApiResponse::ok(
            serde_json::json!({
                "version": plan.version,
                "release_dir": plan.release_dir,
                "steps": steps,
                "rollback": rollback,
            }),
            "Plan de actualización",
        ))?;
        return Ok(ExitCode::SUCCESS);
    }

    println!("PLAN DE ACTUALIZACIÓN A/B: {}", plan.version);
    println!("Release   : {}", plan.release_dir);
    for (index, step) in steps.iter().enumerate() {
        println!("{:>2}. {step}", index + 1);
    }
    if rollback.is_empty() {
        println!();
        println!("Sin versión previa declarada: no hay rollback automático.");
    } else {
        println!();
        println!("ROLLBACK:");
        for (index, step) in rollback.iter().enumerate() {
            println!("{:>2}. {step}", index + 1);
        }
    }

    Ok(ExitCode::SUCCESS)
}

#[derive(Debug, Default)]
struct DeployRequest {
    disk: String,
    workgroup: String,
    netbios: String,
    admin: String,
    role: String,
    keep_data: bool,
    ext4: bool,
    btrfs: bool,
    ssd: bool,
}

fn deploy_plan(request: DeployRequest, json: bool) -> NasResult<ExitCode> {
    if request.ext4 && request.btrfs {
        return Err(NasError::Invalid(
            "--ext4 y --btrfs son mutuamente excluyentes".into(),
        ));
    }

    let role = ServerRole::parse(&request.role)
        .ok_or_else(|| NasError::Invalid(format!("rol inválido: {}", request.role)))?;

    let mut params = DeployParams::new(
        request.disk,
        request.workgroup,
        request.netbios,
        request.admin,
        role,
    );
    params.keep_data = request.keep_data;
    params.rotational = !request.ssd;
    if request.ext4 {
        params.filesystem = FileSystem::Ext4;
    } else if request.btrfs {
        params.filesystem = FileSystem::Btrfs;
    }

    let problems = params.validate();
    if !problems.is_empty() {
        return Err(NasError::Invalid(problems.join("; ")));
    }

    let plan = params.plan();
    let steps: Vec<String> = plan.iter().map(|step| quote_all(step)).collect();
    let invocation = params.deploy_sh_invocation("/opt/nas_debian", "-");

    if json {
        print_json(&ApiResponse::ok(
            serde_json::json!({
                "role": params.role.as_str(),
                "filesystem": params.filesystem.as_str(),
                "mount_options": params.mount_options(),
                "partition": params.partition_path(),
                "steps": steps,
                "deploy_sh": quote_all(&invocation),
            }),
            "Plan de despliegue",
        ))?;
        return Ok(ExitCode::SUCCESS);
    }

    println!("PLAN DE DESPLIEGUE");
    println!(
        "Rol        : {} ({})",
        params.role.as_str(),
        params.role.label()
    );
    println!("Filesystem : {}", params.filesystem.label());
    println!("Montaje    : {}", params.mount_options());
    println!("Disco      : {}", params.disk);
    println!(
        "Partición  : {}",
        params
            .partition_path()
            .unwrap_or_else(|| "(disco del sistema)".into())
    );
    println!();
    for (index, step) in steps.iter().enumerate() {
        println!("{:>2}. {step}", index + 1);
    }
    println!();
    println!("Ejecución transitoria (deploy.sh auditado):");
    println!("  {}", quote_all(&invocation));

    Ok(ExitCode::SUCCESS)
}
