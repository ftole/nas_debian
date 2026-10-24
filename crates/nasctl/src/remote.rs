//! Ejecución remota de la CLI `nas` sobre los hosts del inventario.

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Args, Subcommand};
use nas_core::{ApiResponse, NasError, NasPaths, NasResult};
use nas_inventory::Inventory;
use nas_ssh::{OpenSsh, RemoteExecutor, SshTarget};
use serde::Serialize;

/// Acción remota permitida sobre la CLI `nas`.
#[derive(Debug, Clone, Subcommand)]
pub enum RemoteAction {
    /// Métricas del sistema.
    Status,
    /// Inventario de red.
    Network,
    /// Estado de servicios.
    Services,
    /// Diagnóstico integral.
    Diag,
    /// Lista las tareas de respaldo.
    BackupList,
    /// Plan de una tarea de respaldo.
    BackupPlan {
        /// Identificador de la tarea.
        id: String,
    },
}

impl RemoteAction {
    fn argv(&self) -> Vec<String> {
        match self {
            Self::Status => vec!["status".into()],
            Self::Network => vec!["network".into()],
            Self::Services => vec!["services".into()],
            Self::Diag => vec!["diag".into()],
            Self::BackupList => vec!["backup".into(), "list".into()],
            Self::BackupPlan { id } => vec!["backup".into(), "plan".into(), id.clone()],
        }
    }

    /// Etiqueta legible de la acción.
    pub fn label(&self) -> String {
        match self {
            Self::Status => "status".into(),
            Self::Network => "network".into(),
            Self::Services => "services".into(),
            Self::Diag => "diag".into(),
            Self::BackupList => "backup list".into(),
            Self::BackupPlan { id } => format!("backup plan {id}"),
        }
    }
}

/// Opciones de selección de hosts y transporte.
#[derive(Debug, Default, Args)]
pub struct ExecOptions {
    /// Alias de un host concreto.
    #[arg(long)]
    pub host: Option<String>,
    /// Grupo de hosts del inventario.
    #[arg(long)]
    pub group: Option<String>,
    /// Todos los hosts del inventario.
    #[arg(long)]
    pub all: bool,
    /// Clave privada SSH.
    #[arg(long)]
    pub identity: Option<PathBuf>,
    /// Archivo known_hosts dedicado.
    #[arg(long)]
    pub known_hosts: Option<PathBuf>,
    /// Binario remoto de la CLI.
    #[arg(long, default_value = "nas")]
    pub remote_bin: String,
}

#[derive(Debug, Clone, Serialize)]
struct ExecResult {
    host: String,
    address: String,
    action: String,
    success: bool,
    duration_ms: u128,
    data: Option<serde_json::Value>,
    error: Option<String>,
}

fn default_known_hosts(alias: &str) -> PathBuf {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .map_or_else(|_| PathBuf::from("."), PathBuf::from);
    home.join(".ssh").join("known_hosts.d").join(alias)
}

fn select_hosts(
    inventory: &Inventory,
    host: Option<&str>,
    group: Option<&str>,
    all: bool,
) -> NasResult<Vec<nas_inventory::ResolvedHost>> {
    let selectors = usize::from(host.is_some()) + usize::from(group.is_some()) + usize::from(all);
    if selectors != 1 {
        return Err(NasError::Invalid(
            "indique exactamente un selector: --host, --group o --all".into(),
        ));
    }

    if let Some(alias) = host {
        return Ok(vec![inventory.resolve(alias)?]);
    }

    let hosts = inventory.resolve_all();
    if all {
        return Ok(hosts);
    }

    let group = group.expect("selector validado");
    let selected: Vec<_> = hosts
        .into_iter()
        .filter(|resolved| resolved.group == group)
        .collect();

    if selected.is_empty() {
        return Err(NasError::NotFound(format!(
            "no hay hosts en el grupo '{group}'"
        )));
    }

    Ok(selected)
}

fn parse_envelope(stdout: &str) -> Option<serde_json::Value> {
    serde_json::from_str::<serde_json::Value>(stdout.trim()).ok()
}

