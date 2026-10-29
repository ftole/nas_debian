//! Planificación y ejecución tipada del despliegue del servidor.
//!
//! Valida los parámetros de entrada (rol, disco, NetBIOS, grupo de trabajo y
//! usuario administrador), deriva el sistema de archivos y las opciones de
//! montaje, y genera el plan y artefactos para el despliegue nativo.

use nas_core::{NasError, NasResult, ServerRole};
use serde::Serialize;

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

const SYSCTL_TUNING: &str = "# Ajustes NAS (generado por nas deploy)
fs.inotify.max_user_instances = 2048
fs.inotify.max_user_watches = 524288
net.ipv4.tcp_keepalive_time = 120
net.ipv4.tcp_keepalive_intvl = 15
net.ipv4.tcp_keepalive_probes = 4
vm.vfs_cache_pressure = 30
vm.dirty_background_bytes = 67108864
vm.dirty_bytes = 268435456
vm.dirty_expire_centisecs = 300
net.ipv4.conf.all.rp_filter = 1
net.ipv4.conf.default.rp_filter = 1
net.ipv4.tcp_syncookies = 1
kernel.kptr_restrict = 2
kernel.dmesg_restrict = 1
kernel.yama.ptrace_scope = 2
net.ipv6.conf.all.disable_ipv6 = 1
net.ipv6.conf.default.disable_ipv6 = 1
net.ipv6.conf.lo.disable_ipv6 = 1
";

const UDEV_READAHEAD: &str = "# Readahead por tipo de medio (generado por nas deploy)
ACTION==\"add|change\", KERNEL==\"sd[a-z]|nvme[0-9]n[0-9]\", ATTR{queue/rotational}==\"1\", ATTR{queue/read_ahead_kb}=\"4096\"
ACTION==\"add|change\", KERNEL==\"sd[a-z]|nvme[0-9]n[0-9]\", ATTR{queue/rotational}==\"0\", ATTR{queue/read_ahead_kb}=\"1024\"
";

const LOGROTATE_BACKUPS: &str = "/srv/nas/LOGS_BACKUP/*.log {
    weekly
    rotate 8
    compress
    missingok
    notifempty
    copytruncate
}
";

const AUDIT_RULES: &str = "-w /etc/sudoers -p wa -k sudoers_changes
-w /etc/sudoers.d/ -p wa -k sudoers_changes
-w /etc/samba/smb.conf -p wa -k samba_config
-w /etc/backup-credentials/ -p rwa -k backup_credentials
-w /usr/local/bin/nas -p x -k nas_cli
-w /usr/local/sbin/nas-terminal -p x -k nas_terminal
";

/// Sistema de archivos objetivo.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum FileSystem {
    /// ext4 optimizado para el rol de archivos.
    Ext4,
    /// Btrfs con compresión para la central de respaldos.
    Btrfs,
}

impl FileSystem {
    /// Nombre usado por `mkfs` y `smb.conf`.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ext4 => "ext4",
            Self::Btrfs => "btrfs",
        }
    }

    /// Etiqueta legible.
    pub fn label(self) -> &'static str {
        match self {
            Self::Ext4 => "ext4",
            Self::Btrfs => "Btrfs (zstd)",
        }
    }
}

/// Parámetros de un despliegue.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeployParams {
    /// Disco dedicado (`/dev/sdb`) o `LOCAL` para reutilizar el disco del sistema.
    pub disk: String,
    /// Grupo de trabajo de Samba.
    pub workgroup: String,
    /// Nombre NetBIOS.
    pub netbios: String,
    /// Usuario administrador.
    pub admin_user: String,
    /// Rol del servidor.
    pub role: ServerRole,
    /// Reutiliza datos existentes sin formatear.
    pub keep_data: bool,
    /// Sistema de archivos elegido.
    pub filesystem: FileSystem,
    /// Indica si el disco es mecánico (afecta a las opciones de montaje).
    pub rotational: bool,
}

