//! Vista agregada de red: interfaces, ruta, DNS, Samba, servicios y puertos.

use std::collections::BTreeMap;

use nas_core::NasPaths;
use nas_exec::{CommandSpec, Runner, SudoMode};
use serde::Serialize;

use crate::parsers::{ip, resolv, smb, ss};

/// Estado del cortafuegos del servidor.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FirewallStatus {
    /// Indica si UFW está habilitado.
    pub active: bool,
    /// Indica si las reglas pudieron leerse (requiere privilegios).
    pub rules_visible: bool,
    /// Archivo del que se obtuvo el estado.
    pub source: Option<String>,
}

/// Inventario completo de red consumido por la web y por la CLI.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct NetworkOverview {
    /// Nombre del host.
    pub hostname: String,
    /// Nombre NetBIOS efectivo.
    pub netbios: String,
    /// Grupo de trabajo.
    pub workgroup: Option<String>,
    /// Interfaces detectadas.
    pub interfaces: Vec<ip::Interface>,
    /// IPv4 primaria.
    pub primary_ip: Option<String>,
    /// Puerta de enlace por defecto.
    pub gateway: Option<String>,
    /// Servidores DNS.
    pub dns: Vec<String>,
    /// Indica si la ruta por defecto proviene de DHCP.
    pub dhcp: bool,
    /// Configuración efectiva de Samba.
    pub samba: smb::SambaConfig,
    /// Estado de los demonios de red.
    pub services: BTreeMap<String, bool>,
    /// Puertos TCP/UDP en escucha.
    pub listening: Vec<ss::ListeningPort>,
    /// Estado del cortafuegos.
    pub firewall: FirewallStatus,
    /// Sesiones SMB activas, si se pudo consultar.
    pub smb_sessions: Option<u32>,
    /// Hora de la última verificación (`HH:MM:SS`).
    pub generated_at: String,
    /// Marca de tiempo UNIX de la última verificación.
    pub generated_timestamp: i64,
}

/// Recolecta el inventario de red; en plataformas no Linux devuelve datos simulados.
pub fn collect(runner: &Runner, paths: &NasPaths) -> NetworkOverview {
    if cfg!(target_os = "linux") {
        collect_linux(runner, paths)
    } else {
        simulated()
    }
}

fn read_optional(path: &str) -> Option<String> {
    std::fs::read_to_string(path)
        .ok()
        .map(|text| text.trim().to_string())
        .filter(|text| !text.is_empty())
}

fn collect_linux(runner: &Runner, paths: &NasPaths) -> NetworkOverview {
    let hostname = read_optional("/etc/hostname").unwrap_or_else(|| "SRV-NAS".to_string());

    let interfaces = runner
        .run(&CommandSpec::new("ip").args(["-j", "addr", "show"]))
        .ok()
        .filter(|out| out.is_success())
        .and_then(|out| ip::parse_interfaces(&out.stdout).ok())
        .map(|mut list| {
            for iface in &mut list {
                iface.speed_mbps = read_optional(&format!("/sys/class/net/{}/speed", iface.name))
                    .and_then(|raw| raw.parse::<u32>().ok())
                    .filter(|speed| *speed > 0);
            }
            list
        })
        .unwrap_or_default();

    let route = runner
        .run(&CommandSpec::new("ip").args(["-j", "route", "show", "default"]))
        .ok()
        .filter(|out| out.is_success())
        .and_then(|out| ip::parse_default_route(&out.stdout).ok())
        .flatten();

    let dns = read_optional("/etc/resolv.conf")
        .map(|text| resolv::parse_nameservers(&text))
        .unwrap_or_default();

    let samba = collect_samba(runner, paths);
    let services = ["smbd", "nmbd", "wsdd2"]
        .into_iter()
        .map(|service| {
            let active = runner
                .run(&CommandSpec::new("systemctl").args(["is-active", service]))
                .map(|out| out.stdout == "active")
                .unwrap_or(false);
            (service.to_string(), active)
        })
        .collect();

    let mut listening = runner
        .run(&CommandSpec::new("ss").args(["-H", "-tln"]))
        .map(|out| ss::parse_ss(&out.stdout, "tcp"))
        .unwrap_or_default();
    listening.extend(
        runner
            .run(&CommandSpec::new("ss").args(["-H", "-uln"]))
            .map(|out| ss::parse_ss(&out.stdout, "udp"))
            .unwrap_or_default(),
    );
    listening.sort_by_key(|port| (port.port, port.proto.clone()));

    let now = chrono::Local::now();

    NetworkOverview {
        hostname,
        netbios: samba
            .netbios_name
            .clone()
            .unwrap_or_else(|| "SRV-NAS".to_string()),
        workgroup: samba.workgroup.clone(),
        interfaces,
        primary_ip: runner
            .run(&CommandSpec::new("ip").args(["-4", "-o", "addr", "show", "scope", "global"]))
            .ok()
            .and_then(|out| {
                out.stdout
                    .split_whitespace()
                    .skip_while(|token| *token != "inet")
                    .nth(1)
                    .and_then(|cidr| cidr.split('/').next())
                    .map(str::to_string)
            }),
        gateway: route.as_ref().and_then(|route| route.gateway.clone()),
        dns,
        dhcp: route.is_some_and(|route| route.dhcp),
        samba,
        services,
        listening,
        firewall: collect_firewall(),
        smb_sessions: collect_smb_sessions(runner),
        generated_at: now.format("%H:%M:%S").to_string(),
        generated_timestamp: now.timestamp(),
    }
}

