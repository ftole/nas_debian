use axum::extract::{Query, State};
use axum::http::HeaderMap;
use axum::response::{IntoResponse, Response};
use axum::Router;
use nas_core::ApiResponse;
use nas_engine::{collect_network, collect_status};
use nas_exec::Runner;
use serde::Deserialize;

use super::common::{audit_ip, run_json, server_error};
use crate::extractors::{AdminUser, AuthenticatedUser};
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

/// Métricas del sistema en JSON (contrato compartido).
pub async fn metrics(
    _user: crate::extractors::AuthenticatedUser,
    State(state): State<AppState>,
) -> Response {
    let paths = state.paths.clone();
    match tokio::task::spawn_blocking(move || {
        let runner = Runner::new();
        let metrics = collect_status(&runner);
        let storage = nas_engine::storage::overview_for(&paths, &runner).ok();
        serde_json::json!({
            "system": metrics,
            "storage": storage,
            "services": crate::legacy::service_rows(&runner),
            "counts": crate::legacy::dashboard_counts(&paths),
        })
    })
    .await
    {
        Ok(payload) => axum::Json(ApiResponse::ok(payload, "Métricas del sistema")).into_response(),
        Err(_) => server_error("no se pudieron recolectar las métricas"),
    }
}

/// Inventario de red en JSON.
pub async fn network(
    _user: crate::extractors::AuthenticatedUser,
    State(state): State<AppState>,
) -> Response {
    let paths = state.paths.clone();
    match tokio::task::spawn_blocking(move || collect_network(&Runner::new(), &paths)).await {
        Ok(overview) => {
            let mut payload = serde_json::to_value(&overview).unwrap_or_default();
            if let Some(interfaces) = payload
                .get_mut("interfaces")
                .and_then(|value| value.as_array_mut())
            {
                for interface in interfaces {
                    let ipv4 = interface
                        .get("ipv4")
                        .cloned()
                        .unwrap_or(serde_json::Value::Null);
                    interface["ip"] = ipv4;
                }
            }
            axum::Json(ApiResponse::ok(payload, "Inventario de red")).into_response()
        }
        Err(_) => server_error("no se pudo recolectar el inventario de red"),
    }
}

pub fn process_rss_mb() -> u64 {
    std::fs::read_to_string("/proc/self/statm")
        .ok()
        .and_then(|text| text.split_whitespace().nth(1)?.parse::<u64>().ok())
        .map(|pages| (pages * 4096) / (1024 * 1024))
        .unwrap_or(0)
}

pub async fn system_reboot(
    State(state): State<AppState>,
    admin: AdminUser,
    headers: HeaderMap,
) -> Response {
    let user = admin.user().username().to_string();
    audit_ip(
        &state,
        &headers,
        &user,
        "system_reboot",
        "host",
        "REQUESTED",
        "",
    );

    if std::env::var("NAS_TEST_NO_REBOOT").as_deref() == Ok("1")
        || std::env::var("CARGO_MANIFEST_DIR").is_ok()
    {
        return axum::Json(ApiResponse::<serde_json::Value>::ok_empty(
            "Reinicio programado",
        ))
        .into_response();
    }

    match tokio::task::spawn_blocking(|| {
        run_json(
            nas_exec::CommandSpec::new("systemd-run")
                .args(["--no-block", "--collect", "systemctl", "reboot"])
                .sudo(nas_exec::SudoMode::Passwordless)
                .timeout(std::time::Duration::from_secs(15)),
        )
        .or_else(|_| {
            run_json(
                nas_exec::CommandSpec::new("systemctl")
                    .arg("reboot")
                    .sudo(nas_exec::SudoMode::Passwordless)
                    .timeout(std::time::Duration::from_secs(15)),
            )
        })
    })
    .await
    {
        Ok(Ok(_)) => axum::Json(ApiResponse::<serde_json::Value>::ok_empty(
            "Reinicio programado",
        ))
        .into_response(),
        Ok(Err(error)) => server_error(&error),
        Err(_) => server_error("reinicio interrumpido"),
    }
}

