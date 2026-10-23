//! Parser de `ss -H -tln` / `ss -H -uln` (puertos en escucha).

use serde::Serialize;

/// Puerto en escucha con su servicio asociado, si es conocido.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ListeningPort {
    /// Puerto.
    pub port: u16,
    /// Protocolo (`tcp` o `udp`).
    pub proto: String,
    /// Servicio que lo expone, cuando se puede inferir.
    pub service: Option<String>,
}

/// Puertos UDP relevantes para la operación (se descartan los efímeros del cliente DHCP).
pub const UDP_WHITELIST: &[u16] = &[53, 123, 137, 138, 161, 3702, 5353, 5355];

/// Mapea un puerto conocido con el servicio que lo publica en este servidor.
pub fn service_for_port(port: u16) -> Option<&'static str> {
    match port {
        22 => Some("sshd"),
        25 => Some("exim4"),
        53 => Some("dns"),
        80 => Some("nginx"),
        443 => Some("nginx"),
        137 | 138 => Some("nmbd"),
        139 | 445 => Some("smbd"),
        3702 | 5355 => Some("wsdd2"),
        _ => None,
    }
}

/// Extrae los puertos en escucha de una salida de `ss`.
pub fn parse_ss(text: &str, proto: &str) -> Vec<ListeningPort> {
    let mut ports: Vec<ListeningPort> = Vec::new();

    for line in text.lines() {
        let cols: Vec<&str> = line.split_whitespace().collect();
        if cols.len() < 4 {
            continue;
        }

        let local = cols[3];
        let Some((_, raw_port)) = local.rsplit_once(':') else {
            continue;
        };
        let Ok(port) = raw_port.parse::<u16>() else {
            continue;
        };

        if port == 0 {
            continue;
        }
        if proto.eq_ignore_ascii_case("udp") && !UDP_WHITELIST.contains(&port) {
            continue;
        }
        if ports.iter().any(|p| p.port == port && p.proto == proto) {
            continue;
        }

        ports.push(ListeningPort {
            port,
            proto: proto.to_string(),
            service: service_for_port(port).map(str::to_string),
        });
    }

    ports.sort_by_key(|p| p.port);
    ports
}

#[cfg(test)]
mod tests {
    use super::{parse_ss, service_for_port, UDP_WHITELIST};

    const SS_TCP: &str = "\
LISTEN 0 4096 0.0.0.0:139 0.0.0.0:*
LISTEN 0 4096 0.0.0.0:445 0.0.0.0:*
LISTEN 0 511 0.0.0.0:443 0.0.0.0:*
LISTEN 0 4096 [::]:22 [::]:*
";

    const SS_UDP: &str = "\
UNCONN 0 0 0.0.0.0%eno1:3702 0.0.0.0:*
UNCONN 0 0 0.0.0.0%eno1:5355 0.0.0.0:*
UNCONN 0 0 0.0.0.0:68 0.0.0.0:*
UNCONN 0 0 0.0.0.0:546 0.0.0.0:*
";

    #[test]
    fn extrae_puertos_tcp_con_servicio() {
        let ports = parse_ss(SS_TCP, "tcp");
        let numbers: Vec<u16> = ports.iter().map(|p| p.port).collect();
        assert_eq!(numbers, vec![22, 139, 443, 445]);
        assert_eq!(ports[1].service.as_deref(), Some("smbd"));
        assert_eq!(ports[2].service.as_deref(), Some("nginx"));
    }

    #[test]
    fn descarta_udp_efimeros() {
        let ports = parse_ss(SS_UDP, "udp");
        let numbers: Vec<u16> = ports.iter().map(|p| p.port).collect();
        assert_eq!(numbers, vec![3702, 5355]);
        assert!(!numbers.contains(&68));
        assert!(!UDP_WHITELIST.contains(&546));
    }

    #[test]
    fn ignora_lineas_incompletas() {
        assert!(parse_ss("LISTEN 0 4096\n\n", "tcp").is_empty());
    }

    #[test]
    fn mapea_servicios_conocidos() {
        assert_eq!(service_for_port(445), Some("smbd"));
        assert_eq!(service_for_port(5355), Some("wsdd2"));
        assert_eq!(service_for_port(9999), None);
    }
}
