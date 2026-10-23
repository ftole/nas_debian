//! Parser de `ip -j addr show` y `ip -j route show default`.

use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
struct RawLink {
    #[serde(default)]
    ifname: String,
    #[serde(default)]
    operstate: Option<String>,
    #[serde(default)]
    address: Option<String>,
    #[serde(default)]
    mtu: Option<u32>,
    #[serde(default)]
    addr_info: Vec<RawAddr>,
}

#[derive(Debug, Deserialize)]
struct RawAddr {
    #[serde(default)]
    family: String,
    #[serde(default)]
    local: Option<String>,
    #[serde(default)]
    prefixlen: Option<u8>,
}

#[derive(Debug, Deserialize)]
struct RawRoute {
    #[serde(default)]
    gateway: Option<String>,
    #[serde(default)]
    dev: Option<String>,
    #[serde(default)]
    protocol: Option<String>,
}

/// Interfaz de red normalizada.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Interface {
    /// Nombre de la interfaz.
    pub name: String,
    /// Estado operativo (`UP`, `DOWN`, `UNKNOWN`).
    pub state: String,
    /// Dirección IPv4, si existe.
    pub ipv4: Option<String>,
    /// Longitud de prefijo de la IPv4.
    pub prefix: Option<u8>,
    /// Dirección MAC.
    pub mac: Option<String>,
    /// MTU configurada.
    pub mtu: Option<u32>,
    /// Velocidad del enlace en Mbps (si el driver la expone).
    pub speed_mbps: Option<u32>,
}

/// Ruta por defecto del sistema.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DefaultRoute {
    /// Puerta de enlace.
    pub gateway: Option<String>,
    /// Interfaz de salida.
    pub device: Option<String>,
    /// Indica si la ruta fue aprendida por DHCP.
    pub dhcp: bool,
}

/// Convierte la salida de `ip -j addr show` en interfaces, excluyendo `lo`.
pub fn parse_interfaces(raw: &str) -> Result<Vec<Interface>, serde_json::Error> {
    let links: Vec<RawLink> = serde_json::from_str(raw)?;
    let mut interfaces = Vec::new();

    for link in links {
        if link.ifname.is_empty() || link.ifname == "lo" {
            continue;
        }

        let inet = link.addr_info.iter().find(|addr| addr.family == "inet");

        interfaces.push(Interface {
            name: link.ifname,
            state: link
                .operstate
                .unwrap_or_else(|| "UNKNOWN".to_string())
                .to_uppercase(),
            ipv4: inet.and_then(|addr| addr.local.clone()),
            prefix: inet.and_then(|addr| addr.prefixlen),
            mac: link.address.filter(|mac| !mac.is_empty()),
            mtu: link.mtu,
            speed_mbps: None,
        });
    }

    Ok(interfaces)
}

/// Convierte la salida de `ip -j route show default` en la ruta por defecto.
pub fn parse_default_route(raw: &str) -> Result<Option<DefaultRoute>, serde_json::Error> {
    let routes: Vec<RawRoute> = serde_json::from_str(raw)?;

    Ok(routes.into_iter().next().map(|route| DefaultRoute {
        dhcp: route
            .protocol
            .as_deref()
            .is_some_and(|proto| proto.eq_ignore_ascii_case("dhcp")),
        gateway: route.gateway,
        device: route.dev,
    }))
}

/// Formatea una IPv4 con su prefijo (`10.10.1.222/24`).
pub fn format_cidr(ip: Option<&str>, prefix: Option<u8>) -> Option<String> {
    let ip = ip?;
    Some(match prefix {
        Some(prefix) => format!("{ip}/{prefix}"),
        None => ip.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::{format_cidr, parse_default_route, parse_interfaces};

    const ADDR_JSON: &str = r#"[
      {"ifname":"lo","operstate":"UNKNOWN","address":"00:00:00:00:00:00","mtu":65536,
       "addr_info":[{"family":"inet","local":"127.0.0.1","prefixlen":8}]},
      {"ifname":"eno1","operstate":"UP","address":"b0:22:7a:21:c6:e3","mtu":1500,
       "addr_info":[{"family":"inet","local":"10.10.1.222","prefixlen":24}]}
    ]"#;

    #[test]
    fn omite_loopback_y_extrae_ipv4() {
        let interfaces = parse_interfaces(ADDR_JSON).expect("json válido");
        assert_eq!(interfaces.len(), 1);
        assert_eq!(interfaces[0].name, "eno1");
        assert_eq!(interfaces[0].state, "UP");
        assert_eq!(interfaces[0].ipv4.as_deref(), Some("10.10.1.222"));
        assert_eq!(interfaces[0].prefix, Some(24));
        assert_eq!(interfaces[0].mtu, Some(1500));
    }

    #[test]
    fn detecta_ruta_dhcp() {
        let json = r#"[{"dst":"default","gateway":"10.10.1.1","dev":"eno1","protocol":"dhcp","metric":1002}]"#;
        let route = parse_default_route(json)
            .expect("json válido")
            .expect("ruta");
        assert_eq!(route.gateway.as_deref(), Some("10.10.1.1"));
        assert_eq!(route.device.as_deref(), Some("eno1"));
        assert!(route.dhcp);
    }

    #[test]
    fn ruta_estatica_no_es_dhcp() {
        let json = r#"[{"dst":"default","gateway":"192.168.1.1","dev":"eth0"}]"#;
        let route = parse_default_route(json)
            .expect("json válido")
            .expect("ruta");
        assert!(!route.dhcp);
    }

    #[test]
    fn formatea_cidr() {
        assert_eq!(
            format_cidr(Some("10.10.1.222"), Some(24)).as_deref(),
            Some("10.10.1.222/24")
        );
        assert_eq!(
            format_cidr(Some("10.10.1.222"), None).as_deref(),
            Some("10.10.1.222")
        );
        assert_eq!(format_cidr(None, Some(24)), None);
    }
}
