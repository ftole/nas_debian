//! Artefactos de configuración del sistema, hardening y persistencia en `/etc/fstab`.

use nas_core::{NasError, NasResult, ResourceBudget, SystemResources};

use super::DeployParams;

/// Archivo de configuración generado por el despliegue.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Artifact {
    /// Ruta absoluta de destino.
    pub path: String,
    /// Contenido completo.
    pub content: String,
    /// Permisos octales.
    pub mode: u32,
}

const SYSCTL_TUNING: &str = include_str!("../../../../packaging/99-nas-tuning.conf");

const UDEV_READAHEAD: &str = include_str!("../../../../packaging/60-nas-readahead.rules");

const LOGROTATE_BACKUPS: &str = "/srv/nas/LOGS_BACKUP/*.log {
    su root grp_samba
    weekly
    rotate 8
    compress
    missingok
    notifempty
    copytruncate
}
";

const AUDIT_RULES: &str = include_str!("../../../../packaging/nas.rules");

const AIDE_NAS_RULES: &str = include_str!("../../../../packaging/75_aide_nas");

const JOURNALD_LIMITS: &str = include_str!("../../../../packaging/99-nas-journald.conf");

/// Renderiza `99-nas-tuning.conf` ajustando `vm.dirty_background_bytes` y `vm.dirty_bytes`
/// de forma proporcional a la RAM total (`total_mem_mb`).
pub fn render_sysctl_tuning(total_mem_mb: u64) -> String {
    let mem = total_mem_mb.max(256);
    let snap = SystemResources::from_snapshot(1, None, mem, mem, None, 0.0, 0.0, 0.0, 0.0);
    let (bg_bytes, dirty_bytes) = ResourceBudget::from_resources(snap).sysctl_dirty_bytes();

    let mut lines: Vec<String> = Vec::new();
    for line in SYSCTL_TUNING.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("vm.dirty_background_bytes") {
            lines.push(format!("vm.dirty_background_bytes = {bg_bytes}"));
        } else if trimmed.starts_with("vm.dirty_bytes") {
            lines.push(format!("vm.dirty_bytes = {dirty_bytes}"));
        } else {
            lines.push(line.to_string());
        }
    }
    let mut rendered = lines.join("\n");
    if SYSCTL_TUNING.ends_with('\n') && !rendered.ends_with('\n') {
        rendered.push('\n');
    }
    rendered
}

impl DeployParams {
    /// Configuración base limpia `[global]` de Samba optimizada para Debian 13.
    pub fn base_smb_conf(&self) -> String {
        super::samba::global_smb_conf(&self.workgroup, &self.netbios)
    }

    /// Archivos de configuración del sistema que escribe el despliegue nativo.
    pub fn artifacts(&self) -> Vec<Artifact> {
        let total_mem_mb = SystemResources::detect().total_mem_mb;
        self.artifacts_for_mem(total_mem_mb)
    }

    /// Archivos de configuración del sistema calculados para una cantidad dada de RAM (MiB).
    pub fn artifacts_for_mem(&self, total_mem_mb: u64) -> Vec<Artifact> {
        let mut artifacts = vec![
            Artifact {
                path: "/etc/nas/role".into(),
                mode: 0o644,
                content: format!("{}\n", self.role.as_str()),
            },
            Artifact {
                path: "/etc/sysctl.d/99-nas-tuning.conf".into(),
                mode: 0o644,
                content: render_sysctl_tuning(total_mem_mb),
            },
            Artifact {
                path: "/etc/udev/rules.d/60-nas-readahead.rules".into(),
                mode: 0o644,
                content: UDEV_READAHEAD.to_string(),
            },
            Artifact {
                path: "/etc/logrotate.d/nas-backups".into(),
                mode: 0o644,
                content: LOGROTATE_BACKUPS.to_string(),
            },
            Artifact {
                path: "/etc/audit/rules.d/nas.rules".into(),
                mode: 0o640,
                content: AUDIT_RULES.to_string(),
            },
            Artifact {
                path: "/etc/aide/aide.conf.d/75_aide_nas".into(),
                mode: 0o644,
                content: AIDE_NAS_RULES.to_string(),
            },
            Artifact {
                path: "/etc/systemd/journald.conf.d/99-nas-limits.conf".into(),
                mode: 0o644,
                content: JOURNALD_LIMITS.to_string(),
            },
            Artifact {
                path: "/etc/cron.daily/nas-recycle-purge".into(),
                mode: 0o755,
                content: "#!/bin/bash\n\
# nas-recycle-purge: Purga automática de archivos en papeleras Samba con más de 15 días\n\
if [ -x /usr/local/bin/nas ]; then\n\
    /usr/local/bin/nas helper purge-recycle --days 15 >/dev/null 2>&1 || true\n\
else\n\
    find /srv/nas -type f -path '*/.recycle/*' -mtime +15 -delete 2>/dev/null || true\n\
    find /srv/nas -type d -path '*/.recycle/*' -empty -delete 2>/dev/null || true\n\
fi\n"
                    .to_string(),
            },
        ];

        if !self.role.as_str().eq("ARCHIVOS") {
            artifacts.push(Artifact {
                path: "/etc/cron.d/nas-btrfs-scrub".into(),
                mode: 0o644,
                content: "SHELL=/bin/bash\n\
PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin\n\
0 2 1 * * root nice -n 19 ionice -c 3 btrfs scrub start -B /srv/nas >/dev/null 2>&1\n"
                    .to_string(),
            });
        }

        artifacts
    }

