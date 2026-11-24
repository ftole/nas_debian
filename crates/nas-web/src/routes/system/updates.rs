use std::path::{Path, PathBuf};

use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use nas_core::ApiResponse;
use nas_engine::update::{is_valid_version, list_installed_releases};
use nas_exec::Runner;
use serde::Deserialize;

use crate::extractors::{AdminUser, AuthenticatedUser};
use crate::routes::common::{audit_ip, bad_request, error_response, server_error};
use crate::AppState;

/// Extrae números de una versión tipo `v1.0.2` → `[1, 0, 2]`.
pub fn parse_semver(value: &str) -> Vec<u64> {
    let clean = value.trim().trim_start_matches('v');
    clean
        .split(&['.', '-', '+'][..])
        .map_while(|part| part.parse::<u64>().ok())
        .collect()
}

/// Compara dos versiones; devuelve `true` si `latest` es mayor que `current`.
pub fn is_newer_version(latest: &str, current: &str) -> bool {
    let a = parse_semver(latest);
    let b = parse_semver(current);
    let max_len = a.len().max(b.len());
    for i in 0..max_len {
        let av = *a.get(i).unwrap_or(&0);
        let bv = *b.get(i).unwrap_or(&0);
        if av > bv {
            return true;
        }
        if av < bv {
            return false;
        }
    }
    false
}

/// Consulta el último release publicado en GitHub usando el motor Rust.
///
/// Falla silenciosamente si no hay red (devuelve `None`), de modo que la
/// vista muestre solo el estado local mientras el acceso remoto no esté AP.
pub fn query_github_latest() -> Option<nas_engine::RemoteReleaseInfo> {
    let runner = Runner::new();
    nas_engine::fetch_latest_release(&runner, "ftole/nas_debian").ok()
}

fn is_systemd_unit_active(unit: &str) -> bool {
    let runner = Runner::new();
    match runner.run(
        &nas_exec::CommandSpec::new("systemctl")
            .args(["is-active", unit])
            .timeout(std::time::Duration::from_secs(3)),
    ) {
        Ok(out) => matches!(out.stdout.trim(), "active" | "activating"),
        _ => false,
    }
}

/// Especificación del comando privilegiado para lanzar la actualización en segundo plano.
pub fn system_update_command_spec() -> nas_exec::CommandSpec {
    nas_exec::CommandSpec::new("/usr/bin/systemd-run")
        .args([
            "--no-block",
            "--collect",
            "--unit=nas-system-update",
            "/usr/local/bin/nas",
            "--json",
            "update",
            "apply",
            "--repo",
            "ftole/nas_debian",
            "--root",
            "/opt/nas_debian",
            "--yes",
        ])
        .sudo(nas_exec::SudoMode::Passwordless)
        .timeout(std::time::Duration::from_secs(30))
}

/// Especificación del comando privilegiado para lanzar la regresión A/B en segundo plano.
pub fn system_rollback_command_spec(version: &str) -> nas_exec::CommandSpec {
    nas_exec::CommandSpec::new("/usr/bin/systemd-run")
        .args([
            "--no-block",
            "--collect",
            "--unit=nas-system-rollback",
            "/usr/local/bin/nas",
            "--json",
            "update",
            "rollback",
            "--root",
            "/opt/nas_debian",
            "--yes",
            "--version",
            version,
        ])
        .sudo(nas_exec::SudoMode::Passwordless)
        .timeout(std::time::Duration::from_secs(30))
}

/// Parámetros opcionales para `/api/system/updates`.
#[derive(Debug, Default, Deserialize)]
pub struct UpdatesQuery {
    /// Si es `1` o `true`, omite la consulta remota a GitHub (usado en sondeo).
    #[serde(default)]
    pub local: Option<String>,
}

/// Cuerpo de solicitud para revertir a una release local instalada.
#[derive(Debug, Deserialize)]
pub struct RollbackBody {
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub confirm: String,
}

fn latest_audit_entry(audit_path: &Path, action: &str) -> Option<serde_json::Value> {
    let text = std::fs::read_to_string(audit_path).ok()?;
    for line in text.lines().rev() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Ok(entry) = serde_json::from_str::<nas_audit::AuditEntry>(trimmed) {
            if entry.action == action {
                return Some(serde_json::json!({
                    "ts": entry.ts,
                    "result": entry.result,
                    "target": entry.target,
                    "detail": entry.detail,
                }));
            }
        }
    }
    None
}

