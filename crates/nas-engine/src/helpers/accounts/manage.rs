//! Ciclo de vida de cuentas locales (alta, modificación, suspensión y eliminación).

use std::collections::HashSet;
use std::time::Duration;

use nas_core::{NasError, NasResult};
use nas_exec::{CommandSpec, Runner};

use super::samba::{samba_user_exists, sync_samba, sync_system_password};
use super::validation::{
    ensure_manageable_existing_user, is_superadmin_account, is_valid_group, is_valid_user,
    sanitize_gecos, validate_password, CreateUserArgs, UpdateUserArgs,
};

fn sync_groups(
    runner: &Runner,
    user: &str,
    admin: bool,
    web: bool,
    extra_groups: &[String],
) -> NasResult<()> {
    let mut keep: HashSet<String> = HashSet::new();

    let out = runner.run(
        &CommandSpec::new("id")
            .args(["-nG", user])
            .timeout(Duration::from_secs(10)),
    )?;
    if !out.is_success() {
        return Err(NasError::Denied(format!(
            "no se pudieron consultar los grupos actuales de '{user}': {}",
            out.stderr.trim()
        )));
    }
    for g in out.stdout.split_whitespace() {
        if !g.starts_with("grp_") || g == "grp_superadmin" {
            keep.insert(g.to_string());
        }
    }

    for g in extra_groups {
        let g = g.trim();
        if g.is_empty() {
            continue;
        }
        if !is_valid_group(g) {
            return Err(NasError::Invalid(format!("grupo inválido: '{g}'")));
        }
        keep.insert(g.to_string());
    }

    if admin {
        keep.insert("grp_samba".to_string());
    } else if web {
        keep.insert("grp_web".to_string());
    }

    let joined: Vec<String> = keep.into_iter().collect();
    let csv = joined.join(",");
    runner
        .run(
            &CommandSpec::new("usermod")
                .args(["-G", &csv, user])
                .timeout(Duration::from_secs(20)),
        )?
        .ensure_success()?;
    Ok(())
}

fn rollback_created_user(runner: &Runner, user: &str) {
    let _ = runner.run(&CommandSpec::new("smbpasswd").args(["-x", user]));
    let del = runner.run(&CommandSpec::new("userdel").args(["-r", user]));
    if !matches!(del, Ok(ref out) if out.is_success()) {
        let _ = runner.run(&CommandSpec::new("userdel").args([user]));
    }
}

fn configure_created_user(runner: &Runner, user: &str, args: &CreateUserArgs) -> NasResult<()> {
    sync_system_password(runner, user, &args.password)?;
    sync_groups(runner, user, args.admin, args.web, &args.groups)?;
    sync_samba(runner, user, args.samba, Some(&args.password))?;
    Ok(())
}

/// Crea un usuario nuevo.
pub fn create_user(runner: &Runner, args: &CreateUserArgs) -> NasResult<String> {
    let u = args.username.trim();
    if !is_valid_user(u) {
        return Err(NasError::Invalid(
            "usuario inválido o reservado del sistema".into(),
        ));
    }
    validate_password(&args.password)?;

    if let Ok(out) = runner.run(
        &CommandSpec::new("id")
            .args(["-u", u])
            .timeout(Duration::from_secs(10)),
    ) {
        if out.is_success() {
            return Err(NasError::Invalid(format!("el usuario '{u}' ya existe")));
        }
    }

    let full = sanitize_gecos(&args.full_name);
    let shell = if args.admin {
        "/bin/bash"
    } else {
        "/usr/sbin/nologin"
    };

    runner
        .run(
            &CommandSpec::new("useradd")
                .args(["-m", "-s", shell, "-c", &full, u])
                .timeout(Duration::from_secs(30)),
        )?
        .ensure_success()?;

    if let Err(err) = configure_created_user(runner, u, args) {
        rollback_created_user(runner, u);
        return Err(err);
    }

    Ok(format!("usuario {u} creado"))
}

