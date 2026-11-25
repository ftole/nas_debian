//! `nasctl`: herramienta de producción.
//!
//! Esta variante **sólo** opera sobre despliegues declarados en un inventario y
//! está pensada para acompañar releases firmados. Nunca lee el árbol de
//! desarrollo ni ejecuta código local sin validar.

mod remote;

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use nas_core::{ApiResponse, NasResult};
use nas_inventory::{Inventory, ResolvedHost};
use serde::Serialize;

#[derive(Debug, Parser)]
#[command(
    name = "nasctl",
    version,
    about = "Administración de producción del servidor NAS (multi-servidor)",
    long_about = "Herramienta de producción: valida el inventario, resuelve perfiles por host \
                  y ejecuta operaciones remotas vía SSH sobre despliegues declarados."
)]
struct Cli {
    /// Ruta del inventario YAML (por defecto `inventory.yaml` o `$NAS_INVENTORY`).
    #[arg(long, global = true, value_name = "PATH")]
    inventory: Option<PathBuf>,

    /// Emite la respuesta en JSON.
    #[arg(long, global = true)]
    json: bool,

    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Lista los hosts declarados.
    Hosts,
    /// Valida el inventario y reporta problemas.
    Check,
    /// Muestra el perfil resuelto de un host.
    Status {
        /// Alias del host.
        #[arg(long)]
        host: String,
    },
    /// Ejecuta una acción permitida en uno o varios hosts.
    Exec {
        #[command(flatten)]
        options: remote::ExecOptions,
        #[command(subcommand)]
        action: remote::RemoteAction,
    },
}

#[derive(Debug, Serialize)]
struct HostSummary {
    alias: String,
    address: String,
    group: String,
    env: String,
    role: String,
    port: u16,
    user: String,
}

impl From<&ResolvedHost> for HostSummary {
    fn from(host: &ResolvedHost) -> Self {
        Self {
            alias: host.alias.clone(),
            address: host.address.clone(),
            group: host.group.clone(),
            env: host.env.clone(),
            role: host.role.clone(),
            port: host.port,
            user: host.user.clone(),
        }
    }
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let json_output = cli.json;
    match run(cli) {
        Ok(code) => code,
        Err(error) => {
            if json_output {
                let payload = ApiResponse::<serde_json::Value>::error(error.to_string());
                eprintln!(
                    "{}",
                    serde_json::to_string_pretty(&payload).unwrap_or_else(|_| error.to_string())
                );
            } else {
                eprintln!("Error: {error}");
            }
            ExitCode::from(u8::try_from(error.exit_code()).unwrap_or(1))
        }
    }
}

fn run(cli: Cli) -> NasResult<ExitCode> {
    let inventory_arg = cli.inventory.clone();
    let path = inventory_path(cli.inventory);
    let inventory = Inventory::load(&path)?;

    match cli.command {
        Command::Hosts => hosts(&inventory, &path, cli.json),
        Command::Check => check(&inventory, &path, cli.json),
        Command::Status { host } => status(&inventory, &host, cli.json),
        Command::Exec { options, action } => {
            remote::exec(inventory_arg, &action, &options, cli.json)
        }
    }
}

fn inventory_path(explicit: Option<PathBuf>) -> PathBuf {
    explicit
        .or_else(|| std::env::var("NAS_INVENTORY").ok().map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from("inventory.yaml"))
}

fn hosts(inventory: &Inventory, path: &std::path::Path, json: bool) -> NasResult<ExitCode> {
    let list: Vec<HostSummary> = inventory
        .resolve_all()
        .iter()
        .map(HostSummary::from)
        .collect();

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&ApiResponse::ok(
                serde_json::json!({ "inventory": path.display().to_string(), "hosts": list }),
                "Hosts del inventario"
            ))?
        );
        return Ok(ExitCode::SUCCESS);
    }

    println!("HOSTS DECLARADOS ({})", list.len());
    for host in &list {
        println!(
            "{:<14} {:<16} grupo={:<6} env={:<5} rol={:<16} {}:{}",
            host.alias, host.address, host.group, host.env, host.role, host.user, host.port
        );
    }

    Ok(ExitCode::SUCCESS)
}

fn check(inventory: &Inventory, path: &std::path::Path, json: bool) -> NasResult<ExitCode> {
    let problems = inventory.validate();
    let ok = problems.is_empty();

    if json {
        let data = serde_json::json!({
            "inventory": path.display().to_string(),
            "ok": ok,
            "problems": problems,
        });
        let payload = if ok {
            ApiResponse::ok(data, "Validación del inventario")
        } else {
            ApiResponse::err(data, "Validación del inventario con errores")
        };
        println!("{}", serde_json::to_string_pretty(&payload)?);
    } else {
        println!("VALIDACIÓN DEL INVENTARIO: {}", path.display());
        if ok {
            println!("[OK] Inventario válido ({} hosts)", inventory.hosts.len());
        } else {
            for problem in &problems {
                println!("[FALLO] {problem}");
            }
        }
    }

    if ok {
        Ok(ExitCode::SUCCESS)
    } else {
        Ok(ExitCode::from(1))
    }
}

fn status(inventory: &Inventory, alias: &str, json: bool) -> NasResult<ExitCode> {
    let host = inventory.resolve(alias)?;

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&ApiResponse::ok(&host, "Perfil del host"))?
        );
        return Ok(ExitCode::SUCCESS);
    }

    println!("PERFIL DEL HOST: {}", host.alias);
    println!(
        "Destino         : {}@{}:{}",
        host.user, host.address, host.port
    );
    println!("Grupo / entorno : {} ({})", host.group, host.env);
    println!("Rol             : {}", host.role);
    println!("Repositorio     : {}", host.repo);
    println!("Docroot web     : {}", host.web);
    println!("Estado          : {}", host.state);
    println!("Timeout         : {}s", host.timeout_secs);

    if !host.is_production() {
        println!();
        println!(
            "Aviso: este host pertenece al entorno '{}'; use `nas-dev` para desarrollo.",
            host.env
        );
    }

    Ok(ExitCode::SUCCESS)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cli_parsea_subcomandos_y_flag_json() {
        let cli = Cli::try_parse_from(["nasctl", "--json", "hosts"]).expect("hosts");
        assert!(cli.json);
        assert!(matches!(cli.command, Command::Hosts));

        let check = Cli::try_parse_from(["nasctl", "check"]).expect("check");
        assert!(!check.json);
        assert!(matches!(check.command, Command::Check));

        let exec =
            Cli::try_parse_from(["nasctl", "exec", "--host", "prod-nas", "status"]).expect("exec");
        assert!(matches!(exec.command, Command::Exec { .. }));
    }

    #[test]
    fn inventory_path_usa_valor_explicito_si_se_provee() {
        let custom = PathBuf::from("/etc/nas/custom-inv.yaml");
        assert_eq!(inventory_path(Some(custom.clone())), custom);
    }
}
