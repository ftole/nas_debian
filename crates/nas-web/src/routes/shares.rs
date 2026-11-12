mod access;
mod helpers;

use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Router;
use nas_core::ApiResponse;
use nas_engine::parsers::smb::parse_shares;
use serde::Deserialize;

use super::common::{
    audit_ip, bad_request, error_response, forbidden, is_admin, require_auth, server_error,
};
use crate::AppState;

pub use access::{share_access_repair, share_access_update, shares_access, shares_access_audit};
pub use helpers::{
    apply_acl, apply_spec_acls, create_share_dir, normalize_group, revoke_removed_acls, share_path,
    share_spec, stage_and_apply, validate_share_spec,
};

#[derive(Debug, Deserialize)]
pub struct ShareBody {
    pub name: String,
    #[serde(default)]
    pub comment: String,
    #[serde(default)]
    pub scheme: u8,
    #[serde(default)]
    pub hidden: bool,
    #[serde(default)]
    pub unit: String,
    #[serde(default)]
    pub groups: Vec<String>,
    #[serde(default)]
    pub write_group: String,
}

#[derive(Debug, Deserialize)]
pub struct ShareNameBody {
    pub name: String,
}

/// Recursos compartidos declarados en `smb.conf`.
pub async fn shares(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if let Err(response) = require_auth(&state, &headers) {
        return *response;
    }

    if !state.role.has_shares() {
        return (
            StatusCode::FORBIDDEN,
            axum::Json(ApiResponse::<serde_json::Value>::error(
                "el rol BACKUP no expone recursos compartidos",
            )),
        )
            .into_response();
    }

    let path = state.paths.samba_conf.clone();
    match tokio::task::spawn_blocking(move || std::fs::read_to_string(&path)).await {
        Ok(Ok(text)) => {
            let shares: Vec<serde_json::Value> = parse_shares(&text)
                .into_iter()
                .map(|share| {
                    let scheme_name = if share.guest_ok {
                        "Acceso público / Invitados"
                    } else if share.read_only {
                        "Solo lectura estricta"
                    } else if !share.write_list.is_empty() {
                        "Lectura general + escritura exclusiva"
                    } else {
                        "Lectura y escritura por grupo"
                    };
                    serde_json::json!({
                        "name": share.name,
                        "path": share.path,
                        "read_only": share.read_only,
                        "browseable": share.browseable,
                        "hidden": !share.browseable,
                        "guest_ok": share.guest_ok,
                        "valid_users": share.valid_users,
                        "write_list": share.write_list,
                        "comment": share.comment,
                        "scheme_name": scheme_name,
                        "scheme": if share.guest_ok { 4 } else if share.read_only { 3 } else if !share.write_list.is_empty() { 2 } else { 1 },
                    })
                })
                .collect();
            axum::Json(ApiResponse::ok(shares, "Recursos compartidos")).into_response()
        }
        Ok(Err(error)) => server_error(&format!("no se pudo leer smb.conf: {error}")),
        Err(_) => server_error("lectura de smb.conf interrumpida"),
    }
}

/// Crea un recurso compartido.
pub async fn share_create(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::Json(body): axum::Json<ShareBody>,
) -> Response {
    let user = match require_auth(&state, &headers) {
        Ok(user) => user,
        Err(response) => return *response,
    };
    if !is_admin(&state, &user) {
        return forbidden("se requieren privilegios de administrador");
    }
    if !state.role.has_shares() {
        return forbidden("el rol BACKUP no expone recursos compartidos");
    }
    let unit = body.unit.trim().to_string();
    if !unit.is_empty() && unit != "principal" && !nas_engine::storage_ops::valid_name(&unit) {
        return bad_request("unidad de almacenamiento inválida");
    }

    let path = share_path(body.name.trim(), &unit);
    let spec = share_spec(&body, &path);
    if let Err(error) = validate_share_spec(&spec) {
        return error_response(error);
    }
    let share_name = spec.name.clone();
    let conf_path = state.paths.samba_conf.clone();

    let outcome = tokio::task::spawn_blocking(move || -> Result<(), String> {
        let conf = std::fs::read_to_string(&conf_path).map_err(|e| e.to_string())?;
        let updated =
            nas_engine::samba::upsert_share(&conf, &spec).map_err(|error| error.to_string())?;
        create_share_dir(&spec.name, &unit)?;
        stage_and_apply(&updated)?;
        apply_spec_acls(&spec);
        Ok(())
    })
    .await
    .unwrap_or_else(|_| Err("creación interrumpida".to_string()));

    match outcome {
        Ok(()) => {
            audit_ip(
                &state,
                &headers,
                &user,
                "share_create",
                &share_name,
                "SUCCESS",
                "",
            );
            axum::Json(ApiResponse::<serde_json::Value>::ok_empty("Recurso creado")).into_response()
        }
        Err(error) => {
            audit_ip(
                &state,
                &headers,
                &user,
                "share_create",
                &share_name,
                "FAILED",
                &error,
            );
            server_error(&error)
        }
    }
}

