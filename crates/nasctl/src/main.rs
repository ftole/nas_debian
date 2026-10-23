//! `nasctl`: herramienta de producción.
//!
//! Esta variante **sólo** opera sobre despliegues declarados en un inventario y
//! está pensada para acompañar releases firmados. Nunca lee el árbol de
//! desarrollo ni ejecuta código local sin validar.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use nas_core::{ApiResponse, NasError, NasResult, ServerRole};
use serde::{Deserialize, Serialize};

#[derive(Debug, Parser)]
#[command(
    name = "nasctl",
    version,
    about = "Administración de producción del servidor NAS (multi-servidor)",
    long_about = "Herramienta de producción: valida el inventario, resuelve perfiles por host \
                  y opera únicamente sobre despliegues declarados. El transporte SSH y la \
                  ejecución remota se incorporan en la fase F3."
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
}

/// Valores por defecto aplicables a todos los hosts.
#[derive(Debug, Clone, Deserialize, Default)]
struct Defaults {
    port: Option<u16>,
    user: Option<String>,
    timeout_secs: Option<u64>,
}

/// Grupo de hosts (dev o prod) con sus rutas.
#[derive(Debug, Clone, Deserialize)]
struct Group {
    env: String,
    #[serde(default)]
    paths: BTreeMap<String, String>,
}

/// Host declarado en el inventario.
#[derive(Debug, Clone, Deserialize)]
struct Host {
    host: String,
    group: String,
    #[serde(default)]
    role: Option<String>,
    #[serde(default)]
    port: Option<u16>,
    #[serde(default)]
    user: Option<String>,
}

/// Inventario completo.
#[derive(Debug, Clone, Deserialize, Default)]
struct Inventory {
    #[serde(default)]
    defaults: Defaults,
    #[serde(default)]
    groups: BTreeMap<String, Group>,
    #[serde(default)]
    hosts: BTreeMap<String, Host>,
}

/// Perfil resuelto de un host concreto.
#[derive(Debug, Clone, Serialize)]
struct ResolvedHost {
    alias: String,
    host: String,
    group: String,
    env: String,
    role: String,
    port: u16,
    user: String,
    repo: String,
    timeout_secs: u64,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(cli) {
        Ok(code) => code,
        Err(error) => {
            let payload = ApiResponse::<serde_json::Value>::error(error.to_string());
            eprintln!(
                "{}",
                serde_json::to_string_pretty(&payload).unwrap_or_else(|_| error.to_string())
            );
            ExitCode::from(u8::try_from(error.exit_code()).unwrap_or(1))
        }
    }
}

fn run(cli: Cli) -> NasResult<ExitCode> {
    let path = inventory_path(cli.inventory);
    let inventory = load_inventory(&path)?;

    match cli.command {
        Command::Hosts => hosts(&inventory, cli.json),
        Command::Check => check(&inventory, &path, cli.json),
        Command::Status { host } => status(&inventory, &host, cli.json),
    }
}

fn inventory_path(explicit: Option<PathBuf>) -> PathBuf {
    explicit
        .or_else(|| std::env::var("NAS_INVENTORY").ok().map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from("inventory.yaml"))
}

fn load_inventory(path: &Path) -> NasResult<Inventory> {
    let raw = std::fs::read_to_string(path)
        .map_err(|_| NasError::NotFound(format!("inventario no encontrado: {}", path.display())))?;
    serde_yaml::from_str(&raw)
        .map_err(|error| NasError::Invalid(format!("inventario inválido: {error}")))
}

fn hosts(inventory: &Inventory, json: bool) -> NasResult<ExitCode> {
    let list: Vec<ResolvedHost> = inventory
        .hosts
        .keys()
        .filter_map(|alias| resolve(inventory, alias).ok())
        .collect();

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&ApiResponse::ok(&list, "Hosts del inventario"))?
        );
        return Ok(ExitCode::SUCCESS);
    }

    println!("HOSTS DECLARADOS ({})", list.len());
    for host in &list {
        println!(
            "{:<14} {:<16} grupo={:<6} env={:<5} rol={:<16} {}@{}:{}",
            host.alias, host.host, host.group, host.env, host.role, host.user, host.host, host.port
        );
    }

    Ok(ExitCode::SUCCESS)
}

fn check(inventory: &Inventory, path: &Path, json: bool) -> NasResult<ExitCode> {
    let problems = validate(inventory);

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&ApiResponse::ok(
                serde_json::json!({ "inventory": path, "ok": problems.is_empty(), "problems": problems }),
                "Validación del inventario"
            ))?
        );
    } else {
        println!("VALIDACIÓN DEL INVENTARIO: {}", path.display());
        if problems.is_empty() {
            println!("[OK] Inventario válido ({} hosts)", inventory.hosts.len());
        } else {
            for problem in &problems {
                println!("[FALLO] {problem}");
            }
        }
    }

    if problems.is_empty() {
        Ok(ExitCode::SUCCESS)
    } else {
        Ok(ExitCode::from(1))
    }
}