    /// Conjunto completo de artefactos del despliegue (incluye superadmin y `smb.conf` base).
    pub fn all_artifacts(&self) -> Vec<Artifact> {
        let mut list = self.artifacts();
        list.push(Artifact {
            path: "/etc/nas/superadmin".into(),
            mode: 0o644,
            content: format!("{}\n", self.admin_user),
        });
        if !self.keep_data {
            list.push(Artifact {
                path: "/etc/samba/smb.conf".into(),
                mode: 0o644,
                content: self.base_smb_conf(),
            });
        }
        list
    }
}

/// Aplica el hardening generado (sysctl, udev, auditd) en el sistema.
pub fn apply_hardening(runner: &nas_exec::Runner, sudo: bool) -> NasResult<()> {
    let configure = |spec: nas_exec::CommandSpec| nas_exec::CommandSpec {
        sudo: if sudo {
            nas_exec::SudoMode::Passwordless
        } else {
            nas_exec::SudoMode::None
        },
        ..spec
    };

    // sysctl
    let _ = runner.run(&configure(
        nas_exec::CommandSpec::new("sysctl")
            .args(["--system"])
            .timeout(std::time::Duration::from_secs(30)),
    ));

    // udev
    let _ = runner.run(&configure(
        nas_exec::CommandSpec::new("udevadm")
            .args(["control", "--reload-rules"])
            .timeout(std::time::Duration::from_secs(30)),
    ));
    let _ = runner.run(&configure(
        nas_exec::CommandSpec::new("udevadm")
            .args(["trigger"])
            .timeout(std::time::Duration::from_secs(30)),
    ));

    // auditd y journald
    let _ = runner.run(&configure(
        nas_exec::CommandSpec::new("systemctl")
            .args(["restart", "auditd"])
            .timeout(std::time::Duration::from_secs(30)),
    ));
    let _ = runner.run(&configure(
        nas_exec::CommandSpec::new("systemctl")
            .args(["restart", "systemd-journald"])
            .timeout(std::time::Duration::from_secs(30)),
    ));
    let _ = runner.run(&configure(
        nas_exec::CommandSpec::new("systemctl")
            .args([
                "mask",
                "sleep.target",
                "suspend.target",
                "hibernate.target",
                "hybrid-sleep.target",
            ])
            .timeout(std::time::Duration::from_secs(30)),
    ));

    Ok(())
}

/// Agrega `nofail` y timeout acotado a puntos de montaje secundarios en `/etc/fstab`
/// para evitar bloqueos de arranque sin SSH tras fallos de almacenamiento o apagones.
pub fn fstab_options_for_target(target: &str, options: &str) -> String {
    if target.trim() == "/" {
        return options.to_string();
    }
    let mut parts: Vec<String> = options
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToString::to_string)
        .collect();
    if !parts.iter().any(|p| p == "nofail") {
        parts.push("nofail".to_string());
    }
    if !parts
        .iter()
        .any(|p| p.starts_with("x-systemd.device-timeout="))
    {
        parts.push("x-systemd.device-timeout=15s".to_string());
    }
    parts.join(",")
}

