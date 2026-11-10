use std::process::ExitCode;

use nas_core::{ApiResponse, NasPaths, NasResult};
use nas_engine::collect_network;
use nas_exec::Runner;

use super::common::print_json;

pub fn network(json: bool) -> NasResult<ExitCode> {
    let paths = NasPaths::from_env();
    let runner = Runner::new();
    let overview = collect_network(&runner, &paths);

    if json {
        print_json(&ApiResponse::ok(overview, "Inventario de red"))?;
        return Ok(ExitCode::SUCCESS);
    }

    println!("CONFIGURACIÓN Y VISIBILIDAD DE RED");
    println!("Hostname        : {}", overview.hostname);
    println!("NetBIOS         : {}", overview.netbios);
    println!(
        "Workgroup       : {}",
        overview.workgroup.as_deref().unwrap_or("N/D")
    );
    for iface in &overview.interfaces {
        println!(
            "Interfaz        : {} {} {}{} (MTU {})",
            iface.name,
            iface.state,
            iface.ipv4.as_deref().unwrap_or("sin IPv4"),
            iface.prefix.map(|p| format!("/{p}")).unwrap_or_default(),
            iface
                .mtu
                .map(|m| m.to_string())
                .unwrap_or_else(|| "N/D".into())
        );
    }
    println!(
        "Puerta enlace   : {}",
        overview.gateway.as_deref().unwrap_or("N/D")
    );
    println!(
        "DNS             : {}",
        if overview.dns.is_empty() {
            "N/D".to_string()
        } else {
            overview.dns.join(", ")
        }
    );
    println!(
        "SMB             : {} → {} (firma: {}, cifrado: {})",
        overview.samba.min_protocol.as_deref().unwrap_or("N/D"),
        overview.samba.max_protocol.as_deref().unwrap_or("N/D"),
        overview.samba.signing.as_deref().unwrap_or("N/D"),
        overview.samba.encrypt.as_deref().unwrap_or("N/D")
    );
    let services = overview
        .services
        .iter()
        .map(|(name, active)| format!("{name}={}", if *active { "activo" } else { "inactivo" }))
        .collect::<Vec<_>>()
        .join(" ");
    println!("Demonios        : {services}");
    let ports = overview
        .listening
        .iter()
        .map(|port| format!("{}/{}", port.port, port.proto))
        .collect::<Vec<_>>()
        .join(" ");
    println!("Puertos         : {ports}");
    println!(
        "Cortafuegos     : {}",
        if overview.firewall.active {
            "activo"
        } else {
            "inactivo"
        }
    );
    println!(
        "Sesiones SMB    : {}",
        overview
            .smb_sessions
            .map(|n| n.to_string())
            .unwrap_or_else(|| "no verificable".to_string())
    );
    println!("Actualizado     : {}", overview.generated_at);

    Ok(ExitCode::SUCCESS)
}
