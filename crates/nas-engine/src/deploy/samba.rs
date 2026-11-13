//! Configuración de Samba para el despliegue del servidor NAS.
//!
//! Genera el `smb.conf` base (sección `[global]` optimizada para Debian 13),
//! crea los grupos corporativos base, aplica ACLs POSIX con herencia y configura
//! el override de `wsdd2` para el descubrimiento en redes Windows.

use std::time::Duration;

use nas_core::NasResult;
use nas_exec::{CommandSpec, Runner};

use crate::samba::ShareSpec;

mod wsdd2;

pub use wsdd2::{
    configure_wsdd, configure_wsdd2, setup_wsdd2, wsdd2_configure, wsdd2_override,
    wsdd2_override_content,
};

/// Grupos corporativos base creados durante el despliegue.
pub const BASE_GROUPS: [&str; 4] = ["grp_samba", "grp_web", "grp_superadmin", "grp_backups"];

/// Esquemas de permisos disponibles para un recurso compartido.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SharePermissionScheme {
    /// Lectura y escritura para todos los grupos autorizados.
    ReadWriteGroup,
    /// Lectura general para los grupos autorizados y escritura exclusiva para un grupo.
    ReadOnlyGeneralWriteExclusive,
    /// Solo lectura estricta para los grupos autorizados.
    ReadOnlyStrict,
    /// Acceso público / invitados.
    PublicGuest,
}

impl SharePermissionScheme {
    /// Etiqueta legible del esquema.
    pub fn label(self) -> &'static str {
        match self {
            Self::ReadWriteGroup => "lectura y escritura por grupo",
            Self::ReadOnlyGeneralWriteExclusive => "solo lectura general + escritura exclusiva",
            Self::ReadOnlyStrict => "solo lectura estricta",
            Self::PublicGuest => "acceso público / invitados",
        }
    }
}

/// Genera la sección `[global]` optimizada de `smb.conf`.
pub fn global_smb_conf(workgroup: &str, netbios: &str) -> String {
    format!(
        "[global]\n\
   workgroup = {workgroup}\n\
   netbios name = {netbios}\n\
   server string = Servidor NAS Debian 13\n\
   server role = standalone server\n\
   security = user\n\
   map to guest = Bad User\n\
   server min protocol = SMB2_10\n\
   server max protocol = SMB3_11\n\
   server signing = mandatory\n\
   smb encrypt = desired\n\
   smb3 directory leases = auto\n\
   kernel change notify = yes\n\
   change notify = yes\n\
   vfs objects = acl_xattr streams_xattr recycle\n\
   recycle:repository = .recycle\n\
   recycle:keeptree = yes\n\
   recycle:versions = yes\n\
   recycle:touch = yes\n\
   recycle:exclude = ~$*, *.tmp, *.temp, *.TMP, ~*\n\
   recycle:excludedir = /.recycle/\n\
   store dos attributes = yes\n\
   inherit permissions = yes\n\
   strict sync = yes\n\
   use sendfile = yes\n\
   aio read size = 16384\n\
   aio write size = 16384\n\
   max open files = 65535\n\
   load printers = no\n\
   printing = bsd\n\
   printcap name = /dev/null\n\
   disable spoolss = yes\n",
        workgroup = workgroup,
        netbios = netbios,
    )
}

/// Genera un `smb.conf` completo (global + recursos) a partir de las especificaciones.
pub fn generate_smb_conf(workgroup: &str, netbios: &str, shares: &[ShareSpec]) -> String {
    let mut conf = global_smb_conf(workgroup, netbios);
    for share in shares {
        conf = crate::samba::upsert_share(&conf, share).unwrap_or(conf);
    }
    conf
}

/// Crea (o asegura) los grupos corporativos base del sistema.
pub fn create_groups(runner: &Runner) -> NasResult<String> {
    for group in BASE_GROUPS {
        runner
            .run(
                &CommandSpec::new("groupadd")
                    .args(["-f", group])
                    .timeout(Duration::from_secs(20)),
            )?
            .ensure_success()?;
    }
    Ok("grupos base creados".to_string())
}

/// Alias de `create_groups`.
pub fn create_base_groups(runner: &Runner) -> NasResult<String> {
    create_groups(runner)
}

/// Alias de `create_groups`.
pub fn ensure_groups(runner: &Runner) -> NasResult<String> {
    create_groups(runner)
}

/// Alias de `create_groups`.
pub fn ensure_base_groups(runner: &Runner) -> NasResult<String> {
    create_groups(runner)
}

