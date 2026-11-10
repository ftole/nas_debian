use std::process::ExitCode;
use std::time::Duration;

use nas_core::shell::quote_all;
use nas_core::{ApiResponse, NasError, NasPaths, NasResult, ServerRole};
use nas_engine::deploy::{DeployParams, FileSystem};
use nas_exec::{CommandSpec, Runner};

use super::common::{print_json, run_step};
use crate::DeployArgs;

/// Opciones de ejecución del despliegue.
#[derive(Debug, Default)]
pub struct DeployRunOptions {
    pub confirmed: bool,
    pub ignore_in_use: bool,
    pub format_token: Option<String>,
    pub sudo: bool,
}

/// Aplica la contraseña del administrador del despliegue.
///
/// Eleva con `sudo -n`, propaga los fallos de `chpasswd`/`smbpasswd` y deja el
/// acceso del panel web sincronizado (misma contraseña en `web_users`).
pub fn apply_admin_credentials(
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
pub fn sync_panel_password(user: &str, password: &str) -> Result<(), String> {
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
pub fn panel_sha256(password: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(password.as_bytes());
    format!("{:x}", hasher.finalize())
}

pub fn build_deploy_params(request: &DeployArgs) -> NasResult<DeployParams> {
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

pub fn write_artifacts(
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

pub fn uninstall_command(
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
                println!("\nRepita con --yes para ejecutar.");
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
