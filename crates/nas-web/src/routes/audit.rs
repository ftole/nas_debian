use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Router,
};
use serde::Deserialize;

use crate::{
    routes::common::{error_response, server_error, ApiResponse},
    AppState,
};

fn default_audit_limit() -> usize {
    50
}

/// Parámetro `?limit=` de la auditoría.
#[derive(Debug, Deserialize)]
pub struct AuditQuery {
    #[serde(default = "default_audit_limit")]
    pub limit: usize,
}

/// Últimas entradas de auditoría del panel.
pub async fn audit(
    _admin: crate::extractors::AdminUser,
    State(state): State<AppState>,
    Query(query): Query<AuditQuery>,
) -> Response {
    match state.auth.db().recent_audit(query.limit.clamp(1, 500)) {
        Ok(rows) => axum::Json(ApiResponse::ok(rows, "Auditoría del panel")).into_response(),
        Err(error) => error_response(error),
    }
}

/// Verifica la integridad criptográfica de la cadena SHA-256 de nas-audit.
pub async fn audit_verify(
    _admin: crate::extractors::AdminUser,
    State(state): State<AppState>,
) -> Response {
    let audit_path = state.paths.audit_log();
    if !audit_path.is_file() {
        return axum::Json(ApiResponse::ok(
            serde_json::json!({
                "valid": true,
                "count": 0,
                "status": "EMPTY",
                "message": "Bitácora criptográfica sin registros iniciales",
                "path": audit_path.display().to_string(),
            }),
            "Cadena de auditoría verificada",
        ))
        .into_response();
    }

    let log = nas_audit::AuditLog::new(&audit_path);
    match tokio::task::spawn_blocking(move || {
        let verify_res = log.verify();
        let last_hash = log.last_hash().unwrap_or_else(|_| "0".to_string());
        (verify_res, last_hash)
    })
    .await
    {
        Ok((Ok(count), last_hash)) => axum::Json(ApiResponse::ok(
            serde_json::json!({
                "valid": true,
                "count": count,
                "last_hash": last_hash,
                "status": "VERIFIED",
                "message": format!("Cadena criptográfica íntegra: {count} registros verificados correctamente"),
                "path": audit_path.display().to_string(),
            }),
            "Auditoría íntegra y verificada",
        ))
        .into_response(),
        Ok((Err(err), last_hash)) => (
            StatusCode::OK,
            axum::Json(ApiResponse::ok(
                serde_json::json!({
                    "valid": false,
                    "count": 0,
                    "last_hash": last_hash,
                    "status": "CORRUPTED",
                    "error": err.to_string(),
                    "message": format!("Alerta: la integridad de la cadena de auditoría ha fallado: {err}"),
                    "path": audit_path.display().to_string(),
                }),
                "Fallo en la verificación criptográfica",
            )),
        )
            .into_response(),
        Err(_) => server_error("verificación de auditoría interrumpida"),
    }
}

/// Router del dominio de auditoría.
pub fn router() -> Router<crate::AppState> {
    use axum::routing::get;
    Router::new()
        .route("/api/audit", get(audit))
        .route("/api/audit/verify", get(audit_verify).post(audit_verify))
        .route(
            "/api/system/audit/verify",
            get(audit_verify).post(audit_verify),
        )
}
