use std::path::{Path, PathBuf};
use std::process::ExitCode;

use nas_core::shell::quote_all;
use nas_core::{ApiResponse, NasError, NasPaths, NasResult};
use nas_engine::update::ReleaseManifest;
use nas_exec::Runner;

use super::common::{print_json, run_step};

pub struct UpdateDispatchArgs {
    pub manifest: Option<PathBuf>,
    pub artifact: Option<PathBuf>,
    pub repo: String,
    pub root: String,
    pub previous: Option<String>,
    pub pubkey: Option<String>,
    pub yes: bool,
    pub sudo: bool,
}

fn read_deployed_version(candidates: &[&Path], fallback: &str) -> String {
    for path in candidates {
        if path.is_file() {
            if let Some(version) = std::fs::read_to_string(path)
                .ok()
                .and_then(|txt| serde_json::from_str::<serde_json::Value>(&txt).ok())
                .and_then(|v| v.get("version").and_then(|s| s.as_str().map(String::from)))
                .filter(|s| !s.trim().is_empty())
            {
                return version;
            }
        }
    }
    fallback.to_string()
}

fn detect_previous_version(root: &str, target_version: &str) -> Option<String> {
    let base = Path::new(root.trim_end_matches('/'));
    let link = base.join("current");
    let target = std::fs::read_link(link).ok()?;
    let name = target.file_name()?.to_str()?.trim();
    if name.is_empty()
        || name == "."
        || name == ".."
        || name.contains('/')
        || name == target_version
    {
        return None;
    }
    if base.join("releases").join(name).is_dir() {
        Some(name.to_string())
    } else {
        None
    }
}

pub fn update_status(json: bool) -> NasResult<ExitCode> {
    let pkg_version = env!("CARGO_PKG_VERSION");
    let deployed_version = read_deployed_version(
        &[
            Path::new("/opt/nas_debian/current/version.json"),
            Path::new("/etc/nas/version.json"),
            Path::new("/opt/nas_debian/version.json"),
        ],
        pkg_version,
    );

    if json {
        print_json(&ApiResponse::ok(
            serde_json::json!({
                "version": deployed_version,
                "pkg_version": pkg_version,
                "status": "up_to_date",
                "require_signed_tags": true,
            }),
            "Estado del actualizador",
        ))?;
    } else {
        println!("Estado del actualizador NAS:");
        println!("  Versión instalada : {deployed_version}");
        println!("  Requisito de firma: Ed25519 obligatorio");
        println!("  Canal             : producción (main)");
    }

    Ok(ExitCode::SUCCESS)
}

pub fn update_check(repo: &str, json: bool) -> NasResult<ExitCode> {
    let runner = Runner::new();
    let current_version = env!("CARGO_PKG_VERSION");
    let release = nas_engine::fetch_latest_release(&runner, repo)?;

    let clean_tag = release.tag_name.trim_start_matches('v');
    let has_update = clean_tag != current_version;

    if json {
        print_json(&ApiResponse::ok(
            serde_json::json!({
                "installed_version": current_version,
                "latest_release": release.tag_name,
                "release_name": release.name,
                "has_update": has_update,
                "manifest_url": release.manifest_url,
                "artifact_url": release.artifact_url,
            }),
            "Consulta de releases de GitHub",
        ))?;
    } else {
        println!("Consulta de releases oficiales (GitHub: {repo}):");
        println!("  Versión instalada : v{current_version}");
        println!("  Última release    : {}", release.tag_name);
        if let Some(name) = &release.name {
            println!("  Título            : {name}");
        }
        if has_update {
            println!("  Estado            : ¡Nueva versión disponible!");
            println!("  Para actualizar   : sudo nas update apply --repo {repo} --yes");
        } else {
            println!("  Estado            : El sistema se encuentra en la versión más reciente.");
        }
    }

    Ok(ExitCode::SUCCESS)
}