fn status(inventory: &Inventory, alias: &str, json: bool) -> NasResult<ExitCode> {
    let host = resolve(inventory, alias)?;

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
        host.user, host.host, host.port
    );
    println!("Grupo / entorno : {} ({})", host.group, host.env);
    println!("Rol             : {}", host.role);
    println!("Repositorio     : {}", host.repo);
    println!();
    println!("Nota: el transporte SSH y la ejecución remota se habilitan en la fase F3.");

    Ok(ExitCode::SUCCESS)
}

fn validate(inventory: &Inventory) -> Vec<String> {
    let mut problems = Vec::new();

    if inventory.hosts.is_empty() {
        problems.push("el inventario no declara ningún host".to_string());
    }

    for (alias, host) in &inventory.hosts {
        if host.host.trim().is_empty() {
            problems.push(format!("{alias}: 'host' vacío"));
        }
        match inventory.groups.get(&host.group) {
            None => problems.push(format!("{alias}: grupo '{}' no definido", host.group)),
            Some(group) => {
                if !matches!(group.env.as_str(), "dev" | "prod") {
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
        let port = host.port.or(inventory.defaults.port).unwrap_or(22);
        if port == 0 {
            problems.push(format!("{alias}: puerto inválido"));
        }
    }

    problems
}

fn resolve(inventory: &Inventory, alias: &str) -> NasResult<ResolvedHost> {
    let host = inventory
        .hosts
        .get(alias)
        .ok_or_else(|| NasError::NotFound(format!("host '{alias}' no está en el inventario")))?;

    let group = inventory
        .groups
        .get(&host.group)
        .ok_or_else(|| NasError::Invalid(format!("grupo '{}' no definido", host.group)))?;

    let role = host
        .role
        .as_deref()
        .and_then(ServerRole::parse)
        .unwrap_or(ServerRole::ArquivosBackup);

    Ok(ResolvedHost {
        alias: alias.to_string(),
        host: host.host.clone(),
        group: host.group.clone(),
        env: group.env.clone(),
        role: role.as_str().to_string(),
        port: host.port.or(inventory.defaults.port).unwrap_or(22),
        user: host
            .user
            .clone()
            .or_else(|| inventory.defaults.user.clone())
            .unwrap_or_else(|| "nasadmin".to_string()),
        repo: group
            .paths
            .get("repo")
            .cloned()
            .unwrap_or_else(|| "/opt/nas_debian".to_string()),
        timeout_secs: inventory.defaults.timeout_secs.unwrap_or(30),
    })
}

#[cfg(test)]
mod tests {
    use super::{validate, Inventory};

    const YAML: &str = r#"
defaults:
  port: 22
  user: nasadmin
groups:
  dev:
    env: dev
    paths: { repo: /opt/nas_debian-dev }
  prod:
    env: prod
    paths: { repo: /opt/nas_debian }
hosts:
  lab-dev:
    host: 10.10.1.222
    group: dev
    role: ARCHIVOS_BACKUP
  srv-nas:
    host: 10.10.1.2
    group: prod
    role: ARCHIVOS
"#;

    #[test]
    fn inventario_valido_no_reporta_problemas() {
        let inventory: Inventory = serde_yaml::from_str(YAML).expect("yaml válido");
        assert!(validate(&inventory).is_empty());
    }

    #[test]
    fn detecta_grupo_inexistente() {
        let yaml = "hosts:\n  h1:\n    host: 1.2.3.4\n    group: inexistente\n";
        let inventory: Inventory = serde_yaml::from_str(yaml).expect("yaml válido");
        let problems = validate(&inventory);
        assert!(problems.iter().any(|p| p.contains("no definido")));
    }

    #[test]
    fn detecta_rol_invalido() {
        let yaml = "groups:\n  g:\n    env: dev\nhosts:\n  h1:\n    host: 1.2.3.4\n    group: g\n    role: OTRO\n";
        let inventory: Inventory = serde_yaml::from_str(yaml).expect("yaml válido");
        assert!(validate(&inventory).iter().any(|p| p.contains("rol")));
    }

    #[test]
    fn detecta_entorno_invalido() {
        let yaml =
            "groups:\n  g:\n    env: staging\nhosts:\n  h1:\n    host: 1.2.3.4\n    group: g\n";
        let inventory: Inventory = serde_yaml::from_str(yaml).expect("yaml válido");
        assert!(validate(&inventory).iter().any(|p| p.contains("env")));
    }
}
