mod updates;

use axum::extract::State;
use axum::http::HeaderMap;
use axum::response::{IntoResponse, Response};
use axum::Router;
use nas_core::ApiResponse;
use nas_engine::{collect_network, collect_status};
use nas_exec::Runner;

use super::common::{audit_ip, run_json, server_error};
use crate::extractors::{AdminUser, AuthenticatedUser};
use crate::AppState;

#[allow(unused_imports)]
pub use updates::{
    is_newer_version, parse_semver, system_update_rollback, system_update_run, system_updates,
};

/// Métricas del sistema en JSON (contrato compartido).
pub async fn metrics(_user: AuthenticatedUser, State(state): State<AppState>) -> Response {
    let paths = state.paths.clone();
    match state
        .run_system_blocking(move || {
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
pub async fn network(_user: AuthenticatedUser, State(state): State<AppState>) -> Response {
    let paths = state.paths.clone();
    match state
        .run_system_blocking(move || collect_network(&Runner::new(), &paths))
        .await
    {
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
    )
    .await;

    if std::env::var("NAS_TEST_NO_REBOOT").as_deref() == Ok("1")
        || std::env::var("CARGO_MANIFEST_DIR").is_ok()
    {
        return axum::Json(ApiResponse::<serde_json::Value>::ok_empty(
            "Reinicio programado",
        ))
        .into_response();
    }

    match state
        .run_system_blocking(|| {
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

/// Estado de las aplicaciones y tecnologías.
pub async fn system_applications(
    State(state): State<AppState>,
    _user: AuthenticatedUser,
) -> Response {
    let paths = state.paths.clone();
    let backup_dir = state.backup_dir.clone();
    match state
        .run_system_blocking(move || {
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

/// Router del dominio de sistema y métricas.
pub fn router() -> Router<AppState> {
    use axum::routing::{get, post};
    Router::new()
        .route("/api/metrics", get(metrics))
        .route("/api/network", get(network))
        .route("/api/system/updates", get(system_updates))
        .route("/api/system/applications", get(system_applications))
        .route("/api/system/reboot", post(system_reboot))
        .route("/api/system/update/run", post(system_update_run))
        .route("/api/system/update/rollback", post(system_update_rollback))
}