fn resolve_releases_root(repo_dir: &Path) -> PathBuf {
    if repo_dir.join("releases").is_dir() {
        return repo_dir.to_path_buf();
    }
    let default_root = Path::new("/opt/nas_debian");
    if default_root.join("releases").is_dir() {
        return default_root.to_path_buf();
    }
    repo_dir.to_path_buf()
}

/// Información de versión, actualizaciones y releases instaladas para regresión A/B.
pub async fn system_updates(
    State(state): State<AppState>,
    _user: AuthenticatedUser,
    Query(query): Query<UpdatesQuery>,
) -> Response {
    let local_only = matches!(query.local.as_deref(), Some("1" | "true" | "yes"));
    let repo_dir = state.paths.repo.clone();
    let audit_path = state.paths.audit_log();

    let collected = state
        .run_system_blocking(move || {
            let candidates = [
                repo_dir.join("current/version.json"),
                repo_dir.join("version.json"),
                PathBuf::from("/opt/nas_debian/current/version.json"),
                PathBuf::from("/etc/nas/version.json"),
                PathBuf::from("/opt/nas_debian/version.json"),
                PathBuf::from("/opt/nas/version.json"),
            ];
            let version = candidates
                .iter()
                .find_map(|path| std::fs::read_to_string(path).ok())
                .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
                .unwrap_or_else(|| {
                    serde_json::json!({
                        "version": env!("CARGO_PKG_VERSION"),
                        "commit": "local",
                    })
                });

            let ed25519_verified = version
                .get("ed25519_verified")
                .and_then(|v| v.as_bool())
                .or_else(|| version.get("signature_verified").and_then(|v| v.as_bool()))
                .or_else(|| version.get("gpg_verified").and_then(|v| v.as_bool()))
                .unwrap_or(false);

            let version_str = version
                .get("version")
                .and_then(|v| v.as_str())
                .unwrap_or(env!("CARGO_PKG_VERSION"))
                .trim_start_matches('v')
                .to_string();
            let commit_str = version
                .get("commit")
                .and_then(|v| v.as_str())
                .unwrap_or("release")
                .to_string();
            let channel_str = version
                .get("channel")
                .and_then(|v| v.as_str())
                .unwrap_or("stable")
                .to_string();
            let signature_scheme = version
                .get("signature_scheme")
                .and_then(|v| v.as_str())
                .unwrap_or("Ed25519")
                .to_string();
            let commit_date = version
                .get("date")
                .and_then(|v| v.as_str())
                .map(String::from);
            let notes_str = version
                .get("notes")
                .and_then(|v| v.as_str())
                .unwrap_or("Release oficial verificado con firma Ed25519")
                .to_string();

            let (latest_version, has_updates) = if local_only {
                (version_str.clone(), false)
            } else {
                match query_github_latest() {
                    Some(release) => {
                        let latest = release.tag_name.trim_start_matches('v').to_string();
                        let newer = is_newer_version(&latest, &version_str);
                        (latest, newer)
                    }
                    None => (version_str.clone(), false),
                }
            };

            let releases_root = resolve_releases_root(&repo_dir);
            let installed_releases = list_installed_releases(&releases_root.to_string_lossy());
            let previous_version = installed_releases
                .iter()
                .find(|r| !r.current && r.has_binaries)
                .map(|r| r.version.clone());
            let rollback_available = previous_version.is_some();

            let update_in_progress = is_systemd_unit_active("nas-system-update");
            let rollback_in_progress = is_systemd_unit_active("nas-system-rollback");
            let last_update = latest_audit_entry(&audit_path, "update-apply");
            let last_rollback = latest_audit_entry(&audit_path, "update-rollback");

            serde_json::json!({
                "version": version_str,
                "latest_version": latest_version,
                "installed_commit": commit_str,
                "commit_date": commit_date,
                "has_updates": has_updates,
                "update_in_progress": update_in_progress,
                "rollback_in_progress": rollback_in_progress,
                "last_update": last_update,
                "last_rollback": last_rollback,
                "installed_releases": installed_releases,
                "rollback_available": rollback_available,
                "previous_version": previous_version,
                "channel": channel_str,
                "signature_scheme": signature_scheme,
                "ed25519_verified": ed25519_verified,
                "release_notes": notes_str,
                "commit": commit_str,
            })
        })
        .await
        .unwrap_or_else(|_| {
            serde_json::json!({
                "version": env!("CARGO_PKG_VERSION"),
                "latest_version": env!("CARGO_PKG_VERSION"),
                "installed_commit": "local",
                "commit_date": serde_json::Value::Null,
                "has_updates": false,
                "update_in_progress": false,
                "rollback_in_progress": false,
                "last_update": serde_json::Value::Null,
                "last_rollback": serde_json::Value::Null,
                "installed_releases": [],
                "rollback_available": false,
                "previous_version": serde_json::Value::Null,
                "channel": "stable",
                "signature_scheme": "Ed25519",
                "ed25519_verified": false,
                "release_notes": "Release oficial verificado con firma Ed25519",
                "commit": "local",
            })
        });

    axum::Json(ApiResponse::ok(collected, "Información de actualizaciones")).into_response()
}

