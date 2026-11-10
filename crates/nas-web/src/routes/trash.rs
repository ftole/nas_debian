use axum::{
    extract::{Query, State},
    http::HeaderMap,
    response::{IntoResponse, Response},
};
use serde::Deserialize;

use crate::{
    routes::{
        common::{
            audit_ip, deserialize_string_or_int, error_response, forbidden, is_admin, require_auth,
            server_error, ApiResponse, ExtQuery,
        },
        files::file_root_for,
    },
    AppState,
};

/// Cuerpo para borrado (papelera o definitivo).
#[derive(Debug, Deserialize)]
pub struct DeleteBody {
    #[serde(default)]
    pub root: Option<String>,
    #[serde(default)]
    pub path: String,
    #[serde(default)]
    pub permanent: bool,
}

#[derive(Debug, Deserialize, Default)]
pub struct TrashBody {
    #[serde(default, deserialize_with = "deserialize_string_or_int")]
    pub id: String,
    #[serde(default)]
    pub root: Option<String>,
}

/// Envía un elemento a la papelera interna o lo borra definitivamente.
pub async fn files_delete(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::Json(body): axum::Json<DeleteBody>,
) -> Response {
    let user = match require_auth(&state, &headers) {
        Ok(user) => user,
        Err(response) => return *response,
    };
    let root = match file_root_for(&state, body.root.as_deref()) {
        Ok(root) => root,
        Err(response) => return *response,
    };
    let path = body.path;

    let client = user.clone();
    let audit_path = path.clone();
    let permanent = body.permanent;

    let outcome = tokio::task::spawn_blocking(move || {
        if permanent {
            root.delete_permanent(&path).map(|_| None)
        } else {
            root.trash_with_actor(&path, &client).map(Some)
        }
    })
    .await;

    match outcome {
        Ok(Ok(target)) => {
            let action = if permanent {
                "file_delete_permanent"
            } else {
                "file_delete"
            };
            audit_ip(&state, &headers, &user, action, &audit_path, "SUCCESS", "");
            let msg = if permanent {
                "Elemento eliminado definitivamente"
            } else {
                "Elemento movido a la papelera"
            };
            axum::Json(ApiResponse::ok(
                serde_json::json!({
                    "trash": target.map(|t| t.display().to_string()),
                    "permanent": permanent,
                }),
                msg,
            ))
            .into_response()
        }
        Ok(Err(error)) => error_response(error),
        Err(_) => server_error("borrado interrumpido"),
    }
}

/// Lista la papelera interna.
pub async fn files_trash(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<ExtQuery>,
) -> Response {
    if let Err(response) = require_auth(&state, &headers) {
        return *response;
    }
    let root = match file_root_for(&state, query.root.as_deref()) {
        Ok(root) => root,
        Err(response) => return *response,
    };

    match tokio::task::spawn_blocking(move || root.trash_entries()).await {
        Ok(Ok(items)) => axum::Json(ApiResponse::ok(
            crate::legacy::trash_listing(&items),
            "Papelera",
        ))
        .into_response(),
        Ok(Err(error)) => error_response(error),
        Err(_) => server_error("papelera interrumpida"),
    }
}

/// Restaura un elemento de la papelera.
pub async fn trash_restore(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::Json(body): axum::Json<TrashBody>,
) -> Response {
    if let Err(response) = require_auth(&state, &headers) {
        return *response;
    }
    let root = match file_root_for(&state, body.root.as_deref()) {
        Ok(root) => root,
        Err(response) => return *response,
    };
    match tokio::task::spawn_blocking(move || root.restore_trash(&body.id)).await {
        Ok(Ok(name)) => axum::Json(ApiResponse::ok(
            serde_json::json!({ "restored": name }),
            "Elemento restaurado",
        ))
        .into_response(),
        Ok(Err(error)) => error_response(error),
        Err(_) => server_error("restauración interrumpida"),
    }
}

/// Elimina definitivamente un elemento de la papelera.
pub async fn trash_delete(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::Json(body): axum::Json<TrashBody>,
) -> Response {
    let user = match require_auth(&state, &headers) {
        Ok(user) => user,
        Err(response) => return *response,
    };
    if !is_admin(&state, &user) {
        return forbidden("se requieren privilegios de administrador");
    }
    let root = match file_root_for(&state, body.root.as_deref()) {
        Ok(root) => root,
        Err(response) => return *response,
    };
    match tokio::task::spawn_blocking(move || root.delete_trash(&body.id)).await {
        Ok(Ok(())) => axum::Json(ApiResponse::<serde_json::Value>::ok_empty(
            "Elemento eliminado",
        ))
        .into_response(),
        Ok(Err(error)) => error_response(error),
        Err(_) => server_error("borrado interrumpido"),
    }
}

/// Vacía la papelera.
pub async fn trash_empty(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Response {
    let user = match require_auth(&state, &headers) {
        Ok(user) => user,
        Err(response) => return *response,
    };
    if !is_admin(&state, &user) {
        return forbidden("se requieren privilegios de administrador");
    }
    let trash_body: Option<TrashBody> = if body.is_empty() {
        None
    } else {
        serde_json::from_slice(&body).ok()
    };
    let root_param = trash_body.as_ref().and_then(|b| b.root.as_deref());
    let root = match file_root_for(&state, root_param) {
        Ok(root) => root,
        Err(response) => return *response,
    };
    match tokio::task::spawn_blocking(move || root.empty_trash()).await {
        Ok(Ok(removed)) => axum::Json(ApiResponse::ok(
            serde_json::json!({ "removed": removed }),
            "Papelera vaciada",
        ))
        .into_response(),
        Ok(Err(error)) => error_response(error),
        Err(_) => server_error("vaciado interrumpido"),
    }
}

/// Restaura todos los elementos de la papelera.
pub async fn trash_restore_all(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Response {
    if let Err(response) = require_auth(&state, &headers) {
        return *response;
    }
    let trash_body: Option<TrashBody> = if body.is_empty() {
        None
    } else {
        serde_json::from_slice(&body).ok()
    };
    let root_param = trash_body.as_ref().and_then(|b| b.root.as_deref());
    let root = match file_root_for(&state, root_param) {
        Ok(root) => root,
        Err(response) => return *response,
    };
    match tokio::task::spawn_blocking(move || {
        let entries = root.trash_entries()?;
        let mut restored = 0u32;
        for entry in entries {
            if root.restore_trash(&entry.id).is_ok() {
                restored += 1;
            }
        }
        Ok::<_, nas_core::NasError>(restored)
    })
    .await
    {
        Ok(Ok(restored)) => axum::Json(ApiResponse::ok(
            serde_json::json!({ "restored": restored }),
            "Papelera restaurada",
        ))
        .into_response(),
        Ok(Err(error)) => error_response(error),
        Err(_) => server_error("restauración interrumpida"),
    }
}
