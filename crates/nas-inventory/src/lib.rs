//! Inventario multi-servidor.
//!
//! Declara hosts agrupados por entorno (`dev`/`prod`) con sus rutas, valida el
//! documento y resuelve perfiles completos. Los secretos nunca viven aquí: se
//! obtienen del llavero del sistema.

#![forbid(unsafe_code)]
#![deny(missing_docs)]

use std::collections::BTreeMap;
use std::path::Path;

use nas_core::{NasError, NasResult, ServerRole};
use serde::{Deserialize, Serialize};

/// Entornos admitidos por el inventario.
pub const ENV_DEV: &str = "dev";
/// Entorno de producción.
pub const ENV_PROD: &str = "prod";

/// Valores por defecto aplicables a todos los hosts.
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct Defaults {
    /// Puerto SSH por defecto.
    pub port: Option<u16>,
    /// Usuario SSH por defecto.
    pub user: Option<String>,
    /// Timeout de conexión/ejecución en segundos.
    pub timeout_secs: Option<u64>,
}

/// Grupo de hosts con su entorno y rutas.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Group {
    /// Entorno del grupo (`dev` o `prod`).
    pub env: String,
    /// Rutas declaradas (`repo`, `web`, `state`).
    #[serde(default)]
    pub paths: BTreeMap<String, String>,
}

/// Host declarado en el inventario.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Host {
    /// Dirección o nombre resoluble.
    pub host: String,
    /// Grupo al que pertenece.
    pub group: String,
    /// Rol del servidor (`ARCHIVOS`, `BACKUP`, `ARCHIVOS_BACKUP`).
    #[serde(default)]
    pub role: Option<String>,
    /// Puerto específico del host.
    #[serde(default)]
    pub port: Option<u16>,
    /// Usuario específico del host.
    #[serde(default)]
    pub user: Option<String>,
}

/// Inventario completo.
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct Inventory {
    /// Valores por defecto.
    #[serde(default)]
    pub defaults: Defaults,
    /// Grupos por nombre.
    #[serde(default)]
    pub groups: BTreeMap<String, Group>,
    /// Hosts por alias.
    #[serde(default)]
    pub hosts: BTreeMap<String, Host>,
}

/// Perfil completamente resuelto de un host.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ResolvedHost {
    /// Alias en el inventario.
    pub alias: String,
    /// Dirección del servidor.
    pub address: String,
    /// Grupo.
    pub group: String,
    /// Entorno (`dev`/`prod`).
    pub env: String,
    /// Rol del servidor.
    pub role: String,
    /// Puerto SSH.
    pub port: u16,
    /// Usuario SSH.
    pub user: String,
    /// Ruta del repositorio en el servidor.
    pub repo: String,
    /// Docroot del panel web.
    pub web: String,
    /// Directorio de estado.
    pub state: String,
    /// Timeout de ejecución.
    pub timeout_secs: u64,
}

impl ResolvedHost {
    /// Indica si el host pertenece al entorno de desarrollo.
    pub fn is_development(&self) -> bool {
        self.env == ENV_DEV
    }

    /// Indica si el host pertenece a producción.
    pub fn is_production(&self) -> bool {
        self.env == ENV_PROD
    }
}

impl Inventory {
    /// Carga el inventario desde un archivo YAML.
    pub fn load(path: impl AsRef<Path>) -> NasResult<Self> {
        let path = path.as_ref();
        let raw = std::fs::read_to_string(path).map_err(|_| {
            NasError::NotFound(format!("inventario no encontrado: {}", path.display()))
        })?;
        Self::from_yaml(&raw)
    }

    /// Interpreta el inventario desde texto YAML.
    pub fn from_yaml(raw: &str) -> NasResult<Self> {
        serde_yaml::from_str(raw)
            .map_err(|error| NasError::Invalid(format!("inventario inválido: {error}")))
    }

    /// Alias declarados, en orden alfabético.
    pub fn aliases(&self) -> Vec<String> {
        self.hosts.keys().cloned().collect()
    }

    /// Valida el inventario y devuelve los problemas encontrados.
    pub fn validate(&self) -> Vec<String> {
        let mut problems = Vec::new();

        if self.hosts.is_empty() {
            problems.push("el inventario no declara ningún host".to_string());
        }

        for (alias, host) in &self.hosts {
            if host.host.trim().is_empty() {
                problems.push(format!("{alias}: 'host' vacío"));
            }

            match self.groups.get(&host.group) {
                None => problems.push(format!("{alias}: grupo '{}' no definido", host.group)),
                Some(group) => {
                    if !matches!(group.env.as_str(), ENV_DEV | ENV_PROD) {
                        problems.push(format!(
                            "{alias}: el grupo '{}' usa env '{}' (debe ser dev o prod)",
                            host.group, group.env
                        ));
                    }
                }
            }

            if let Some(role) = &host.role {
                if ServerRole::parse(role).is_none() {
                    problems.push(format!("{alias}: rol '{role}' inválido"));
                }
            }

            if host.port.or(self.defaults.port).unwrap_or(22) == 0 {
                problems.push(format!("{alias}: puerto inválido"));
            }

            if host.user.as_deref().unwrap_or_default().trim().is_empty()
                && self
                    .defaults
                    .user
                    .as_deref()
                    .unwrap_or_default()
                    .trim()
                    .is_empty()
            {
                problems.push(format!("{alias}: usuario SSH no definido"));
            }
        }

        problems
    }

