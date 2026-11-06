//! Helpers para gestión validada de cuentas del sistema y de Samba.

use std::collections::HashSet;
use std::time::Duration;

use nas_core::{NasError, NasResult};
use nas_exec::{CommandSpec, Runner, SecretInput};
use regex::Regex;

/// Cuentas reservadas del sistema que nunca pueden gestionarse mediante el helper.
pub const RESERVED_USERS: &[&str] = &[
    "root",
    "daemon",
    "bin",
    "sys",
    "sync",
    "nobody",
    "nogroup",
    "www-data",
    "nas-web",
    "sshd",
    "messagebus",
];

/// Grupos protegidos de la plataforma que no pueden eliminarse ni renombrarse.
pub const PROTECTED_GROUPS: &[&str] = &["grp_samba", "grp_web", "grp_superadmin", "grp_backups"];

/// Valida si el nombre de usuario cumple con la sintaxis POSIX y no está reservado.
pub fn is_valid_user(username: &str) -> bool {
    let re = Regex::new(r"^[a-z_][a-z0-9_-]{0,31}$").expect("regex válido");
    if !re.is_match(username) {
        return false;
    }
    if username.starts_with("systemd-") {
        return false;
    }
    !RESERVED_USERS.contains(&username)
}

/// Valida si el nombre de grupo cumple con la convención corporativa `grp_*`.
pub fn is_valid_group(group: &str) -> bool {
    let re = Regex::new(r"^grp_[a-z0-9_-]{1,28}$").expect("regex válido");
    re.is_match(group)
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

fn sync_groups(
    runner: &Runner,
    user: &str,
    admin: bool,
    web: bool,
    extra_groups: &[String],
) -> NasResult<()> {
    let mut keep: HashSet<String> = HashSet::new();

    // Obtener grupos actuales para no perder grupos del sistema como sudo o adm
    if let Ok(out) = runner.run(
        &CommandSpec::new("id")
            .args(["-nG", user])
            .timeout(Duration::from_secs(10)),
    ) {
        if out.is_success() {
            for g in out.stdout.split_whitespace() {
                if !g.starts_with("grp_") || g == "grp_superadmin" {
                    keep.insert(g.to_string());
                }
            }
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
    runner.run(
        &CommandSpec::new("usermod")
            .args(["-G", &csv, user])
            .timeout(Duration::from_secs(20)),
    )?;
    Ok(())
}

fn sync_samba(runner: &Runner, user: &str, enable: bool, password: Option<&str>) -> NasResult<()> {
    let fail = |action: &str, output: &nas_exec::CommandOutput| -> NasError {
        let detail = if output.stderr.is_empty() {
            output.stdout.clone()
        } else {
            output.stderr.clone()
        };
        NasError::Invalid(format!("smbpasswd {action} falló para '{user}': {detail}"))
    };

    if enable {
        if let Some(pass) = password {
            if !pass.is_empty() {
                let spec = CommandSpec::new("smbpasswd")
                    .args(["-a", "-s", user])
                    .stdin_secret(SecretInput::new(format!("{pass}\n{pass}\n")))
                    .timeout(Duration::from_secs(30));
                let out = runner.run(&spec)?;
                if !out.is_success() {
                    return Err(fail("-a", &out));
                }
            }
        }
        let out = runner.run(
            &CommandSpec::new("smbpasswd")
                .args(["-e", user])
                .timeout(Duration::from_secs(20)),
        )?;
        if !out.is_success() {
            return Err(fail("-e", &out));
        }
    } else {
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
fn sync_system_password(runner: &Runner, user: &str, password: &str) -> NasResult<()> {
    let spec = CommandSpec::new("chpasswd")
        .stdin_secret(SecretInput::new(format!("{user}:{password}\n")))
        .timeout(Duration::from_secs(30));
    let out = runner.run(&spec)?;
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

/// Crea un usuario nuevo.
pub fn create_user(runner: &Runner, args: &CreateUserArgs) -> NasResult<String> {
    let u = args.username.trim();
    if !is_valid_user(u) {
        return Err(NasError::Invalid(
            "usuario inválido o reservado del sistema".into(),
        ));
    }
    if args.password.len() < 6 {
        return Err(NasError::Invalid(
            "contraseña demasiado corta (mínimo 6 caracteres)".into(),
        ));
    }

    // Comprobar si ya existe
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

    // Fijar contraseña de sistema (el fallo se propaga)
    sync_system_password(runner, u, &args.password)?;

    sync_groups(runner, u, args.admin, args.web, &args.groups)?;
    sync_samba(runner, u, args.samba, Some(&args.password))?;

    Ok(format!("usuario {u} creado"))
}

/// Actualiza un usuario existente.
pub fn update_user(runner: &Runner, args: &UpdateUserArgs) -> NasResult<String> {
    let u = args.username.trim();
    let _uid = ensure_manageable_existing_user(runner, u)?;
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
    if let Some(pass) = &args.password {
        if !pass.is_empty() && pass != "--keep-password" {
            if pass.len() < 6 {
                return Err(NasError::Invalid(
                    "contraseña demasiado corta (mínimo 6 caracteres)".into(),
                ));
            }
            sync_system_password(runner, u, pass)?;
            has_new_password = true;
        }
    }

    sync_groups(runner, u, args.admin, args.web, &args.groups)?;
    sync_samba(
        runner,
        u,
        args.samba,
        if has_new_password {
            args.password.as_deref()
        } else {
            None
        },
    )?;

    Ok(format!("usuario {u} actualizado"))
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

    if enable {
        for args in [vec!["-e", ""], vec!["-U"]] {
            runner
                .run(
                    &CommandSpec::new("usermod")
                        .args(args)
                        .timeout(Duration::from_secs(20)),
                )?
                .ensure_success()?;
        }
        runner
            .run(
                &CommandSpec::new("smbpasswd")
                    .args(["-e", u])
                    .timeout(Duration::from_secs(20)),
            )?
            .ensure_success()?;
        Ok(format!("usuario {u} reactivado"))
    } else {
        for args in [vec!["-L"], vec!["-e", "1"]] {
            runner
                .run(
                    &CommandSpec::new("usermod")
                        .args(args)
                        .timeout(Duration::from_secs(20)),
                )?
                .ensure_success()?;
        }
        runner
            .run(
                &CommandSpec::new("smbpasswd")
                    .args(["-d", u])
                    .timeout(Duration::from_secs(20)),
            )?
            .ensure_success()?;
        Ok(format!("usuario {u} suspendido"))
    }
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

    // `smbpasswd -x` es best-effort: puede que el usuario no existiera en Samba.
    let _ = runner.run(&CommandSpec::new("smbpasswd").args(["-x", u]));
    let del = runner.run(&CommandSpec::new("userdel").args(["-r", u]))?;
    if !del.is_success() {
        // Reintento sin --r por si el home no existe o está compartido.
        runner
            .run(&CommandSpec::new("userdel").args([u]))?
            .ensure_success()?;
    }

    Ok(format!("usuario {u} eliminado"))
}

/// Crea un grupo corporativo (`grp_*`).
pub fn create_group(runner: &Runner, group: &str) -> NasResult<String> {
    let g = group.trim();
    if !is_valid_group(g) {
        return Err(NasError::Invalid(format!(
            "grupo inválido: '{g}' (debe ser grp_<nombre>)"
        )));
    }
    runner
        .run(&CommandSpec::new("groupadd").args([g]))?
        .ensure_success()?;
    Ok(format!("grupo {g} creado"))
}

/// Renombra un grupo corporativo.
pub fn rename_group(runner: &Runner, old_name: &str, new_name: &str) -> NasResult<String> {
    let old = old_name.trim();
    let new = new_name.trim();
    if !is_valid_group(old) || !is_valid_group(new) {
        return Err(NasError::Invalid("nombre de grupo inválido".into()));
    }
    if PROTECTED_GROUPS.contains(&old) {
        return Err(NasError::Denied(format!("el grupo '{old}' está protegido")));
    }
    runner
        .run(&CommandSpec::new("groupmod").args(["-n", new, old]))?
        .ensure_success()?;
    Ok(format!("grupo {old} renombrado a {new}"))
}

/// Elimina un grupo corporativo.
pub fn delete_group(runner: &Runner, group: &str) -> NasResult<String> {
    let g = group.trim();
    if !is_valid_group(g) {
        return Err(NasError::Invalid(format!("grupo inválido: '{g}'")));
    }
    if PROTECTED_GROUPS.contains(&g) {
        return Err(NasError::Denied(format!("el grupo '{g}' está protegido")));
    }
    runner
        .run(&CommandSpec::new("groupdel").args([g]))?
        .ensure_success()?;
    Ok(format!("grupo {g} eliminado"))
}

/// Añade un usuario a un grupo corporativo.
pub fn add_group_member(runner: &Runner, username: &str, group: &str) -> NasResult<String> {
    let u = username.trim();
    let g = group.trim();
    let _uid = ensure_manageable_existing_user(runner, u)?;
    if !is_valid_group(g) {
        return Err(NasError::Invalid("grupo inválido".into()));
    }
    if g == "grp_superadmin" {
        return Err(NasError::Denied(
            "el grupo grp_superadmin está protegido".into(),
        ));
    }
    runner
        .run(&CommandSpec::new("gpasswd").args(["-a", u, g]))?
        .ensure_success()?;
    Ok(format!("{u} add-member {g}"))
}

/// Quita un usuario de un grupo corporativo.
pub fn remove_group_member(runner: &Runner, username: &str, group: &str) -> NasResult<String> {
    let u = username.trim();
    let g = group.trim();
    let _uid = ensure_manageable_existing_user(runner, u)?;
    if !is_valid_group(g) {
        return Err(NasError::Invalid("grupo inválido".into()));
    }
    if g == "grp_superadmin" {
        return Err(NasError::Denied(
            "el grupo grp_superadmin está protegido".into(),
        ));
    }
    runner
        .run(&CommandSpec::new("gpasswd").args(["-d", u, g]))?
        .ensure_success()?;
    Ok(format!("{u} remove-member {g}"))
}

/// Reporta el estado TSV de las cuentas gestionables.
pub fn accounts_status(runner: &Runner) -> NasResult<String> {
    let mut smb_active = HashSet::new();
    if let Ok(out) = runner.run(&CommandSpec::new("pdbedit").args(["-L", "-w"])) {
        if out.is_success() {
            for line in out.stdout.lines() {
                let parts: Vec<&str> = line.split(':').collect();
                if parts.len() >= 4 {
                    let u = parts[0];
                    let flags = parts[3];
                    if !u.is_empty() && !flags.contains('D') {
                        smb_active.insert(u.to_string());
                    }
                }
            }
        }
    }

    let mut tsv = String::new();
    let (users, _) = crate::users::load_local();
    for u in users {
        if u.system || u.uid < 1000 || u.uid >= 60000 {
            continue;
        }
        let mut enabled = "1";
        if let Ok(out) = runner.run(&CommandSpec::new("passwd").args(["-S", &u.name])) {
            if out.stdout.split_whitespace().nth(1) == Some("L") {
                enabled = "0";
            }
        }
        let smb = if smb_active.contains(&u.name) {
            "1"
        } else {
            "0"
        };
        tsv.push_str(&format!("{}\t{}\t{}\n", u.name, enabled, smb));
    }
    Ok(tsv)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validacion_de_usuarios() {
        assert!(is_valid_user("juan"));
        assert!(is_valid_user("operador_1"));
        assert!(!is_valid_user("root"));
        assert!(!is_valid_user("nas-web"));
        assert!(!is_valid_user("systemd-coredump"));
        assert!(!is_valid_user("Juan")); // mayúsculas
        assert!(!is_valid_user(""));
    }

    #[test]
    fn validacion_de_grupos() {
        assert!(is_valid_group("grp_sistemas"));
        assert!(is_valid_group("grp_contabilidad"));
        assert!(!is_valid_group("sistemas"));
        assert!(!is_valid_group("grp_"));
        assert!(!is_valid_group(
            "grp_nombre_demasiado_largo_que_excede_treinta_caracteres"
        ));
    }

    #[test]
    fn grupos_protegidos_no_se_pueden_eliminar() {
        let runner = Runner::new();
        let err = delete_group(&runner, "grp_superadmin").unwrap_err();
        assert!(err.to_string().contains("está protegido"));
        let err2 = rename_group(&runner, "grp_samba", "grp_otro").unwrap_err();
        assert!(err2.to_string().contains("está protegido"));
    }

    #[test]
    fn sanitizacion_gecos() {
        assert_eq!(sanitize_gecos("Juan:Pérez\r\n"), "JuanPérez");
    }
}
