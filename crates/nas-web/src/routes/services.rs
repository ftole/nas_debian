use axum::extract::State;
use axum::http::HeaderMap;
use axum::response::{IntoResponse, Response};
use axum::Router;
use nas_core::ApiResponse;
use nas_exec::Runner;
use serde::Deserialize;

use super::common::{audit_ip, bad_request, server_error};
use crate::AppState;

/// Cuerpo para gestionar un servicio.
#[derive(Debug, Deserialize)]
pub struct ServiceBody {
    pub service: String,
    pub action: String,
}

/// Estado de los demonios clave.
pub async fn services(
    _user: crate::extractors::AuthenticatedUser,
    State(_state): State<AppState>,
) -> Response {
    match tokio::task::spawn_blocking(|| crate::legacy::service_rows(&Runner::new())).await {
        Ok(list) => axum::Json(ApiResponse::ok(list, "Estado de servicios")).into_response(),
        Err(_) => server_error("consulta de servicios interrumpida"),
    }
}

/// Reinicia o recarga un servicio de la lista blanca.
pub async fn service_manage(
    admin: crate::extractors::AdminUser,
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::Json(body): axum::Json<ServiceBody>,
) -> Response {
    let user = admin.username().to_string();
    if !nas_engine::services::is_allowed_service(&body.service)
        || !nas_engine::services::is_allowed_action(&body.action)
    {
        audit_ip(
            &state,
            &headers,
            &user,
            "service_manage",
            &body.service,
            "DENIED",
            &body.action,
        );
        return bad_request("servicio o acción no permitidos");
    }

    let service = body.service.clone();
    let action = body.action.clone();
    let outcome = tokio::task::spawn_blocking(move || {
        nas_engine::services::manage(&Runner::new(), &service, &action, true)
    })
    .await
    .unwrap_or_else(|_| Err("ejecución interrumpida".to_string()));

    match outcome {
        Ok(()) => {
            audit_ip(
                &state,
                &headers,
                &user,
                "service_manage",
                &body.service,
                "SUCCESS",
                &body.action,
            );
            axum::Json(ApiResponse::<serde_json::Value>::ok_empty(format!(
                "{} {} ejecutado",
                body.action, body.service
            )))
            .into_response()
        }
        Err(error) => {
            audit_ip(
                &state,
                &headers,
                &user,
                "service_manage",
                &body.service,
                "FAILED",
                &error,
            );
            server_error(&error)
        }
    }
}

/// Router del dominio de servicios.
pub fn router() -> Router<crate::AppState> {
    use axum::routing::{get, post};
    Router::new()
        .route("/api/services", get(services))
        .route("/api/services/manage", post(service_manage))
}