/// Ejecuta la actualización verificada del servidor desde el panel.
pub async fn system_update_run(
    admin: AdminUser,
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Response {
    let actor = admin.username().to_string();
    audit_ip(
        &state,
        &headers,
        &actor,
        "system_update",
        "sistema",
        "STARTED",
        "",
    )
    .await;

    let result = state
        .run_system_blocking(move || -> Result<String, String> {
            if is_systemd_unit_active("nas-system-update")
                || is_systemd_unit_active("nas-system-rollback")
            {
                return Err("ya hay una operación de actualización/regresión en ejecución".into());
            }

            let runner = Runner::new();
            let output = runner.run(&system_update_command_spec()).map_err(|error| {
                format!("no se pudo iniciar la unidad de actualización: {error}")
            })?;

            let combined = [output.stdout.trim(), output.stderr.trim()]
                .into_iter()
                .filter(|part| !part.is_empty())
                .collect::<Vec<_>>()
                .join("\n");

            if output.is_success() {
                Ok(if combined.is_empty() {
                    "Unidad nas-system-update iniciada en segundo plano".to_string()
                } else {
                    combined
                })
            } else {
                Err(if combined.is_empty() {
                    "systemd-run devolvió un error al iniciar nas-system-update".to_string()
                } else {
                    combined
                })
            }
        })
        .await
        .unwrap_or_else(|_| Err("actualización interrumpida".to_string()));

    match result {
        Ok(output) => {
            audit_ip(
                &state,
                &headers,
                &actor,
                "system_update",
                "sistema",
                "QUEUED",
                "",
            )
            .await;
            let payload = serde_json::json!({
                "queued": true,
                "unit": "nas-system-update",
                "output": output,
                "version": env!("CARGO_PKG_VERSION"),
            });
            axum::Json(ApiResponse::ok(
                payload,
                "Actualización del sistema iniciada en segundo plano",
            ))
            .into_response()
        }
        Err(error) => {
            audit_ip(
                &state,
                &headers,
                &actor,
                "system_update",
                "sistema",
                "FAILED",
                &error,
            )
            .await;
            server_error(&error)
        }
    }
}

/// Ejecuta una regresión (downgrade A/B) hacia una release local instalada desde el panel.
pub async fn system_update_rollback(
    admin: AdminUser,
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::Json(body): axum::Json<RollbackBody>,
) -> Response {
    if body.confirm.trim() != "SI-REVERTIR" {
        return bad_request("debes escribir exactamente SI-REVERTIR para confirmar la regresión");
    }

    let target_version = body.version.trim().trim_start_matches('v').to_string();
    if !is_valid_version(&target_version) {
        return bad_request("versión objetivo inválida para regresión");
    }

    let releases_root = resolve_releases_root(&state.paths.repo);
    let installed = list_installed_releases(&releases_root.to_string_lossy());
    let Some(found) = installed.iter().find(|r| r.version == target_version) else {
        return error_response(nas_core::NasError::NotFound(format!(
            "no existe la release instalada {target_version}"
        )));
    };
    if found.current {
        return bad_request("la versión seleccionada ya se encuentra activa");
    }
    if !found.has_binaries {
        return bad_request(
            "la release seleccionada no contiene binarios completos para regresión",
        );
    }

    let actor = admin.username().to_string();
    audit_ip(
        &state,
        &headers,
        &actor,
        "system_rollback",
        &target_version,
        "STARTED",
        "",
    )
    .await;

    let ver_for_cmd = target_version.clone();
    let result = state
        .run_system_blocking(move || -> Result<String, (StatusCode, String)> {
            if is_systemd_unit_active("nas-system-update")
                || is_systemd_unit_active("nas-system-rollback")
            {
                return Err((
                    StatusCode::CONFLICT,
                    "ya hay una operación de actualización/regresión en ejecución".into(),
                ));
            }

            let runner = Runner::new();
            let output = runner
                .run(&system_rollback_command_spec(&ver_for_cmd))
                .map_err(|error| {
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        format!("no se pudo iniciar la unidad de regresión: {error}"),
                    )
                })?;

            let combined = [output.stdout.trim(), output.stderr.trim()]
                .into_iter()
                .filter(|part| !part.is_empty())
                .collect::<Vec<_>>()
                .join("\n");

            if output.is_success() {
                Ok(if combined.is_empty() {
                    format!("Unidad nas-system-rollback iniciada hacia v{ver_for_cmd}")
                } else {
                    combined
                })
            } else {
                Err((
                    StatusCode::INTERNAL_SERVER_ERROR,
                    if combined.is_empty() {
                        "systemd-run devolvió un error al iniciar nas-system-rollback".to_string()
                    } else {
                        combined
                    },
                ))
            }
        })
        .await
        .unwrap_or_else(|_| {
            Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                "regresión interrumpida".to_string(),
            ))
        });

    match result {
        Ok(output) => {
            audit_ip(
                &state,
                &headers,
                &actor,
                "system_rollback",
                &target_version,
                "QUEUED",
                "",
            )
            .await;
            let payload = serde_json::json!({
                "queued": true,
                "unit": "nas-system-rollback",
                "output": output,
                "target_version": target_version,
            });
            axum::Json(ApiResponse::ok(
                payload,
                "Regresión del sistema iniciada en segundo plano",
            ))
            .into_response()
        }
        Err((status, error)) => {
            audit_ip(
                &state,
                &headers,
                &actor,
                "system_rollback",
                &target_version,
                "FAILED",
                &error,
            )
            .await;
            (
                status,
                axum::Json(ApiResponse::<serde_json::Value>::error(error)),
            )
                .into_response()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{system_rollback_command_spec, system_update_command_spec};
    use nas_exec::SudoMode;

    #[test]
    fn comando_update_usa_systemd_run_con_sudo_nopasswd_y_coincide_con_sudoers() {
        let spec = system_update_command_spec();
        assert_eq!(spec.program, "/usr/bin/systemd-run");
        assert!(matches!(spec.sudo, SudoMode::Passwordless));
        assert_eq!(
            spec.args,
            vec![
                "--no-block",
                "--collect",
                "--unit=nas-system-update",
                "/usr/local/bin/nas",
                "--json",
                "update",
                "apply",
                "--repo",
                "ftole/nas_debian",
                "--root",
                "/opt/nas_debian",
                "--yes",
            ]
        );
        let full_cmd = format!("{} {}", spec.program, spec.args.join(" "));
        let install_sh = include_str!("../../../../../packaging/install.sh");
        assert!(
            install_sh.contains(&full_cmd),
            "la regla sudoers de packaging/install.sh debe autorizar exactamente: {full_cmd}"
        );
    }

    #[test]
    fn comando_rollback_usa_systemd_run_y_coincide_con_sudoers() {
        let spec = system_rollback_command_spec("0.1.11");
        assert_eq!(spec.program, "/usr/bin/systemd-run");
        assert!(matches!(spec.sudo, SudoMode::Passwordless));
        assert_eq!(
            spec.args,
            vec![
                "--no-block",
                "--collect",
                "--unit=nas-system-rollback",
                "/usr/local/bin/nas",
                "--json",
                "update",
                "rollback",
                "--root",
                "/opt/nas_debian",
                "--yes",
                "--version",
                "0.1.11",
            ]
        );
        let install_sh = include_str!("../../../../../packaging/install.sh");
        let pattern = "/usr/bin/systemd-run --no-block --collect --unit=nas-system-rollback /usr/local/bin/nas --json update rollback --root /opt/nas_debian --yes --version *";
        assert!(
            install_sh.contains(pattern),
            "la regla sudoers de packaging/install.sh debe autorizar exactamente: {pattern}"
        );
    }
}