    /// Resuelve el perfil completo de un host.
    pub fn resolve(&self, alias: &str) -> NasResult<ResolvedHost> {
        let host = self.hosts.get(alias).ok_or_else(|| {
            NasError::NotFound(format!("host '{alias}' no está en el inventario"))
        })?;

        let group = self
            .groups
            .get(&host.group)
            .ok_or_else(|| NasError::Invalid(format!("grupo '{}' no definido", host.group)))?;

        let role = host
            .role
            .as_deref()
            .and_then(ServerRole::parse)
            .unwrap_or(ServerRole::ArquivosBackup);

        let defaults = if group.env == ENV_DEV {
            (
                "/opt/nas_debian-dev",
                "/var/www/nas-web-dev",
                "/var/lib/nas-dev",
            )
        } else {
            ("/opt/nas_debian", "/var/www/nas-web", "/var/lib/nas")
        };

        Ok(ResolvedHost {
            alias: alias.to_string(),
            address: host.host.clone(),
            group: host.group.clone(),
            env: group.env.clone(),
            role: role.as_str().to_string(),
            port: host.port.or(self.defaults.port).unwrap_or(22),
            user: host
                .user
                .clone()
                .or_else(|| self.defaults.user.clone())
                .unwrap_or_else(|| "nasadmin".to_string()),
            repo: group
                .paths
                .get("repo")
                .cloned()
                .unwrap_or_else(|| defaults.0.to_string()),
            web: group
                .paths
                .get("web")
                .cloned()
                .unwrap_or_else(|| defaults.1.to_string()),
            state: group
                .paths
                .get("state")
                .cloned()
                .unwrap_or_else(|| defaults.2.to_string()),
            timeout_secs: self.defaults.timeout_secs.unwrap_or(30),
        })
    }

    /// Resuelve todos los hosts que sean válidos.
    pub fn resolve_all(&self) -> Vec<ResolvedHost> {
        self.aliases()
            .into_iter()
            .filter_map(|alias| self.resolve(&alias).ok())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::Inventory;

    const YAML: &str = r#"
defaults:
  port: 22
  user: nasadmin
  timeout_secs: 45
groups:
  dev:
    env: dev
  prod:
    env: prod
    paths:
      repo: /opt/nas_debian
      web: /var/www/nas-web
      state: /var/lib/nas
hosts:
  lab-dev:
    host: 10.10.1.222
    group: dev
    role: ARCHIVOS_BACKUP
  srv-nas:
    host: 10.10.1.2
    group: prod
    role: ARCHIVOS
    port: 2222
"#;

    #[test]
    fn valida_inventario_correcto() {
        let inventory = Inventory::from_yaml(YAML).expect("yaml");
        assert!(inventory.validate().is_empty());
        assert_eq!(inventory.aliases(), vec!["lab-dev", "srv-nas"]);
    }

    #[test]
    fn resuelve_rutas_por_entorno() {
        let inventory = Inventory::from_yaml(YAML).expect("yaml");
        let dev = inventory.resolve("lab-dev").expect("dev");
        let prod = inventory.resolve("srv-nas").expect("prod");

        assert_eq!(dev.web, "/var/www/nas-web-dev");
        assert_eq!(dev.state, "/var/lib/nas-dev");
        assert_eq!(dev.repo, "/opt/nas_debian-dev");
        assert_eq!(dev.user, "nasadmin");
        assert_eq!(dev.port, 22);
        assert_eq!(dev.timeout_secs, 45);
        assert!(dev.is_development());

        assert_eq!(prod.web, "/var/www/nas-web");
        assert_eq!(prod.port, 2222);
        assert!(prod.is_production());
    }

    #[test]
    fn respeta_rutas_explicitas_del_grupo() {
        let yaml = "groups:\n  g:\n    env: prod\n    paths: { repo: /custom/repo }\nhosts:\n  h:\n    host: 1.1.1.1\n    group: g\n";
        let inventory = Inventory::from_yaml(yaml).expect("yaml");
        let host = inventory.resolve("h").expect("host");
        assert_eq!(host.repo, "/custom/repo");
        assert_eq!(host.web, "/var/www/nas-web");
    }

    #[test]
    fn detecta_grupo_inexistente_y_rol_invalido() {
        let yaml = "hosts:\n  h:\n    host: 1.1.1.1\n    group: nope\n    role: OTRO\n";
        let inventory = Inventory::from_yaml(yaml).expect("yaml");
        let problems = inventory.validate();
        assert!(problems.iter().any(|p| p.contains("no definido")));
        assert!(problems.iter().any(|p| p.contains("rol")));
    }

    #[test]
    fn host_desconocido_devuelve_error() {
        let inventory = Inventory::from_yaml(YAML).expect("yaml");
        assert!(inventory.resolve("fantasma").is_err());
    }
}
