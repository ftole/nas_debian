//! Cortafuegos UFW/iptables-nft con detección de subred LAN y protección anti-lockout.

use std::net::Ipv4Addr;
use std::path::Path;
use std::time::Duration;

use nas_core::{NasError, NasResult};
use nas_exec::{CommandSpec, Runner};

use crate::deploy::web::write_artifact;
use crate::deploy::{deps, Artifact};

use super::{service_is_active, spec_with_sudo, HardenOptions};

pub(super) fn is_ufw_enabled_file(path: &Path) -> bool {
    std::fs::read_to_string(path).is_ok_and(|raw| {
        raw.lines().any(|line| {
            line.split_once('=').is_some_and(|(k, v)| {
                k.trim().eq_ignore_ascii_case("ENABLED") && v.trim().eq_ignore_ascii_case("yes")
            })
        })
    })
}

fn is_ufw_active(runner: &Runner, sudo: bool) -> bool {
    if is_ufw_enabled_file(Path::new("/etc/ufw/ufw.conf")) {
        return true;
    }
    let out = runner.run(&spec_with_sudo(
        CommandSpec::new("ufw")
            .arg("status")
            .timeout(Duration::from_secs(10)),
        sudo,
    ));
    out.is_ok_and(|o| o.is_success() && o.stdout.contains("Status: active"))
}

/// Calcula el prefijo de red CIDR IPv4 (descartando loopback y link-local).
pub fn ipv4_cidr_network(cidr: &str) -> Option<String> {
    let (ip_str, prefix_str) = cidr.trim().split_once('/')?;
    let ip: Ipv4Addr = ip_str.trim().parse().ok()?;
    let prefix: u32 = prefix_str.trim().parse().ok()?;
    if prefix > 32 {
        return None;
    }
    if ip.is_loopback() || ip.is_link_local() {
        return None;
    }
    let mask = if prefix == 0 {
        0u32
    } else {
        u32::MAX << (32 - prefix)
    };
    let net = Ipv4Addr::from(u32::from(ip) & mask);
    Some(format!("{net}/{prefix}"))
}

/// Valida una lista separada por comas de subredes CIDR IPv4.
pub fn parse_cidr_list(raw: &str) -> NasResult<Vec<String>> {
    let mut out = Vec::new();
    for item in raw.split(',') {
        let trimmed = item.trim();
        if trimmed.is_empty() {
            continue;
        }
        let net = ipv4_cidr_network(trimmed)
            .ok_or_else(|| NasError::Invalid(format!("subred CIDR IPv4 inválida: '{trimmed}'")))?;
        if !out.contains(&net) {
            out.push(net);
        }
    }
    Ok(out)
}

/// Detecta las subredes LAN IPv4 activas a partir de la salida de `ip -4 -o addr show scope global`.
pub fn parse_ip_addr_cidrs(stdout: &str) -> Vec<String> {
    let mut cidrs = Vec::new();
    for line in stdout.lines() {
        let mut tokens = line.split_whitespace();
        while let Some(tok) = tokens.next() {
            if tok == "inet" {
                if let Some(addr_cidr) = tokens.next() {
                    if let Some(net) = ipv4_cidr_network(addr_cidr) {
                        if !cidrs.contains(&net) {
                            cidrs.push(net);
                        }
                    }
                }
                break;
            }
        }
    }
    cidrs
}

/// Extrae la IPv4 de origen de `SSH_CONNECTION` para la regla anti-lockout de UFW (`IPV6=no`).
pub fn parse_ssh_client_ip(ssh_connection: &str) -> Option<String> {
    let ip_raw = ssh_connection.split_whitespace().next()?.trim();
    if let Ok(ip) = ip_raw.parse::<Ipv4Addr>() {
        if !ip.is_loopback() && !ip.is_link_local() {
            return Some(ip.to_string());
        }
    }
    None
}

/// Extrae el puerto de servidor SSH desde `SSH_CONNECTION` (4º campo; por defecto 22).
pub fn parse_ssh_server_port(ssh_connection: &str) -> u16 {
    ssh_connection
        .split_whitespace()
        .nth(3)
        .and_then(|p| p.trim().parse::<u16>().ok())
        .filter(|&p| p > 0)
        .unwrap_or(22)
}

/// Actualiza `IPV6=no` en `/etc/default/ufw` de forma idempotente.
pub fn render_ufw_default_ipv6_disabled(existing: &str) -> String {
    let mut lines = Vec::new();
    let mut found = false;
    for line in existing.lines() {
        if line.trim_start().starts_with("IPV6=") {
            lines.push("IPV6=no".to_string());
            found = true;
        } else {
            lines.push(line.to_string());
        }
    }
    if !found {
        lines.push("IPV6=no".to_string());
    }
    let mut out = lines.join("\n");
    out.push('\n');
    out
}