/// Persiste una entrada de montaje en `/etc/fstab` usando el UUID si está disponible.
pub fn update_fstab(
    runner: &nas_exec::Runner,
    source: &str,
    target: &str,
    options: &str,
) -> NasResult<()> {
    let fstab_path = std::path::Path::new("/etc/fstab");
    if !fstab_path.exists() {
        return Ok(());
    }

    let uuid_res = runner.run(
        &nas_exec::CommandSpec::new("blkid")
            .args(["-s", "UUID", "-o", "value", source])
            .timeout(std::time::Duration::from_secs(10)),
    );
    let fstab_src = match uuid_res {
        Ok(out) if out.is_success() && !out.stdout.trim().is_empty() => {
            format!("UUID={}", out.stdout.trim())
        }
        _ => source.to_string(),
    };

    let content = std::fs::read_to_string(fstab_path).unwrap_or_default();
    let mut new_lines: Vec<String> = content
        .lines()
        .filter(|line| {
            let trimmed = line.trim();
            if trimmed.starts_with('#') {
                return true;
            }
            let parts: Vec<&str> = trimmed.split_whitespace().collect();
            !(parts.len() >= 2 && parts[1] == target)
        })
        .map(ToString::to_string)
        .collect();

    let fstab_options = fstab_options_for_target(target, options);
    new_lines.push(format!("{fstab_src} {target} auto {fstab_options} 0 0"));
    let final_content = format!("{}\n", new_lines.join("\n"));
    if std::fs::write(fstab_path, &final_content).is_err() {
        let staged = std::env::temp_dir().join(format!("nas-fstab-{}", std::process::id()));
        std::fs::write(&staged, &final_content).map_err(|e| {
            NasError::Denied(format!(
                "no se pudo preparar archivo temporal de fstab: {e}"
            ))
        })?;
        let res = runner.run(
            &nas_exec::CommandSpec::new("install")
                .args([
                    "-m",
                    "0644",
                    "-o",
                    "root",
                    "-g",
                    "root",
                    &staged.to_string_lossy(),
                    "/etc/fstab",
                ])
                .sudo(nas_exec::SudoMode::Passwordless)
                .timeout(std::time::Duration::from_secs(30)),
        );
        let _ = std::fs::remove_file(&staged);
        match res {
            Ok(out) if !out.is_success() => {
                return Err(NasError::Denied(format!(
                    "no se pudo actualizar /etc/fstab: {}",
                    out.stderr
                )));
            }
            Err(e) => {
                return Err(NasError::Denied(format!(
                    "no se pudo actualizar /etc/fstab: {e}"
                )));
            }
            _ => {}
        }
    }
    Ok(())
}

#[cfg(test)]
mod artifact_tests {
    use super::{Artifact, DeployParams};
    use crate::deploy::FileSystem;
    use nas_core::ServerRole;

    fn params(role: ServerRole) -> DeployParams {
        DeployParams {
            disk: "/dev/sdb".into(),
            workgroup: "TEAM-JOFRATO".into(),
            netbios: "SRV-NAS".into(),
            admin_user: "admin".into(),
            admin_password: None,
            role,
            keep_data: false,
            filesystem: FileSystem::Ext4,
            rotational: true,
        }
    }

    #[test]
    fn escribe_rol_y_ajustes_del_sistema() {
        let artifacts = params(ServerRole::Arquivos).artifacts();
        let paths: Vec<&str> = artifacts.iter().map(|a| a.path.as_str()).collect();
        assert!(paths.contains(&"/etc/nas/role"));
        assert!(paths.contains(&"/etc/sysctl.d/99-nas-tuning.conf"));
        assert!(paths.contains(&"/etc/udev/rules.d/60-nas-readahead.rules"));
        assert!(paths.contains(&"/etc/logrotate.d/nas-backups"));
        assert!(paths.contains(&"/etc/audit/rules.d/nas.rules"));
        assert!(paths.contains(&"/etc/aide/aide.conf.d/75_aide_nas"));
        assert!(paths.contains(&"/etc/systemd/journald.conf.d/99-nas-limits.conf"));
        assert!(paths.contains(&"/etc/cron.daily/nas-recycle-purge"));

        let role = artifacts
            .iter()
            .find(|a| a.path == "/etc/nas/role")
            .map(|a| a.content.clone())
            .expect("rol");
        assert_eq!(role.trim(), "ARCHIVOS");
    }

    #[test]
    fn solo_backup_programa_el_scrub() {
        let arquivos = params(ServerRole::Arquivos).artifacts();
        assert!(!arquivos.iter().any(|a| a.path.contains("nas-btrfs-scrub")));

        let backup = params(ServerRole::Backup).artifacts();
        let scrub = backup
            .iter()
            .find(|a| a.path == "/etc/cron.d/nas-btrfs-scrub")
            .expect("scrub");
        assert!(scrub.content.contains("btrfs scrub start"));
    }

