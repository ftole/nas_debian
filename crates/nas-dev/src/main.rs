//! `nas-dev`: herramienta de desarrollo.
//!
//! Despliega cambios locales en un entorno de desarrollo **sin publicar** en el
//! repositorio remoto. Nunca toca hosts de producción ni ejecuta el actualizador
//! firmado; esas capacidades pertenecen a `nasctl`.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use nas_core::{ApiResponse, NasError, NasResult};
use nas_exec::{CommandSpec, Runner};
use nas_inventory::{Inventory, ResolvedHost};
use nas_ssh::{with_sudo, OpenSsh, RemoteExecutor, SshTarget};
use serde::Serialize;
use sha2::{Digest, Sha256};

/// Directorios que nunca se empaquetan ni se analizan.
const SKIP_DIRS: &[&str] = &[
    ".git",
    "target",
    "scratch",
    "node_modules",
    ".venv",
    "__pycache__",
];

#[derive(Debug, Parser)]
#[command(
    name = "nas-dev",
    version,
    about = "Herramienta de desarrollo del servidor NAS (sin push)",
    long_about = "Valida, empaqueta y sincroniza los cambios locales hacia un entorno de \
                  desarrollo aislado. Rechaza por diseño cualquier host de producción."
)]
struct Cli {
    /// Directorio fuente del proyecto.
    #[arg(long, global = true, value_name = "DIR", default_value = ".")]
    source: PathBuf,

    /// Emite la respuesta en JSON.
    #[arg(long, global = true)]
    json: bool,

    /// Ruta del inventario YAML.
    #[arg(long, global = true, value_name = "PATH")]
    inventory: Option<PathBuf>,

    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Describe el plan de sincronización paso a paso (sin ejecutar nada).
    Plan,
    /// Ejecuta las validaciones previas (git, JS, Bash).
    Preflight,
    /// Empaqueta el árbol de trabajo en un tar.gz reproducible.
    Pack {
        /// Archivo de salida.
        #[arg(long, value_name = "FILE")]
        out: PathBuf,
    },
    /// Sincroniza el árbol local con un host de desarrollo del inventario.
    Sync {
        /// Alias del host en el inventario.
        #[arg(long)]
        host: String,

        /// Simula la sincronización sin modificar el servidor.
        #[arg(long)]
        dry_run: bool,

        /// Confirma la ejecución real.
        #[arg(long)]
        yes: bool,

        /// No elevar privilegios con sudo en el servidor remoto.
        #[arg(long)]
        no_sudo: bool,

        /// Usuario SSH (sobrescribe el inventario).
        #[arg(long)]
        user: Option<String>,

        /// Clave privada SSH.
        #[arg(long)]
        identity: Option<PathBuf>,

        /// Archivo known_hosts dedicado.
        #[arg(long)]
        known_hosts: Option<PathBuf>,
    },
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
struct Check {
    name: String,
    status: String,
    detail: String,
}

impl Check {
    fn new(name: impl Into<String>, status: &str, detail: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            status: status.to_string(),
            detail: detail.into(),
        }
    }

    fn failed(&self) -> bool {
        self.status == "FAIL"
    }
}

/// Paso remoto del plan de sincronización.
#[derive(Debug, Clone, PartialEq, Eq)]
struct RemoteStep {
    label: String,
    argv: Vec<String>,
    ignore_failure: bool,
    healthcheck: bool,
}

impl RemoteStep {
    fn new(label: &str, argv: Vec<String>) -> Self {
        Self {
            label: label.to_string(),
            argv,
            ignore_failure: false,
            healthcheck: false,
        }
    }

    fn optional(mut self) -> Self {
        self.ignore_failure = true;
        self
    }

    fn healthcheck(mut self) -> Self {
        self.healthcheck = true;
        self
    }
}