pub(super) fn configure_ufw_firewall(
    runner: &Runner,
    options: &HardenOptions,
    actions: &mut Vec<String>,
    warnings: &mut Vec<String>,
) -> NasResult<()> {
    let ufw_present = Path::new("/usr/sbin/ufw").is_file() || Path::new("/sbin/ufw").is_file();
    if !ufw_present {
        if !options.apply {
            actions.push("[dry-run] instalar paquete ufw y activar reglas LAN".into());
            return Ok(());
        }
        if let Err(err) = deps::ensure_packages(runner, &["ufw"], options.sudo) {
            warnings.push(format!("no se pudo instalar ufw automáticamente: {err}"));
            return Ok(());
        }
    }

    let ufw_active = is_ufw_active(runner, options.sudo);
    let nft_active = service_is_active(runner, "nftables");
    if (ufw_active || nft_active) && !options.force_firewall {
        actions.push(format!(
            "cortafuegos ya activo ({}): reglas existentes preservadas",
            if ufw_active { "ufw" } else { "nftables" }
        ));
        return Ok(());
    }

    let explicit_cidrs = options
        .lan_cidr
        .clone()
        .or_else(|| std::env::var("NAS_LAN_CIDR").ok());
    let cidrs = if let Some(raw) = explicit_cidrs.filter(|s| !s.trim().is_empty()) {
        parse_cidr_list(&raw)?
    } else {
        let out = runner.run(
            &CommandSpec::new("ip")
                .args(["-4", "-o", "addr", "show", "scope", "global"])
                .timeout(Duration::from_secs(10)),
        );
        out.ok()
            .filter(|o| o.is_success())
            .map(|o| parse_ip_addr_cidrs(&o.stdout))
            .unwrap_or_default()
    };

    let ssh_conn_env = std::env::var("SSH_CONNECTION").ok();
    let ssh_client_ip = ssh_conn_env.as_deref().and_then(parse_ssh_client_ip);
    let ssh_port = ssh_conn_env
        .as_deref()
        .map(parse_ssh_server_port)
        .unwrap_or(22);
    let ssh_port_str = ssh_port.to_string();

    if !options.apply {
        actions.push(format!(
            "[dry-run] configurar ufw (default deny incoming, LAN={:?}, ssh_anti_lockout={:?}, ssh_port={})",
            cidrs, ssh_client_ip, ssh_port
        ));
        return Ok(());
    }

    if let Ok(default_ufw) = std::fs::read_to_string("/etc/default/ufw") {
        let updated = render_ufw_default_ipv6_disabled(&default_ufw);
        if updated != default_ufw {
            let _ = write_artifact(
                runner,
                &Artifact {
                    path: "/etc/default/ufw".into(),
                    mode: 0o644,
                    content: updated,
                },
                options.sudo,
            );
        }
    }

    let run_ufw = |args: &[&str]| -> NasResult<()> {
        let out = runner.run(&spec_with_sudo(
            CommandSpec::new("ufw")
                .args(args.iter().copied())
                .timeout(Duration::from_secs(20)),
            options.sudo,
        ))?;
        if !out.is_success() {
            return Err(NasError::Denied(format!(
                "ufw {} falló: {}",
                args.join(" "),
                if out.stderr.trim().is_empty() {
                    out.stdout.trim()
                } else {
                    out.stderr.trim()
                }
            )));
        }
        Ok(())
    };

    run_ufw(&["default", "deny", "incoming"])?;
    run_ufw(&["default", "allow", "outgoing"])?;

    if let Some(ref ssh_ip) = ssh_client_ip {
        run_ufw(&[
            "allow",
            "from",
            ssh_ip,
            "to",
            "any",
            "port",
            &ssh_port_str,
            "proto",
            "tcp",
            "comment",
            "SSH admin anti-lockout",
        ])?;
    }

    let mut ports = vec![
        ("22", "tcp", "SSH LAN"),
        ("80", "tcp", "Panel NAS HTTP redirect"),
        ("443", "tcp", "Panel NAS HTTPS"),
        ("139", "tcp", "NetBIOS SMB"),
        ("445", "tcp", "Samba SMB"),
        ("137", "udp", "NetBIOS NS"),
        ("138", "udp", "NetBIOS DGM"),
        ("3702", "udp", "WSDD2 Discovery"),
        ("5355", "tcp", "LLMNR TCP"),
        ("5355", "udp", "LLMNR UDP"),
        ("5357", "tcp", "WSDD2 HTTP"),
    ];
    if ssh_port != 22 {
        ports.push((&ssh_port_str, "tcp", "SSH LAN custom"));
    }

    if cidrs.is_empty() {
        warnings.push(
            "no se detectó subred LAN IPv4; aplicando reglas UFW por puerto sin restricción de origen para evitar lockout".into(),
        );
        for (port, proto, comment) in &ports {
            let rule = format!("{port}/{proto}");
            run_ufw(&["allow", &rule, "comment", comment])?;
        }
    } else {
        for cidr in &cidrs {
            for (port, proto, comment) in &ports {
                run_ufw(&[
                    "allow", "from", cidr, "to", "any", "port", port, "proto", proto, "comment",
                    comment,
                ])?;
            }
        }
    }

    run_ufw(&["--force", "enable"])?;
    actions.push(format!(
        "cortafuegos ufw activado (deny incoming, LAN={})",
        if cidrs.is_empty() {
            "any".to_string()
        } else {
            cidrs.join(",")
        }
    ));
    Ok(())
}