fn collect_samba(runner: &Runner, paths: &NasPaths) -> smb::SambaConfig {
    // `testparm -s` puede devolver código distinto de cero por avisos (p. ej.
    // al ejecutarse sin privilegios) pero su salida sigue siendo válida.
    let effective = runner
        .run(&CommandSpec::new("testparm").arg("-s"))
        .ok()
        .map(|out| smb::parse_testparm(&out.stdout))
        .filter(|config| config.min_protocol.is_some() || config.max_protocol.is_some());

    let mut config = std::fs::read_to_string(&paths.samba_conf)
        .ok()
        .map(|text| smb::parse_smb_conf(&text))
        .unwrap_or_default();

    if let Some(effective) = effective {
        config.merge_from(&effective);
    }

    config
}

fn collect_firewall() -> FirewallStatus {
    let source = "/etc/ufw/ufw.conf";
    let active = read_optional(source).is_some_and(|text| {
        text.lines().any(|line| match line.split_once('=') {
            Some((key, value)) => {
                key.trim().eq_ignore_ascii_case("ENABLED")
                    && value.trim().eq_ignore_ascii_case("yes")
            }
            None => false,
        })
    });

    FirewallStatus {
        active,
        rules_visible: false,
        source: if std::path::Path::new(source).exists() {
            Some(source.to_string())
        } else {
            None
        },
    }
}

fn collect_smb_sessions(runner: &Runner) -> Option<u32> {
    let output = runner
        .run(
            &CommandSpec::new("smbstatus")
                .args(["-b"])
                .sudo(SudoMode::Passwordless)
                .timeout(std::time::Duration::from_secs(10)),
        )
        .ok()?;

    if !output.is_success() {
        return None;
    }

    Some(
        output
            .stdout
            .lines()
            .filter(|line| line.trim_start().starts_with(|c: char| c.is_ascii_digit()))
            .count() as u32,
    )
}

fn simulated() -> NetworkOverview {
    let now = chrono::Local::now();
    let mut services = BTreeMap::new();
    services.insert("smbd".to_string(), true);
    services.insert("nmbd".to_string(), true);
    services.insert("wsdd2".to_string(), true);

    NetworkOverview {
        hostname: "SRV-NAS".to_string(),
        netbios: "SRV-NAS".to_string(),
        workgroup: Some("TEAM-JOFRATO".to_string()),
        interfaces: vec![ip::Interface {
            name: "eth0".to_string(),
            state: "UP".to_string(),
            ipv4: Some("10.10.1.2".to_string()),
            prefix: Some(24),
            mac: Some("00:11:22:33:44:55".to_string()),
            mtu: Some(1500),
            speed_mbps: Some(1000),
        }],
        primary_ip: Some("10.10.1.2".to_string()),
        gateway: Some("10.10.1.1".to_string()),
        dns: vec!["10.16.0.23".to_string(), "10.16.0.60".to_string()],
        dhcp: true,
        samba: smb::SambaConfig {
            workgroup: Some("TEAM-JOFRATO".to_string()),
            netbios_name: Some("SRV-NAS".to_string()),
            min_protocol: Some("smb2_02".to_string()),
            max_protocol: Some("smb3".to_string()),
            signing: Some("required".to_string()),
            encrypt: Some("desired".to_string()),
            netbios_enabled: true,
        },
        services,
        listening: vec![
            ss::ListeningPort {
                port: 22,
                proto: "tcp".to_string(),
                service: Some("sshd".to_string()),
            },
            ss::ListeningPort {
                port: 80,
                proto: "tcp".to_string(),
                service: Some("nginx".to_string()),
            },
            ss::ListeningPort {
                port: 139,
                proto: "tcp".to_string(),
                service: Some("smbd".to_string()),
            },
            ss::ListeningPort {
                port: 443,
                proto: "tcp".to_string(),
                service: Some("nginx".to_string()),
            },
            ss::ListeningPort {
                port: 445,
                proto: "tcp".to_string(),
                service: Some("smbd".to_string()),
            },
            ss::ListeningPort {
                port: 3702,
                proto: "udp".to_string(),
                service: Some("wsdd2".to_string()),
            },
            ss::ListeningPort {
                port: 5355,
                proto: "udp".to_string(),
                service: Some("wsdd2".to_string()),
            },
        ],
        firewall: FirewallStatus {
            active: true,
            rules_visible: false,
            source: Some("/etc/ufw/ufw.conf".to_string()),
        },
        smb_sessions: Some(0),
        generated_at: now.format("%H:%M:%S").to_string(),
        generated_timestamp: now.timestamp(),
    }
}

#[cfg(test)]
mod tests {

    #[test]
    fn inventario_simulado_tiene_todos_los_campos() {
        let overview = super::simulated();
        assert!(!overview.hostname.is_empty());
        assert!(!overview.interfaces.is_empty());
        assert!(overview.primary_ip.is_some());
        assert!(overview.samba.min_protocol.is_some());
        assert_eq!(overview.services.len(), 3);
        assert!(!overview.listening.is_empty());
        assert!(!overview.generated_at.is_empty());
    }

    #[test]
    fn puertos_ordenados_y_sin_duplicados() {
        let overview = super::simulated();
        let mut previous = 0;
        let mut seen = std::collections::HashSet::new();
        for port in &overview.listening {
            assert!(port.port >= previous);
            previous = port.port;
            assert!(seen.insert((port.port, port.proto.clone())));
        }
    }
}
