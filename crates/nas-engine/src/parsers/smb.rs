//! Parser de `smb.conf` y de `testparm -s`.

use serde::Serialize;

/// Configuración efectiva relevante de Samba para la vista de red.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SambaConfig {
    /// Grupo de trabajo.
    pub workgroup: Option<String>,
    /// Nombre NetBIOS del servidor.
    pub netbios_name: Option<String>,
    /// Protocolo SMB mínimo aceptado.
    pub min_protocol: Option<String>,
    /// Protocolo SMB máximo ofrecido.
    pub max_protocol: Option<String>,
    /// Política de firma SMB.
    pub signing: Option<String>,
    /// Política de cifrado SMB.
    pub encrypt: Option<String>,
    /// Indica si NetBIOS está habilitado.
    pub netbios_enabled: bool,
}

impl Default for SambaConfig {
    fn default() -> Self {
        Self {
            workgroup: None,
            netbios_name: None,
            min_protocol: None,
            max_protocol: None,
            signing: None,
            encrypt: None,
            netbios_enabled: true,
        }
    }
}

impl SambaConfig {
    /// Valor por defecto explícito (NetBIOS habilitado).
    pub fn with_defaults() -> Self {
        Self::default()
    }

    /// Aplica sobre `self` los valores definidos en `other`.
    pub fn merge_from(&mut self, other: &Self) {
        if other.workgroup.is_some() {
            self.workgroup = other.workgroup.clone();
        }
        if other.netbios_name.is_some() {
            self.netbios_name = other.netbios_name.clone();
        }
        if other.min_protocol.is_some() {
            self.min_protocol = other.min_protocol.clone();
        }
        if other.max_protocol.is_some() {
            self.max_protocol = other.max_protocol.clone();
        }
        if other.signing.is_some() {
            self.signing = other.signing.clone();
        }
        if other.encrypt.is_some() {
            self.encrypt = other.encrypt.clone();
        }
        self.netbios_enabled = other.netbios_enabled;
    }
}

fn normalize_value(key: &str, value: &str) -> String {
    let value = value.trim().to_string();
    match key {
        "workgroup" | "netbios name" => value.to_uppercase(),
        _ => value.to_lowercase(),
    }
}

fn apply_pair(config: &mut SambaConfig, key: &str, value: &str) {
    let value = normalize_value(key, value);
    match key {
        "workgroup" => config.workgroup = Some(value),
        "netbios name" => config.netbios_name = Some(value),
        "server min protocol" => config.min_protocol = Some(value),
        "server max protocol" => config.max_protocol = Some(value),
        "server signing" => config.signing = Some(value),
        "server smb encrypt" => config.encrypt = Some(value),
        "disable netbios" => {
            config.netbios_enabled = !matches!(value.as_str(), "yes" | "true" | "1");
        }
        _ => {}
    }
}

/// Extrae la configuración de la sección `[global]` de un `smb.conf`.
pub fn parse_smb_conf(text: &str) -> SambaConfig {
    let mut config = SambaConfig::with_defaults();
    let mut in_global = false;

    for raw_line in text.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
            continue;
        }

        if line.starts_with('[') && line.ends_with(']') {
            in_global = line[1..line.len() - 1]
                .trim()
                .eq_ignore_ascii_case("global");
            continue;
        }

        if !in_global {
            continue;
        }

        if let Some((key, value)) = line.split_once('=') {
            apply_pair(&mut config, &key.trim().to_lowercase(), value);
        }
    }

    config
}

/// Extrae la configuración efectiva de la salida de `testparm -s`.
pub fn parse_testparm(text: &str) -> SambaConfig {
    let mut config = SambaConfig::with_defaults();
    // `testparm -s` imprime primero `[global]`; si no aparece, se asume global.
    let mut in_global = true;

    for raw_line in text.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with(';') || line.starts_with('#') {
            continue;
        }

        if line.starts_with('[') && line.ends_with(']') {
            in_global = line[1..line.len() - 1]
                .trim()
                .eq_ignore_ascii_case("global");
            continue;
        }

        if !in_global {
            continue;
        }

        if let Some((key, value)) = line.split_once('=') {
            apply_pair(&mut config, &key.trim().to_lowercase(), value);
        }
    }

    config
}

/// Recurso compartido declarado en `smb.conf`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Share {
    /// Nombre de la sección.
    pub name: String,
    /// Ruta en disco.
    pub path: Option<String>,
    /// Solo lectura.
    pub read_only: bool,
    /// Acceso de invitados.
    pub guest_ok: bool,
    /// Visible en el explorador de red.
    pub browseable: bool,
    /// Usuarios autorizados.
    pub valid_users: Vec<String>,
    /// Principales con permiso de escritura.
    pub write_list: Vec<String>,
    /// Comentario descriptivo.
    pub comment: Option<String>,
}

