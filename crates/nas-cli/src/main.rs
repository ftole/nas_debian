//! CLI unificada del servidor NAS.
//!
//! Sustituye a `bin/nas-cli` (PHP) manteniendo el contrato `--json` con la
//! envoltura `{success, message, data}` que consumen el panel y los scripts.

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Duration;

use clap::{Args, Parser, Subcommand};
use nas_core::shell::quote_all;
use nas_core::{ApiResponse, NasError, NasPaths, NasResult, ServerRole};
use nas_engine::deploy::{DeployParams, FileSystem};
use nas_engine::tasks::BackupTask;
use nas_engine::update::ReleaseManifest;
use nas_engine::{collect_network, collect_status};
use nas_exec::{CommandOutput, CommandSpec, Runner, SudoMode};
use serde::Serialize;

mod helper;
use helper::{run_helper, HelperAction};

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
    /// Estado del retiro del stack anterior.
    Migrate {
        #[command(subcommand)]
        action: MigrateAction,
    },
    /// Tareas de respaldo instaladas en el servidor.
    Backup {
        #[command(subcommand)]
        action: BackupAction,
    },
    /// Verificación y planificación de actualizaciones firmadas.
    Update {
        #[command(subcommand)]
        action: Option<UpdateAction>,
    },
    /// Planificación del despliegue del servidor.
    Deploy {
        #[command(subcommand)]
        action: DeployAction,
    },
    /// Inventario de almacenamiento.
    Storage {
        #[command(subcommand)]
        action: StorageAction,
    },
    /// Desinstala la plataforma Rust (no elimina /srv/nas).
    Uninstall {
        /// Confirma la desinstalación.
        #[arg(long)]
        yes: bool,
        /// Elimina también el estado del panel (`/var/lib/nas` y `/etc/nas`).
        #[arg(long)]
        purge: bool,
        /// Alcance: `panel` (por defecto) o `os` (incluye ajustes del sistema).
        #[arg(long, default_value = "panel")]
        scope: String,
        /// Elevar privilegios con `sudo -n`.
        #[arg(long)]
        sudo: bool,
        /// Solo muestra el plan.
        #[arg(long)]
        dry_run: bool,
    },
    /// Inventario de usuarios y grupos.
    Users {
        #[command(subcommand)]
        action: UsersAction,
    },
    /// Operaciones privilegiadas de helpers acotados para el panel y sistema.
    Helper {
        #[command(subcommand)]
        action: HelperAction,
    },
}

#[derive(Debug, Subcommand)]
enum StorageAction {
    /// Lista los dispositivos de bloque detectados.
    List,
}

#[derive(Debug, Subcommand)]
enum UsersAction {
    /// Lista usuarios y grupos locales.
    List,
}

#[derive(Debug, Subcommand)]
enum UpdateAction {
    /// Muestra el estado del actualizador y versión instalada.
    Status,
    /// Comprueba si existe una versión más reciente en GitHub Releases.
    Check {
        /// Repositorio en GitHub (`propietario/repo`).
        #[arg(long, default_value = "ftole/nas_debian")]
        repo: String,
    },
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
    /// Aplica la actualización verificada (requiere --yes).
    Apply {
        /// Manifiesto JSON de la release (opcional si se descarga de GitHub).
        manifest: Option<PathBuf>,
        /// Artefacto descargado (opcional si se descarga de GitHub).
        #[arg(long)]
        artifact: Option<PathBuf>,
        /// Repositorio en GitHub (`propietario/repo`) si se descarga por red.
        #[arg(long, default_value = "ftole/nas_debian")]
        repo: String,
        /// Raíz del despliegue.
        #[arg(long, default_value = "/opt/nas_debian")]
        root: String,
        /// Versión instalada actualmente (para el rollback).
        #[arg(long)]
        previous: Option<String>,
        /// Clave pública Ed25519 en hexadecimal o ruta al archivo pubkey.
        #[arg(long)]
        pubkey: Option<String>,
        /// Confirma la ejecución real.
        #[arg(long)]
        yes: bool,
        /// Elevar privilegios con `sudo -n`.
        #[arg(long)]
        sudo: bool,
    },
    /// Firma un manifiesto con una clave privada Ed25519 (hex de 32 bytes).
    Sign {
        /// Manifiesto JSON de la release.
        manifest: PathBuf,
        /// Archivo con la clave privada Ed25519 en hexadecimal.
        #[arg(long)]
        key: PathBuf,
        /// Archivo de salida (por defecto, sobrescribe el manifiesto).
        #[arg(long)]
        out: Option<PathBuf>,
    },
    /// Deriva la clave pública (hex) de una clave privada Ed25519.
    Pubkey {
        /// Archivo con la clave privada Ed25519 en hexadecimal.
        #[arg(long)]
        key: PathBuf,
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
        #[command(flatten)]
        args: DeployArgs,
    },
    /// Ejecuta el despliegue (requiere --yes; formatear exige --format-token).
    Apply {
        #[command(flatten)]
        args: DeployArgs,
        /// Confirma la ejecución real.
        #[arg(long)]
        yes: bool,
        /// Permite formatear un disco montado o en uso.
        #[arg(long)]
        ignore_in_use: bool,
        /// Token textual obligatorio para formatear (`SI-FORMATEAR`).
        #[arg(long)]
        format_token: Option<String>,
        /// Elevar privilegios con `sudo -n`.
        #[arg(long)]
        sudo: bool,
    },
}

/// Argumentos comunes de despliegue.
#[derive(Debug, Args)]
struct DeployArgs {
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
    /// Contraseña del usuario administrador.
    #[arg(long)]
    password: Option<String>,
    /// Lee la contraseña del administrador desde STDIN.
    #[arg(long)]
    password_stdin: bool,
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
}

#[derive(Debug, Subcommand)]
enum MigrateAction {
    /// Muestra qué componentes legacy siguen presentes y su sustituto.
    Status,
}

