use std::process::ExitCode;
use std::time::Duration;

use nas_core::{ApiResponse, NasError, NasPaths, NasResult};
use nas_exec::{CommandSpec, Runner};

use super::super::common::{print_json, run_step};

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
