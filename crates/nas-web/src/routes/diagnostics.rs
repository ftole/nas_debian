use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use nas_core::ApiResponse;
use nas_engine::collect_status;
use nas_exec::Runner;
use serde::Deserialize;

use super::common::{require_auth, run_json, server_error};
use crate::AppState;

#[derive(Debug, Deserialize)]
pub struct DiagBody {
    #[serde(default)]
    pub r#type: String,
}

pub async fn diagnostics(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if let Err(response) = require_auth(&state, &headers) {
        return *response;
    }

    let paths = state.paths.clone();
    match tokio::task::spawn_blocking(move || {
        let runner = Runner::new();
        let checks = nas_engine::diagnostics::collect(&paths, &runner);
        let services = crate::legacy::service_rows(&runner);
        let active = services.iter().filter(|row| row["active"] == true).count();
        let total = services.len();
        let metrics = collect_status(&runner);
        let storage = nas_engine::storage::overview_for(&paths, &runner).ok();
        let counts = crate::legacy::dashboard_counts(&paths);
        let testparm_ok = runner
            .run(&nas_exec::CommandSpec::new("testparm").arg("-s"))
            .map(|output| output.is_success())
            .unwrap_or(false);
        serde_json::json!({
            "overall_status": if active == total { "OK" } else { "Warning" },
            "services_active": active,
            "services_total": total,
            "services": services,
            "storage": storage,
            "shares_count": counts["shares"],
            "backups_count": counts["backups"],
            "testparm_ok": testparm_ok,
            "hostname": metrics.hostname,
            "uptime": metrics.uptime,
            "kernel": metrics.kernel,
            "checks": checks,
        })
    })
    .await
    {
        Ok(payload) => axum::Json(ApiResponse::ok(payload, "Diagnóstico integral")).into_response(),
        Err(_) => server_error("diagnóstico interrumpido"),
    }
}

pub async fn diagnostics_run(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::Json(body): axum::Json<DiagBody>,
) -> Response {
    if let Err(response) = require_auth(&state, &headers) {
        return *response;
    }
    let kind = if body.r#type.is_empty() {
        "services".to_string()
    } else {
        body.r#type.clone()
    };
    let data_root = state.paths.data_root.to_string_lossy().to_string();

    let outcome = tokio::task::spawn_blocking(move || match kind.as_str() {
        "ping" => run_json(
            nas_exec::CommandSpec::new("ping")
                .args(["-c", "1", "-W", "2", "1.1.1.1"])
                .timeout(std::time::Duration::from_secs(10)),
        ),
        "dns" => run_json(
            nas_exec::CommandSpec::new("getent")
                .args(["hosts", "localhost"])
                .timeout(std::time::Duration::from_secs(10)),
        ),
        "samba" => run_json(
            nas_exec::CommandSpec::new("testparm")
                .arg("-s")
                .timeout(std::time::Duration::from_secs(15)),
        ),
        "disk" => run_json(
            nas_exec::CommandSpec::new("df")
                .args(["-Pk", &data_root])
                .timeout(std::time::Duration::from_secs(10)),
        ),
        _ => run_json(
            nas_exec::CommandSpec::new("systemctl")
                .args(["is-active", "smbd"])
                .timeout(std::time::Duration::from_secs(10)),
        ),
    })
    .await;

    match outcome {
        Ok(Ok(output)) => axum::Json(serde_json::json!({
            "success": true,
            "status": "success",
            "message": "Prueba completada",
            "output": output,
            "data": { "status": "success", "output": output },
        }))
        .into_response(),
        Ok(Err(error)) => (
            StatusCode::OK,
            axum::Json(serde_json::json!({
                "success": true,
                "status": "failed",
                "message": "La prueba falló",
                "output": error,
                "data": { "status": "failed", "output": error },
            })),
        )
            .into_response(),
        Err(_) => server_error("prueba interrumpida"),
    }
}
