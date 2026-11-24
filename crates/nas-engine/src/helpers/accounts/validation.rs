//! Validadores y tipos compartidos para gestión de cuentas del sistema y Samba.

use std::time::Duration;

use nas_core::{NasError, NasResult};
use nas_exec::{CommandSpec, Runner};

/// Cuentas reservadas del sistema que nunca pueden gestionarse mediante el helper.
pub use crate::validation::RESERVED_SYSTEM_USERS as RESERVED_USERS;

/// Grupos protegidos de la plataforma que no pueden eliminarse ni renombrarse.
pub const PROTECTED_GROUPS: &[&str] = &["grp_samba", "grp_web", "grp_superadmin", "grp_backups"];

/// Valida si el nombre de usuario cumple con la sintaxis POSIX y no está reservado.
pub fn is_valid_user(username: &str) -> bool {
    crate::validation::is_valid_username_syntax(username)
        && !crate::validation::is_reserved_system_user(username)
}

/// Valida si el nombre de grupo cumple con la convención corporativa `grp_*`.
pub fn is_valid_group(group: &str) -> bool {
    crate::validation::is_valid_group_name(group)
}

/// Comprueba si la cuenta es la superadministradora (inmutable).
pub fn is_superadmin_account(runner: &Runner, username: &str) -> bool {
    if let Ok(sa) = std::fs::read_to_string("/etc/nas/superadmin") {
        let trimmed = sa.trim();
        if !trimmed.is_empty() && trimmed == username {
            return true;
        }
    }
    if let Ok(out) = runner.run(
        &CommandSpec::new("id")
            .args(["-nG", username])
            .timeout(Duration::from_secs(10)),
    ) {
        if out.is_success() && out.stdout.split_whitespace().any(|g| g == "grp_superadmin") {
            return true;
        }
    }
    false
}

/// Valida que el usuario exista y tenga un UID en el rango gestionable (1000..60000).
pub fn ensure_manageable_existing_user(runner: &Runner, username: &str) -> NasResult<u32> {
    if !is_valid_user(username) {
        return Err(NasError::Denied(
            "usuario inválido o reservado del sistema".into(),
        ));
    }

    let out = runner.run(
        &CommandSpec::new("id")
            .args(["-u", username])
            .timeout(Duration::from_secs(10)),
    )?;
    if !out.is_success() {
        return Err(NasError::NotFound("el usuario no existe".into()));
    }

    let uid: u32 = out
        .stdout
        .trim()
        .parse()
        .map_err(|_| NasError::Invalid("no se pudo determinar el UID del usuario".into()))?;

    if !(1000..60000).contains(&uid) {
        return Err(NasError::Denied(format!(
            "operación prohibida sobre cuenta del sistema (UID {uid})"
        )));
    }

    Ok(uid)
}

/// Sanea el campo GECOS eliminando dos puntos, retornos de carro y bytes nulos.
pub fn sanitize_gecos(name: &str) -> String {
    name.chars()
        .filter(|c| !matches!(c, ':' | '\r' | '\n' | '\0'))
        .collect()
}

/// Valida que la contraseña cumpla la longitud mínima y carezca de caracteres de control.
pub fn validate_password(password: &str) -> NasResult<()> {
    if password.chars().any(char::is_control) {
        return Err(NasError::Invalid(
            "la contraseña contiene caracteres de control no permitidos".into(),
        ));
    }
    if password.len() < 6 {
        return Err(NasError::Invalid(
            "contraseña demasiado corta (mínimo 6 caracteres)".into(),
        ));
    }
    Ok(())
}

/// Argumentos para crear una cuenta.
#[derive(Debug, Clone)]
pub struct CreateUserArgs {
    /// Nombre de usuario POSIX.
    pub username: String,
    /// Nombre completo (GECOS).
    pub full_name: String,
    /// Contraseña (mínimo 6 caracteres).
    pub password: String,
    /// Conceder privilegios de administración (bash y grp_samba).
    pub admin: bool,
    /// Conceder acceso web (grp_web).
    pub web: bool,
    /// Habilitar cuenta en Samba.
    pub samba: bool,
    /// Lista de grupos corporativos (`grp_*`).
    pub groups: Vec<String>,
}

/// Argumentos para actualizar una cuenta existente.
#[derive(Debug, Clone)]
pub struct UpdateUserArgs {
    /// Nombre de usuario POSIX.
    pub username: String,
    /// Nombre completo (GECOS).
    pub full_name: String,
    /// Nueva contraseña (None o vacía para conservar la existente).
    pub password: Option<String>,
    /// Conceder privilegios de administración.
    pub admin: bool,
    /// Conceder acceso web.
    pub web: bool,
    /// Habilitar cuenta en Samba.
    pub samba: bool,
    /// Lista de grupos corporativos (`grp_*`).
    pub groups: Vec<String>,
}