pub fn update_sign(
    manifest_path: &Path,
    key_path: &Path,
    out: Option<&Path>,
    json: bool,
) -> NasResult<ExitCode> {
    let secret = std::fs::read_to_string(key_path)
        .map_err(|error| nas_core::NasError::io(key_path, error))?;
    let mut manifest = ReleaseManifest::load(manifest_path)?;
    nas_engine::update::sign_manifest(&mut manifest, secret.trim())?;
    let target = out.unwrap_or(manifest_path);
    let text = serde_json::to_string_pretty(&manifest)
        .map_err(|error| nas_core::NasError::Invalid(error.to_string()))?;
    std::fs::write(target, format!("{text}\n"))
        .map_err(|error| nas_core::NasError::io(target, error))?;

    if json {
        print_json(&ApiResponse::ok(
            serde_json::json!({ "manifest": target, "version": manifest.version }),
            "Manifiesto firmado",
        ))?;
    } else {
        println!(
            "Manifiesto firmado: {} (v{})",
            target.display(),
            manifest.version
        );
    }
    Ok(ExitCode::SUCCESS)
}

pub fn update_pubkey(key_path: &Path, json: bool) -> NasResult<ExitCode> {
    let secret = std::fs::read_to_string(key_path)
        .map_err(|error| nas_core::NasError::io(key_path, error))?;
    let public = nas_engine::update::public_key_from_secret(secret.trim())?;
    if json {
        print_json(&ApiResponse::ok(
            serde_json::json!({ "pubkey": public }),
            "Clave pública",
        ))?;
    } else {
        println!("{public}");
    }
    Ok(ExitCode::SUCCESS)
}

pub fn update_verify(
    manifest_path: &Path,
    artifact: Option<&Path>,
    public_key: &str,
    json: bool,
) -> NasResult<ExitCode> {
    let manifest = ReleaseManifest::load(manifest_path)?;
    let artifact = match artifact {
        Some(explicit) => explicit.to_path_buf(),
        None => {
            let declared = PathBuf::from(&manifest.artifact);
            if declared.is_absolute() || declared.exists() {
                declared
            } else {
                let manifest_dir = manifest_path
                    .parent()
                    .map(Path::to_path_buf)
                    .unwrap_or_else(|| {
                        std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
                    });
                manifest_dir.join(&declared)
            }
        }
    };

    let artifact_check = manifest.verify_artifact(&artifact);
    let signature_check = manifest.verify_signature(public_key);
    let ok = artifact_check.is_ok() && signature_check.is_ok();

    let mut detail: Vec<String> = Vec::new();
    if let Err(error) = &artifact_check {
        detail.push(error.to_string());
    }
    if let Err(error) = &signature_check {
        detail.push(error.to_string());
    }

    if json {
        print_json(&ApiResponse::ok(
            serde_json::json!({
                "version": manifest.version,
                "artifact": artifact.display().to_string(),
                "sha256_ok": artifact_check.is_ok(),
                "signature_ok": signature_check.is_ok(),
                "detail": detail,
            }),
            "Verificación de release",
        ))?;
    } else {
        println!("VERIFICACIÓN DE RELEASE {}", manifest.version);
        println!("Artefacto : {}", artifact.display());
        println!(
            "SHA-256   : {}",
            if artifact_check.is_ok() {
                "OK"
            } else {
                "FALLO"
            }
        );
        println!(
            "Firma     : {}",
            if signature_check.is_ok() {
                "OK"
            } else {
                "FALLO"
            }
        );
        if let Err(error) = &artifact_check {
            println!("  ↳ {error}");
        }
        if let Err(error) = &signature_check {
            println!("  ↳ {error}");
        }
    }

    if ok {
        Ok(ExitCode::SUCCESS)
    } else {
        Ok(ExitCode::from(3))
    }
}

pub fn update_plan(
    manifest_path: &Path,
    artifact: &Path,
    root: &str,
    previous: Option<&str>,
    json: bool,
) -> NasResult<ExitCode> {
    let manifest = ReleaseManifest::load(manifest_path)?;
    let plan =
        nas_engine::build_update_plan(&manifest, root, &artifact.to_string_lossy(), previous);

    let steps: Vec<String> = plan.steps.iter().map(|step| quote_all(step)).collect();
    let rollback: Vec<String> = plan.rollback.iter().map(|step| quote_all(step)).collect();

    if json {
        print_json(&ApiResponse::ok(
            serde_json::json!({
                "version": plan.version,
                "release_dir": plan.release_dir,
                "steps": steps,
                "rollback": rollback,
            }),
            "Plan de actualización",
        ))?;
        return Ok(ExitCode::SUCCESS);
    }

    println!("PLAN DE ACTUALIZACIÓN A/B: {}", plan.version);
    println!("Release   : {}", plan.release_dir);
    for (index, step) in steps.iter().enumerate() {
        println!("{:>2}. {step}", index + 1);
    }
    if rollback.is_empty() {
        println!();
        println!("Sin versión previa declarada: no hay rollback automático.");
    } else {
        println!();
        println!("ROLLBACK:");
        for (index, step) in rollback.iter().enumerate() {
            println!("{:>2}. {step}", index + 1);
        }
    }

    Ok(ExitCode::SUCCESS)
}