#[derive(Debug, Subcommand)]
enum BackupAction {
    /// Lista las tareas de respaldo instaladas (`backup_*.sh`).
    List {
        /// Directorio de runners.
        #[arg(long, default_value = "/usr/local/bin")]
        dir: PathBuf,
        /// Muestra respaldos permanentes inmutables (Golden Snapshots).
        #[arg(long)]
        permanent: bool,
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
    /// Congela el último snapshot de una tarea como respaldo permanente inmutable (Golden Snapshot a costo 0).
    Freeze {
        /// Identificador de la tarea.
        id: String,
        /// Etiqueta descriptiva opcional.
        tag: Option<String>,
        /// Directorio de runners.
        #[arg(long, default_value = "/usr/local/bin")]
        dir: PathBuf,
    },
    /// Lista los respaldos permanentes inmutables (Golden Snapshots).
    Permanent {
        /// Identificador opcional de la tarea para filtrar.
        id: Option<String>,
    },
    /// Restaura un snapshot hacia el origen de red o destino local alternativo.
    Restore {
        /// Identificador de la tarea.
        id: String,
        /// Nombre o ruta del snapshot a restaurar.
        snapshot: String,
        /// Destino local alternativo opcional.
        #[arg(long)]
        dest: Option<String>,
        /// Confirmación obligatoria de restauración.
        #[arg(long)]
        yes: bool,
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
            BackupAction::List { dir, permanent } => {
                if permanent {
                    backup_permanent_list(None, cli.json)
                } else {
                    backup_list(&dir, cli.json)
                }
            }
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
            BackupAction::Freeze { id, tag, dir: _ } => {
                backup_freeze(&id, tag.as_deref(), cli.json)
            }
            BackupAction::Permanent { id } => backup_permanent_list(id.as_deref(), cli.json),
            BackupAction::Restore {
                id,
                snapshot,
                dest,
                yes,
            } => backup_restore(&id, &snapshot, dest.as_deref(), yes, cli.json),
        },
        Command::Update { action } => match action {
            None | Some(UpdateAction::Status) => update_status(cli.json),
            Some(UpdateAction::Check { repo }) => update_check(&repo, cli.json),
            Some(UpdateAction::Verify {
                manifest,
                artifact,
                pubkey,
            }) => update_verify(&manifest, artifact.as_deref(), &pubkey, cli.json),
            Some(UpdateAction::Apply {
                manifest,
                artifact,
                repo,
                root,
                previous,
                pubkey,
                yes,
                sudo,
            }) => update_apply_dispatch(
                UpdateDispatchArgs {
                    manifest,
                    artifact,
                    repo,
                    root,
                    previous,
                    pubkey,
                    yes,
                    sudo,
                },
                cli.json,
            ),
            Some(UpdateAction::Sign { manifest, key, out }) => {
                update_sign(&manifest, &key, out.as_deref(), cli.json)
            }
            Some(UpdateAction::Pubkey { key }) => update_pubkey(&key, cli.json),
            Some(UpdateAction::Plan {
                manifest,
                artifact,
                root,
                previous,
            }) => update_plan(&manifest, &artifact, &root, previous.as_deref(), cli.json),
        },
        Command::Migrate { action } => match action {
            MigrateAction::Status => migrate_status(cli.json),
        },
        Command::Uninstall {
            yes,
            purge,
            scope,
            sudo,
            dry_run,
        } => uninstall_command(yes, purge, &scope, sudo, dry_run, cli.json),
        Command::Storage { action } => match action {
            StorageAction::List => storage_list(cli.json),
        },
        Command::Users { action } => match action {
            UsersAction::List => users_list(cli.json),
        },
        Command::Deploy { action } => match action {
            DeployAction::Plan { args } => deploy_plan(&args, cli.json),
            DeployAction::Apply {
                args,
                yes,
                ignore_in_use,
                format_token,
                sudo,
            } => deploy_apply(
                &args,
                DeployRunOptions {
                    confirmed: yes,
                    ignore_in_use,
                    format_token,
                    sudo,
                },
                cli.json,
            ),
        },
        Command::Helper { action } => run_helper(action, cli.json),
    }
}

#[derive(Debug, Clone, serde::Serialize)]
struct LegacyItem {
    path: String,
    replacement: &'static str,
    wave: &'static str,
    present: bool,
}

fn legacy_inventory(root: &Path) -> Vec<LegacyItem> {
    let items: [(&str, &str, &str); 10] = [
        ("web", "nas-web", "A"),
        ("bin/nas-cli", "nas (CLI Rust)", "A"),
        ("src/asistente.sh", "nas-tui", "A"),
        ("src/modules", "nas-tui", "A"),
        ("nas_admin.py", "nasctl", "A"),
        ("test_remote.py", "nas-ssh + nas-dev", "A"),
        ("src/core/deploy.sh", "nas deploy", "B"),
        ("src/core/updater.sh", "nas update", "B"),
        ("src/core/uninstall.sh", "nas uninstall", "B"),
        ("install.sh", "packaging/install.sh", "B"),
    ];

    items
        .iter()
        .map(|(path, replacement, wave)| LegacyItem {
            path: (*path).to_string(),
            replacement,
            wave,
            present: root.join(path).exists(),
        })
        .collect()
}