impl DeployParams {
    /// Crea parámetros con el sistema de archivos recomendado para el rol.
    pub fn new(
        disk: impl Into<String>,
        workgroup: impl Into<String>,
        netbios: impl Into<String>,
        admin_user: impl Into<String>,
        role: ServerRole,
    ) -> Self {
        let filesystem = match role {
            ServerRole::Arquivos => FileSystem::Ext4,
            ServerRole::Backup | ServerRole::ArquivosBackup => FileSystem::Btrfs,
        };

        Self {
            disk: disk.into(),
            workgroup: workgroup.into(),
            netbios: netbios.into(),
            admin_user: admin_user.into(),
            role,
            keep_data: false,
            filesystem,
            rotational: true,
        }
    }

    /// Valida los parámetros y devuelve los problemas detectados.
    pub fn validate(&self) -> Vec<String> {
        let mut problems = Vec::new();

        if self.disk.trim().is_empty() {
            problems.push("el disco es obligatorio (o 'LOCAL')".to_string());
        } else if self.disk != "LOCAL" {
            let valid = self.disk.strip_prefix("/dev/").is_some_and(|rest| {
                !rest.is_empty()
                    && rest
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || c == '/' || c == '-' || c == '_')
            });
            if !valid {
                problems.push(format!("disco inválido: '{}'", self.disk));
            }
        }

        if self.keep_data && self.disk == "LOCAL" {
            problems.push("--keep-data requiere un disco dedicado, no LOCAL".to_string());
        }

        if !(1..=15).contains(&self.netbios.chars().count())
            || !self
                .netbios
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-')
        {
            problems
                .push("el nombre NetBIOS debe tener 1-15 caracteres alfanuméricos o '-'".into());
        }

        if !(1..=15).contains(&self.workgroup.chars().count())
            || !self
                .workgroup
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        {
            problems
                .push("el workgroup debe tener 1-15 caracteres alfanuméricos, '-' o '_'".into());
        }

        let mut user_chars = self.admin_user.chars();
        let first_ok = user_chars
            .next()
            .is_some_and(|c| c.is_ascii_lowercase() || c == '_');
        let rest_ok = user_chars
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_');
        if !(first_ok && rest_ok) || self.admin_user.chars().count() > 32 {
            problems.push(format!(
                "usuario administrador inválido: '{}'",
                self.admin_user
            ));
        }

        problems
    }

    /// Opciones de montaje derivadas del rol y del tipo de disco.
    pub fn mount_options(&self) -> String {
        match (self.filesystem, self.rotational) {
            (FileSystem::Ext4, true) => "rw,noatime,commit=2".to_string(),
            (FileSystem::Ext4, false) => "rw,noatime,commit=5".to_string(),
            (FileSystem::Btrfs, true) => {
                "rw,noatime,compress=zstd:3,space_cache=v2,autodefrag".to_string()
            }
            (FileSystem::Btrfs, false) => {
                "rw,noatime,compress=zstd:3,space_cache=v2,ssd,discard=async".to_string()
            }
        }
    }

    /// Ruta de la partición principal del disco indicado.
    pub fn partition_path(&self) -> Option<String> {
        if self.disk == "LOCAL" {
            return None;
        }
        let suffix = if self.disk.starts_with("/dev/nvme") || self.disk.starts_with("/dev/mmcblk") {
            "p1"
        } else {
            "1"
        };
        Some(format!("{}{suffix}", self.disk.trim_end_matches('/')))
    }

    /// Plan de referencia (informativo) del despliegue.
    pub fn plan(&self) -> Vec<Vec<String>> {
        let mut steps = Vec::new();
        let partition = self.partition_path();
        let target = partition.clone().unwrap_or_else(|| "/srv/nas".to_string());

        if !self.keep_data {
            if let Some(partition) = &partition {
                steps.push(vec![
                    "parted".into(),
                    "-s".into(),
                    self.disk.clone(),
                    "mklabel".into(),
                    "gpt".into(),
                    "mkpart".into(),
                    "primary".into(),
                    self.filesystem.as_str().into(),
                    "0%".into(),
                    "100%".into(),
                ]);
                steps.push(vec!["partprobe".into(), self.disk.clone()]);
                steps.push(vec!["udevadm".into(), "settle".into()]);

                match self.filesystem {
                    FileSystem::Ext4 => steps.push(vec![
                        "mkfs.ext4".into(),
                        "-F".into(),
                        "-m".into(),
                        "1".into(),
                        "-L".into(),
                        "NAS_DATA".into(),
                        partition.clone(),
                    ]),
                    FileSystem::Btrfs => steps.push(vec![
                        "mkfs.btrfs".into(),
                        "-f".into(),
                        "-L".into(),
                        "NAS_DATA".into(),
                        partition.clone(),
                    ]),
                }
            }
        }

        steps.push(vec!["mkdir".into(), "-p".into(), "/srv/nas".into()]);
        if self.disk != "LOCAL" {
            steps.push(vec![
                "mount".into(),
                "-o".into(),
                self.mount_options(),
                target,
                "/srv/nas".into(),
            ]);
        }
        steps.push(vec![
            "mkdir".into(),
            "-p".into(),
            "/srv/nas/SISTEMAS".into(),
        ]);
        if self.role != ServerRole::Arquivos {
            for dir in ["BACKUPS_HISTORICOS", "LOGS_BACKUP"] {
                steps.push(vec!["mkdir".into(), "-p".into(), format!("/srv/nas/{dir}")]);
            }
            steps.push(vec![
                "mkdir".into(),
                "-p".into(),
                "/etc/backup-credentials".into(),
            ]);
        }
        tips(&mut steps, &self.admin_user);

        steps
    }

    /// Invocación equivalente a `deploy.sh` (ejecución transitoria auditada).
    pub fn deploy_sh_invocation(&self, repo_dir: &str, password_token: &str) -> Vec<String> {
        let mut argv = vec![
            "bash".to_string(),
            format!("{}/src/core/deploy.sh", repo_dir.trim_end_matches('/')),
            self.disk.clone(),
            self.workgroup.clone(),
            self.netbios.clone(),
            self.admin_user.clone(),
            password_token.to_string(),
            self.role.as_str().to_string(),
            "--force".to_string(),
            "--confirm".to_string(),
        ];

        if self.keep_data {
            argv.push("--keep-data".to_string());
        }

        argv
    }
}