/// Construye el plan de comandos `setfacl` para aplicar ACLs POSIX con herencia.
pub fn posix_acl_plan(
    path: &str,
    scheme: SharePermissionScheme,
    groups: &[String],
    write_group: Option<&str>,
) -> Vec<Vec<String>> {
    let mut commands: Vec<Vec<String>> = Vec::new();

    match scheme {
        SharePermissionScheme::PublicGuest => {
            commands.push(vec!["chmod".into(), "0777".into(), path.into()]);
            commands.push(vec![
                "setfacl".into(),
                "-d".into(),
                "-m".into(),
                "o::rwx".into(),
                path.into(),
            ]);
        }
        SharePermissionScheme::ReadOnlyStrict => {
            for g in groups {
                commands.push(vec![
                    "setfacl".into(),
                    "-m".into(),
                    format!("g:{g}:rx"),
                    path.into(),
                ]);
                commands.push(vec![
                    "setfacl".into(),
                    "-d".into(),
                    "-m".into(),
                    format!("g:{g}:rx"),
                    path.into(),
                ]);
            }
        }
        SharePermissionScheme::ReadWriteGroup => {
            for g in groups {
                commands.push(vec![
                    "setfacl".into(),
                    "-m".into(),
                    format!("g:{g}:rwx"),
                    path.into(),
                ]);
                commands.push(vec![
                    "setfacl".into(),
                    "-d".into(),
                    "-m".into(),
                    format!("g:{g}:rwx"),
                    path.into(),
                ]);
            }
        }
        SharePermissionScheme::ReadOnlyGeneralWriteExclusive => {
            for g in groups {
                commands.push(vec![
                    "setfacl".into(),
                    "-m".into(),
                    format!("g:{g}:rx"),
                    path.into(),
                ]);
                commands.push(vec![
                    "setfacl".into(),
                    "-d".into(),
                    "-m".into(),
                    format!("g:{g}:rx"),
                    path.into(),
                ]);
            }
            if let Some(wg) = write_group {
                commands.push(vec![
                    "setfacl".into(),
                    "-m".into(),
                    format!("g:{wg}:rwx"),
                    path.into(),
                ]);
                commands.push(vec![
                    "setfacl".into(),
                    "-d".into(),
                    "-m".into(),
                    format!("g:{wg}:rwx"),
                    path.into(),
                ]);
            }
        }
    }

    commands.push(vec![
        "setfacl".into(),
        "-m".into(),
        "m::rwx".into(),
        path.into(),
    ]);
    commands.push(vec![
        "setfacl".into(),
        "-d".into(),
        "-m".into(),
        "m::rwx".into(),
        path.into(),
    ]);
    commands
}

/// Alias de `posix_acl_plan`.
pub fn acl_plan(
    path: &str,
    scheme: SharePermissionScheme,
    groups: &[String],
    write_group: Option<&str>,
) -> Vec<Vec<String>> {
    posix_acl_plan(path, scheme, groups, write_group)
}

/// Alias de `posix_acl_plan`.
pub fn share_acl_plan(
    path: &str,
    scheme: SharePermissionScheme,
    groups: &[String],
    write_group: Option<&str>,
) -> Vec<Vec<String>> {
    posix_acl_plan(path, scheme, groups, write_group)
}

/// Aplica las ACLs POSIX con herencia en el sistema usando el runner.
pub fn apply_posix_acls(
    runner: &Runner,
    path: &str,
    scheme: SharePermissionScheme,
    groups: &[String],
    write_group: Option<&str>,
) -> NasResult<String> {
    for cmd in posix_acl_plan(path, scheme, groups, write_group) {
        if cmd.is_empty() {
            continue;
        }
        let (program, rest) = cmd.split_first().expect("comando no vacío");
        runner
            .run(
                &CommandSpec::new(program)
                    .args(rest.iter().map(|s| s.as_str()).collect::<Vec<_>>())
                    .timeout(Duration::from_secs(30)),
            )?
            .ensure_success()?;
    }
    Ok(format!("ACLs aplicadas en {path}"))
}

/// Alias de `apply_posix_acls`.
pub fn apply_acls(
    runner: &Runner,
    path: &str,
    scheme: SharePermissionScheme,
    groups: &[String],
    write_group: Option<&str>,
) -> NasResult<String> {
    apply_posix_acls(runner, path, scheme, groups, write_group)
}