fn resolve_pubkey(specified: Option<&str>) -> NasResult<String> {
    if let Some(key) = specified {
        let trimmed = key.trim();
        let path = Path::new(trimmed);
        if path.is_file() {
            let content = std::fs::read_to_string(path).map_err(|e| NasError::io(path, e))?;
            return Ok(content.trim().to_string());
        }
        return Ok(trimmed.to_string());
    }
    if let Ok(env_key) = std::env::var("NAS_RELEASE_PUBKEY") {
        let trimmed = env_key.trim();
        let path = Path::new(trimmed);
        if path.is_file() {
            let content = std::fs::read_to_string(path).map_err(|e| NasError::io(path, e))?;
            return Ok(content.trim().to_string());
        }
        return Ok(trimmed.to_string());
    }
    if let Ok(candidates_env) = std::env::var("NAS_RELEASE_PUBKEY_CANDIDATES") {
        if candidates_env == "none" || candidates_env.is_empty() {
            return Err(NasError::Denied(
                "clave pública Ed25519 obligatoria para verificar la actualización (--pubkey)"
                    .into(),
            ));
        }
    }
    for candidate in [
        "/etc/nas/release.pub",
        "/opt/nas_debian/packaging/release.pub",
    ] {
        let p = Path::new(candidate);
        if p.is_file() {
            if let Ok(c) = std::fs::read_to_string(p) {
                let trimmed = c.trim();
                if !trimmed.is_empty() {
                    return Ok(trimmed.to_string());
                }
            }
        }
    }
    Err(NasError::Denied(
        "clave pública Ed25519 obligatoria para verificar la actualización (--pubkey)".into(),
    ))
}

pub fn update_apply_dispatch(args: UpdateDispatchArgs, json: bool) -> NasResult<ExitCode> {
    let resolved_key = resolve_pubkey(args.pubkey.as_deref())?;

    let (manifest_path, artifact_path) = match (args.manifest, args.artifact) {
        (Some(m), Some(a)) => (m, a),
        _ => {
            let runner = Runner::new();
            let cache_dir = Path::new("/var/cache/nas/updates");
            if !json {
                println!(
                    "Consultando último release desde GitHub ({}) ...",
                    args.repo
                );
            }
            let release_info = nas_engine::fetch_latest_release(&runner, &args.repo)?;
            if !json {
                println!(
                    "Descargando y verificando release {} ...",
                    release_info.tag_name
                );
            }
            let (_manifest, m_path, a_path) = nas_engine::download_and_verify_release(
                &runner,
                &release_info,
                cache_dir,
                &resolved_key,
            )?;
            (m_path, a_path)
        }
    };

    update_apply(
        UpdateApplyOptions {
            manifest_path: &manifest_path,
            artifact: &artifact_path,
            root: &args.root,
            previous: args.previous.as_deref(),
            public_key: &resolved_key,
            confirmed: args.yes,
            sudo: args.sudo,
        },
        json,
    )
}

struct UpdateApplyOptions<'a> {
    manifest_path: &'a Path,
    artifact: &'a Path,
    root: &'a str,
    previous: Option<&'a str>,
    public_key: &'a str,
    confirmed: bool,
    sudo: bool,
}

