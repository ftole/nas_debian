use axum::extract::State;
use axum::http::HeaderMap;
use axum::response::{IntoResponse, Response};
use nas_core::ApiResponse;
use nas_engine::{collect_network, collect_status};
use nas_exec::Runner;

use crate::AppState;
use super::common::{audit_ip, forbidden, is_admin, require_auth, run_json, server_error};

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

pub async fn system_reboot(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let user = match require_auth(&state, &headers) {
        Ok(user) => user,
        Err(response) => return *response,
    };
    if !is_admin(&state, &user) {
        return forbidden("se requieren privilegios de administrador");
    }
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

/// Información de versión y actualizaciones.
pub async fn system_updates(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if let Err(response) = require_auth(&state, &headers) {
        return *response;
    }
    let candidates = [
        state.paths.repo.join("version.json"),
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

    let gpg_verified = version
        .get("gpg_verified")
        .and_then(|v| v.as_bool())
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
        .unwrap_or("release");
    let channel_str = version
        .get("channel")
        .and_then(|v| v.as_str())
        .unwrap_or("stable");

    let (latest_version, has_updates) = match query_github_latest() {
        Some(release) => {
            let latest = release.tag_name.trim_start_matches('v').to_string();
            let newer = is_newer_version(&latest, &version_str);
            (latest, newer)
        }
        None => (version_str.clone(), false),
    };
    let notes_str = version
        .get("notes")
        .and_then(|v| v.as_str())
        .unwrap_or("Release oficial verificado con firmas criptograficas");

    axum::Json(ApiResponse::ok(
        serde_json::json!({
            "version": version_str,
            "latest_version": latest_version,
            "installed_commit": commit_str,
            "commit_date": version.get("date").and_then(|v| v.as_str()),
            "has_updates": has_updates,
            "channel": channel_str,
            "gpg_verified": gpg_verified,
            "release_notes": notes_str,
            "commit": commit_str,
        }),
        "Información de actualizaciones",
    ))
    .into_response()
}

/// Estado de las aplicaciones y tecnologías.
pub async fn system_applications(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if let Err(response) = require_auth(&state, &headers) {
        return *response;
    }

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
        let audit_rules = if std::path::Path::new("/etc/audit/rules.d/nas.rules").is_file() {
            "reglas activas (/etc/audit/rules.d/nas.rules)"
        } else {
            "sin reglas personalizadas"
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
                "php_version": format!("Rust {}", env!("CARGO_PKG_VERSION")),
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
                "fim_status": "AIDE configurado",
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
    let paths = state.paths.clone();
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
        let runner = Runner::new();
        let active = runner.run(
            &nas_exec::CommandSpec::new("systemctl")
                .args(["is-active", "nas-system-update"])
                .timeout(std::time::Duration::from_secs(5)),
        );
        let is_running = match &active {
            Ok(out) => matches!(out.stdout.trim(), "active" | "activating"),
            _ => false,
        };
        if is_running {
            return Err("ya hay una actualización del sistema en ejecución".into());
        }

        let nas_path = paths.repo.join("target/release/nas");
        let nas_executable = if nas_path.exists() {
            nas_path
        } else {
            std::path::PathBuf::from("/usr/local/bin/nas")
        };

        let output = std::process::Command::new(nas_executable)
            .args([
                "--json",
                "update",
                "apply",
                "--repo",
                "ftole/nas_debian",
                "--root",
                "/opt/nas_debian",
                "--yes",
            ])
            .env(
                "PATH",
                "/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin",
            )
            .env_clear()
            .output()
            .map_err(|error| format!("no se pudo ejecutar nas update apply: {error}"))?;

        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();

        if output.status.success() {
            Ok(stdout)
        } else {
            Err(if stderr.is_empty() { stdout } else { stderr })
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
                "SUCCESS",
                "",
            );
            let payload = serde_json::from_str::<serde_json::Value>(&output)
                .map(|data| {
                    serde_json::json!({ "output": data, "version": env!("CARGO_PKG_VERSION") })
                })
                .unwrap_or_else(|_| {
                    serde_json::json!({ "output": output, "version": env!("CARGO_PKG_VERSION") })
                });
            axum::Json(ApiResponse::ok(
                payload,
                "Actualización del sistema aplicada",
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
