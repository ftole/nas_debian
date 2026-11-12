use axum::{
    extract::State,
    http::HeaderMap,
    response::{IntoResponse, Response},
    Router,
};
use nas_exec::Runner;
use serde::Deserialize;

use crate::{
    extractors::{AdminUser, AuthenticatedUser},
    routes::common::{audit_ip, bad_request, run_json, server_error, ApiResponse},
    AppState,
};

/// Cuerpo para unirse a un dominio.
#[derive(Debug, Deserialize)]
pub struct DomainJoinBody {
    pub domain: String,
    pub user: String,
    pub password: String,
}

/// Cuerpo para descubrir un dominio Active Directory.
#[derive(Debug, Deserialize)]
pub struct DomainDiscoverBody {
    pub domain: String,
}

/// Estado de la integración con Active Directory.
pub async fn domain_status(State(state): State<AppState>, _user: AuthenticatedUser) -> Response {
    let samba_conf = state.paths.samba_conf.clone();
    match tokio::task::spawn_blocking(|| nas_engine::domain::status(&Runner::new())).await {
        Ok(status) => {
            let workgroup = std::fs::read_to_string(&samba_conf)
                .ok()
                .and_then(|text| {
                    text.lines().find_map(|line| {
                        line.trim()
                            .strip_prefix("workgroup =")
                            .map(str::trim)
                            .map(str::to_string)
                    })
                })
                .unwrap_or_else(|| "WORKGROUP".to_string());
            let payload = serde_json::json!({
                "status": if status.joined { "joined" } else { "not_joined" },
                "workgroup": workgroup,
                "message": if status.joined { "Equipo unido al dominio" } else { "Equipo no unido a un dominio" },
                "joined": status.joined,
                "realm": status.realm,
                "domain": status.domain,
                "sssd_active": status.sssd_active,
                "details": status.details,
            });
            axum::Json(ApiResponse::ok(payload, "Estado del dominio")).into_response()
        }
        Err(_) => server_error("consulta de dominio interrumpida"),
    }
}

/// Une el servidor al dominio (requiere administrador).
pub async fn domain_join(
    State(state): State<AppState>,
    admin: AdminUser,
    headers: HeaderMap,
    axum::Json(body): axum::Json<DomainJoinBody>,
) -> Response {
    let user = admin.user().username().to_string();
    if !nas_engine::domain::is_valid_domain(&body.domain)
        || !nas_engine::domain::is_valid_user(&body.user)
    {
        audit_ip(
            &state,
            &headers,
            &user,
            "domain_join",
            &body.domain,
            "DENIED",
            "parámetros inválidos",
        );
        return bad_request("dominio o usuario inválidos");
    }

    let domain = body.domain.clone();
    let domain_user = body.user.clone();
    let password = body.password.clone();
    let outcome = tokio::task::spawn_blocking(move || {
        nas_engine::domain::join(&Runner::new(), &domain, &domain_user, &password)
    })
    .await
    .unwrap_or_else(|_| Err("unión interrumpida".to_string()));

    match outcome {
        Ok(()) => {
            audit_ip(
                &state,
                &headers,
                &user,
                "domain_join",
                &body.domain,
                "SUCCESS",
                "",
            );
            axum::Json(ApiResponse::<serde_json::Value>::ok_empty(
                "Equipo unido al dominio",
            ))
            .into_response()
        }
        Err(error) => {
            audit_ip(
                &state,
                &headers,
                &user,
                "domain_join",
                &body.domain,
                "FAILED",
                &error,
            );
            server_error(&error)
        }
    }
}

/// Saca el servidor del dominio (requiere administrador).
pub async fn domain_leave(
    State(state): State<AppState>,
    admin: AdminUser,
    headers: HeaderMap,
) -> Response {
    let user = admin.user().username().to_string();

    let outcome = tokio::task::spawn_blocking(|| nas_engine::domain::leave(&Runner::new()))
        .await
        .unwrap_or_else(|_| Err("salida interrumpida".to_string()));

    match outcome {
        Ok(()) => {
            audit_ip(
                &state,
                &headers,
                &user,
                "domain_leave",
                "domain",
                "SUCCESS",
                "",
            );
            axum::Json(ApiResponse::<serde_json::Value>::ok_empty(
                "Equipo retirado del dominio",
            ))
            .into_response()
        }
        Err(error) => {
            audit_ip(
                &state,
                &headers,
                &user,
                "domain_leave",
                "domain",
                "FAILED",
                &error,
            );
            server_error(&error)
        }
    }
}

pub async fn domain_discover(
    _admin: AdminUser,
    axum::Json(body): axum::Json<DomainDiscoverBody>,
) -> Response {
    if !nas_engine::domain::is_valid_domain(&body.domain) {
        return bad_request("dominio inválido");
    }

    let domain = body.domain.clone();
    match tokio::task::spawn_blocking(move || {
        run_json(
            nas_exec::CommandSpec::new("realm")
                .args(["discover", "--verbose", &domain])
                .sudo(nas_exec::SudoMode::Passwordless)
                .timeout(std::time::Duration::from_secs(30)),
        )
    })
    .await
    {
        Ok(Ok(raw)) => {
            let realm = raw
                .lines()
                .find_map(|line| line.strip_prefix("realm-name:"))
                .map(str::trim)
                .unwrap_or_default()
                .to_string();
            axum::Json(ApiResponse::ok(
                serde_json::json!({
                    "domain_name": body.domain,
                    "realm_name": if realm.is_empty() { body.domain.to_uppercase() } else { realm },
                    "raw": raw,
                }),
                "Dominio descubierto",
            ))
            .into_response()
        }
        Ok(Err(error)) => server_error(&error),
        Err(_) => server_error("descubrimiento interrumpido"),
    }
}

/// Router del dominio de Active Directory.
pub fn router() -> Router<crate::AppState> {
    use axum::routing::{get, post};
    Router::new()
        .route("/api/domain", get(domain_status))
        .route("/api/domain/join", post(domain_join))
        .route("/api/domain/leave", post(domain_leave))
        .route("/api/domain/status", get(domain_status))
        .route("/api/domain/discover", post(domain_discover))
}
