use std::path::{Path, PathBuf};
use std::process::ExitCode;

use nas_core::shell::quote_all;
use nas_core::{ApiResponse, NasError, NasPaths, NasResult};
use nas_engine::tasks::BackupTask;
use nas_exec::Runner;

use super::common::print_json;

pub struct RunOptions {
    pub dry_run: bool,
    pub confirmed: bool,
    pub sudo: bool,
    pub lock_dir: PathBuf,
}

pub fn discover_tasks(dir: &Path) -> NasResult<Vec<BackupTask>> {
    if !dir.exists() {
        return Err(NasError::NotFound(format!(
            "directorio de tareas inexistente: {}",
            dir.display()
        )));
    }
    BackupTask::discover(dir)
}

pub fn backup_list(dir: &Path, json: bool) -> NasResult<ExitCode> {
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

pub fn backup_run(dir: &Path, id: &str, options: RunOptions, json: bool) -> NasResult<ExitCode> {
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

pub fn backup_plan(dir: &Path, id: &str, json: bool) -> NasResult<ExitCode> {
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

pub fn backup_freeze(id: &str, tag: Option<&str>, json: bool) -> NasResult<ExitCode> {
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

pub fn backup_permanent_list(id: Option<&str>, json: bool) -> NasResult<ExitCode> {
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

pub fn backup_restore(
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

pub struct DeduplicateCliOptions {
    pub task: Option<String>,
    pub runner_dir: PathBuf,
    pub lock_dir: PathBuf,
    pub state_dir: PathBuf,
    pub dry_run: bool,
    pub migration: bool,
    pub force: bool,
}

pub fn backup_repair_runners(dir: &Path, json: bool) -> NasResult<ExitCode> {
    let runner = Runner::new();
    let report = nas_engine::backup_dedup::repair_runners(&runner, dir)?;

    if json {
        print_json(&ApiResponse::ok(&report, "Runners verificados"))?;
        return Ok(ExitCode::SUCCESS);
    }

    println!("REPARACIÓN DE RUNNERS ({})", dir.display());
    println!("  Inspeccionados : {}", report.inspected);
    println!("  Actualizados   : {}", report.updated.len());
    println!("  Sin cambios    : {}", report.already_ok);
    for path in &report.updated {
        println!("  -> actualizado: {path}");
    }
    for err in &report.errors {
        eprintln!("  [!] {err}");
    }
    Ok(ExitCode::SUCCESS)
}

pub fn backup_deduplicate(options: DeduplicateCliOptions, json: bool) -> NasResult<ExitCode> {
    let runner = Runner::new();
    let dedup_opts = nas_engine::backup_dedup::DeduplicateOptions {
        roots: Vec::new(),
        runner_dir: options.runner_dir,
        lock_dir: options.lock_dir,
        log_dir: PathBuf::from("/srv/nas/LOGS_BACKUP"),
        task_filter: options.task,
        dry_run: options.dry_run,
        use_flock: true,
    };

    if options.migration {
        let mig_opts = nas_engine::backup_dedup::MigrationOptions {
            state_dir: options.state_dir,
            dedup: dedup_opts,
            force: options.force,
        };
        let report = nas_engine::backup_dedup::run_dedup_migration_if_needed(&runner, &mig_opts)?;
        if json {
            print_json(&ApiResponse::ok(&report, "Migración de deduplicación"))?;
            return Ok(ExitCode::SUCCESS);
        }
        if report.already_completed {
            println!("Migración de deduplicación ya aplicada previamente (omitida).");
        } else if let Some(d) = &report.dedup {
            println!(
                "Migración completada: {} hardlinks, {} reflinks, {} permisos reparados, {} bytes recuperados",
                d.total_hardlinks,
                d.total_reflinks,
                d.total_permissions_repaired,
                d.total_bytes_reclaimed
            );
        }
        return Ok(ExitCode::SUCCESS);
    }

    let report = nas_engine::backup_dedup::deduplicate_backups(&runner, &dedup_opts)?;
    if json {
        print_json(&ApiResponse::ok(&report, "Deduplicación de respaldos"))?;
        return Ok(ExitCode::SUCCESS);
    }

    println!(
        "DEDUPLICACIÓN DE RESPALDOS{}",
        if report.dry_run { " (SIMULACIÓN)" } else { "" }
    );
    for task in &report.tasks {
        if task.skipped_locked {
            println!("  {} : omitida (respaldo activo en curso)", task.task);
        } else {
            println!(
                "  {} : {} snapshots, {} archivos, {} permisos, {} hardlinks, {} reflinks, {} bytes recuperados",
                task.task,
                task.snapshots_scanned,
                task.files_scanned,
                task.permissions_repaired,
                task.hardlinks_created,
                task.reflinks_created,
                task.bytes_reclaimed
            );
        }
    }
    println!(
        "Total: {} hardlinks, {} reflinks, {} bytes recuperados",
        report.total_hardlinks, report.total_reflinks, report.total_bytes_reclaimed
    );
    Ok(ExitCode::SUCCESS)
}