/// Plan completo de sincronización hacia un entorno de desarrollo.
#[derive(Debug, Clone)]
struct SyncPlan {
    incoming: String,
    stage: String,
    snapshot: String,
    steps: Vec<RemoteStep>,
    rollback: RemoteStep,
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
    let source = cli.source;
    if !source.is_dir() {
        return Err(NasError::NotFound(format!(
            "directorio fuente inexistente: {}",
            source.display()
        )));
    }

    match cli.command {
        Command::Plan => plan(&source, cli.json),
        Command::Preflight => preflight(&source, cli.json),
        Command::Pack { out } => pack(&source, &out, cli.json),
        Command::Sync {
            host,
            dry_run,
            yes,
            no_sudo,
            user,
            identity,
            known_hosts,
        } => sync(
            &source,
            &host,
            cli.inventory,
            SyncOptions {
                dry_run,
                confirmed: yes,
                sudo: !no_sudo,
                user,
                identity,
                known_hosts,
            },
            cli.json,
        ),
    }
}

fn plan(source: &Path, json: bool) -> NasResult<ExitCode> {
    let steps = [
        ("preflight", "validar git, JavaScript y scripts Bash"),
        (
            "pack",
            "empaquetar el árbol de trabajo con manifiesto SHA-256",
        ),
        ("upload", "transferir el paquete al host de desarrollo"),
        ("stage", "extraer en el directorio de staging aislado"),
        (
            "verify",
            "revalidar sintaxis en destino y comparar manifiesto",
        ),
        ("snapshot", "respaldar el docroot de desarrollo actual"),
        ("switch", "promover el staging y recargar servicios"),
        ("health", "verificar /login y /api/metrics"),
        (
            "rollback",
            "revertir automáticamente si el healthcheck falla",
        ),
    ];

    if json {
        let payload: Vec<_> = steps
            .iter()
            .map(|(name, detail)| serde_json::json!({ "step": name, "detail": detail }))
            .collect();
        println!(
            "{}",
            serde_json::to_string_pretty(&ApiResponse::ok(
                serde_json::json!({ "source": source.display().to_string(), "steps": payload }),
                "Plan de sincronización"
            ))?
        );
        return Ok(ExitCode::SUCCESS);
    }

    println!("PLAN DE SINCRONIZACIÓN (desarrollo)");
    println!("Origen: {}", source.display());
    for (index, (name, detail)) in steps.iter().enumerate() {
        println!("{:>2}. {:<10} {detail}", index + 1, name);
    }

    Ok(ExitCode::SUCCESS)
}

fn preflight(source: &Path, json: bool) -> NasResult<ExitCode> {
    let runner = Runner::new();
    let mut checks = Vec::new();

    let git_args: Vec<String> = vec![
        "-C".to_string(),
        source.to_string_lossy().to_string(),
        "status".to_string(),
        "--porcelain".to_string(),
    ];
    match runner.run(&CommandSpec::new("git").args(git_args)) {
        Ok(output) if output.is_success() && output.stdout.is_empty() => {
            checks.push(Check::new("git", "PASS", "árbol de trabajo limpio"));
        }
        Ok(output) if output.is_success() => {
            let count = output.stdout.lines().count();
            checks.push(Check::new(
                "git",
                "WARN",
                format!("{count} cambio(s) sin confirmar (se empaquetarán igualmente)"),
            ));
        }
        Ok(output) => checks.push(Check::new("git", "WARN", output.stderr)),
        Err(_) => checks.push(Check::new("git", "SKIP", "git no disponible")),
    }

    checks.extend(lint_files(&runner, source, "js", "node", ["--check"]));
    checks.extend(lint_files(&runner, source, "sh", "bash", ["-n"]));

    let failed = checks.iter().filter(|check| check.failed()).count();

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&ApiResponse::ok(
                &checks,
                format!("Preflight: {failed} fallo(s)")
            ))?
        );
    } else {
        println!("PREFLIGHT DE DESARROLLO: {}", source.display());
        for check in &checks {
            println!("[{}] {:<28} {}", check.status, check.name, check.detail);
        }
        println!();
        println!("Fallos: {failed}");
    }

    if failed > 0 {
        Ok(ExitCode::from(1))
    } else {
        Ok(ExitCode::SUCCESS)
    }
}

