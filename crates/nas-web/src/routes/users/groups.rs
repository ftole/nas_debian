use axum::extract::State;
use axum::http::HeaderMap;
use axum::response::{IntoResponse, Response};
use nas_core::ApiResponse;
use serde::Deserialize;

use super::{
    is_master_grp, is_protected_posix_account, run_accounts, valid_grp, valid_username, MemberBody,
};
use crate::extractors::{AdminUser, AuthenticatedUser};
use crate::routes::common::{audit_ip, bad_request, forbidden, server_error};
use crate::AppState;

#[derive(Debug, Deserialize)]
pub struct GroupBody {
    pub name: String,
}

#[derive(Debug, Deserialize)]
pub struct RenameGroupBody {
    pub old: String,
    #[serde(default)]
    pub new: String,
    #[serde(default)]
    pub new_name: String,
}

/// Grupos corporativos (formato que consume `app.js`).
pub async fn groups(_user: AuthenticatedUser) -> Response {
    match tokio::task::spawn_blocking(|| {
        let (_, groups) = nas_engine::users::load_local();
        let list: Vec<serde_json::Value> = groups
            .into_iter()
            .filter(|group| group.name.starts_with("grp_"))
            .map(|group| {
                let is_special = matches!(
                    group.name.as_str(),
                    "grp_samba" | "grp_web" | "grp_superadmin" | "grp_backups"
                );
                serde_json::json!({
                    "name": group.name,
                    "gid": group.gid,
                    "members": group.members,
                    "is_special": is_special,
                    "is_master": matches!(
                        group.name.as_str(),
                        "grp_samba" | "grp_superadmin" | "grp_backups" | "grp_web"
                    ),
                })
            })
            .collect();
        list
    })
    .await
    {
        Ok(list) => axum::Json(ApiResponse::ok(list, "Grupos")).into_response(),
        Err(_) => server_error("inventario de grupos interrumpido"),
    }
}

/// Añade o quita un miembro de un grupo.
pub async fn user_groups(_admin: AdminUser, axum::Json(body): axum::Json<MemberBody>) -> Response {
    if !valid_username(&body.username)
        || is_protected_posix_account(&body.username)
        || !valid_grp(&body.group)
    {
        return bad_request("usuario o grupo inválido");
    }
    if body.group == "grp_superadmin" {
        return forbidden("el grupo grp_superadmin es inmutable");
    }

    let action = if body.action == "add" {
        "add-member"
    } else {
        "remove-member"
    };
    let username = body.username.clone();
    let group = body.group.clone();
    let result =
        tokio::task::spawn_blocking(move || run_accounts(&[action, &username, &group], None))
            .await
            .unwrap_or_else(|_| Err("membresía interrumpida".to_string()));

    match result {
        Ok(_) => axum::Json(ApiResponse::<serde_json::Value>::ok_empty(
            "Membresía actualizada",
        ))
        .into_response(),
        Err(error) => server_error(&error),
    }
}

/// Crea un grupo corporativo.
pub async fn group_create(
    State(state): State<AppState>,
    admin: AdminUser,
    headers: HeaderMap,
    axum::Json(body): axum::Json<GroupBody>,
) -> Response {
    let actor = admin.user().username().to_string();
    let raw = body.name.trim();
    let clean = raw.trim_start_matches("grp_");
    let name = format!("grp_{clean}");
    if clean.is_empty() || !valid_grp(&name) {
        return bad_request(
            "el grupo debe tener un nombre válido (letras minúsculas, números, guiones y hasta 32 caracteres)",
        );
    }
    if is_master_grp(&name) {
        return bad_request("no se puede crear un grupo maestro reservado del sistema");
    }

    let created = name.clone();
    let result =
        tokio::task::spawn_blocking(move || run_accounts(&["create-group", &created], None))
            .await
            .unwrap_or_else(|_| Err("creación interrumpida".to_string()));

    match result {
        Ok(_) => {
            audit_ip(
                &state,
                &headers,
                &actor,
                "group_create",
                &name,
                "SUCCESS",
                "",
            );
            axum::Json(ApiResponse::<serde_json::Value>::ok_empty("Grupo creado")).into_response()
        }
        Err(error) => server_error(&error),
    }
}

/// Renombra un grupo.
pub async fn group_rename(
    _admin: AdminUser,
    axum::Json(body): axum::Json<RenameGroupBody>,
) -> Response {
    let raw_new = if body.new.is_empty() {
        body.new_name.trim()
    } else {
        body.new.trim()
    };
    let clean_new = raw_new.trim_start_matches("grp_");
    let new_name = format!("grp_{clean_new}");
    if clean_new.is_empty()
        || !valid_grp(&body.old)
        || !valid_grp(&new_name)
        || is_master_grp(&body.old)
        || is_master_grp(&new_name)
    {
        return bad_request("nombre de grupo inválido o grupo maestro protegido");
    }

    let old = body.old.clone();
    let target = new_name.clone();
    let result =
        tokio::task::spawn_blocking(move || run_accounts(&["rename-group", &old, &target], None))
            .await
            .unwrap_or_else(|_| Err("renombrado interrumpido".to_string()));

    match result {
        Ok(_) => axum::Json(ApiResponse::<serde_json::Value>::ok_empty(
            "Grupo renombrado",
        ))
        .into_response(),
        Err(error) => server_error(&error),
    }
}

/// Elimina un grupo corporativo.
pub async fn group_delete(
    State(state): State<AppState>,
    admin: AdminUser,
    headers: HeaderMap,
    axum::Json(body): axum::Json<GroupBody>,
) -> Response {
    let actor = admin.user().username().to_string();
    let name = body.name.trim().to_string();
    if !valid_grp(&name) || is_master_grp(&name) {
        return bad_request("grupo inválido o grupo maestro protegido");
    }

    let target = name.clone();
    let result =
        tokio::task::spawn_blocking(move || run_accounts(&["delete-group", &target], None))
            .await
            .unwrap_or_else(|_| Err("borrado interrumpido".to_string()));

    match result {
        Ok(_) => {
            audit_ip(
                &state,
                &headers,
                &actor,
                "group_delete",
                &name,
                "SUCCESS",
                "",
            );
            axum::Json(ApiResponse::<serde_json::Value>::ok_empty(
                "Grupo eliminado",
            ))
            .into_response()
        }
        Err(error) => server_error(&error),
    }
}
