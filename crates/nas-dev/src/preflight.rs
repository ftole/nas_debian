//! Validaciones preflight y empaquetado local para `nas-dev`.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use nas_core::{ApiResponse, NasError, NasResult};
use nas_exec::{CommandSpec, Runner};
use serde::Serialize;
use sha2::{Digest, Sha256};

/// Directorios que nunca se empaquetan ni se analizan.
pub(crate) const SKIP_DIRS: &[&str] = &[
    ".git",
    "target",
    "scratch",
    "node_modules",
    ".venv",
    "__pycache__",
];

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub(crate) struct Check {
    pub(crate) name: String,
    pub(crate) status: String,
    pub(crate) detail: String,
}

impl Check {
    pub(crate) fn new(name: impl Into<String>, status: &str, detail: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            status: status.to_string(),
            detail: detail.into(),
        }
    }

    pub(crate) fn failed(&self) -> bool {
        self.status == "FAIL"
    }
}

pub(crate) fn plan(source: &Path, json: bool) -> NasResult<ExitCode> {
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

pub(crate) fn run_preflight_checks(source: &Path) -> (Vec<Check>, usize) {
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
    (checks, failed)
}

pub(crate) fn preflight(source: &Path, json: bool) -> NasResult<ExitCode> {
    let (checks, failed) = run_preflight_checks(source);

    if json {
        let msg = format!("Preflight: {failed} fallo(s)");
        let payload = if failed > 0 {
            ApiResponse::err(&checks, msg)
        } else {
            ApiResponse::ok(&checks, msg)
        };
        println!("{}", serde_json::to_string_pretty(&payload)?);
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

pub(crate) fn collect_by_extension(root: &Path, extension: &str, out: &mut Vec<PathBuf>) {
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

pub(crate) fn pack(source: &Path, out: &Path, json: bool) -> NasResult<ExitCode> {
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

pub(crate) fn create_archive(source: &Path, out: &Path) -> NasResult<u64> {
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

pub(crate) fn sha256_file(path: &Path) -> NasResult<String> {
    use std::io::Read;
    let mut file = std::fs::File::open(path).map_err(|e| NasError::io(path, e))?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = file.read(&mut buf).map_err(|e| NasError::io(path, e))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hex::encode(hasher.finalize()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha256_file_calcula_digest_en_streaming_multibloque() {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let path = std::env::temp_dir().join(format!("nas-dev-sha-{}-{nanos}", std::process::id()));
        // > 64 KiB para ejercitar múltiples lecturas del buffer
        let data = vec![b'A'; 150 * 1024];
        std::fs::write(&path, &data).unwrap();
        let digest = sha256_file(&path).expect("sha256");
        let _ = std::fs::remove_file(&path);
        assert_eq!(digest.len(), 64);

        let mut expected = Sha256::new();
        expected.update(&data);
        assert_eq!(digest, hex::encode(expected.finalize()));
    }

    #[test]
    fn run_preflight_checks_sobre_repositorio_actual_no_falla() {
        let (checks, failed) = run_preflight_checks(Path::new("."));
        assert!(!checks.is_empty());
        assert_eq!(failed, 0);
    }
}