/// Extrae los recursos compartidos (todas las secciones salvo `[global]`).
pub fn parse_shares(text: &str) -> Vec<Share> {
    let mut shares: Vec<Share> = Vec::new();
    let mut current: Option<Share> = None;

    for raw_line in text.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
            continue;
        }

        if line.starts_with('[') && line.ends_with(']') {
            if let Some(share) = current.take() {
                shares.push(share);
            }
            let name = line[1..line.len() - 1].trim().to_string();
            if name.eq_ignore_ascii_case("global") || name.is_empty() {
                continue;
            }
            current = Some(Share {
                name,
                path: None,
                read_only: true,
                guest_ok: false,
                browseable: true,
                valid_users: Vec::new(),
                write_list: Vec::new(),
                comment: None,
            });
            continue;
        }

        let Some(share) = current.as_mut() else {
            continue;
        };
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim().to_lowercase();
        let value = value.trim();

        match key.as_str() {
            "path" => share.path = Some(value.to_string()),
            "read only" => {
                share.read_only = !matches!(value.to_lowercase().as_str(), "no" | "false" | "0")
            }
            "guest ok" | "public" => {
                share.guest_ok = matches!(value.to_lowercase().as_str(), "yes" | "true" | "1")
            }
            "browseable" | "browsable" => {
                share.browseable = !matches!(value.to_lowercase().as_str(), "no" | "false" | "0")
            }
            "valid users" => {
                share.valid_users = value
                    .split(|c: char| c == ',' || c.is_whitespace())
                    .filter(|user| !user.is_empty())
                    .map(str::to_string)
                    .collect();
            }
            "write list" => {
                share.write_list = value
                    .split(|c: char| c == ',' || c.is_whitespace())
                    .filter(|user| !user.is_empty())
                    .map(str::to_string)
                    .collect();
            }
            "comment" => share.comment = Some(value.to_string()),
            _ => {}
        }
    }

    if let Some(share) = current {
        shares.push(share);
    }

    shares
}

#[cfg(test)]
mod tests {
    use super::{parse_smb_conf, parse_testparm};

    const REAL_SMB_CONF: &str = r#"
[global]
   workgroup = WORKGROUP
   netbios name = TSDEBIAN
   server min protocol = SMB2_02
   server smb encrypt = desired
   server signing = mandatory

[SISTEMAS]
   path = /srv/nas/SISTEMAS
   workgroup = IGNORADO
"#;

    #[test]
    fn lee_solo_la_seccion_global() {
        let config = parse_smb_conf(REAL_SMB_CONF);
        assert_eq!(config.workgroup.as_deref(), Some("WORKGROUP"));
        assert_eq!(config.netbios_name.as_deref(), Some("TSDEBIAN"));
        assert_eq!(config.min_protocol.as_deref(), Some("smb2_02"));
        assert_eq!(config.encrypt.as_deref(), Some("desired"));
        assert_eq!(config.signing.as_deref(), Some("mandatory"));
        assert!(config.netbios_enabled);
    }

    #[test]
    fn normaliza_valores_de_testparm() {
        let text = "\tworkgroup = WORKGROUP\n\tnetbios name = TSDEBIAN\n\tserver max protocol = SMB3\n\tserver signing = required\n\tdisable netbios = No\n";
        let config = parse_testparm(text);
        assert_eq!(config.workgroup.as_deref(), Some("WORKGROUP"));
        assert_eq!(config.max_protocol.as_deref(), Some("smb3"));
        assert_eq!(config.signing.as_deref(), Some("required"));
        assert!(config.netbios_enabled);
    }

    #[test]
    fn default_es_netbios_habilitado() {
        assert!(super::SambaConfig::default().netbios_enabled);
        assert!(parse_smb_conf("").netbios_enabled);
    }

    #[test]
    fn extrae_recursos_compartidos_completos() {
        let text = "[global]
   workgroup = TEAM-JOFRATO

[SISTEMAS]
   path = /srv/nas/SISTEMAS
   read only = no
   valid users = @grp_samba
   comment = Sistemas

[CONTABILIDAD$]
   path = /srv/nas/CONTABILIDAD
   read only = yes
   guest ok = no
   browseable = no
";

        let shares = super::parse_shares(text);
        assert_eq!(shares.len(), 2);
        assert_eq!(shares[0].name, "SISTEMAS");
        assert_eq!(shares[0].path.as_deref(), Some("/srv/nas/SISTEMAS"));
        assert!(!shares[0].read_only);
        assert_eq!(shares[0].valid_users, vec!["@grp_samba"]);
        assert_eq!(shares[0].comment.as_deref(), Some("Sistemas"));

        assert_eq!(shares[1].name, "CONTABILIDAD$");
        assert!(shares[1].read_only);
        assert!(!shares[1].browseable);
    }

    #[test]
    fn sin_recursos_devuelve_lista_vacia() {
        assert!(super::parse_shares(
            "[global]
 workgroup = WG
"
        )
        .is_empty());
    }

    #[test]
    fn merge_prioriza_valores_efectivos() {
        let mut base = parse_smb_conf(REAL_SMB_CONF);
        let effective = parse_testparm("\tserver signing = required\n");
        base.merge_from(&effective);
        assert_eq!(base.signing.as_deref(), Some("required"));
        assert_eq!(base.workgroup.as_deref(), Some("WORKGROUP"));
    }
}