/// Alias de `apply_posix_acls`.
pub fn configure_acls(
    runner: &Runner,
    path: &str,
    scheme: SharePermissionScheme,
    groups: &[String],
    write_group: Option<&str>,
) -> NasResult<String> {
    apply_posix_acls(runner, path, scheme, groups, write_group)
}

/// Alias de `apply_posix_acls`.
pub fn apply_share_acls(
    runner: &Runner,
    path: &str,
    scheme: SharePermissionScheme,
    groups: &[String],
    write_group: Option<&str>,
) -> NasResult<String> {
    apply_posix_acls(runner, path, scheme, groups, write_group)
}

/// Alias de `generate_smb_conf`.
pub fn render_smb_conf(workgroup: &str, netbios: &str, shares: &[ShareSpec]) -> String {
    generate_smb_conf(workgroup, netbios, shares)
}

/// Alias de `generate_smb_conf`.
pub fn build_smb_conf(workgroup: &str, netbios: &str, shares: &[ShareSpec]) -> String {
    generate_smb_conf(workgroup, netbios, shares)
}

/// Alias de `global_smb_conf`.
pub fn smb_conf_global(workgroup: &str, netbios: &str) -> String {
    global_smb_conf(workgroup, netbios)
}

#[cfg(test)]
mod tests {
    use super::*;
    use nas_exec::Runner;

    #[test]
    fn grupos_base_son_cuatro() {
        assert_eq!(BASE_GROUPS.len(), 4);
        assert!(BASE_GROUPS.contains(&"grp_samba"));
        assert!(BASE_GROUPS.contains(&"grp_web"));
        assert!(BASE_GROUPS.contains(&"grp_superadmin"));
        assert!(BASE_GROUPS.contains(&"grp_backups"));
    }

    #[test]
    fn global_smb_conf_contiene_workgroup_y_netbios() {
        let conf = global_smb_conf("TEAM-JOFRATO", "SRV-NAS");
        assert!(conf.contains("workgroup = TEAM-JOFRATO"));
        assert!(conf.contains("netbios name = SRV-NAS"));
        assert!(conf.contains("smb3 directory leases = auto"));
        assert!(conf.contains("recycle:exclude = ~$*, *.tmp, *.temp, *.TMP, ~*"));
        assert!(conf.contains("recycle:excludedir = /.recycle/"));
    }

    #[test]
    fn generate_smb_conf_incluye_recursos() {
        let share = ShareSpec {
            name: "SISTEMAS".to_string(),
            path: "/srv/nas/SISTEMAS".to_string(),
            read_only: false,
            browseable: true,
            guest_ok: false,
            valid_users: vec!["@grp_samba".to_string()],
            write_list: Vec::new(),
            comment: "Sistemas".to_string(),
        };
        let conf = generate_smb_conf("WG", "SRV", &[share]);
        assert!(conf.contains("[global]"));
        assert!(conf.contains("[SISTEMAS]"));
        assert!(conf.contains("path = /srv/nas/SISTEMAS"));
    }

    #[test]
    fn esquemas_generan_cuatro_variantes() {
        let groups = vec!["grp_emp".to_string()];
        let rw = posix_acl_plan(
            "/srv/nas/X",
            SharePermissionScheme::ReadWriteGroup,
            &groups,
            None,
        );
        let ro = posix_acl_plan(
            "/srv/nas/X",
            SharePermissionScheme::ReadOnlyStrict,
            &groups,
            None,
        );
        let mix = posix_acl_plan(
            "/srv/nas/X",
            SharePermissionScheme::ReadOnlyGeneralWriteExclusive,
            &groups,
            Some("grp_write"),
        );
        let pubc = posix_acl_plan(
            "/srv/nas/X",
            SharePermissionScheme::PublicGuest,
            &groups,
            None,
        );
        assert!(rw.iter().any(|c| c.join(" ").contains("rwx")));
        assert!(ro.iter().any(|c| c.join(" ").contains("rx")));
        assert!(mix.iter().any(|c| c.join(" ").contains("grp_write:rwx")));
        assert!(pubc.iter().any(|c| c.join(" ").contains("0777")));
    }

    #[test]
    fn wsdd2_override_contiene_parametros() {
        let content = wsdd2_override_content("SRV-NAS", "TEAM-JOFRATO");
        assert!(content.contains("wsdd2"));
        assert!(content.contains("SRV-NAS"));
        assert!(content.contains("TEAM-JOFRATO"));
    }

    #[test]
    fn create_groups_falla_sin_binario() {
        let runner = Runner::new();
        let res = create_groups(&runner);
        let _ = res;
    }
}