fn tips(steps: &mut Vec<Vec<String>>, admin_user: &str) {
    for group in ["grp_samba", "grp_web", "grp_superadmin", "grp_backups"] {
        steps.push(vec!["groupadd".into(), "-f".into(), group.into()]);
    }
    steps.push(vec![
        "usermod".into(),
        "-aG".into(),
        "sudo,adm,grp_samba,grp_superadmin".into(),
        admin_user.into(),
    ]);
    steps.push(vec![
        "chown".into(),
        "-R".into(),
        "root:grp_samba".into(),
        "/srv/nas".into(),
    ]);
    steps.push(vec!["chmod".into(), "2770".into(), "/srv/nas".into()]);
    steps.push(vec!["sysctl".into(), "--system".into()]);
    steps.push(vec![
        "udevadm".into(),
        "control".into(),
        "--reload-rules".into(),
    ]);
    steps.push(vec!["augenrules".into(), "--load".into()]);
    steps.push(vec!["systemctl".into(), "enable".into(), "auditd".into()]);
    steps.push(vec!["systemctl".into(), "restart".into(), "auditd".into()]);
}

impl DeployParams {
    /// Configuración base limpia `[global]` de Samba optimizada para Debian 13.
    pub fn base_smb_conf(&self) -> String {
        format!(
            "[global]\n\
   workgroup = {}\n\
   netbios name = {}\n\
   server string = Servidor NAS Debian 13\n\
   server role = standalone server\n\
   security = user\n\
   map to guest = Bad User\n\
   server min protocol = SMB2_10\n\
   server max protocol = SMB3_11\n\
   server signing = mandatory\n\
   smb encrypt = desired\n\
   smb3 directory leases = no\n\
   kernel change notify = yes\n\
   change notify = yes\n\
   vfs objects = acl_xattr streams_xattr\n\
   map acl inherit = yes\n\
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
            self.workgroup, self.netbios
        )
    }

