use axum::{
    extract::{Query, State},
    http::HeaderMap,
    response::{IntoResponse, Response},
    Router,
};
use serde::Deserialize;

use crate::{
    extractors::{AdminUser, AuthenticatedUser},
    routes::{
        common::{
            audit_ip, deserialize_string_or_int, error_response, forbidden, server_error,
            ApiResponse, ExtQuery,
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
    user: AuthenticatedUser,
    headers: HeaderMap,
    axum::Json(body): axum::Json<DeleteBody>,
) -> Response {
    if body.permanent && !user.is_admin() {
        return forbidden("se requieren privilegios de administrador");
    }
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
            audit_ip(
                &state,
                &headers,
                user.username(),
                action,
                &audit_path,
                "SUCCESS",
                "",
            );
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
    _user: AuthenticatedUser,
    Query(query): Query<ExtQuery>,
) -> Response {
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
    _user: AuthenticatedUser,
    axum::Json(body): axum::Json<TrashBody>,
) -> Response {
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
    _admin: AdminUser,
    axum::Json(body): axum::Json<TrashBody>,
) -> Response {
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
    _admin: AdminUser,
    body: axum::body::Bytes,
) -> Response {
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
    _user: AuthenticatedUser,
    body: axum::body::Bytes,
) -> Response {
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

/// Router del dominio de papelera.
pub fn router() -> Router<crate::AppState> {
    use axum::routing::{get, post};
    Router::new()
        .route("/api/files/trash", get(files_trash))
        .route("/api/files/trash/restore", post(trash_restore))
        .route("/api/files/trash/restore-all", post(trash_restore_all))
        .route("/api/files/trash/delete", post(trash_delete))
        .route("/api/files/trash/empty", post(trash_empty))
        .route("/api/files/delete", post(files_delete))
}