/// Actualiza un usuario existente.
pub fn update_user(runner: &Runner, args: &UpdateUserArgs) -> NasResult<String> {
    let u = args.username.trim();
    let _uid = ensure_manageable_existing_user(runner, u)?;

    let mut new_password: Option<&str> = None;
    if let Some(pass) = &args.password {
        if !pass.is_empty() && pass != "--keep-password" {
            validate_password(pass)?;
            new_password = Some(pass.as_str());
        }
    }

    for g in &args.groups {
        let g = g.trim();
        if !g.is_empty() && !is_valid_group(g) {
            return Err(NasError::Invalid(format!("grupo inválido: '{g}'")));
        }
    }

    let is_super = is_superadmin_account(runner, u);

    let full = sanitize_gecos(&args.full_name);
    runner
        .run(
            &CommandSpec::new("usermod")
                .args(["-c", &full, u])
                .timeout(Duration::from_secs(20)),
        )?
        .ensure_success()?;

    let shell = if args.admin || is_super {
        "/bin/bash"
    } else {
        "/usr/sbin/nologin"
    };
    runner
        .run(
            &CommandSpec::new("usermod")
                .args(["-s", shell, u])
                .timeout(Duration::from_secs(20)),
        )?
        .ensure_success()?;

    let mut has_new_password = false;
    if let Some(pass) = new_password {
        sync_system_password(runner, u, pass)?;
        has_new_password = true;
    }

    sync_groups(runner, u, args.admin, args.web, &args.groups)?;
    sync_samba(
        runner,
        u,
        args.samba,
        if has_new_password { new_password } else { None },
    )?;

    Ok(format!("usuario {u} actualizado"))
}

pub(super) fn toggle_usermod_steps(username: &str, enable: bool) -> Vec<Vec<String>> {
    if enable {
        vec![
            vec!["-e".into(), String::new(), username.into()],
            vec!["-U".into(), username.into()],
        ]
    } else {
        vec![
            vec!["-L".into(), username.into()],
            vec!["-e".into(), "1".into(), username.into()],
        ]
    }
}

/// Activa o suspende una cuenta de usuario.
pub fn toggle_user(runner: &Runner, username: &str, enable: bool) -> NasResult<String> {
    let u = username.trim();
    let _uid = ensure_manageable_existing_user(runner, u)?;

    if !enable && is_superadmin_account(runner, u) {
        return Err(NasError::Denied(
            "la cuenta superadministradora no puede suspenderse".into(),
        ));
    }

    for args in toggle_usermod_steps(u, enable) {
        runner
            .run(
                &CommandSpec::new("usermod")
                    .args(args)
                    .timeout(Duration::from_secs(20)),
            )?
            .ensure_success()?;
    }

    if samba_user_exists(runner, u) {
        let flag = if enable { "-e" } else { "-d" };
        runner
            .run(
                &CommandSpec::new("smbpasswd")
                    .args([flag, u])
                    .timeout(Duration::from_secs(20)),
            )?
            .ensure_success()?;
    }

    Ok(if enable {
        format!("usuario {u} reactivado")
    } else {
        format!("usuario {u} suspendido")
    })
}

/// Elimina una cuenta de usuario del sistema y de Samba.
pub fn delete_user(runner: &Runner, username: &str) -> NasResult<String> {
    let u = username.trim();
    let _uid = ensure_manageable_existing_user(runner, u)?;

    if is_superadmin_account(runner, u) {
        return Err(NasError::Denied(
            "la cuenta superadministradora está protegida".into(),
        ));
    }

    let _ = runner.run(&CommandSpec::new("smbpasswd").args(["-x", u]));
    let del = runner.run(&CommandSpec::new("userdel").args(["-r", u]))?;
    if !del.is_success() {
        runner
            .run(&CommandSpec::new("userdel").args([u]))?
            .ensure_success()?;
    }

    Ok(format!("usuario {u} eliminado"))
}