    #[test]
    fn permisos_de_auditoria_son_restringidos() {
        let artifacts = params(ServerRole::ArquivosBackup).artifacts();
        let audit: &Artifact = artifacts
            .iter()
            .find(|a| a.path == "/etc/audit/rules.d/nas.rules")
            .expect("audit");
        assert_eq!(audit.mode, 0o640);
        assert_eq!(artifacts.len(), 9);
        assert!(audit.content.contains("-D"));
        assert!(audit.content.contains("-b 8192"));
        assert!(audit.content.contains("-f 1"));
        assert!(audit
            .content
            .contains("-w /etc/samba/smb.conf -p wa -k samba_conf_changes"));
        assert!(audit.content.contains("-w /etc/backup-credentials/ -p rwa"));
        assert!(audit
            .content
            .contains("-w /usr/local/sbin/nas-terminal -p x -k nas_terminal"));
    }

    #[test]
    fn plan_incluye_recarga_de_reglas_de_auditoria() {
        let plan = params(ServerRole::Arquivos).plan();
        assert!(plan.iter().any(|step| step == &["augenrules", "--load"]));
    }

    #[test]
    fn guarda_os_disk_permite_disco_local_o_keep_data() {
        let runner = nas_exec::Runner::new();
        let mut local = params(ServerRole::Arquivos);
        local.disk = "LOCAL".into();
        assert!(local.ensure_not_os_disk(&runner).is_ok());

        let mut keep = params(ServerRole::Arquivos);
        keep.keep_data = true;
        assert!(keep.ensure_not_os_disk(&runner).is_ok());
    }

    #[test]
    fn valida_longitud_minima_de_password() {
        let mut p = params(ServerRole::Arquivos);
        p.admin_password = Some("123".into());
        let problems = p.validate();
        assert!(problems
            .iter()
            .any(|s| s.contains("contraseña del administrador debe tener al menos 6 caracteres")));

        p.admin_password = Some("clave-segura-123".into());
        assert!(!p.validate().iter().any(|s| s.contains("contraseña")));
    }

    #[test]
    fn base_smb_conf_configura_directory_leases_auto() {
        let conf = params(ServerRole::Arquivos).base_smb_conf();
        assert!(conf.contains("smb3 directory leases = auto"));
    }

    #[test]
    fn base_smb_conf_coincide_con_global_smb_conf() {
        let p = params(ServerRole::Arquivos);
        assert_eq!(
            p.base_smb_conf(),
            super::super::samba::global_smb_conf(&p.workgroup, &p.netbios)
        );
    }

    #[test]
    fn artefactos_coinciden_con_packaging() {
        let artifacts = params(ServerRole::ArquivosBackup).artifacts_for_mem(4096);
        let sysctl = artifacts
            .iter()
            .find(|a| a.path == "/etc/sysctl.d/99-nas-tuning.conf")
            .expect("sysctl");
        assert_eq!(
            sysctl.content,
            include_str!("../../../../packaging/99-nas-tuning.conf")
        );

        let small_sysctl = super::render_sysctl_tuning(1024);
        assert!(small_sysctl.contains("vm.dirty_background_bytes = 16777216"));
        assert!(small_sysctl.contains("vm.dirty_bytes = 67108864"));

        let big_sysctl = super::render_sysctl_tuning(16384);
        assert!(big_sysctl.contains("vm.dirty_background_bytes = 134217728"));
        assert!(big_sysctl.contains("vm.dirty_bytes = 536870912"));
        let udev = artifacts
            .iter()
            .find(|a| a.path == "/etc/udev/rules.d/60-nas-readahead.rules")
            .expect("udev");
        assert_eq!(
            udev.content,
            include_str!("../../../../packaging/60-nas-readahead.rules")
        );
        let aide = artifacts
            .iter()
            .find(|a| a.path == "/etc/aide/aide.conf.d/75_aide_nas")
            .expect("aide");
        assert_eq!(
            aide.content,
            include_str!("../../../../packaging/75_aide_nas")
        );
    }

    #[test]
    fn logrotate_backups_incluye_su_root_grp_samba() {
        let artifacts = params(ServerRole::Backup).artifacts();
        let logrotate = artifacts
            .iter()
            .find(|a| a.path == "/etc/logrotate.d/nas-backups")
            .expect("logrotate");
        assert!(logrotate.content.contains("su root grp_samba"));
    }

    #[test]
    fn fstab_agrega_nofail_a_montajes_secundarios() {
        let sec = super::fstab_options_for_target("/srv/nas", "rw,noatime,commit=5");
        assert!(sec.contains("nofail"));
        assert!(sec.contains("x-systemd.device-timeout=15s"));
        assert_eq!(
            super::fstab_options_for_target("/", "rw,relatime"),
            "rw,relatime"
        );
    }
}
