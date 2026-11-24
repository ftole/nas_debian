//! Helpers para gestión validada de cuentas del sistema y de Samba.

use std::collections::{HashMap, HashSet};
use std::time::Duration;

use nas_core::{NasError, NasResult};
use nas_exec::{CommandSpec, Runner, SecretInput};

mod groups;

pub use groups::{
    accounts_status, add_group_member, create_group, delete_group, remove_group_member,
    rename_group,
};

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

fn sync_groups(
    runner: &Runner,
    user: &str,
    admin: bool,
    web: bool,
    extra_groups: &[String],
) -> NasResult<()> {
    let mut keep: HashSet<String> = HashSet::new();

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
    runner
        .run(
            &CommandSpec::new("usermod")
                .args(["-G", &csv, user])
                .timeout(Duration::from_secs(20)),
        )?
        .ensure_success()?;
    Ok(())
}

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

fn samba_user_exists(runner: &Runner, user: &str) -> bool {
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

fn chpasswd_payload(user: &str, password: &str) -> NasResult<String> {
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

fn smbpasswd_payload(password: &str) -> NasResult<String> {
    if password.chars().any(char::is_control) {
        return Err(NasError::Invalid(
            "la contraseña contiene caracteres de control no permitidos".into(),
        ));
    }
    Ok(format!("{password}\n{password}"))
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
fn sync_system_password(runner: &Runner, user: &str, password: &str) -> NasResult<()> {
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

fn toggle_usermod_steps(username: &str, enable: bool) -> Vec<Vec<String>> {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validacion_de_usuarios() {
        assert!(is_valid_user("juan"));
        assert!(is_valid_user("operador_1"));
        assert!(!is_valid_user("root"));
        assert!(!is_valid_user("nas"));
        assert!(!is_valid_user("nas-web"));
        assert!(!is_valid_user("systemd-coredump"));
        assert!(!is_valid_user("Juan"));
        assert!(!is_valid_user(""));
    }

    #[test]
    fn validacion_de_contrasenas_rechaza_cortas_y_control() {
        assert!(validate_password("ClaveSegura1!").is_ok());
        assert!(validate_password("abc:def").is_ok());
        assert!(validate_password("12345").is_err());
        assert!(validate_password("Clave\nroot:hack").is_err());
        assert!(validate_password("Clave\r123").is_err());
        assert!(validate_password("Clave\x00123").is_err());
        assert!(validate_password("Clave\t123").is_err());
    }

    #[test]
    fn create_user_rechaza_nas_y_contrasena_con_salto() {
        let runner = Runner::new();
        let err_nas = create_user(
            &runner,
            &CreateUserArgs {
                username: "nas".into(),
                full_name: "Operador".into(),
                password: "ClaveSegura1!".into(),
                admin: false,
                web: false,
                samba: false,
                groups: vec![],
            },
        )
        .unwrap_err();
        assert!(err_nas.to_string().contains("reservado"));

        let err_ctrl = create_user(
            &runner,
            &CreateUserArgs {
                username: "operador_test".into(),
                full_name: "Operador".into(),
                password: "Clave123\nroot:evil".into(),
                admin: false,
                web: false,
                samba: false,
                groups: vec![],
            },
        )
        .unwrap_err();
        assert!(err_ctrl.to_string().contains("caracteres de control"));
    }

    #[test]
    fn update_user_rechaza_nas_y_contrasena_invalida() {
        let runner = Runner::new();
        let err_nas = update_user(
            &runner,
            &UpdateUserArgs {
                username: "nas".into(),
                full_name: "Nas".into(),
                password: None,
                admin: false,
                web: false,
                samba: false,
                groups: vec![],
            },
        )
        .unwrap_err();
        assert!(err_nas.to_string().contains("reservado"));

        let err_root_ctrl = update_user(
            &runner,
            &UpdateUserArgs {
                username: "root".into(),
                full_name: "Root".into(),
                password: Some("Clave\n123".into()),
                admin: false,
                web: false,
                samba: false,
                groups: vec![],
            },
        )
        .unwrap_err();
        assert!(err_root_ctrl.to_string().contains("reservado"));
    }

    #[test]
    fn payloads_no_agregan_salto_final_redundante_ni_permiten_inyeccion() {
        assert_eq!(
            chpasswd_payload("juan", "Clave123!").unwrap(),
            "juan:Clave123!"
        );
        assert_eq!(
            smbpasswd_payload("Clave123!").unwrap(),
            "Clave123!\nClave123!"
        );
        assert!(chpasswd_payload("juan", "abc\nroot:evil").is_err());
        assert!(chpasswd_payload("juan", "abc\r123").is_err());
        assert!(chpasswd_payload("juan", "abc\x00123").is_err());
        assert!(chpasswd_payload("juan:root", "Clave123!").is_err());
        assert!(smbpasswd_payload("abc\n123").is_err());
        assert!(smbpasswd_payload("abc\r123").is_err());
        assert!(smbpasswd_payload("abc\x00123").is_err());
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

    #[test]
    fn parsea_salida_pdbedit_w() {
        let sample = "\
operador1:1001:XXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX:ABCDEF0123456789:[U          ]:LCT-65000000:
suspendido:1002:XXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX:ABCDEF0123456789:[DU         ]:LCT-65000000:
";
        let map = parse_pdbedit_w(sample);
        assert_eq!(map.get("operador1"), Some(&true));
        assert_eq!(map.get("suspendido"), Some(&false));
        assert_eq!(map.get("inexistente"), None);
    }

    #[test]
    fn toggle_usermod_siempre_incluye_usuario() {
        let enable_steps = toggle_usermod_steps("operador1", true);
        assert_eq!(
            enable_steps,
            vec![
                vec!["-e".to_string(), String::new(), "operador1".to_string()],
                vec!["-U".to_string(), "operador1".to_string()],
            ]
        );
        let disable_steps = toggle_usermod_steps("operador1", false);
        assert_eq!(
            disable_steps,
            vec![
                vec!["-L".to_string(), "operador1".to_string()],
                vec!["-e".to_string(), "1".to_string(), "operador1".to_string()],
            ]
        );
    }
}
