//! Configuración del servidor: rol, rutas y variables de entorno.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{NasError, NasResult};

/// Rol operativo del servidor, persistido en `/etc/nas/role`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ServerRole {
    /// NAS departamental de archivos.
    Arquivos,
    /// Central de respaldos.
    Backup,
    /// Servidor híbrido (archivos + respaldos).
    ArquivosBackup,
}

impl ServerRole {
    /// Ruta por defecto del archivo de rol.
    pub const ROLE_FILE: &'static str = "/etc/nas/role";

    /// Normaliza el contenido del archivo de rol, incluyendo alias heredados.
    pub fn parse(raw: &str) -> Option<Self> {
        let normalized: String = raw
            .trim()
            .to_uppercase()
            .chars()
            .filter(|c| c.is_ascii_alphabetic() || *c == '_')
            .collect();

        match normalized.as_str() {
            "ARCHIVOS" => Some(Self::Arquivos),
            "BACKUP" => Some(Self::Backup),
            "ARCHIVOS_BACKUP" | "HIBRIDO" | "ARCHIVOSBACKUP" => Some(Self::ArquivosBackup),
            _ => None,
        }
    }

    /// Representación canónica persistida en disco.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Arquivos => "ARCHIVOS",
            Self::Backup => "BACKUP",
            Self::ArquivosBackup => "ARCHIVOS_BACKUP",
        }
    }

    /// Etiqueta legible para la interfaz.
    pub fn label(self) -> &'static str {
        match self {
            Self::Arquivos => "ARCHIVOS",
            Self::Backup => "BACKUP",
            Self::ArquivosBackup => "ARCHIVOS & BACKUP",
        }
    }

    /// Indica si el rol expone recursos compartidos.
    pub fn has_shares(self) -> bool {
        matches!(self, Self::Arquivos | Self::ArquivosBackup)
    }

    /// Indica si el rol administra copias de seguridad.
    pub fn has_backups(self) -> bool {
        matches!(self, Self::Backup | Self::ArquivosBackup)
    }

    /// Lee el rol desde disco o desde `NAS_SERVER_ROLE`.
    pub fn from_file(path: impl AsRef<Path>) -> NasResult<Option<Self>> {
        if let Ok(env_role) = std::env::var("NAS_SERVER_ROLE") {
            if let Some(role) = Self::parse(&env_role) {
                return Ok(Some(role));
            }
        }

        let path = path.as_ref();
        if !path.exists() {
            return Ok(None);
        }

        let raw = std::fs::read_to_string(path).map_err(|e| NasError::io(path, e))?;
        Ok(Self::parse(&raw))
    }
}

/// Rutas relevantes del despliegue, parametrizables para separar dev y producción.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NasPaths {
    /// Directorio del repositorio/instalación del proyecto.
    pub repo: PathBuf,
    /// Raíz del panel web servido por Nginx.
    pub web_root: PathBuf,
    /// Directorio de estado (SQLite, cachés, auditoría).
    pub state_dir: PathBuf,
    /// Punto de montaje del almacenamiento NAS.
    pub data_root: PathBuf,
    /// Configuración de Samba.
    pub samba_conf: PathBuf,
    /// Archivo con el rol del servidor.
    pub role_file: PathBuf,
}

impl Default for NasPaths {
    fn default() -> Self {
        Self {
            repo: PathBuf::from("/opt/nas_debian"),
            web_root: PathBuf::from("/var/www/nas-web"),
            state_dir: PathBuf::from("/var/lib/nas"),
            data_root: PathBuf::from("/srv/nas"),
            samba_conf: PathBuf::from("/etc/samba/smb.conf"),
            role_file: PathBuf::from(ServerRole::ROLE_FILE),
        }
    }
}

impl NasPaths {
    /// Perfil de producción (rutas del despliegue oficial).
    pub fn production() -> Self {
        Self::default()
    }

