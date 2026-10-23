//! Planificación tipada del despliegue del servidor.
//!
//! Valida los parámetros de entrada (rol, disco, NetBIOS, grupo de trabajo y
//! usuario administrador), deriva el sistema de archivos y las opciones de
//! montaje, y genera el plan de referencia. La ejecución del despliegue sigue
//! delegada en `src/core/deploy.sh` hasta completar la paridad de F2.

use nas_core::{NasError, NasResult, ServerRole};
use serde::Serialize;

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

                match self.filesystem {
                    FileSystem::Ext4 => steps.push(vec![
                        "mkfs.ext4".into(),
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
        steps.push(vec![
            "mount".into(),
            "-o".into(),
            self.mount_options(),
            target,
            "/srv/nas".into(),
        ]);
        for dir in ["SISTEMAS", "BACKUPS_HISTORICOS", "LOGS_BACKUP"] {
            steps.push(vec!["mkdir".into(), "-p".into(), format!("/srv/nas/{dir}")]);
        }
        tips(&mut steps);

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

fn tips(steps: &mut Vec<Vec<String>>) {
    for group in ["grp_samba", "grp_web", "grp_superadmin"] {
        steps.push(vec!["groupadd".into(), "-f".into(), group.into()]);
    }
    steps.push(vec![
        "chown".into(),
        "-R".into(),
        "root:grp_samba".into(),
        "/srv/nas".into(),
    ]);
    steps.push(vec!["chmod".into(), "2770".into(), "/srv/nas".into()]);
}

impl DeployParams {
    /// Valida y falla con el primer conjunto de problemas encontrados.
    pub fn ensure_valid(&self) -> NasResult<()> {
        let problems = self.validate();
        if problems.is_empty() {
            Ok(())
        } else {
            Err(NasError::Invalid(problems.join("; ")))
        }
    }
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
