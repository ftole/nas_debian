use std::path::Path;
use std::process::ExitCode;

use nas_core::{ApiResponse, NasError, NasPaths, NasResult};
use nas_engine::update::{
    build_rollback_plan, is_valid_version, list_installed_releases, UpdatePlan,
};
use nas_exec::Runner;

use super::super::common::{print_json, run_step};
use super::{post_update_step_timeout_secs, post_update_sync_steps};

pub(super) fn execute_ab_plan(runner: &Runner, plan: &UpdatePlan, sudo: bool) -> Option<NasError> {
    let mut failure: Option<NasError> = None;

    for step in &plan.steps {
        let is_health = step.first().is_some_and(|arg| arg == "curl");
        if is_health {
            let mut last_code = String::new();
            let mut last_err: Option<NasError> = None;
            let mut healthy = false;
            for attempt in 0..6 {
                match run_step(runner, step, sudo, 30) {
                    Ok(output) => {
                        let code = output.stdout.trim().to_string();
                        if code == "200" || code == "302" {
                            healthy = true;
                            break;
                        }
                        last_code = code;
                        last_err = None;
                    }
                    Err(error) => {
                        last_err = Some(error);
                    }
                }
                if attempt < 5 {
                    std::thread::sleep(std::time::Duration::from_secs(1));
                }
            }
            if !healthy {
                failure = Some(last_err.unwrap_or_else(|| {
                    NasError::Denied(format!(
                        "healthcheck devolvió {last_code}; se revierte la release"
                    ))
                }));
                break;
            }
            continue;
        }

        match run_step(runner, step, sudo, 900) {
            Ok(_) => {}
            Err(error) => {
                failure = Some(error);
                break;
            }
        }
    }

    if failure.is_some() {
        for step in &plan.rollback {
            let _ = run_step(runner, step, sudo, 300);
        }
    }

    failure
}

pub fn update_list(root: &str, json: bool) -> NasResult<ExitCode> {
    let releases = list_installed_releases(root);
    let current = releases
        .iter()
        .find(|r| r.current)
        .map(|r| r.version.clone());
    let previous_version = releases
        .iter()
        .find(|r| !r.current && r.has_binaries)
        .map(|r| r.version.clone());
    let rollback_available = previous_version.is_some();

    if json {
        print_json(&ApiResponse::ok(
            serde_json::json!({
                "root": root,
                "current": current,
                "rollback_available": rollback_available,
                "previous_version": previous_version,
                "releases": releases,
            }),
            "Releases instaladas",
        ))?;
    } else if releases.is_empty() {
        println!("No se encontraron releases instaladas en {root}/releases.");
    } else {
        println!("Releases instaladas en {root}/releases:");
        for r in &releases {
            let state = if r.current {
                "activa"
            } else if r.has_binaries {
                "disponible"
            } else {
                "incompleta"
            };
            println!(
                "  {:<12} [{:<10}] commit={}",
                r.version,
                state,
                r.commit.as_deref().unwrap_or("-")
            );
        }
    }

    Ok(ExitCode::SUCCESS)
}

pub fn update_rollback(
    root: &str,
    version: Option<&str>,
    confirmed: bool,
    sudo: bool,
    json: bool,
) -> NasResult<ExitCode> {
    let releases = list_installed_releases(root);
    let current = releases
        .iter()
        .find(|r| r.current)
        .map(|r| r.version.clone());

    let target_version = match version {
        Some(explicit) => {
            let trimmed = explicit.trim();
            if !is_valid_version(trimmed) {
                return Err(NasError::Invalid(format!(
                    "versión inválida para regresión: {explicit}"
                )));
            }
            trimmed.to_string()
        }
        None => releases
            .iter()
            .find(|r| !r.current && r.has_binaries)
            .map(|r| r.version.clone())
            .ok_or_else(|| {
                NasError::NotFound(format!(
                    "no hay ninguna release anterior disponible con binarios en {root}/releases"
                ))
            })?,
    };

    if !confirmed {
        return Err(NasError::Denied(
            "la regresión modifica el sistema: repita con --yes para confirmar".into(),
        ));
    }

    let plan = build_rollback_plan(root, &target_version, current.as_deref())?;
    let _lock = nas_engine::TaskLock::acquire("/var/lock/nas-update.lock")?;

    let audit = nas_audit::AuditLog::new(NasPaths::from_env().audit_log());
    let actor = std::env::var("USER").unwrap_or_else(|_| "root".to_string());
    let _ = audit.append(nas_audit::AuditEntry::new(
        &actor,
        "local",
        "update-rollback",
        &target_version,
        "START",
        format!("from={}", current.as_deref().unwrap_or("desconocida")),
    ));

    let runner = Runner::new();
    let failure = execute_ab_plan(&runner, &plan, sudo);

    let (status, detail) = match &failure {
        None => ("SUCCESS", format!("release={target_version}")),
        Some(error) => ("FAILED", error.to_string()),
    };
    let _ = audit.append(nas_audit::AuditEntry::new(
        &actor,
        "local",
        "update-rollback",
        &target_version,
        status,
        detail,
    ));

    if let Some(error) = failure {
        return Err(error);
    }

    let nas_bin = Path::new("/usr/local/bin/nas");
    let dst_release_pub = Path::new("/etc/nas/release.pub");
    let post_steps = post_update_sync_steps(root, nas_bin, dst_release_pub);
    for step in &post_steps {
        let timeout_secs = post_update_step_timeout_secs(step);
        if let Err(err) = run_step(&runner, step, sudo, timeout_secs) {
            eprintln!(
                "aviso: paso post-regresión ({}) falló: {err}",
                step.join(" ")
            );
        }
    }

    if json {
        print_json(&ApiResponse::ok(
            serde_json::json!({
                "version": target_version,
                "previous_version": current,
                "release_dir": plan.release_dir,
                "rollback_available": !plan.rollback.is_empty(),
            }),
            "Regresión aplicada",
        ))?;
    } else {
        println!("Regresión aplicada: {target_version}");
        println!("Release: {}", plan.release_dir);
    }

    Ok(ExitCode::SUCCESS)
}