fn lint_files<const N: usize>(
    runner: &Runner,
    source: &Path,
    extension: &str,
    tool: &str,
    tool_args: [&str; N],
) -> Vec<Check> {
    let mut files = Vec::new();
    collect_by_extension(source, extension, &mut files);

    if files.is_empty() {
        return vec![Check::new(
            format!("{extension}-syntax"),
            "SKIP",
            format!("sin archivos .{extension}"),
        )];
    }

    let mut checks = Vec::new();
    let mut tool_missing = false;

    for file in files {
        if tool_missing {
            break;
        }
        let relative = file
            .strip_prefix(source)
            .unwrap_or(&file)
            .display()
            .to_string();

        let mut spec = CommandSpec::new(tool);
        for arg in tool_args {
            spec = spec.arg(arg);
        }
        spec = spec.arg(file.to_string_lossy().to_string());

        match runner.run(&spec) {
            Ok(output) if output.is_success() => {}
            Ok(output) => checks.push(Check::new(
                format!("{extension}:{relative}"),
                "FAIL",
                if output.stderr.is_empty() {
                    output.stdout
                } else {
                    output.stderr
                },
            )),
            Err(NasError::Io { source: cause, .. })
                if cause.kind() == std::io::ErrorKind::NotFound =>
            {
                tool_missing = true;
                checks.push(Check::new(
                    format!("{extension}-syntax"),
                    "SKIP",
                    format!("{tool} no disponible en este equipo"),
                ));
            }
            Err(error) => checks.push(Check::new(
                format!("{extension}:{relative}"),
                "FAIL",
                error.to_string(),
            )),
        }
    }

    if checks.is_empty() {
        checks.push(Check::new(
            format!("{extension}-syntax"),
            "PASS",
            format!("{tool} validó la sintaxis del conjunto"),
        ));
    }

    checks
}

fn collect_by_extension(root: &Path, extension: &str, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(file_type) = entry.file_type() else {
            continue;
        };

        if file_type.is_dir() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if SKIP_DIRS.contains(&name.as_ref()) {
                continue;
            }
            collect_by_extension(&path, extension, out);
        } else if path.extension().is_some_and(|ext| ext == extension) {
            out.push(path);
        }
    }
}

fn pack(source: &Path, out: &Path, json: bool) -> NasResult<ExitCode> {
    let size = create_archive(source, out)?;

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&ApiResponse::ok(
                serde_json::json!({ "archive": out.display().to_string(), "bytes": size }),
                "Paquete generado"
            ))?
        );
    } else {
        println!("Paquete generado: {} ({size} bytes)", out.display());
    }

    Ok(ExitCode::SUCCESS)
}

fn create_archive(source: &Path, out: &Path) -> NasResult<u64> {
    let canonical = std::fs::canonicalize(source).map_err(|e| NasError::io(source, e))?;
    let parent = canonical
        .parent()
        .ok_or_else(|| NasError::Invalid("el origen no tiene directorio padre".into()))?;
    let leaf = canonical
        .file_name()
        .ok_or_else(|| NasError::Invalid("el origen no tiene nombre de directorio".into()))?;

    if let Some(dir) = out.parent() {
        if !dir.as_os_str().is_empty() {
            std::fs::create_dir_all(dir).map_err(|e| NasError::io(dir, e))?;
        }
    }

    let spec = CommandSpec::new("tar")
        .arg("-czf")
        .arg(out.to_string_lossy().to_string())
        .arg("--exclude=.git")
        .arg("--exclude=target")
        .arg("--exclude=scratch")
        .arg("--exclude=node_modules")
        .arg("--exclude=.venv")
        .arg("--exclude=__pycache__")
        .arg("-C")
        .arg(parent.to_string_lossy().to_string())
        .arg(leaf.to_string_lossy().to_string())
        .timeout(std::time::Duration::from_secs(300));

    let output = Runner::new().run(&spec)?;
    if !output.is_success() {
        return Err(NasError::Command {
            command: spec.describe(),
            code: output.code,
            stderr: output.stderr,
        });
    }

    Ok(std::fs::metadata(out).map(|meta| meta.len()).unwrap_or(0))
}