/// Actualiza un recurso compartido existente.
pub async fn share_update(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::Json(body): axum::Json<ShareBody>,
) -> Response {
    let user = match require_auth(&state, &headers) {
        Ok(user) => user,
        Err(response) => return *response,
    };
    if !is_admin(&state, &user) {
        return forbidden("se requieren privilegios de administrador");
    }
    if !state.role.has_shares() {
        return forbidden("el rol BACKUP no expone recursos compartidos");
    }
    if let Err(error) = nas_engine::samba::validate_name(body.name.trim()) {
        return error_response(error);
    }
    if let Err(error) = nas_engine::samba::validate_comment(&body.comment) {
        return error_response(error);
    }

    let share_name = body.name.trim().to_string();
    let conf_path = state.paths.samba_conf.clone();

    let outcome = tokio::task::spawn_blocking(move || -> Result<(), String> {
        let conf = std::fs::read_to_string(&conf_path).map_err(|e| e.to_string())?;
        let existing = nas_engine::parsers::smb::parse_shares(&conf)
            .into_iter()
            .find(|share| share.name.eq_ignore_ascii_case(body.name.trim()))
            .ok_or_else(|| format!("no existe el recurso '{}'", body.name))?;
        let preserved = existing.clone();
        let path = existing
            .path
            .unwrap_or_else(|| share_path(body.name.trim(), "principal"));
        let spec = share_spec(&body, &path);
        validate_share_spec(&spec).map_err(|error| error.to_string())?;
        let updated =
            nas_engine::samba::upsert_share(&conf, &spec).map_err(|error| error.to_string())?;
        stage_and_apply(&updated)?;
        apply_spec_acls(&spec);
        revoke_removed_acls(&preserved, &spec);
        Ok(())
    })
    .await
    .unwrap_or_else(|_| Err("actualización interrumpida".to_string()));

    match outcome {
        Ok(()) => {
            audit_ip(
                &state,
                &headers,
                &user,
                "share_update",
                &share_name,
                "SUCCESS",
                "",
            );
            axum::Json(ApiResponse::<serde_json::Value>::ok_empty(
                "Recurso actualizado",
            ))
            .into_response()
        }
        Err(error) => {
            audit_ip(
                &state,
                &headers,
                &user,
                "share_update",
                &share_name,
                "FAILED",
                &error,
            );
            server_error(&error)
        }
    }
}

/// Elimina un recurso compartido (conserva los archivos).
pub async fn share_delete(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::Json(body): axum::Json<ShareNameBody>,
) -> Response {
    let user = match require_auth(&state, &headers) {
        Ok(user) => user,
        Err(response) => return *response,
    };
    if !is_admin(&state, &user) {
        return forbidden("se requieren privilegios de administrador");
    }
    if !state.role.has_shares() {
        return forbidden("el rol BACKUP no expone recursos compartidos");
    }
    if let Err(error) = nas_engine::samba::validate_name(body.name.trim()) {
        return error_response(error);
    }

    let name = body.name.trim().to_string();
    let conf_path = state.paths.samba_conf.clone();
    let outcome = tokio::task::spawn_blocking(move || -> Result<(), String> {
        let conf = std::fs::read_to_string(&conf_path).map_err(|e| e.to_string())?;
        let updated =
            nas_engine::samba::remove_share(&conf, &name).map_err(|error| error.to_string())?;
        stage_and_apply(&updated)
    })
    .await
    .unwrap_or_else(|_| Err("borrado interrumpido".to_string()));

    match outcome {
        Ok(()) => {
            audit_ip(
                &state,
                &headers,
                &user,
                "share_delete",
                &body.name,
                "SUCCESS",
                "",
            );
            axum::Json(ApiResponse::<serde_json::Value>::ok_empty(
                "Recurso eliminado",
            ))
            .into_response()
        }
        Err(error) => {
            audit_ip(
                &state,
                &headers,
                &user,
                "share_delete",
                &body.name,
                "FAILED",
                &error,
            );
            server_error(&error)
        }
    }
}

/// Router del dominio de recursos compartidos.
pub fn router() -> Router<crate::AppState> {
    use axum::routing::{get, post};
    Router::new()
        .route("/api/shares", get(shares).post(share_create))
        .route("/api/shares/update", post(share_update))
        .route("/api/shares/delete", post(share_delete))
        .route(
            "/api/shares/access",
            get(shares_access).post(share_access_update),
        )
        .route("/api/shares/access/repair", post(share_access_repair))
        .route("/api/shares/access/audit", get(shares_access_audit))
}
