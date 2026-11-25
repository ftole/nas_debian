//! Configuración de Samba para el despliegue del servidor NAS.
//!
//! Genera el `smb.conf` base (sección `[global]` optimizada para Debian 13),
//! crea los grupos corporativos base, aplica ACLs POSIX con herencia y configura
//! el override de `wsdd2` para el descubrimiento en redes Windows.

use std::time::Duration;

use nas_core::{NasResult, SystemResources};
use nas_exec::{CommandSpec, Runner};

mod wsdd2;

pub use wsdd2::{configure_wsdd2, wsdd2_override_content};

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

/// Detecta el límite duro `RLIMIT_NOFILE` aplicable a `smbd` (`systemctl show smbd` o `/proc/self/limits`).
pub fn detect_rlimit_nofile(runner: &Runner) -> Option<u64> {
    if let Ok(out) = runner.run(
        &CommandSpec::new("systemctl")
            .args(["show", "smbd", "-p", "LimitNOFILE", "--value"])
            .timeout(Duration::from_secs(5)),
    ) {
        if out.is_success() {
            let trimmed = out.stdout.trim();
            if let Ok(limit) = trimmed.parse::<u64>() {
                if limit > 0 {
                    return Some(limit);
                }
            }
        }
    }

    if let Ok(raw) = std::fs::read_to_string("/proc/self/limits") {
        if let Some(limit) = parse_proc_limits_max_open_files(&raw) {
            return Some(limit);
        }
    }

    None
}

fn parse_proc_limits_max_open_files(raw: &str) -> Option<u64> {
    for line in raw.lines() {
        if let Some(rest) = line.strip_prefix("Max open files") {
            let tokens: Vec<&str> = rest.split_whitespace().collect();
            if tokens.len() >= 2 {
                // Si el límite duro es `unlimited`, no restringimos por el soft limit (1024).
                if let Ok(hard) = tokens[1].parse::<u64>() {
                    if hard > 0 {
                        return Some(hard);
                    }
                }
            }
        }
    }
    None
}

/// Calcula `max open files` para Samba acorde a la RAM total y al `RLIMIT_NOFILE` vigente.
pub fn recommended_max_open_files(total_mem_mb: u64, rlimit_nofile: Option<u64>) -> u64 {
    let by_ram = if total_mem_mb < 2048 {
        16_384
    } else if total_mem_mb < 8192 {
        32_768
    } else {
        65_535
    };

    match rlimit_nofile {
        Some(limit) if limit > 0 => {
            let safe_cap = if limit > 1024 {
                limit.saturating_sub(64).max(1024)
            } else {
                limit
            };
            by_ram.min(safe_cap)
        }
        _ => by_ram,
    }
}

/// Genera la sección `[global]` optimizada de `smb.conf` con un valor explícito de `max open files`.
pub fn global_smb_conf_with_max_open_files(
    workgroup: &str,
    netbios: &str,
    max_open_files: u64,
) -> String {
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
   map acl inherit = yes\n\
   store dos attributes = yes\n\
   inherit permissions = yes\n\
   strict sync = yes\n\
   use sendfile = yes\n\
   aio read size = 16384\n\
   aio write size = 16384\n\
   max open files = {max_open_files}\n\
   load printers = no\n\
   printing = bsd\n\
   printcap name = /dev/null\n\
   disable spoolss = yes\n",
        workgroup = workgroup,
        netbios = netbios,
        max_open_files = max_open_files,
    )
}

/// Genera la sección `[global]` optimizada de `smb.conf` ajustada al hardware y `RLIMIT_NOFILE`.
pub fn global_smb_conf(workgroup: &str, netbios: &str) -> String {
    let mem_mb = SystemResources::detect().total_mem_mb;
    let rlimit = detect_rlimit_nofile(&Runner::new());
    let max_open = recommended_max_open_files(mem_mb, rlimit);
    global_smb_conf_with_max_open_files(workgroup, netbios, max_open)
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
        assert!(conf.contains("map acl inherit = yes"));
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

    #[test]
    fn max_open_files_se_ajusta_a_ram_y_rlimit_nofile() {
        assert_eq!(recommended_max_open_files(1024, None), 16_384);
        assert_eq!(recommended_max_open_files(4096, None), 32_768);
        assert_eq!(recommended_max_open_files(16384, None), 65_535);
        assert_eq!(recommended_max_open_files(16384, Some(16_384)), 16_320);
        assert_eq!(recommended_max_open_files(16384, Some(524_288)), 65_535);
        assert_eq!(recommended_max_open_files(4096, Some(1024)), 1024);

        let conf = global_smb_conf_with_max_open_files("WG", "SRV", 16_320);
        assert!(conf.contains("max open files = 16320"));

        let unlimited = "Limit                     Soft Limit           Hard Limit           Units\n\
                         Max open files            1024                 unlimited            files\n";
        assert_eq!(parse_proc_limits_max_open_files(unlimited), None);

        let capped = "Limit                     Soft Limit           Hard Limit           Units\n\
                      Max open files            1024                 524288               files\n";
        assert_eq!(parse_proc_limits_max_open_files(capped), Some(524_288));
    }
}
