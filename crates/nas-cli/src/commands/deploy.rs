use std::process::ExitCode;
use std::time::Duration;

use nas_core::shell::quote_all;
use nas_core::{ApiResponse, NasError, NasPaths, NasResult, ServerRole};
use nas_engine::deploy::{DeployParams, FileSystem};
use nas_exec::{CommandSpec, Runner};

use super::common::{print_json, run_step};
use crate::DeployArgs;

mod credentials;

pub use credentials::{apply_admin_credentials, uninstall_command, write_artifacts};

/// Opciones de ejecución del despliegue.
#[derive(Debug, Default)]
pub struct DeployRunOptions {
    pub confirmed: bool,
    pub ignore_in_use: bool,
    pub format_token: Option<String>,
    pub sudo: bool,
}

pub fn build_deploy_params(request: &DeployArgs) -> NasResult<DeployParams> {
    if request.ext4 && request.btrfs {
        return Err(NasError::Invalid(
            "--ext4 y --btrfs son mutuamente excluyentes".into(),
        ));
    }

    let role = nas_engine::deploy::env::normalize_role(&request.role)
        .ok_or_else(|| NasError::Invalid(format!("rol inválido: {}", request.role)))?;

    let netbios = if request.netbios.trim().is_empty() {
        nas_engine::deploy::env::default_netbios(&Runner::new())
    } else {
        request.netbios.clone()
    };
    let workgroup = if request.workgroup.trim().is_empty() {
        nas_engine::deploy::env::default_workgroup("/etc/samba/smb.conf")
    } else {
        request.workgroup.clone()
    };

    let mut params = DeployParams::new(
        request.disk.clone(),
        workgroup,
        netbios,
        request.admin.clone(),
        role,
    );
    params.keep_data = request.keep_data;
    params.rotational = !request.ssd;

    nas_engine::deploy::env::validate_keep_data(&params.disk, params.keep_data)?;
    let disk_problems =
        nas_engine::deploy::env::validate_disk(&params.disk, params.keep_data, false);
    if !disk_problems.is_empty() {
        return Err(NasError::Invalid(disk_problems.join("; ")));
    }
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

pub fn deploy_plan(request: &DeployArgs, json: bool) -> NasResult<ExitCode> {
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

pub fn disk_in_use(runner: &Runner, disk: &str) -> bool {
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

pub fn deploy_apply(
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

    let mut failure = None;
    let mut steps_executed: usize = 0;

    if let Err(error) = write_artifacts(&runner, &params.all_artifacts(), options.sudo) {
        failure = Some(NasError::Denied(error));
    }

    if failure.is_none() {
        if let Err(error) = nas_engine::deploy::apply_hardening(&runner, options.sudo) {
            failure = Some(error);
        }
    }

    if failure.is_none() {
        if let Err(error) = nas_engine::deploy::samba::create_base_groups(&runner) {
            failure = Some(NasError::Denied(error.to_string()));
        }
    }

    if failure.is_none() {
        let _ =
            nas_engine::deploy::samba::configure_wsdd2(&runner, &params.netbios, &params.workgroup);
    }

    if failure.is_none() {
        if let Err(error) = nas_engine::deploy::web::apply_web(&runner, options.sudo) {
            failure = Some(NasError::Denied(error.to_string()));
        }
    }

    if failure.is_none() {
        let source_dir = std::env::var("NAS_HELPERS_DIR").unwrap_or_else(|_| {
            if std::path::Path::new("/opt/nas_debian/current/packaging").is_dir() {
                "/opt/nas_debian/current/packaging".to_string()
            } else {
                "/opt/nas_debian/packaging".to_string()
            }
        });
        if let Err(error) = nas_engine::deploy::helpers::install_all_helpers(&runner, &source_dir) {
            failure = Some(NasError::Denied(error.to_string()));
        }
    }

    if failure.is_none() {
        let _ = nas_engine::deploy::users::provision_superadmin(
            &runner,
            &params.admin_user,
            params.admin_password.as_deref(),
            "",
        );
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

        if failure.is_none() && !params.keep_data && params.disk != "LOCAL" {
            let fs = match params.filesystem {
                nas_engine::deploy::FileSystem::Ext4 => {
                    nas_engine::deploy::storage::FileSystem::Ext4
                }
                nas_engine::deploy::FileSystem::Btrfs => {
                    nas_engine::deploy::storage::FileSystem::Btrfs
                }
            };
            if let Err(error) = nas_engine::deploy::storage::format_disk(
                &runner,
                &params.disk,
                fs,
                params.keep_data,
            ) {
                failure = Some(error);
            } else {
                steps_executed += 1;
            }
        }

        if failure.is_none() {
            let spec = CommandSpec::new("mkdir")
                .args(["-p", "/srv/nas"])
                .sudo(if options.sudo {
                    nas_exec::SudoMode::Passwordless
                } else {
                    nas_exec::SudoMode::None
                })
                .timeout(std::time::Duration::from_secs(30));
            if let Err(error) = runner
                .run(&spec)
                .map_err(|e| NasError::Denied(e.to_string()))
            {
                failure = Some(error);
            } else {
                steps_executed += 1;
            }
        }

        if failure.is_none() && params.disk != "LOCAL" {
            let part = params.partition_path().unwrap_or_default();
            let opts = params.mount_options();
            if let Err(error) =
                nas_engine::deploy::storage::mount(&runner, &part, "/srv/nas", &opts)
            {
                failure = Some(error);
            } else {
                steps_executed += 1;
            }
        }

        if failure.is_none() {
            let spec = CommandSpec::new("mkdir")
                .args(["-p", "/srv/nas/SISTEMAS"])
                .sudo(if options.sudo {
                    nas_exec::SudoMode::Passwordless
                } else {
                    nas_exec::SudoMode::None
                })
                .timeout(std::time::Duration::from_secs(30));
            let _ = runner.run(&spec);
            steps_executed += 1;
            if params.role != ServerRole::Arquivos {
                for dir in ["BACKUPS_HISTORICOS", "BACKUPS_PERMANENTES", "LOGS_BACKUP"] {
                    let path = format!("/srv/nas/{dir}");
                    let spec = CommandSpec::new("mkdir")
                        .args(["-p", &path])
                        .sudo(if options.sudo {
                            nas_exec::SudoMode::Passwordless
                        } else {
                            nas_exec::SudoMode::None
                        })
                        .timeout(std::time::Duration::from_secs(30));
                    let _ = runner.run(&spec);
                }
                let spec = CommandSpec::new("mkdir")
                    .args(["-p", "/etc/backup-credentials"])
                    .sudo(if options.sudo {
                        nas_exec::SudoMode::Passwordless
                    } else {
                        nas_exec::SudoMode::None
                    })
                    .timeout(std::time::Duration::from_secs(30));
                let _ = runner.run(&spec);
            }
        }
    }

    if failure.is_none() {
        if let Err(error) = params.update_fstab(&runner) {
            failure = Some(error);
        }
    }

    if failure.is_none() && params.disk != "LOCAL" {
        if params.filesystem == nas_engine::deploy::FileSystem::Ext4 && !params.rotational {
            let _ = nas_engine::deploy::storage::enable_fstrim(&runner);
        }
        if params.filesystem == nas_engine::deploy::FileSystem::Btrfs {
            let _ = nas_engine::deploy::storage::install_btrfs_scrub_cron(&runner);
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
                "steps": steps_executed,
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
        println!("Pasos ejecutados: {}", steps_executed);
    }

    Ok(ExitCode::SUCCESS)
}
