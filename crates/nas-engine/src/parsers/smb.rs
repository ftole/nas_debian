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
    fn merge_prioriza_valores_efectivos() {
        let mut base = parse_smb_conf(REAL_SMB_CONF);
        let effective = parse_testparm("\tserver signing = required\n");
        base.merge_from(&effective);
        assert_eq!(base.signing.as_deref(), Some("required"));
        assert_eq!(base.workgroup.as_deref(), Some("WORKGROUP"));
    }
}