fn sha256_file(path: &Path) -> NasResult<String> {
    let bytes = std::fs::read(path).map_err(|e| NasError::io(path, e))?;
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    Ok(hex::encode(hasher.finalize()))
}

/// Opciones de la sincronización.
#[derive(Debug, Default)]
struct SyncOptions {
    dry_run: bool,
    confirmed: bool,
    sudo: bool,
    user: Option<String>,
    identity: Option<PathBuf>,
    known_hosts: Option<PathBuf>,
}

fn sync(
    source: &Path,
    alias: &str,
    inventory_path: Option<PathBuf>,
    options: SyncOptions,
    json: bool,
) -> NasResult<ExitCode> {
    let inventory = load_inventory(inventory_path)?;
    let host = inventory.resolve(alias)?;

    if !host.is_development() {
        return Err(NasError::Denied(format!(
            "el host '{}' pertenece al entorno '{}': nas-dev sólo sincroniza entornos de desarrollo",
            host.alias, host.env
        )));
    }

    let mut target = SshTarget::new(
        host.address.clone(),
        host.port,
        options.user.clone().unwrap_or_else(|| host.user.clone()),
    )
    .with_connect_timeout(host.timeout_secs);
    if let Some(identity) = &options.identity {
        target = target.with_identity(identity.clone());
    }
    if let Some(known_hosts) = &options.known_hosts {
        target = target.with_known_hosts(known_hosts.clone());
    } else {
        target = target.with_known_hosts(default_known_hosts(&host.alias));
    }

    if options.dry_run {
        return render_dry_run(&host, &target, options.sudo, json);
    }

    if !options.confirmed {
        return Err(NasError::Denied(
            "la sincronización modifica el servidor: repita con --yes para confirmar".into(),
        ));
    }

    ensure_known_hosts_dir(&target)?;

    let preflight_code = preflight(source, json)?;
    if preflight_code != ExitCode::SUCCESS {
        return Ok(preflight_code);
    }

    let workdir = std::env::temp_dir().join(format!("nas-dev-{}", std::process::id()));
    std::fs::create_dir_all(&workdir).map_err(|e| NasError::io(&workdir, e))?;

    let bundle = workdir.join("bundle.tar.gz");
    create_archive(source, &bundle)?;
    let sha = sha256_file(&bundle)?;

    let archive = workdir.join(format!("{sha}.tar.gz"));
    std::fs::rename(&bundle, &archive).map_err(|e| NasError::io(&archive, e))?;

    let timestamp = chrono::Local::now().format("%Y%m%d_%H%M%S").to_string();
    let plan = build_plan(&host, &sha, &timestamp);

    let executor = OpenSsh::new();
    let audit = nas_audit::AuditLog::new(local_audit_path());

    let _ = audit.append(nas_audit::AuditEntry::new(
        current_actor(),
        &host.alias,
        "dev-sync",
        &host.web,
        "START",
        format!("sha={sha} archive={}", archive.display()),
    ));

    let result = execute(&plan, &executor, &target, &archive, options.sudo);

    let (status, detail) = match &result {
        Ok(()) => ("SUCCESS", format!("sha={sha}")),
        Err(error) => ("FAILED", error.to_string()),
    };
    let _ = audit.append(nas_audit::AuditEntry::new(
        current_actor(),
        &host.alias,
        "dev-sync",
        &host.web,
        status,
        &detail,
    ));

    result?;

    let _ = std::fs::remove_file(&archive);

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&ApiResponse::ok(
                serde_json::json!({
                    "host": host.alias,
                    "sha256": sha,
                    "stage": plan.stage,
                    "snapshot": plan.snapshot,
                }),
                "Sincronización completada"
            ))?
        );
    } else {
        println!();
        println!("Sincronización completada en {}.", host.alias);
        println!("sha256 : {sha}");
        println!("stage  : {}", plan.stage);
        println!("backup : {}", plan.snapshot);
    }

    Ok(ExitCode::SUCCESS)
}