    /// Perfil de desarrollo aislado, que nunca toca las rutas de producción.
    pub fn development() -> Self {
        Self {
            repo: PathBuf::from("/opt/nas_debian-dev"),
            web_root: PathBuf::from("/var/www/nas-web-dev"),
            state_dir: PathBuf::from("/var/lib/nas-dev"),
            data_root: PathBuf::from("/srv/nas-dev"),
            samba_conf: PathBuf::from("/etc/samba/smb.conf"),
            role_file: PathBuf::from("/etc/nas/role"),
        }
    }

    /// Sobrescribe rutas con variables `NAS_*` (útil en pruebas y multi-servidor).
    pub fn with_env_overrides(mut self) -> Self {
        let overrides = [
            ("NAS_REPO_DIR", &mut self.repo),
            ("NAS_WEB_ROOT", &mut self.web_root),
            ("NAS_STATE_DIR", &mut self.state_dir),
            ("NAS_DATA_ROOT", &mut self.data_root),
            ("NAS_SAMBA_CONF", &mut self.samba_conf),
            ("NAS_ROLE_FILE", &mut self.role_file),
        ];

        for (key, slot) in overrides {
            if let Ok(value) = std::env::var(key) {
                let trimmed = value.trim();
                if !trimmed.is_empty() {
                    *slot = PathBuf::from(trimmed);
                }
            }
        }

        self
    }

    /// Rutas efectivas del proceso actual.
    pub fn from_env() -> Self {
        Self::default().with_env_overrides()
    }

    /// Perfil de desarrollo con sobrescrituras de entorno aplicadas.
    pub fn development_from_env() -> Self {
        Self::development().with_env_overrides()
    }

    /// Ruta del archivo SQLite del panel.
    pub fn database(&self) -> PathBuf {
        self.state_dir.join("nas.sqlite")
    }

    /// Bitácora de auditoría local de la CLI.
    pub fn audit_log(&self) -> PathBuf {
        self.state_dir.join("audit").join("nas-audit.jsonl")
    }
}

/// Extrae el `workgroup` de un `smb.conf`, normalizado en mayúsculas.
pub fn parse_workgroup(smb_conf: &str) -> Option<String> {
    smb_conf.lines().find_map(|line| {
        let (key, value) = line.split_once('=')?;
        if !key.trim().eq_ignore_ascii_case("workgroup") {
            return None;
        }
        let value = value.trim();
        if value.is_empty() {
            None
        } else {
            Some(value.to_uppercase())
        }
    })
}

#[cfg(test)]
mod tests {
    use super::{parse_workgroup, NasPaths, ServerRole};

    #[test]
    fn normaliza_alias_heredados() {
        assert_eq!(
            ServerRole::parse("hibrido"),
            Some(ServerRole::ArquivosBackup)
        );
        assert_eq!(
            ServerRole::parse("ARCHIVOSBACKUP\n"),
            Some(ServerRole::ArquivosBackup)
        );
        assert_eq!(ServerRole::parse("backup"), Some(ServerRole::Backup));
        assert_eq!(ServerRole::parse("otro"), None);
    }

    #[test]
    fn expone_capacidades_por_rol() {
        assert!(ServerRole::Arquivos.has_shares());
        assert!(!ServerRole::Arquivos.has_backups());
        assert!(ServerRole::Backup.has_backups());
        assert!(!ServerRole::Backup.has_shares());
        assert!(ServerRole::ArquivosBackup.has_shares());
        assert!(ServerRole::ArquivosBackup.has_backups());
    }

    #[test]
    fn extrae_workgroup_de_smb_conf() {
        let conf = "[global]\n   workgroup = team-jofrato\n   netbios name = SRV-NAS\n";
        assert_eq!(parse_workgroup(conf).as_deref(), Some("TEAM-JOFRATO"));
        assert_eq!(parse_workgroup("[global]\n"), None);
    }

    #[test]
    fn separa_rutas_de_desarrollo_y_produccion() {
        let prod = NasPaths::production();
        let dev = NasPaths::development();
        assert_eq!(prod.web_root.to_string_lossy(), "/var/www/nas-web");
        assert_eq!(dev.web_root.to_string_lossy(), "/var/www/nas-web-dev");
        assert_ne!(prod.state_dir, dev.state_dir);
        assert!(dev.database().ends_with("nas.sqlite"));
    }
}
