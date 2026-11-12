//! Aprovisionamiento de usuarios, roles y sudoers durante el despliegue.
//!
//! Porta la lógica de `src/core/deploy.sh` relacionada con la creación del
//! superadministrador (cuenta nueva o existente), la asignación de roles
//! (`superadmin`, `admin`, `operator`), la creación de grupos corporativos y
//! la instalación de la plantilla de sudoers para el panel web.

use std::time::Duration;

use nas_core::{NasError, NasResult};
use nas_exec::{CommandSpec, Runner};

use super::Artifact;

/// Grupos corporativos que el despliegue garantiza que existan.
pub const DEPLOY_GROUPS: &[&str] = &["grp_samba", "grp_web", "grp_superadmin", "grp_backups"];

/// Roles de despliegue disponibles para una cuenta.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeployRole {
    /// Control total e inmutable.
    Superadmin,
    /// Administrador del panel (sudo + Samba).
    Admin,
    /// Operador del panel (solo grp_web).
    Operator,
}

impl DeployRole {
    /// Nombre canónico del rol.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Superadmin => "superadmin",
            Self::Admin => "admin",
            Self::Operator => "operator",
        }
    }

    /// Parsea un rol a partir de su nombre canónico.
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(value: &str) -> NasResult<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "superadmin" => Ok(Self::Superadmin),
            "admin" => Ok(Self::Admin),
            "operator" => Ok(Self::Operator),
            other => Err(NasError::Invalid(format!(
                "rol de despliegue desconocido: '{other}'"
            ))),
        }
    }

    /// Grupos POSIX que corresponden al rol.
    pub fn groups(self) -> &'static [&'static str] {
        match self {
            Self::Superadmin => &["sudo", "adm", "grp_samba", "grp_superadmin"],
            Self::Admin => &["sudo", "adm", "grp_samba"],
            Self::Operator => &["grp_web"],
        }
    }

    /// Shell de login asignado al rol.
    pub fn shell(self) -> &'static str {
        match self {
            Self::Superadmin | Self::Admin => "/bin/bash",
            Self::Operator => "/usr/sbin/nologin",
        }
    }
}

/// Asegura la existencia de los grupos corporativos del despliegue.
pub fn ensure_groups(runner: &Runner) -> NasResult<()> {
    for group in DEPLOY_GROUPS {
        runner
            .run(
                &CommandSpec::new("groupadd")
                    .args(["-f", group])
                    .timeout(Duration::from_secs(20)),
            )?
            .ensure_success()?;
    }
    Ok(())
}

/// Comprueba si una cuenta ya existe en el sistema.
pub fn account_exists(runner: &Runner, username: &str) -> bool {
    runner
        .run(
            &CommandSpec::new("id")
                .args(["-u", username])
                .timeout(Duration::from_secs(10)),
        )
        .map(|out| out.is_success())
        .unwrap_or(false)
}

/// Asegura que los grupos del rol estén asignados a la cuenta.
pub fn apply_role(runner: &Runner, username: &str, role: DeployRole) -> NasResult<()> {
    let groups = role.groups().join(",");
    runner
        .run(
            &CommandSpec::new("usermod")
                .args(["-s", role.shell(), username])
                .timeout(Duration::from_secs(20)),
        )?
        .ensure_success()?;
    if !groups.is_empty() {
        runner
            .run(
                &CommandSpec::new("usermod")
                    .args(["-aG", &groups, username])
                    .timeout(Duration::from_secs(20)),
            )?
            .ensure_success()?;
    }
    Ok(())
}