fn execute(
    plan: &SyncPlan,
    executor: &dyn RemoteExecutor,
    target: &SshTarget,
    archive: &Path,
    sudo: bool,
) -> NasResult<()> {
    executor.upload(target, archive, &plan.incoming)?;

    for step in &plan.steps {
        let argv = if sudo {
            with_sudo(&step.argv)
        } else {
            step.argv.clone()
        };

        let output = executor.run(target, &argv)?;
        let success = output.is_success();

        if step.healthcheck {
            if success && health_ok(&output.stdout) {
                continue;
            }
            let _ = executor.run(
                target,
                &if sudo {
                    with_sudo(&plan.rollback.argv)
                } else {
                    plan.rollback.argv.clone()
                },
            );
            return Err(NasError::Denied(format!(
                "healthcheck falló (salida: '{}'); se restauró el snapshot {}",
                output.stdout, plan.snapshot
            )));
        }

        if !success && !step.ignore_failure {
            return Err(NasError::Command {
                command: format!("{}: {}", step.label, output.command),
                code: output.code,
                stderr: output.stderr,
            });
        }
    }

    Ok(())
}

fn render_dry_run(
    host: &ResolvedHost,
    target: &SshTarget,
    sudo: bool,
    json: bool,
) -> NasResult<ExitCode> {
    let plan = build_plan(host, "<sha256>", "<timestamp>");
    let prefix = if sudo { "sudo -n -- " } else { "" };

    if json {
        let steps: Vec<_> = plan
            .steps
            .iter()
            .map(|step| {
                serde_json::json!({
                    "label": step.label,
                    "command": format!("{prefix}{}", step.argv.join(" ")),
                    "ignore_failure": step.ignore_failure,
                    "healthcheck": step.healthcheck,
                })
            })
            .collect();
        println!(
            "{}",
            serde_json::to_string_pretty(&ApiResponse::ok(
                serde_json::json!({
                    "host": host.alias,
                    "target": target.destination(),
                    "upload": plan.incoming,
                    "steps": steps,
                    "rollback": format!("{prefix}{}", plan.rollback.argv.join(" ")),
                }),
                "Simulación de sincronización"
            ))?
        );
        return Ok(ExitCode::SUCCESS);
    }

    println!("SIMULACIÓN DE SINCRONIZACIÓN (dry-run)");
    println!("Host    : {} ({})", host.alias, host.env);
    println!("Destino : {}", target.destination());
    println!("Docroot : {}", host.web);
    println!();
    println!("[upload] paquete -> {}", plan.incoming);
    for (index, step) in plan.steps.iter().enumerate() {
        println!(
            "{:>2}. {:<26} {prefix}{}",
            index + 1,
            step.label,
            step.argv.join(" ")
        );
    }
    println!(
        "RB. {:<26} {prefix}{}",
        plan.rollback.label,
        plan.rollback.argv.join(" ")
    );

    Ok(ExitCode::SUCCESS)
}