/// Ejecuta la acción remota sobre los hosts seleccionados.
pub fn exec(
    inventory_path: Option<PathBuf>,
    action: &RemoteAction,
    options: &ExecOptions,
    json: bool,
) -> NasResult<ExitCode> {
    let path = inventory_path.unwrap_or_else(|| {
        std::env::var("NAS_INVENTORY")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("inventory.yaml"))
    });
    let inventory = Inventory::load(&path)?;
    let hosts = select_hosts(
        &inventory,
        options.host.as_deref(),
        options.group.as_deref(),
        options.all,
    )?;

    let executor = OpenSsh::new();
    let audit = nas_audit::AuditLog::new(NasPaths::from_env().audit_log());
    let actor = std::env::var("USER").unwrap_or_else(|_| "nasctl".to_string());

    let mut results = Vec::new();
    let mut failures = 0usize;

    for host in &hosts {
        let mut target = SshTarget::new(host.address.clone(), host.port, host.user.clone())
            .with_connect_timeout(host.timeout_secs)
            .with_known_hosts(
                options
                    .known_hosts
                    .clone()
                    .unwrap_or_else(|| default_known_hosts(&host.alias)),
            );
        if let Some(identity) = &options.identity {
            target = target.with_identity(identity.clone());
        }

        let mut argv = vec![options.remote_bin.clone(), "--json".to_string()];
        argv.extend(action.argv());

        let started = std::time::Instant::now();
        let outcome = executor.run(&target, &argv);
        let duration = started.elapsed().as_millis();

        let result = match outcome {
            Ok(output) if output.is_success() => match parse_envelope(&output.stdout) {
                Some(envelope) if envelope["success"] == serde_json::Value::Bool(true) => {
                    ExecResult {
                        host: host.alias.clone(),
                        address: host.address.clone(),
                        action: action.label(),
                        success: true,
                        duration_ms: duration,
                        data: envelope.get("data").cloned(),
                        error: None,
                    }
                }
                Some(envelope) => ExecResult {
                    host: host.alias.clone(),
                    address: host.address.clone(),
                    action: action.label(),
                    success: false,
                    duration_ms: duration,
                    data: None,
                    error: envelope["error"].as_str().map(str::to_string),
                },
                None => ExecResult {
                    host: host.alias.clone(),
                    address: host.address.clone(),
                    action: action.label(),
                    success: false,
                    duration_ms: duration,
                    data: None,
                    error: Some("respuesta remota no es JSON válido".to_string()),
                },
            },
            Ok(output) => ExecResult {
                host: host.alias.clone(),
                address: host.address.clone(),
                action: action.label(),
                success: false,
                duration_ms: duration,
                data: None,
                error: Some(if output.stderr.is_empty() {
                    output.stdout
                } else {
                    output.stderr
                }),
            },
            Err(error) => ExecResult {
                host: host.alias.clone(),
                address: host.address.clone(),
                action: action.label(),
                success: false,
                duration_ms: duration,
                data: None,
                error: Some(error.to_string()),
            },
        };

        let _ = audit.append(nas_audit::AuditEntry::new(
            &actor,
            &host.alias,
            format!("exec:{}", action.label()),
            &host.address,
            if result.success { "SUCCESS" } else { "FAILED" },
            result.error.clone().unwrap_or_default(),
        ));

        if !result.success {
            failures += 1;
        }
        results.push(result);
    }

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&ApiResponse::ok(
                &results,
                format!("{} host(s) consultados", results.len())
            ))?
        );
    } else {
        println!(
            "EJECUCIÓN REMOTA: {} ({} host(s))",
            action.label(),
            results.len()
        );
        for result in &results {
            let mark = if result.success { "OK" } else { "FALLO" };
            println!(
                "[{mark:<5}] {:<14} {:<16} {:>6} ms",
                result.host, result.address, result.duration_ms
            );
            if let Some(error) = &result.error {
                println!("        ↳ {error}");
            }
        }
    }

    if failures == 0 {
        Ok(ExitCode::SUCCESS)
    } else {
        Ok(ExitCode::from(1))
    }
}

#[cfg(test)]
mod tests {
    use super::{parse_envelope, select_hosts, RemoteAction};
    use nas_inventory::Inventory;

    const YAML: &str = "
groups:
  dev: { env: dev }
  prod: { env: prod }
hosts:
  a: { host: 10.0.0.1, group: dev }
  b: { host: 10.0.0.2, group: prod }
  c: { host: 10.0.0.3, group: prod }
";

    #[test]
    fn mapea_acciones_a_argv() {
        assert_eq!(RemoteAction::Status.argv(), vec!["status"]);
        assert_eq!(
            RemoteAction::BackupList.argv(),
            vec!["backup".to_string(), "list".to_string()]
        );
        assert_eq!(
            RemoteAction::BackupPlan { id: "demo".into() }.argv(),
            vec!["backup".to_string(), "plan".to_string(), "demo".to_string()]
        );
    }

    #[test]
    fn selecciona_host_grupo_o_todos() {
        let inventory = Inventory::from_yaml(YAML).expect("yaml");

        let one = select_hosts(&inventory, Some("a"), None, false).expect("host");
        assert_eq!(one.len(), 1);
        assert_eq!(one[0].alias, "a");

        let group = select_hosts(&inventory, None, Some("prod"), false).expect("grupo");
        assert_eq!(group.len(), 2);

        let all = select_hosts(&inventory, None, None, true).expect("todos");
        assert_eq!(all.len(), 3);

        assert!(select_hosts(&inventory, None, None, false).is_err());
        assert!(select_hosts(&inventory, Some("a"), Some("prod"), false).is_err());
        assert!(select_hosts(&inventory, None, Some("inexistente"), false).is_err());
    }

    #[test]
    fn interpreta_el_contrato_json() {
        let ok = parse_envelope(r#"{"success":true,"message":"ok","data":{"a":1}}"#).expect("json");
        assert_eq!(ok["success"], true);
        assert_eq!(ok["data"]["a"], 1);
        assert!(parse_envelope("texto plano").is_none());
    }
}