/// Aprovisiona el superadministrador: crea la cuenta si no existe o reutiliza la
/// existente, y le asigna el rol `superadmin`.
pub fn provision_superadmin(
    runner: &Runner,
    username: &str,
    password: Option<&str>,
    full_name: &str,
) -> NasResult<bool> {
    let created = if account_exists(runner, username) {
        false
    } else {
        let mut args = vec!["-m".to_string(), "-s".to_string(), "/bin/bash".to_string()];
        if !full_name.trim().is_empty() {
            args.push("-c".to_string());
            args.push(full_name.to_string());
        }
        args.push(username.to_string());
        runner
            .run(
                &CommandSpec::new("useradd")
                    .args(args)
                    .timeout(Duration::from_secs(30)),
            )?
            .ensure_success()?;

        if let Some(pass) = password {
            if !pass.is_empty() {
                let spec = CommandSpec::new("chpasswd")
                    .stdin_secret(nas_exec::SecretInput::new(format!("{username}:{pass}\n")))
                    .timeout(Duration::from_secs(30));
                runner.run(&spec)?.ensure_success()?;
            }
        }
        true
    };

    apply_role(runner, username, DeployRole::Superadmin)?;
    Ok(created)
}

/// Contenido de la plantilla de sudoers para el panel web.
pub fn sudoers_content() -> String {
    "Cmnd_Alias NAS_USERS = /usr/sbin/useradd, /usr/sbin/userdel, /usr/sbin/usermod,\n\
     /usr/sbin/groupadd, /usr/sbin/groupdel, /usr/sbin/groupmod\n\
# nas-web sudoers template (generado por nas deploy)\n\
%grp_superadmin ALL=(ALL) NOPASSWD: ALL\n"
        .to_string()
}

/// Artefacto de sudoers generado por el despliegue.
pub fn sudoers_artifact() -> Artifact {
    Artifact {
        path: "/etc/sudoers.d/nas-web".into(),
        mode: 0o440,
        content: sudoers_content(),
    }
}

/// Escribe la plantilla de sudoers en disco validándola con visudo.
pub fn install_sudoers(runner: &Runner) -> NasResult<()> {
    let artifact = sudoers_artifact();
    let spec = CommandSpec::new("tee")
        .args([&artifact.path])
        .stdin_secret(nas_exec::SecretInput::new(artifact.content.clone()))
        .timeout(Duration::from_secs(20));
    runner.run(&spec)?.ensure_success()?;

    let check = runner.run(
        &CommandSpec::new("visudo")
            .args(["-cf", &artifact.path])
            .timeout(Duration::from_secs(20)),
    )?;
    if !check.is_success() {
        return Err(NasError::Invalid(format!(
            "sudoers inválido: {}",
            if check.stderr.is_empty() {
                check.stdout
            } else {
                check.stderr
            }
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parseo_de_roles() {
        assert_eq!(
            DeployRole::from_str("superadmin").unwrap(),
            DeployRole::Superadmin
        );
        assert_eq!(DeployRole::from_str("ADMIN").unwrap(), DeployRole::Admin);
        assert_eq!(
            DeployRole::from_str("operator").unwrap(),
            DeployRole::Operator
        );
        assert!(DeployRole::from_str("root").is_err());
    }

    #[test]
    fn roles_tienen_grupos_y_shell() {
        assert!(DeployRole::Superadmin.groups().contains(&"grp_superadmin"));
        assert_eq!(DeployRole::Operator.shell(), "/usr/sbin/nologin");
        assert_eq!(DeployRole::Admin.shell(), "/bin/bash");
    }

    #[test]
    fn grupos_de_despliegue_incluyen_los_base() {
        assert!(DEPLOY_GROUPS.contains(&"grp_samba"));
        assert!(DEPLOY_GROUPS.contains(&"grp_superadmin"));
    }

    #[test]
    fn sudoers_contiene_superadmin() {
        let content = sudoers_content();
        assert!(content.contains("grp_superadmin"));
        assert!(content.contains("nas-web"));
    }

    #[test]
    fn artefacto_sudoers_tiene_ruta() {
        let art = sudoers_artifact();
        assert_eq!(art.path, "/etc/sudoers.d/nas-web");
        assert_eq!(art.mode, 0o440);
    }
}