fn build_plan(host: &ResolvedHost, sha: &str, timestamp: &str) -> SyncPlan {
    let base = format!("{}/dev-sync", host.state);
    let incoming = format!("{base}/incoming/{sha}.tar.gz");
    let stage = format!("{base}/stage/{sha}");
    let snapshot = format!("{base}/backups/{timestamp}.tar.gz");

    let (web_parent, web_name) = split_path(&host.web);

    let steps = vec![
        RemoteStep::new(
            "crear directorios",
            vec![
                "mkdir".into(),
                "-p".into(),
                format!("{base}/incoming"),
                format!("{base}/stage"),
                format!("{base}/backups"),
            ],
        ),
        RemoteStep::new(
            "limpiar staging",
            vec!["rm".into(), "-rf".into(), stage.clone()],
        ),
        RemoteStep::new(
            "preparar staging",
            vec!["mkdir".into(), "-p".into(), stage.clone()],
        ),
        RemoteStep::new(
            "extraer paquete",
            vec![
                "tar".into(),
                "-xzf".into(),
                incoming.clone(),
                "-C".into(),
                stage.clone(),
            ],
        ),
        RemoteStep::new(
            "verificar árbol",
            vec!["test".into(), "-d".into(), format!("{stage}/web")],
        ),
        RemoteStep::new(
            "snapshot del docroot",
            vec![
                "tar".into(),
                "-czf".into(),
                snapshot.clone(),
                "-C".into(),
                web_parent.clone(),
                web_name.clone(),
            ],
        )
        .optional(),
        RemoteStep::new(
            "promover cambios",
            vec![
                "rsync".into(),
                "-a".into(),
                "--delete".into(),
                "--exclude=data/".into(),
                format!("{stage}/web/"),
                format!("{}/", host.web),
            ],
        ),
        RemoteStep::new(
            "recargar nginx",
            vec!["systemctl".into(), "reload".into(), "nginx".into()],
        )
        .optional(),
        RemoteStep::new(
            "recargar php-fpm",
            vec!["systemctl".into(), "reload".into(), "php8.4-fpm".into()],
        )
        .optional(),
        RemoteStep::new(
            "healthcheck",
            vec![
                "curl".into(),
                "-k".into(),
                "-s".into(),
                "-o".into(),
                "/dev/null".into(),
                "-w".into(),
                "%{http_code}".into(),
                "https://127.0.0.1/login".into(),
            ],
        )
        .healthcheck(),
    ];

    let rollback = RemoteStep::new(
        "restaurar snapshot",
        vec![
            "tar".into(),
            "-xzf".into(),
            snapshot.clone(),
            "-C".into(),
            web_parent,
        ],
    );

    SyncPlan {
        incoming,
        stage,
        snapshot,
        steps,
        rollback,
    }
}

fn split_path(path: &str) -> (String, String) {
    match path.rsplit_once('/') {
        Some(("", name)) => ("/".to_string(), name.to_string()),
        Some((parent, name)) if !name.is_empty() => (parent.to_string(), name.to_string()),
        _ => ("/".to_string(), path.to_string()),
    }
}

fn health_ok(body: &str) -> bool {
    let code = body.trim();
    code == "200" || code == "302"
}

fn load_inventory(explicit: Option<PathBuf>) -> NasResult<Inventory> {
    let path = explicit
        .or_else(|| std::env::var("NAS_INVENTORY").ok().map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from("inventory.yaml"));
    Inventory::load(path)
}

fn ensure_known_hosts_dir(target: &SshTarget) -> NasResult<()> {
    if let Some(path) = &target.known_hosts {
        if let Some(dir) = path.parent() {
            if !dir.as_os_str().is_empty() {
                std::fs::create_dir_all(dir).map_err(|e| NasError::io(dir, e))?;
            }
        }
    }
    Ok(())
}

fn default_known_hosts(alias: &str) -> PathBuf {
    home_dir().join(".ssh").join("known_hosts.d").join(alias)
}

fn home_dir() -> PathBuf {
    std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .map_or_else(|_| PathBuf::from("."), PathBuf::from)
}

fn local_audit_path() -> PathBuf {
    std::env::var("NAS_DEV_AUDIT").map_or_else(
        |_| {
            if cfg!(windows) {
                std::env::var("LOCALAPPDATA").map_or_else(
                    |_| PathBuf::from(".nas-dev/audit.jsonl"),
                    |base| PathBuf::from(base).join("nas-dev").join("audit.jsonl"),
                )
            } else {
                home_dir().join(".local/state/nas-dev/audit.jsonl")
            }
        },
        PathBuf::from,
    )
}