fn migrate_status(json: bool) -> NasResult<ExitCode> {
    let root = std::env::var("NAS_REPO_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("."));
    let items = legacy_inventory(&root);
    let pending = items.iter().filter(|item| item.present).count();

    if json {
        print_json(&ApiResponse::ok(
            serde_json::json!({
                "root": root.display().to_string(),
                "pending": pending,
                "items": items,
            }),
            "Estado del retiro del stack anterior",
        ))?;
        return Ok(ExitCode::SUCCESS);
    }

    println!("RETIRO DEL STACK ANTERIOR (raíz: {})", root.display());
    println!(
        "{:<26} {:<22} {:<5} ESTADO",
        "COMPONENTE", "SUSTITUTO", "OLA"
    );
    for item in &items {
        println!(
            "{:<26} {:<22} {:<5} {}",
            item.path,
            item.replacement,
            item.wave,
            if item.present { "presente" } else { "retirado" }
        );
    }
    println!();
    println!("Pendientes de retiro: {pending}");

    Ok(ExitCode::SUCCESS)
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

struct RunOptions {
    dry_run: bool,
    confirmed: bool,
    sudo: bool,
    lock_dir: PathBuf,
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

    let run_options = nas_engine::RunOptions {
        dry_run: options.dry_run,
        confirmed: options.confirmed,
        sudo: options.sudo,
        lock_dir: options.lock_dir.clone(),
    };

    let audit = nas_audit::AuditLog::new(NasPaths::from_env().audit_log());
    let actor = std::env::var("USER").unwrap_or_else(|_| "root".to_string());

    let result = nas_engine::run_backup(&Runner::new(), task, &run_options, &chrono::Local::now());

    let (status, detail) = match &result {
        Ok(report) => ("SUCCESS", format!("snapshot={}", report.snapshot)),
        Err(error) => ("FAILED", error.to_string()),
    };
    if !options.dry_run {
        let _ = audit.append(nas_audit::AuditEntry::new(
            &actor,
            "local",
            "backup-run",
            &task.id,
            status,
            detail,
        ));
    }

    let report = result?;

    if json {
        print_json(&ApiResponse::ok(
            &report,
            if report.dry_run {
                "Simulación de respaldo"
            } else {
                "Respaldo completado"
            },
        ))?;
        return Ok(ExitCode::SUCCESS);
    }

    if report.dry_run {
        println!("SIMULACIÓN DE RESPALDO: {}", report.task);
        println!("Snapshot destino : {}", report.snapshot);
        println!("Espacio          : {}", report.space);
        for (index, step) in report.steps.iter().enumerate() {
            println!("{:>2}. {step}", index + 1);
        }
    } else {
        println!("Respaldo completado: {}", report.task);
        println!("Snapshot : {}", report.snapshot);
        println!("Retención: {} snapshot(s) eliminados", report.removed);
        if let Some(log) = &report.log {
            println!("Bitácora : {log}");
        }
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

    let previous = nas_engine::last_snapshot(task);
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

fn backup_freeze(id: &str, tag: Option<&str>, json: bool) -> NasResult<ExitCode> {
    let runner = Runner::new();
    let info = nas_engine::helpers::backup::freeze_backup_task(&runner, id, tag)?;

    if json {
        print_json(&ApiResponse::ok(
            &info,
            "Respaldo permanente creado exitosamente",
        ))?;
        return Ok(ExitCode::SUCCESS);
    }

    println!("RESPALDO PERMANENTE CREADO (GOLDEN SNAPSHOT)");
    println!("Tarea    : {}", info.task);
    println!("Nombre   : {}", info.name);
    println!("Ruta     : {}", info.path);
    println!("Fecha    : {}", info.date);
    if let Some(t) = &info.tag {
        println!("Etiqueta : {t}");
    }
    println!("Archivos : {}", info.files_count);
    println!(
        "Tamaño   : {} bytes (0 bytes extras en disco por Hardlinks)",
        info.size_bytes
    );

    Ok(ExitCode::SUCCESS)
}

fn backup_permanent_list(id: Option<&str>, json: bool) -> NasResult<ExitCode> {
    let list = nas_engine::helpers::backup::list_permanent_backups(id)?;

    if json {
        print_json(&ApiResponse::ok(&list, "Respaldos permanentes"))?;
        return Ok(ExitCode::SUCCESS);
    }

    println!("RESPALDOS PERMANENTES (GOLDEN SNAPSHOTS) ({})", list.len());
    if list.is_empty() {
        println!("No se encontraron respaldos permanentes.");
        return Ok(ExitCode::SUCCESS);
    }

    println!(
        "{:<15} {:<30} {:<15} {:<10} {:<35}",
        "TAREA", "NOMBRE", "FECHA", "ARCHIVOS", "RUTA"
    );
    println!("{}", "-".repeat(110));
    for item in &list {
        println!(
            "{:<15} {:<30} {:<15} {:<10} {:<35}",
            item.task, item.name, item.date, item.files_count, item.path
        );
    }

    Ok(ExitCode::SUCCESS)
}

fn backup_restore(
    id: &str,
    snapshot: &str,
    dest: Option<&str>,
    yes: bool,
    json: bool,
) -> NasResult<ExitCode> {
    if !yes {
        return Err(NasError::Invalid(
            "la restauración sobrescribirá archivos en el destino; confirma con --yes".into(),
        ));
    }

    let runner = Runner::new();
    let msg = nas_engine::helpers::backup::restore_backup(&runner, id, snapshot, dest)?;

    if json {
        print_json(&ApiResponse::ok(
            &serde_json::json!({ "message": msg }),
            &msg,
        ))?;
        return Ok(ExitCode::SUCCESS);
    }

    println!("{msg}");
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

    if ok {
        Ok(ExitCode::SUCCESS)
    } else {
        Ok(ExitCode::from(1))
    }
}

fn update_status(json: bool) -> NasResult<ExitCode> {
    let pkg_version = env!("CARGO_PKG_VERSION");
    let version_path = Path::new("/opt/nas_debian/version.json");
    let deployed_version = if version_path.is_file() {
        std::fs::read_to_string(version_path)
            .ok()
            .and_then(|txt| serde_json::from_str::<serde_json::Value>(&txt).ok())
            .and_then(|v| v.get("version").and_then(|s| s.as_str().map(String::from)))
            .unwrap_or_else(|| pkg_version.to_string())
    } else {
        pkg_version.to_string()
    };

    if json {
        print_json(&ApiResponse::ok(
            serde_json::json!({
                "version": deployed_version,
                "pkg_version": pkg_version,
                "status": "up_to_date",
                "require_signed_tags": true,
            }),
            "Estado del actualizador",
        ))?;
    } else {
        println!("Estado del actualizador NAS:");
        println!("  Versión instalada : {deployed_version}");
        println!("  Requisito de firma: Ed25519 / GPG obligatorio");
        println!("  Canal             : producción (main)");
    }

    Ok(ExitCode::SUCCESS)
}

fn update_check(repo: &str, json: bool) -> NasResult<ExitCode> {
    let runner = Runner::new();
    let current_version = env!("CARGO_PKG_VERSION");
    let release = nas_engine::fetch_latest_release(&runner, repo)?;

    let clean_tag = release.tag_name.trim_start_matches('v');
    let has_update = clean_tag != current_version;

    if json {
        print_json(&ApiResponse::ok(
            serde_json::json!({
                "installed_version": current_version,
                "latest_release": release.tag_name,
                "release_name": release.name,
                "has_update": has_update,
                "manifest_url": release.manifest_url,
                "artifact_url": release.artifact_url,
            }),
            "Consulta de releases de GitHub",
        ))?;
    } else {
        println!("Consulta de releases oficiales (GitHub: {repo}):");
        println!("  Versión instalada : v{current_version}");
        println!("  Última release    : {}", release.tag_name);
        if let Some(name) = &release.name {
            println!("  Título            : {name}");
        }
        if has_update {
            println!("  Estado            : ¡Nueva versión disponible!");
            println!("  Para actualizar   : sudo nas update apply --repo {repo} --yes");
        } else {
            println!("  Estado            : El sistema se encuentra en la versión más reciente.");
        }
    }

    Ok(ExitCode::SUCCESS)
}

fn update_sign(
    manifest_path: &Path,
    key_path: &Path,
    out: Option<&Path>,
    json: bool,
) -> NasResult<ExitCode> {
    let secret = std::fs::read_to_string(key_path)
        .map_err(|error| nas_core::NasError::io(key_path, error))?;
    let mut manifest = ReleaseManifest::load(manifest_path)?;
    nas_engine::update::sign_manifest(&mut manifest, secret.trim())?;
    let target = out.unwrap_or(manifest_path);
    let text = serde_json::to_string_pretty(&manifest)
        .map_err(|error| nas_core::NasError::Invalid(error.to_string()))?;
    std::fs::write(
        target,
        format!(
            "{text}
"
        ),
    )
    .map_err(|error| nas_core::NasError::io(target, error))?;

    if json {
        print_json(&ApiResponse::ok(
            serde_json::json!({ "manifest": target, "version": manifest.version }),
            "Manifiesto firmado",
        ))?;
    } else {
        println!(
            "Manifiesto firmado: {} (v{})",
            target.display(),
            manifest.version
        );
    }
    Ok(ExitCode::SUCCESS)
}

fn update_pubkey(key_path: &Path, json: bool) -> NasResult<ExitCode> {
    let secret = std::fs::read_to_string(key_path)
        .map_err(|error| nas_core::NasError::io(key_path, error))?;
    let public = nas_engine::update::public_key_from_secret(secret.trim())?;
    if json {
        print_json(&ApiResponse::ok(
            serde_json::json!({ "pubkey": public }),
            "Clave pública",
        ))?;
    } else {
        println!("{public}");
    }
    Ok(ExitCode::SUCCESS)
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

/// Aplica la contraseña del administrador del despliegue.
///
/// Eleva con `sudo -n`, propaga los fallos de `chpasswd`/`smbpasswd` y deja el
/// acceso del panel web sincronizado (misma contraseña en `web_users`).
fn apply_admin_credentials(
    runner: &Runner,
    params: &nas_engine::deploy::DeployParams,
    sudo: bool,
) -> Result<(), String> {
    let Some(password) = &params.admin_password else {
        return Ok(());
    };
    if password.trim().is_empty() {
        return Err("la contraseña del administrador no puede quedar vacía".into());
    }

    let configure_sudo = |spec: nas_exec::CommandSpec| nas_exec::CommandSpec {
        sudo: if sudo {
            nas_exec::SudoMode::Passwordless
        } else {
            nas_exec::SudoMode::None
        },
        ..spec
    };

    let chpasswd = configure_sudo(
        CommandSpec::new("chpasswd")
            .stdin_secret(nas_exec::SecretInput::new(format!(
                "{}:{}\n",
                params.admin_user, password
            )))
            .timeout(Duration::from_secs(30)),
    );
    let output = runner
        .run(&chpasswd)
        .map_err(|error| format!("chpasswd no se pudo ejecutar: {error}"))?;
    if !output.is_success() {
        return Err(format!(
            "chpasswd falló: {}",
            if output.stderr.is_empty() {
                output.stdout
            } else {
                output.stderr
            }
        ));
    }

    let smb_add = configure_sudo(
        CommandSpec::new("smbpasswd")
            .args(["-a", "-s", &params.admin_user])
            .stdin_secret(nas_exec::SecretInput::new(format!(
                "{password}\n{password}\n"
            )))
            .timeout(Duration::from_secs(30)),
    );
    let output = runner
        .run(&smb_add)
        .map_err(|error| format!("smbpasswd -a no se pudo ejecutar: {error}"))?;
    if !output.is_success() {
        return Err(format!(
            "smbpasswd -a falló: {}",
            if output.stderr.is_empty() {
                output.stdout
            } else {
                output.stderr
            }
        ));
    }

    let smb_enable = configure_sudo(
        CommandSpec::new("smbpasswd")
            .args(["-e", &params.admin_user])
            .timeout(Duration::from_secs(20)),
    );
    let output = runner
        .run(&smb_enable)
        .map_err(|error| format!("smbpasswd -e no se pudo ejecutar: {error}"))?;
    if !output.is_success() {
        return Err(format!(
            "smbpasswd -e falló: {}",
            if output.stderr.is_empty() {
                output.stdout
            } else {
                output.stderr
            }
        ));
    }

    sync_panel_password(params.admin_user.trim(), password)
        .map_err(|error| format!("contraseña del panel web: {error}"))
}

/// Sincroniza la contraseña del panel con la del despliegue.
///
/// Actualiza `web_users` en SQLite y `NAS_WEB_PASSWORD_SHA256` en
/// `/etc/nas-web.env` para que ambas vías (panel y sistema) acepten la misma
/// clave. Si el archivo de entorno no existe (instalación previa ausente) se
/// deja constancia y no se bloquea el despliegue.
fn sync_panel_password(user: &str, password: &str) -> Result<(), String> {
    if user.is_empty() {
        return Err("usuario del panel inválido".into());
    }
    let hash = panel_sha256(password);

    let env_path = std::path::Path::new("/etc/nas-web.env");
    if env_path.exists() {
        let content = std::fs::read_to_string(env_path)
            .map_err(|error| format!("lectura de {}: {error}", env_path.display()))?;
        let mut updated = String::new();
        let mut replaced = false;
        for line in content.lines() {
            if line.trim_start().starts_with("NAS_WEB_PASSWORD_SHA256=") {
                updated.push_str(&format!("NAS_WEB_PASSWORD_SHA256={hash}"));
                replaced = true;
            } else {
                updated.push_str(line);
            }
            updated.push('\n');
        }
        if !replaced {
            updated.push_str(&format!("NAS_WEB_PASSWORD_SHA256={hash}\n"));
        }
        let staged = std::env::temp_dir().join(format!("nas-env-{}", std::process::id()));
        std::fs::write(&staged, &updated)
            .map_err(|error| format!("escritura temporal: {error}"))?;
        let argv = vec![
            "install".to_string(),
            "-m".to_string(),
            "0600".to_string(),
            "-o".to_string(),
            "root".to_string(),
            "-g".to_string(),
            "root".to_string(),
            staged.to_string_lossy().to_string(),
            env_path.to_string_lossy().to_string(),
        ];
        let result = run_step(&Runner::new(), &argv, true, 30);
        let _ = std::fs::remove_file(&staged);
        result.map_err(|error| format!("instalación de {}: {error}", env_path.display()))?;
    } else {
        eprintln!(
            "aviso: /etc/nas-web.env no existe; la contraseña del panel se aplicará en packaging/install.sh"
        );
    }

    // La base SQLite puede no existir aún (despliegue previo al primer arranque);
    // se intenta la actualización y se informa de fallos reales. El hash SHA-256
    // es el formato legacy que Auth::verify_password acepta y actualiza a
    // PBKDF2 en el primer login (evita depender de nas-web desde la CLI).
    let paths = NasPaths::from_env();
    if paths.database().exists() {
        let conn = rusqlite::Connection::open(paths.database())
            .map_err(|error| format!("apertura de {}: {error}", paths.database().display()))?;
        conn.execute(
            "INSERT INTO web_users (username, password_sha256, role, created_at)
             VALUES (?1, ?2, 'superadmin', strftime('%s','now'))
             ON CONFLICT(username) DO UPDATE SET password_sha256 = excluded.password_sha256",
            rusqlite::params![user, hash],
        )
        .map_err(|error| format!("actualización de web_users: {error}"))?;
    } else {
        eprintln!(
            "aviso: SQLite no existe aún; la contraseña del panel se creará al primer arranque"
        );
    }
    Ok(())
}

/// Sustituye hash_password del panel con el formato legacy SHA-256 compatible.
fn panel_sha256(password: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(password.as_bytes());
    format!("{:x}", hasher.finalize())
}

fn build_deploy_params(request: &DeployArgs) -> NasResult<DeployParams> {
    if request.ext4 && request.btrfs {
        return Err(NasError::Invalid(
            "--ext4 y --btrfs son mutuamente excluyentes".into(),
        ));
    }

    let role = ServerRole::parse(&request.role)
        .ok_or_else(|| NasError::Invalid(format!("rol inválido: {}", request.role)))?;

    let mut params = DeployParams::new(
        request.disk.clone(),
        request.workgroup.clone(),
        request.netbios.clone(),
        request.admin.clone(),
        role,
    );
    params.keep_data = request.keep_data;
    params.rotational = !request.ssd;
    if request.ext4 {
        params.filesystem = FileSystem::Ext4;
    } else if request.btrfs {
        params.filesystem = FileSystem::Btrfs;
    }

    let password = if let Some(p) = &request.password {
        Some(p.clone())
    } else if request.password_stdin {
        let mut buffer = String::new();
        std::io::stdin()
            .read_line(&mut buffer)
            .map_err(|e| NasError::io("stdin", e))?;
        let trimmed = buffer.trim_end_matches(['\r', '\n']).to_string();
        if !trimmed.is_empty() {
            Some(trimmed)
        } else {
            None
        }
    } else {
        None
    };
    params.admin_password = password;

    let problems = params.validate();
    if !problems.is_empty() {
        return Err(NasError::Invalid(problems.join("; ")));
    }

    Ok(params)
}

fn write_artifacts(
    runner: &Runner,
    artifacts: &[nas_engine::deploy::Artifact],
    sudo: bool,
) -> Result<(), String> {
    for (idx, artifact) in artifacts.iter().enumerate() {
        let path = std::path::Path::new(&artifact.path);
        if sudo {
            let staged =
                std::env::temp_dir().join(format!("nas-artifact-{}-{idx}", std::process::id()));
            std::fs::write(&staged, &artifact.content)
                .map_err(|error| format!("no se pudo preparar {}: {error}", staged.display()))?;
            let mode_str = format!("{:04o}", artifact.mode);
            let argv = vec![
                "install".to_string(),
                "-D".to_string(),
                "-m".to_string(),
                mode_str,
                "-o".to_string(),
                "root".to_string(),
                "-g".to_string(),
                "root".to_string(),
                staged.to_string_lossy().to_string(),
                artifact.path.clone(),
            ];
            let res = run_step(runner, &argv, true, 30);
            let _ = std::fs::remove_file(&staged);
            res.map_err(|error| format!("no se pudo instalar {}: {error}", artifact.path))?;
            continue;
        }
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|error| format!("no se pudo crear {}: {error}", parent.display()))?;
        }
        std::fs::write(path, &artifact.content)
            .map_err(|error| format!("no se pudo escribir {}: {error}", path.display()))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(artifact.mode));
        }
    }
    Ok(())
}

fn uninstall_command(
    confirmed: bool,
    purge: bool,
    scope: &str,
    sudo: bool,
    dry_run: bool,
    json: bool,
) -> NasResult<ExitCode> {
    if !matches!(scope, "panel" | "os") {
        return Err(NasError::Invalid(
            "alcance inválido: use panel u os".to_string(),
        ));
    }
    let plan = nas_engine::uninstall::plan_with_scope(purge, scope == "os");
    if dry_run || !confirmed {
        if json {
            print_json(&ApiResponse::ok(
                serde_json::json!({
                    "steps": plan.steps,
                    "purge_state": plan.purge_state,
                    "executed": false,
                }),
                "Plan de desinstalación",
            ))?;
        } else {
            println!("PLAN DE DESINSTALACIÓN");
            for step in &plan.steps {
                println!("  - {}", step.description);
            }
            println!("  (no se elimina /srv/nas)");
            if !confirmed {
                println!(
                    "
Repita con --yes para ejecutar."
                );
            }
        }
        return Ok(ExitCode::SUCCESS);
    }

    let _lock = nas_engine::TaskLock::acquire("/var/lock/nas-uninstall.lock")?;
    let audit = nas_audit::AuditLog::new(NasPaths::from_env().audit_log());
    let actor = std::env::var("USER").unwrap_or_else(|_| "root".to_string());
    let _ = audit.append(nas_audit::AuditEntry::new(
        &actor,
        "local",
        "uninstall",
        "plataforma",
        "START",
        format!("purge={purge}"),
    ));

    let runner = Runner::new();
    for step in &plan.steps {
        let mut spec = nas_exec::CommandSpec::new(&step.argv[0]).args(step.argv[1..].to_vec());
        if sudo {
            spec = spec.sudo(nas_exec::SudoMode::Passwordless);
        }
        spec = spec.timeout(std::time::Duration::from_secs(120));
        match runner.run(&spec) {
            Ok(output) if output.is_success() => {}
            Ok(_) if step.optional => {}
            Ok(output) => {
                let detail = if output.stderr.is_empty() {
                    output.stdout
                } else {
                    output.stderr
                };
                let _ = audit.append(nas_audit::AuditEntry::new(
                    &actor,
                    "local",
                    "uninstall",
                    &step.description,
                    "FAILED",
                    detail.clone(),
                ));
                return Err(NasError::Denied(format!(
                    "falló el paso '{}': {}",
                    step.description, detail
                )));
            }
            Err(error) if step.optional => {
                let _ = error;
            }
            Err(error) => return Err(NasError::Denied(error.to_string())),
        }
    }

    let _ = audit.append(nas_audit::AuditEntry::new(
        &actor,
        "local",
        "uninstall",
        "plataforma",
        "SUCCESS",
        "desinstalación completada",
    ));

    if json {
        print_json(&ApiResponse::ok(
            serde_json::json!({ "steps": plan.steps.len(), "purge_state": plan.purge_state }),
            "Plataforma desinstalada",
        ))?;
    } else {
        println!("Plataforma desinstalada. /srv/nas se conserva intacto.");
    }
    Ok(ExitCode::SUCCESS)
}

fn deploy_plan(request: &DeployArgs, json: bool) -> NasResult<ExitCode> {
    let params = build_deploy_params(request)?;
    let steps: Vec<String> = params.plan().iter().map(|step| quote_all(step)).collect();
    let invocation = params.deploy_sh_invocation("/opt/nas_debian", "-");

    if json {
        print_json(&ApiResponse::ok(
            serde_json::json!({
                "role": params.role.as_str(),
                "filesystem": params.filesystem.as_str(),
                "mount_options": params.mount_options(),
                "partition": params.partition_path(),
                "steps": steps,
                "artifacts": params.all_artifacts().iter().map(|a| a.path.clone()).collect::<Vec<_>>(),
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

/// Opciones de ejecución del despliegue.
#[derive(Debug, Default)]
struct DeployRunOptions {
    confirmed: bool,
    ignore_in_use: bool,
    format_token: Option<String>,
    sudo: bool,
}

fn disk_in_use(runner: &Runner, disk: &str) -> bool {
    if disk == "LOCAL" {
        return false;
    }
    runner
        .run(
            &CommandSpec::new("findmnt")
                .args(["-R", "-n", "-o", "TARGET", disk])
                .timeout(Duration::from_secs(15)),
        )
        .map(|output| output.is_success() && !output.stdout.trim().is_empty())
        .unwrap_or(false)
}

fn deploy_apply(
    request: &DeployArgs,
    options: DeployRunOptions,
    json: bool,
) -> NasResult<ExitCode> {
    let params = build_deploy_params(request)?;
    let runner = Runner::new();

    if !params.keep_data && params.disk != "LOCAL" {
        params.ensure_not_os_disk(&runner)?;
        let token = options.format_token.as_deref().unwrap_or_default();
        if token != "SI-FORMATEAR" {
            return Err(NasError::Denied(
                "formatear el disco exige --format-token SI-FORMATEAR".into(),
            ));
        }
        if !options.ignore_in_use && disk_in_use(&runner, &params.disk) {
            return Err(NasError::Denied(format!(
                "el disco {} está en uso; use --ignore-in-use tras verificarlo",
                params.disk
            )));
        }
    }

    if !options.confirmed {
        return Err(NasError::Denied(
            "el despliegue modifica el servidor: repita con --yes para confirmar".into(),
        ));
    }

    let _lock = nas_engine::TaskLock::acquire("/var/lock/nas-deploy.lock")?;
    let audit = nas_audit::AuditLog::new(NasPaths::from_env().audit_log());
    let actor = std::env::var("USER").unwrap_or_else(|_| "root".to_string());
    let _ = audit.append(nas_audit::AuditEntry::new(
        &actor,
        "local",
        "deploy-apply",
        &params.disk,
        "START",
        format!(
            "rol={} fs={}",
            params.role.as_str(),
            params.filesystem.as_str()
        ),
    ));

    let plan = params.plan();
    let mut failure = None;

    if let Err(error) = write_artifacts(&runner, &params.all_artifacts(), options.sudo) {
        failure = Some(NasError::Denied(error));
    }

    if failure.is_none() {
        let user_check = runner.run(&CommandSpec::new("id").args(["-u", &params.admin_user]));
        if !user_check.is_ok_and(|out| out.is_success()) {
            let useradd_cmd = vec![
                "useradd".to_string(),
                "-m".to_string(),
                "-s".to_string(),
                "/bin/bash".to_string(),
                params.admin_user.clone(),
            ];
            run_step(&runner, &useradd_cmd, options.sudo, 60).map_err(|error| {
                NasError::Denied(format!(
                    "no se pudo crear el usuario {}: {error}",
                    params.admin_user
                ))
            })?;
        }

        for step in &plan {
            if let Err(error) = run_step(&runner, step, options.sudo, 900) {
                failure = Some(error);
                break;
            }
        }
    }

    if failure.is_none() {
        if let Err(error) = params.update_fstab(&runner) {
            failure = Some(error);
        }
    }

    if failure.is_none() {
        if let Err(error) = apply_admin_credentials(&runner, &params, options.sudo) {
            failure = Some(NasError::Denied(error));
        }
    }

    let (status, detail) = match &failure {
        None => ("SUCCESS", "despliegue aplicado".to_string()),
        Some(error) => ("FAILED", error.to_string()),
    };
    let _ = audit.append(nas_audit::AuditEntry::new(
        &actor,
        "local",
        "deploy-apply",
        &params.disk,
        status,
        detail.clone(),
    ));

    if let Some(error) = failure {
        return Err(error);
    }

    if json {
        print_json(&ApiResponse::ok(
            serde_json::json!({
                "role": params.role.as_str(),
                "filesystem": params.filesystem.as_str(),
                "disk": params.disk,
                "steps": plan.len(),
            }),
            "Despliegue completado",
        ))?;
    } else {
        println!("Despliegue completado sobre {}", params.disk);
        println!(
            "Rol: {} · FS: {}",
            params.role.as_str(),
            params.filesystem.label()
        );
        println!("Pasos ejecutados: {}", plan.len());
    }

    Ok(ExitCode::SUCCESS)
}

struct UpdateDispatchArgs {
    manifest: Option<PathBuf>,
    artifact: Option<PathBuf>,
    repo: String,
    root: String,
    previous: Option<String>,
    pubkey: Option<String>,
    yes: bool,
    sudo: bool,
}

fn resolve_pubkey(specified: Option<&str>) -> NasResult<String> {
    if let Some(key) = specified {
        let trimmed = key.trim();
        let path = Path::new(trimmed);
        if path.is_file() {
            let content = std::fs::read_to_string(path).map_err(|e| NasError::io(path, e))?;
            return Ok(content.trim().to_string());
        }
        return Ok(trimmed.to_string());
    }
    if let Ok(env_key) = std::env::var("NAS_RELEASE_PUBKEY") {
        let trimmed = env_key.trim();
        let path = Path::new(trimmed);
        if path.is_file() {
            let content = std::fs::read_to_string(path).map_err(|e| NasError::io(path, e))?;
            return Ok(content.trim().to_string());
        }
        return Ok(trimmed.to_string());
    }
    for candidate in [
        "/etc/nas/release.pub",
        "/opt/nas_debian/packaging/release.pub",
    ] {
        let p = Path::new(candidate);
        if p.is_file() {
            if let Ok(c) = std::fs::read_to_string(p) {
                let trimmed = c.trim();
                if !trimmed.is_empty() {
                    return Ok(trimmed.to_string());
                }
            }
        }
    }
    Err(NasError::Denied(
        "clave pública Ed25519 obligatoria para verificar la actualización (--pubkey)".into(),
    ))
}

fn update_apply_dispatch(args: UpdateDispatchArgs, json: bool) -> NasResult<ExitCode> {
    let resolved_key = resolve_pubkey(args.pubkey.as_deref())?;

    let (manifest_path, artifact_path) = match (args.manifest, args.artifact) {
        (Some(m), Some(a)) => (m, a),
        _ => {
            let runner = Runner::new();
            let cache_dir = Path::new("/var/cache/nas/updates");
            if !json {
                println!(
                    "Consultando último release desde GitHub ({}) ...",
                    args.repo
                );
            }
            let release_info = nas_engine::fetch_latest_release(&runner, &args.repo)?;
            if !json {
                println!(
                    "Descargando y verificando release {} ...",
                    release_info.tag_name
                );
            }
            let (_manifest, m_path, a_path) = nas_engine::download_and_verify_release(
                &runner,
                &release_info,
                cache_dir,
                &resolved_key,
            )?;
            (m_path, a_path)
        }
    };

    update_apply(
        UpdateApplyOptions {
            manifest_path: &manifest_path,
            artifact: &artifact_path,
            root: &args.root,
            previous: args.previous.as_deref(),
            public_key: &resolved_key,
            confirmed: args.yes,
            sudo: args.sudo,
        },
        json,
    )
}

struct UpdateApplyOptions<'a> {
    manifest_path: &'a Path,
    artifact: &'a Path,
    root: &'a str,
    previous: Option<&'a str>,
    public_key: &'a str,
    confirmed: bool,
    sudo: bool,
}

fn update_apply(options: UpdateApplyOptions<'_>, json: bool) -> NasResult<ExitCode> {
    let UpdateApplyOptions {
        manifest_path,
        artifact,
        root,
        previous,
        public_key,
        confirmed,
        sudo,
    } = options;

    let manifest = ReleaseManifest::load(manifest_path)?;
    manifest.verify_artifact(artifact)?;
    manifest.verify_signature(public_key)?;

    if !confirmed {
        return Err(NasError::Denied(
            "la actualización modifica el sistema: repita con --yes para confirmar".into(),
        ));
    }

    let plan =
        nas_engine::build_update_plan(&manifest, root, &artifact.to_string_lossy(), previous);
    let _lock = nas_engine::TaskLock::acquire("/var/lock/nas-update.lock")?;

    let audit = nas_audit::AuditLog::new(NasPaths::from_env().audit_log());
    let actor = std::env::var("USER").unwrap_or_else(|_| "root".to_string());
    let _ = audit.append(nas_audit::AuditEntry::new(
        &actor,
        "local",
        "update-apply",
        &manifest.version,
        "START",
        format!("artifact={}", artifact.display()),
    ));

    let runner = Runner::new();
    let mut failure: Option<NasError> = None;

    for step in &plan.steps {
        let is_health = step.first().is_some_and(|arg| arg == "curl");
        match run_step(&runner, step, sudo, 900) {
            Ok(output) if is_health => {
                let code = output.stdout.trim();
                if code != "200" && code != "302" {
                    failure = Some(NasError::Denied(format!(
                        "healthcheck devolvió {code}; se revierte la release"
                    )));
                    break;
                }
            }
            Ok(_) => {}
            Err(error) => {
                failure = Some(error);
                break;
            }
        }
    }

    if failure.is_some() {
        for step in &plan.rollback {
            let _ = run_step(&runner, step, sudo, 300);
        }
    }

    let (status, detail) = match &failure {
        None => ("SUCCESS", format!("release={}", manifest.version)),
        Some(error) => ("FAILED", error.to_string()),
    };
    let _ = audit.append(nas_audit::AuditEntry::new(
        &actor,
        "local",
        "update-apply",
        &manifest.version,
        status,
        detail,
    ));

    if let Some(error) = failure {
        return Err(error);
    }

    if json {
        print_json(&ApiResponse::ok(
            serde_json::json!({
                "version": manifest.version,
                "release_dir": plan.release_dir,
                "rollback_available": !plan.rollback.is_empty(),
            }),
            "Actualización aplicada",
        ))?;
    } else {
        println!("Actualización aplicada: {}", manifest.version);
        println!("Release: {}", plan.release_dir);
        println!(
            "Rollback: {}",
            if plan.rollback.is_empty() {
                "no disponible"
            } else {
                "disponible"
            }
        );
    }

    Ok(ExitCode::SUCCESS)
}

fn storage_list(json: bool) -> NasResult<ExitCode> {
    let output = Runner::new().run(&CommandSpec::new("lsblk").args([
        "-J",
        "-o",
        "NAME,SIZE,TYPE,MOUNTPOINT,FSTYPE,MODEL,ROTA",
    ]))?;
    if !output.is_success() {
        return Err(NasError::Command {
            command: "lsblk".into(),
            code: output.code,
            stderr: output.stderr,
        });
    }

    let devices = nas_engine::parsers::lsblk::parse(&output.stdout)
        .map_err(|error| NasError::Invalid(format!("lsblk inválido: {error}")))?;

    if json {
        print_json(&ApiResponse::ok(&devices, "Dispositivos de bloque"))?;
        return Ok(ExitCode::SUCCESS);
    }

    println!("DISPOSITIVOS DE BLOQUE");
    for device in &devices {
        for entry in device.flatten() {
            let mount = entry.mountpoint.as_deref().unwrap_or("-");
            let fs = entry.fstype.as_deref().unwrap_or("-");
            let kind = if entry.rotational == Some(true) {
                "HDD"
            } else {
                "SSD"
            };
            println!(
                "{:<16} {:<8} {:<6} {:<8} {:<18} {}",
                entry.name, entry.size, entry.kind, fs, mount, kind
            );
        }
    }

    Ok(ExitCode::SUCCESS)
}

fn users_list(json: bool) -> NasResult<ExitCode> {
    let (mut users, mut groups) = nas_engine::users::load_local();
    users.retain(|user| !user.system);
    groups.retain(|group| group.name.starts_with("grp_"));

    if json {
        print_json(&ApiResponse::ok(
            serde_json::json!({ "users": users, "groups": groups }),
            "Usuarios y grupos",
        ))?;
        return Ok(ExitCode::SUCCESS);
    }

    println!("USUARIOS DEL PANEL ({})", users.len());
    for user in &users {
        println!(
            "{:<20} uid={:<6} gid={:<6} {:<24} {}",
            user.name, user.uid, user.gid, user.home, user.shell
        );
    }
    println!();
    println!("GRUPOS CORPORATIVOS ({})", groups.len());
    for group in &groups {
        println!(
            "{:<20} gid={:<6} {}",
            group.name,
            group.gid,
            group.members.join(", ")
        );
    }

    Ok(ExitCode::SUCCESS)
}