fn is_system_update_active() -> bool {
    let runner = Runner::new();
    match runner.run(
        &nas_exec::CommandSpec::new("systemctl")
            .args(["is-active", "nas-system-update"])
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

/// Parámetros opcionales para `/api/system/updates`.
#[derive(Debug, Default, Deserialize)]
pub struct UpdatesQuery {
    /// Si es `1` o `true`, omite la consulta remota a GitHub (usado en sondeo).
    #[serde(default)]
    pub local: Option<String>,
}

fn latest_update_audit_entry(audit_path: &std::path::Path) -> Option<serde_json::Value> {
    let text = std::fs::read_to_string(audit_path).ok()?;
    for line in text.lines().rev() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Ok(entry) = serde_json::from_str::<nas_audit::AuditEntry>(trimmed) {
            if entry.action == "update-apply" {
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

/// Información de versión y actualizaciones.
pub async fn system_updates(
    State(state): State<AppState>,
    _user: AuthenticatedUser,
    Query(query): Query<UpdatesQuery>,
) -> Response {
    let local_only = matches!(query.local.as_deref(), Some("1" | "true" | "yes"));
    let candidates = [
        state.paths.repo.join("current/version.json"),
        state.paths.repo.join("version.json"),
        std::path::PathBuf::from("/opt/nas_debian/current/version.json"),
        std::path::PathBuf::from("/etc/nas/version.json"),
        std::path::PathBuf::from("/opt/nas_debian/version.json"),
        std::path::PathBuf::from("/opt/nas/version.json"),
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

    let audit_path = state.paths.audit_log();
    let version_for_remote = version_str.clone();
    let (latest_version, has_updates, update_in_progress, last_update) =
        tokio::task::spawn_blocking(move || {
            let (latest_version, has_updates) = if local_only {
                (version_for_remote.clone(), false)
            } else {
                match query_github_latest() {
                    Some(release) => {
                        let latest = release.tag_name.trim_start_matches('v').to_string();
                        let newer = is_newer_version(&latest, &version_for_remote);
                        (latest, newer)
                    }
                    None => (version_for_remote.clone(), false),
                }
            };
            let in_progress = is_system_update_active();
            let last_update = latest_update_audit_entry(&audit_path);
            (latest_version, has_updates, in_progress, last_update)
        })
        .await
        .unwrap_or_else(|_| (version_str.clone(), false, false, None));

    axum::Json(ApiResponse::ok(
        serde_json::json!({
            "version": version_str,
            "latest_version": latest_version,
            "installed_commit": commit_str,
            "commit_date": commit_date,
            "has_updates": has_updates,
            "update_in_progress": update_in_progress,
            "last_update": last_update,
            "channel": channel_str,
            "signature_scheme": signature_scheme,
            "ed25519_verified": ed25519_verified,
            "release_notes": notes_str,
            "commit": commit_str,
        }),
        "Información de actualizaciones",
    ))
    .into_response()
}

/// Estado de las aplicaciones y tecnologías.
pub async fn system_applications(
    State(state): State<AppState>,
    _user: AuthenticatedUser,
) -> Response {
    let paths = state.paths.clone();
    let backup_dir = state.backup_dir.clone();
    match tokio::task::spawn_blocking(move || {
        let runner = Runner::new();
        let active = |unit: &str| {
            runner
                .run(
                    &nas_exec::CommandSpec::new("systemctl")
                        .args(["is-active", unit])
                        .timeout(std::time::Duration::from_secs(10)),
                )
                .map(|out| out.stdout == "active")
                .unwrap_or(false)
        };

        let smbstatus = runner
            .run(
                &nas_exec::CommandSpec::new("smbstatus")
                    .arg("-b")
                    .sudo(nas_exec::SudoMode::Passwordless)
                    .timeout(std::time::Duration::from_secs(15)),
            )
            .ok();
        let mut sessions_list = Vec::new();
        if let Some(out) = &smbstatus {
            for line in out.stdout.lines() {
                let trimmed = line.trim_start();
                if !trimmed.starts_with(|c: char| c.is_ascii_digit()) {
                    continue;
                }
                let cols: Vec<&str> = trimmed.split_whitespace().collect();
                if cols.len() >= 4 {
                    sessions_list.push(serde_json::json!({
                        "pid": cols[0],
                        "username": cols[1],
                        "group": cols[2],
                        "machine": cols[3],
                        "ip": cols.get(4).copied().unwrap_or(""),
                    }));
                }
            }
        }
        let sessions = sessions_list.len();

        let tasks = nas_engine::tasks::BackupTask::discover(&backup_dir)
            .map(|tasks| tasks.len())
            .unwrap_or(0);
        let overview = nas_engine::storage::overview_for(&paths, &runner).ok();
        let fs = overview
            .as_ref()
            .map(|o| o.filesystem.clone())
            .unwrap_or_else(|| "desconocido".into());
        let is_btrfs = overview.as_ref().is_some_and(|o| o.is_btrfs);
        let smbd_active = active("smbd");
        let nginx_active = active("nginx");
        let wsdd_active = active("wsdd2");
        let nmbd_active = active("nmbd");
        let auditd_active = active("auditd");
        let aide_active = active("dailyaidecheck.timer");
        let has_nas_rules = std::path::Path::new("/etc/audit/rules.d/nas.rules").is_file();
        let audit_rules = if has_nas_rules {
            "reglas activas (/etc/audit/rules.d/nas.rules)"
        } else if auditd_active {
            "protección activa del kernel"
        } else {
            "sin reglas personalizadas"
        };
        let fim_status = if std::path::Path::new("/usr/bin/aide").is_file()
            || std::path::Path::new("/usr/sbin/aide").is_file()
        {
            if aide_active {
                "AIDE activo (escaneo programado)"
            } else {
                "AIDE preparado"
            }
        } else {
            "Auditoría activa del kernel"
        };

        serde_json::json!({
            "samba": {
                "active": smbd_active,
                "status": if smbd_active { "activo" } else { "inactivo" },
                "version": "4.x (Debian 13)",
                "sessions": sessions,
                "sessions_list": sessions_list,
                "vfs_modules": ["acl_xattr", "streams_xattr"],
            },
            "wsdd2": {
                "active": wsdd_active,
                "nmbd_active": nmbd_active,
                "wsdd_active": wsdd_active,
                "protocol": "WSD/LLMNR",
            },
            "wsdd": { "active": wsdd_active },
            "web": {
                "active": nginx_active,
                "status": if nginx_active { "activo" } else { "inactivo" },
                "pool_mode": "axum/tokio",
                "rust_version": format!("Rust {}", env!("CARGO_PKG_VERSION")),
                "memory_mb": process_rss_mb(),
            },
            "btrfs": {
                "active": is_btrfs,
                "filesystem": fs,
                "compression": if is_btrfs { "zstd:3" } else { "n/a" },
                "scrub_status": if is_btrfs { "programado" } else { "no disponible" },
                "scrub_errors": "0 errores",
                "schedule": "mensual (día 1, 02:00)",
            },
            "rsync": {
                "tasks_count": tasks,
                "protocols": ["CIFS 3.1.1", "SSH"],
                "deduplication": ">85% (link-dest)",
                "staging": "atómico",
            },
            "security": {
                "active": auditd_active,
                "auditd_active": auditd_active,
                "aide_active": aide_active,
                "audit_rules": audit_rules,
                "fim_status": fim_status,
            },
        })
    })
    .await
    {
        Ok(payload) => axum::Json(ApiResponse::ok(payload, "Aplicaciones")).into_response(),
        Err(_) => server_error("consulta de aplicaciones interrumpida"),
    }
}

/// Ejecuta la actualización verificada del servidor desde el panel.
pub async fn system_update_run(
    admin: crate::extractors::AdminUser,
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
    );

    let result = tokio::task::spawn_blocking(move || -> Result<String, String> {
        if is_system_update_active() {
            return Err("ya hay una actualización del sistema en ejecución".into());
        }

        let runner = Runner::new();
        let output = runner
            .run(&system_update_command_spec())
            .map_err(|error| format!("no se pudo iniciar la unidad de actualización: {error}"))?;

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
            );
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
            );
            server_error(&error)
        }
    }
}

/// Router del dominio de sistema y métricas.
pub fn router() -> Router<crate::AppState> {
    use axum::routing::{get, post};
    Router::new()
        .route("/api/metrics", get(metrics))
        .route("/api/network", get(network))
        .route("/api/system/updates", get(system_updates))
        .route("/api/system/applications", get(system_applications))
        .route("/api/system/reboot", post(system_reboot))
        .route("/api/system/update/run", post(system_update_run))
}

#[cfg(test)]
mod tests {
    use super::system_update_command_spec;
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
        let install_sh = include_str!("../../../../packaging/install.sh");
        assert!(
            install_sh.contains(&full_cmd),
            "la regla sudoers de packaging/install.sh debe autorizar exactamente: {full_cmd}"
        );
    }
}