fn current_actor() -> String {
    std::env::var("USER")
        .or_else(|_| std::env::var("USERNAME"))
        .unwrap_or_else(|_| "nas-dev".to_string())
}

#[cfg(test)]
mod tests {
    use super::{build_plan, collect_by_extension, health_ok, split_path, Check};
    use nas_inventory::ResolvedHost;
    use std::path::PathBuf;

    fn host(web: &str) -> ResolvedHost {
        ResolvedHost {
            alias: "lab-dev".to_string(),
            address: "10.10.1.222".to_string(),
            group: "dev".to_string(),
            env: "dev".to_string(),
            role: "ARCHIVOS_BACKUP".to_string(),
            port: 22,
            user: "nasdev".to_string(),
            repo: "/opt/nas_debian-dev".to_string(),
            web: web.to_string(),
            state: "/var/lib/nas-dev".to_string(),
            timeout_secs: 30,
        }
    }

    #[test]
    fn check_falla_solo_con_fail() {
        assert!(Check::new("a", "FAIL", "x").failed());
        assert!(!Check::new("a", "WARN", "x").failed());
        assert!(!Check::new("a", "PASS", "x").failed());
        assert!(!Check::new("a", "SKIP", "x").failed());
    }

    #[test]
    fn recorre_extensiones_ignorando_target() {
        let root = std::env::temp_dir().join(format!("nas-dev-{}", std::process::id()));
        let nested = root.join("sub");
        std::fs::create_dir_all(&nested).expect("mkdir");
        std::fs::create_dir_all(root.join("target")).expect("mkdir target");
        std::fs::write(nested.join("a.js"), "const a = 1;").expect("write");
        std::fs::write(root.join("target").join("b.js"), "const b = 2;").expect("write");

        let mut files = Vec::new();
        collect_by_extension(&root, "js", &mut files);

        assert_eq!(files.len(), 1);
        assert!(files[0].ends_with(PathBuf::from("sub/a.js")));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn plan_usa_rutas_del_host_y_snapshot_con_marca() {
        let plan = build_plan(&host("/var/www/nas-web-dev"), "abc123", "20261023_160000");
        assert_eq!(
            plan.incoming,
            "/var/lib/nas-dev/dev-sync/incoming/abc123.tar.gz"
        );
        assert_eq!(plan.stage, "/var/lib/nas-dev/dev-sync/stage/abc123");
        assert_eq!(
            plan.snapshot,
            "/var/lib/nas-dev/dev-sync/backups/20261023_160000.tar.gz"
        );
        assert!(plan
            .steps
            .iter()
            .any(|step| step.argv.iter().any(|arg| arg == "--delete")));
        assert!(plan.steps.iter().any(|step| step.healthcheck));
        assert!(plan.rollback.argv.contains(&"tar".to_string()));
    }

    #[test]
    fn plan_snapshotea_el_docroot_correcto() {
        let plan = build_plan(&host("/srv/dev-web"), "sha", "ts");
        let snapshot = plan
            .steps
            .iter()
            .find(|step| step.label == "snapshot del docroot")
            .expect("paso de snapshot");
        assert!(snapshot.argv.contains(&"/srv".to_string()));
        assert!(snapshot.argv.contains(&"dev-web".to_string()));
        assert!(snapshot.ignore_failure);
    }

    #[test]
    fn separa_rutas_del_docroot() {
        assert_eq!(
            split_path("/var/www/nas-web-dev"),
            ("/var/www".to_string(), "nas-web-dev".to_string())
        );
        assert_eq!(split_path("/web"), ("/".to_string(), "web".to_string()));
    }

    #[test]
    fn interpreta_codigos_de_healthcheck() {
        assert!(health_ok("200"));
        assert!(health_ok("302\n"));
        assert!(!health_ok("500"));
        assert!(!health_ok(""));
    }
}
