//! `nas-dev`: herramienta de desarrollo.
//!
//! Despliega cambios locales en un entorno de desarrollo **sin publicar** en el
//! repositorio remoto. Nunca toca rutas de producción ni ejecuta el actualizador
//! firmado; esas capacidades pertenecen a `nasctl`.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use nas_core::{ApiResponse, NasError, NasResult};
use nas_exec::{CommandSpec, Runner};
use serde::Serialize;

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
    long_about = "Valida, empaqueta y prepara los cambios locales para desplegarlos en un \
                  entorno de desarrollo aislado. Nunca opera sobre producción."
)]
struct Cli {
    /// Directorio fuente del proyecto.
    #[arg(long, global = true, value_name = "DIR", default_value = ".")]
    source: PathBuf,

    /// Emite la respuesta en JSON.
    #[arg(long, global = true)]
    json: bool,

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
    /// Sincroniza hacia el entorno de desarrollo (sólo `--dry-run` en esta fase).
    Sync {
        /// Simula la sincronización sin modificar el servidor.
        #[arg(long)]
        dry_run: bool,
        /// Host destino del inventario.
        #[arg(long)]
        host: Option<String>,
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
        Command::Sync { dry_run, host } => sync(&source, dry_run, host.as_deref(), cli.json),
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
    println!();
    println!("Nota: upload/switch/rollback se habilitan en la fase F2 del plan de migración.");

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

    let size = std::fs::metadata(out).map(|meta| meta.len()).unwrap_or(0);

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

fn sync(source: &Path, dry_run: bool, host: Option<&str>, json: bool) -> NasResult<ExitCode> {
    if !dry_run {
        return Err(NasError::Denied(
            "la sincronización real se habilita en la fase F2; use --dry-run".into(),
        ));
    }

    let mut code = preflight(source, json)?;
    if code != ExitCode::SUCCESS {
        return Ok(code);
    }
    code = plan(source, json)?;

    if !json {
        println!();
        println!(
            "Dry-run completado para el host {}.",
            host.unwrap_or("(no especificado)")
        );
    }

    Ok(code)
}

#[cfg(test)]
mod tests {
    use super::{collect_by_extension, Check};
    use std::path::PathBuf;

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
}