    /// Archivos de configuración del sistema que escribe el despliegue nativo.
    pub fn artifacts(&self) -> Vec<Artifact> {
        let mut artifacts = vec![
            Artifact {
                path: "/etc/nas/role".into(),
                mode: 0o644,
                content: format!("{}\n", self.role.as_str()),
            },
            Artifact {
                path: "/etc/sysctl.d/99-nas-tuning.conf".into(),
                mode: 0o644,
                content: SYSCTL_TUNING.to_string(),
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
        ];

        if !self.role.as_str().eq("ARCHIVOS") {
            artifacts.push(Artifact {
                path: "/etc/cron.d/nas-btrfs-scrub".into(),
                mode: 0o644,
                content: "SHELL=/bin/bash\n\
PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin\n\
0 2 1 * * root btrfs scrub start -B /srv/nas >/dev/null 2>&1\n"
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

    /// Valida y falla con el primer conjunto de problemas encontrados.
    pub fn ensure_valid(&self) -> NasResult<()> {
        let problems = self.validate();
        if problems.is_empty() {
            Ok(())
        } else {
            Err(NasError::Invalid(problems.join("; ")))
        }
    }

    /// Comprueba que el disco no sea el del sistema operativo antes de proceder con el despliegue.
    pub fn ensure_not_os_disk(&self, runner: &nas_exec::Runner) -> NasResult<()> {
        if self.disk == "LOCAL" || self.keep_data {
            return Ok(());
        }
        if crate::helpers::storage::is_os_disk(runner, &self.disk)? {
            return Err(NasError::Denied(format!(
                "el disco {} contiene el sistema operativo y está protegido contra formateo",
                self.disk
            )));
        }
        Ok(())
    }

    /// Persiste el montaje de `/srv/nas` en `/etc/fstab` usando el UUID de la partición.
    pub fn update_fstab(&self, runner: &nas_exec::Runner) -> NasResult<()> {
        if self.disk == "LOCAL" {
            return Ok(());
        }
        let Some(partition) = self.partition_path() else {
            return Ok(());
        };
        update_fstab(runner, &partition, "/srv/nas", &self.mount_options())
    }
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

    new_lines.push(format!("{fstab_src} {target} auto {options} 0 0"));
    let final_content = format!("{}\n", new_lines.join("\n"));
    if std::fs::write(fstab_path, &final_content).is_err() {
        let staged = std::env::temp_dir().join(format!("nas-fstab-{}", std::process::id()));
        if std::fs::write(&staged, &final_content).is_ok() {
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
            if let Err(e) = res {
                return Err(NasError::Denied(format!("no se pudo actualizar /etc/fstab: {e}")));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{DeployParams, FileSystem};
    use nas_core::ServerRole;

    fn params(role: ServerRole) -> DeployParams {
        DeployParams::new("/dev/sdb", "TEAM-JOFRATO", "SRV-NAS", "admin", role)
    }

    #[test]
    fn elige_filesystem_segun_rol() {
        assert_eq!(params(ServerRole::Arquivos).filesystem, FileSystem::Ext4);
        assert_eq!(params(ServerRole::Backup).filesystem, FileSystem::Btrfs);
        assert_eq!(
            params(ServerRole::ArquivosBackup).filesystem,
            FileSystem::Btrfs
        );
    }

    #[test]
    fn opciones_de_montaje_por_hardware() {
        let mut hdd = params(ServerRole::Arquivos);
        hdd.rotational = true;
        assert_eq!(hdd.mount_options(), "rw,noatime,commit=2");

        let mut ssd = params(ServerRole::Arquivos);
        ssd.rotational = false;
        assert_eq!(ssd.mount_options(), "rw,noatime,commit=5");

        let mut bkp = params(ServerRole::Backup);
        bkp.rotational = true;
        assert!(bkp.mount_options().contains("compress=zstd:3"));
        assert!(bkp.mount_options().contains("autodefrag"));

        let mut bkp_ssd = params(ServerRole::Backup);
        bkp_ssd.rotational = false;
        assert!(bkp_ssd.mount_options().contains("discard=async"));
    }

    #[test]
    fn valida_parametros_correctos() {
        assert!(params(ServerRole::Arquivos).validate().is_empty());
    }

    #[test]
    fn detecta_disco_netbios_workgroup_y_usuario_invalidos() {
        let mut bad = DeployParams::new(
            "/dev/../etc",
            "work group!",
            "",
            "Admin",
            ServerRole::Arquivos,
        );
        bad.keep_data = true;
        let problems = bad.validate();

        assert!(problems.iter().any(|p| p.contains("disco inválido")));
        assert!(problems.iter().any(|p| p.contains("NetBIOS")));
        assert!(problems.iter().any(|p| p.contains("workgroup")));
        assert!(problems.iter().any(|p| p.contains("usuario")));
    }

    #[test]
    fn keep_data_exige_disco_dedicado() {
        let mut local = DeployParams::new("LOCAL", "WG", "SRV-NAS", "admin", ServerRole::Arquivos);
        local.keep_data = true;
        assert!(local
            .validate()
            .iter()
            .any(|p| p.contains("disco dedicado")));
    }

    #[test]
    fn calcula_particion_segun_tipo_de_disco() {
        let mut sata = params(ServerRole::Arquivos);
        sata.disk = "/dev/sdb".to_string();
        assert_eq!(sata.partition_path().as_deref(), Some("/dev/sdb1"));

        let mut nvme = params(ServerRole::Arquivos);
        nvme.disk = "/dev/nvme0n1".to_string();
        assert_eq!(nvme.partition_path().as_deref(), Some("/dev/nvme0n1p1"));

        let local = DeployParams::new("LOCAL", "WG", "SRV-NAS", "admin", ServerRole::Arquivos);
        assert_eq!(local.partition_path(), None);
    }

    #[test]
    fn plan_formatea_monta_y_crea_estructura() {
        let plan = params(ServerRole::Backup).plan();
        let labels: Vec<&str> = plan
            .iter()
            .map(|step| step.first().map_or("", String::as_str))
            .collect();

        assert!(labels.contains(&"parted"));
        assert!(labels.contains(&"mkfs.btrfs"));
        assert!(labels.contains(&"mount"));
        assert!(plan
            .iter()
            .any(|step| step.contains(&"/srv/nas/BACKUPS_HISTORICOS".to_string())));
    }

    #[test]
    fn keep_data_no_formatea() {
        let mut keep = params(ServerRole::Arquivos);
        keep.keep_data = true;
        let plan = keep.plan();
        assert!(!plan.iter().any(|step| step[0] == "mkfs.ext4"));
        assert!(!plan.iter().any(|step| step[0] == "parted"));
    }

    #[test]
    fn rol_archivos_no_crea_rutas_de_backup() {
        let plan = params(ServerRole::Arquivos).plan();
        assert!(plan
            .iter()
            .any(|step| step.contains(&"/srv/nas/SISTEMAS".to_string())));
        assert!(!plan
            .iter()
            .any(|step| step.contains(&"/srv/nas/BACKUPS_HISTORICOS".to_string())));
        assert!(!plan
            .iter()
            .any(|step| step.contains(&"/srv/nas/LOGS_BACKUP".to_string())));
        assert!(!plan
            .iter()
            .any(|step| step.contains(&"/etc/backup-credentials".to_string())));
    }

    #[test]
    fn invocacion_de_deploy_sh_incluye_rol_y_banderas() {
        let mut backup = params(ServerRole::Backup);
        backup.keep_data = true;
        let argv = backup.deploy_sh_invocation("/opt/nas_debian", "-");

        assert_eq!(argv[0], "bash");
        assert_eq!(argv[1], "/opt/nas_debian/src/core/deploy.sh");
        assert!(argv.contains(&"BACKUP".to_string()));
        assert!(argv.contains(&"--force".to_string()));
        assert!(argv.contains(&"--keep-data".to_string()));
    }
}

#[cfg(test)]
mod artifact_tests {
    use super::{Artifact, DeployParams, FileSystem};
    use nas_core::ServerRole;

    fn params(role: ServerRole) -> DeployParams {
        DeployParams {
            disk: "/dev/sdb".into(),
            workgroup: "TEAM-JOFRATO".into(),
            netbios: "SRV-NAS".into(),
            admin_user: "admin".into(),
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
        assert_eq!(artifacts.len(), 6);
        assert!(audit.content.contains("-w /etc/backup-credentials/ -p rwa"));
        assert!(audit.content.contains("-w /usr/local/bin/nas -p x -k nas_cli"));
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
}