fn update_apply(options: UpdateApplyOptions<'_>, json: bool) -> NasResult<ExitCode> {
    let UpdateApplyOptions {
        manifest_path,
        artifact,
        root,
        previous,
        public_key,
        confirmed,
        sudo,
    } = options;

    let manifest = ReleaseManifest::load(manifest_path)?;
    manifest.verify_artifact(artifact)?;
    manifest.verify_signature(public_key)?;

    if !confirmed {
        return Err(NasError::Denied(
            "la actualización modifica el sistema: repita con --yes para confirmar".into(),
        ));
    }

    let detected_previous = if previous.is_none() {
        detect_previous_version(root, &manifest.version)
    } else {
        None
    };
    let effective_previous = previous.or(detected_previous.as_deref());

    let plan = nas_engine::build_update_plan(
        &manifest,
        root,
        &artifact.to_string_lossy(),
        effective_previous,
    );
    let _lock = nas_engine::TaskLock::acquire("/var/lock/nas-update.lock")?;

    let audit = nas_audit::AuditLog::new(NasPaths::from_env().audit_log());
    let actor = std::env::var("USER").unwrap_or_else(|_| "root".to_string());
    let _ = audit.append(nas_audit::AuditEntry::new(
        &actor,
        "local",
        "update-apply",
        &manifest.version,
        "START",
        format!("artifact={}", artifact.display()),
    ));

    let runner = Runner::new();
    let mut failure: Option<NasError> = None;

    for step in &plan.steps {
        let is_health = step.first().is_some_and(|arg| arg == "curl");
        match run_step(&runner, step, sudo, 900) {
            Ok(output) if is_health => {
                let code = output.stdout.trim();
                if code != "200" && code != "302" {
                    failure = Some(NasError::Denied(format!(
                        "healthcheck devolvió {code}; se revierte la release"
                    )));
                    break;
                }
            }
            Ok(_) => {}
            Err(error) => {
                failure = Some(error);
                break;
            }
        }
    }

    if failure.is_some() {
        for step in &plan.rollback {
            let _ = run_step(&runner, step, sudo, 300);
        }
    }

    let (status, detail) = match &failure {
        None => ("SUCCESS", format!("release={}", manifest.version)),
        Some(error) => ("FAILED", error.to_string()),
    };
    let _ = audit.append(nas_audit::AuditEntry::new(
        &actor,
        "local",
        "update-apply",
        &manifest.version,
        status,
        detail,
    ));

    if let Some(error) = failure {
        return Err(error);
    }

    if json {
        print_json(&ApiResponse::ok(
            serde_json::json!({
                "version": manifest.version,
                "release_dir": plan.release_dir,
                "rollback_available": !plan.rollback.is_empty(),
            }),
            "Actualización aplicada",
        ))?;
    } else {
        println!("Actualización aplicada: {}", manifest.version);
        println!("Release: {}", plan.release_dir);
        println!(
            "Rollback: {}",
            if plan.rollback.is_empty() {
                "no disponible"
            } else {
                "disponible"
            }
        );
    }

    Ok(ExitCode::SUCCESS)
}

#[cfg(test)]
mod tests {
    use super::{detect_previous_version, read_deployed_version};
    use std::os::unix::fs::symlink;

    #[test]
    fn read_deployed_version_respeta_precedencia_y_respaldo() {
        let dir = std::env::temp_dir().join(format!(
            "nas_cli_ver_test_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(dir.join("current")).expect("mkdir current");
        let p_current = dir.join("current/version.json");
        let p_etc = dir.join("etc_version.json");
        let p_root = dir.join("root_version.json");
        std::fs::write(&p_etc, r#"{"version":"1.0.0"}"#).expect("write etc");
        std::fs::write(&p_root, r#"{"version":"0.9.0"}"#).expect("write root");

        assert_eq!(
            read_deployed_version(&[&p_current, &p_etc, &p_root], "0.0.0"),
            "1.0.0"
        );

        std::fs::write(&p_current, r#"{"version":"1.2.3"}"#).expect("write current");
        assert_eq!(
            read_deployed_version(&[&p_current, &p_etc, &p_root], "0.0.0"),
            "1.2.3"
        );

        assert_eq!(
            read_deployed_version(&[&dir.join("missing.json")], "9.9.9"),
            "9.9.9"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn detect_previous_version_resuelve_enlace_current() {
        let dir = std::env::temp_dir().join(format!(
            "nas_cli_prev_test_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        let prev = dir.join("releases/1.2.2");
        std::fs::create_dir_all(&prev).expect("mkdir prev");
        symlink(&prev, dir.join("current")).expect("symlink current");

        let root_str = dir.to_string_lossy();
        assert_eq!(
            detect_previous_version(&root_str, "1.2.3"),
            Some("1.2.2".to_string())
        );
        assert_eq!(detect_previous_version(&root_str, "1.2.2"), None);

        let _ = std::fs::remove_dir_all(&dir);
    }
}
