//! Sincronización de contraseñas POSIX (`chpasswd`) y cuentas Samba (`smbpasswd`/`pdbedit`).

use std::collections::HashMap;
use std::time::Duration;

use nas_core::{NasError, NasResult};
use nas_exec::{CommandSpec, Runner, SecretInput};

/// Interpreta `pdbedit -L -w` devolviendo `usuario -> habilitado` (sin bandera `D`).
pub(crate) fn parse_pdbedit_w(stdout: &str) -> HashMap<String, bool> {
    let mut map = HashMap::new();
    for line in stdout.lines() {
        let parts: Vec<&str> = line.split(':').collect();
        if parts.len() >= 5 {
            let user = parts[0].trim();
            let flags = parts[4];
            if !user.is_empty() {
                map.insert(user.to_string(), !flags.contains('D'));
            }
        } else if parts.len() >= 4 {
            let user = parts[0].trim();
            let flags = parts[3];
            if !user.is_empty() {
                map.insert(user.to_string(), !flags.contains('D'));
            }
        }
    }
    map
}

pub(super) fn samba_user_exists(runner: &Runner, user: &str) -> bool {
    if let Ok(out) = runner.run(
        &CommandSpec::new("pdbedit")
            .args(["-L", "-w"])
            .timeout(Duration::from_secs(15)),
    ) {
        if out.is_success() {
            return parse_pdbedit_w(&out.stdout).contains_key(user);
        }
    }
    false
}

pub(super) fn chpasswd_payload(user: &str, password: &str) -> NasResult<String> {
    if user.is_empty() || user.contains(':') || user.chars().any(char::is_control) {
        return Err(NasError::Invalid("nombre de usuario inválido".into()));
    }
    if password.chars().any(char::is_control) {
        return Err(NasError::Invalid(
            "la contraseña contiene caracteres de control no permitidos".into(),
        ));
    }
    Ok(format!("{user}:{password}"))
}

pub(super) fn smbpasswd_payload(password: &str) -> NasResult<String> {
    if password.chars().any(char::is_control) {
        return Err(NasError::Invalid(
            "la contraseña contiene caracteres de control no permitidos".into(),
        ));
    }
    Ok(format!("{password}\n{password}"))
}

pub(super) fn sync_samba(
    runner: &Runner,
    user: &str,
    enable: bool,
    password: Option<&str>,
) -> NasResult<()> {
    let fail = |action: &str, output: &nas_exec::CommandOutput| -> NasError {
        let detail = if output.stderr.is_empty() {
            output.stdout.clone()
        } else {
            output.stderr.clone()
        };
        NasError::Invalid(format!("smbpasswd {action} falló para '{user}': {detail}"))
    };

    if enable {
        if let Some(pass) = password.filter(|pass| !pass.is_empty()) {
            let payload = smbpasswd_payload(pass)?;
            let safe_runner = runner.clone().with_secret(pass);
            let spec = CommandSpec::new("smbpasswd")
                .args(["-a", "-s", user])
                .stdin_secret(SecretInput::new(payload))
                .timeout(Duration::from_secs(30));
            let out = safe_runner.run(&spec)?;
            if !out.is_success() {
                return Err(fail("-a", &out));
            }
        } else if !samba_user_exists(runner, user) {
            return Err(NasError::Invalid(format!(
                "se requiere contraseña para habilitar Samba en '{user}'"
            )));
        }
        let out = runner.run(
            &CommandSpec::new("smbpasswd")
                .args(["-e", user])
                .timeout(Duration::from_secs(20)),
        )?;
        if !out.is_success() {
            return Err(fail("-e", &out));
        }
    } else if samba_user_exists(runner, user) {
        let out = runner.run(
            &CommandSpec::new("smbpasswd")
                .args(["-d", user])
                .timeout(Duration::from_secs(20)),
        )?;
        if !out.is_success() {
            return Err(fail("-d", &out));
        }
    }
    Ok(())
}

/// Fija la contraseña de sistema con `chpasswd`; el fallo se propaga.
pub(super) fn sync_system_password(runner: &Runner, user: &str, password: &str) -> NasResult<()> {
    let payload = chpasswd_payload(user, password)?;
    let safe_runner = runner.clone().with_secret(password);
    let spec = CommandSpec::new("chpasswd")
        .stdin_secret(SecretInput::new(payload))
        .timeout(Duration::from_secs(30));
    let out = safe_runner.run(&spec)?;
    if !out.is_success() {
        let detail = if out.stderr.is_empty() {
            out.stdout
        } else {
            out.stderr
        };
        return Err(NasError::Invalid(format!(
            "chpasswd falló para '{user}': {detail}"
        )));
    }
    Ok(())
}
