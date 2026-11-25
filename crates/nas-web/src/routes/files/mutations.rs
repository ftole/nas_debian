#[path = "upload.rs"]
mod upload;

use axum::{
    extract::State,
    response::{IntoResponse, Response},
};

use super::{file_root_for_async, CreateFileBody, MoveBody, PathBody, RenameBody};
use crate::{
    extractors::AuthenticatedUser,
    files::sanitize_filename,
    routes::common::{bad_request, error_response, server_error, ApiResponse},
    AppState,
};

pub use upload::files_upload;

/// Crea un directorio.
pub async fn files_mkdir(
    State(state): State<AppState>,
    _user: AuthenticatedUser,
    axum::Json(body): axum::Json<PathBody>,
) -> Response {
    let root = match file_root_for_async(&state, body.root.as_deref()).await {
        Ok(root) => root,
        Err(response) => return *response,
    };
    let target = match body.name.filter(|n| !n.trim().is_empty()) {
        Some(name) => {
            let safe_name = match sanitize_filename(name.trim()) {
                Ok(n) => n,
                Err(e) => return error_response(e),
            };
            let parent = body.path.trim().trim_matches('/');
            if parent.is_empty() {
                safe_name
            } else {
                format!("{parent}/{safe_name}")
            }
        }
        None => {
            let p = body.path.trim().trim_matches('/').to_string();
            if p.is_empty() {
                return bad_request("debe indicar una ruta o nombre de directorio");
            }
            p
        }
    };

    match state.run_system_blocking(move || root.mkdir(&target)).await {
        Ok(Ok(_)) => axum::Json(ApiResponse::<serde_json::Value>::ok_empty(
            "Directorio creado",
        ))
        .into_response(),
        Ok(Err(error)) => error_response(error),
        Err(_) => server_error("creación interrumpida"),
    }
}

/// Renombra o mueve un elemento.
pub async fn files_rename(
    State(state): State<AppState>,
    _user: AuthenticatedUser,
    axum::Json(body): axum::Json<RenameBody>,
) -> Response {
    let root = match file_root_for_async(&state, body.root.as_deref()).await {
        Ok(root) => root,
        Err(response) => return *response,
    };

    let (from, to) = if !body.from.trim().is_empty() && !body.to.trim().is_empty() {
        (body.from.trim().to_string(), body.to.trim().to_string())
    } else {
        let src = if !body.old_path.trim().is_empty() {
            body.old_path.trim().to_string()
        } else if !body.path.trim().is_empty() {
            body.path.trim().to_string()
        } else {
            return bad_request("ruta de origen requerida");
        };
        let new_name = body.new_name.trim();
        if new_name.is_empty() {
            return bad_request("nuevo nombre requerido");
        }
        let safe_name = match sanitize_filename(new_name) {
            Ok(name) => name,
            Err(error) => return error_response(error),
        };
        let src_clean = src.trim_matches('/');
        let dest = match src_clean.rsplit_once('/') {
            Some((parent, _)) => format!("{parent}/{safe_name}"),
            None => safe_name,
        };
        (src_clean.to_string(), dest)
    };

    match state
        .run_system_blocking(move || root.rename(&from, &to))
        .await
    {
        Ok(Ok(_)) => axum::Json(ApiResponse::<serde_json::Value>::ok_empty(
            "Elemento renombrado",
        ))
        .into_response(),
        Ok(Err(error)) => error_response(error),
        Err(_) => server_error("renombrado interrumpido"),
    }
}

/// Crea un archivo vacío/inicial o carpeta según el payload del explorador.
pub async fn files_create(
    State(state): State<AppState>,
    _user: AuthenticatedUser,
    axum::Json(body): axum::Json<CreateFileBody>,
) -> Response {
    let root = match file_root_for_async(&state, body.root.as_deref()).await {
        Ok(root) => root,
        Err(response) => return *response,
    };

    if body.name.trim().is_empty() {
        let path = body.path.trim().to_string();
        return match state.run_system_blocking(move || root.mkdir(&path)).await {
            Ok(Ok(_)) => axum::Json(ApiResponse::<serde_json::Value>::ok_empty(
                "Directorio creado",
            ))
            .into_response(),
            Ok(Err(error)) => error_response(error),
            Err(_) => server_error("creación interrumpida"),
        };
    }

    let safe_name = match sanitize_filename(body.name.trim()) {
        Ok(name) => name,
        Err(error) => return error_response(error),
    };
    let parent = body.path.trim().trim_matches('/');
    let rel = if parent.is_empty() {
        safe_name
    } else {
        format!("{parent}/{safe_name}")
    };
    let created = rel.clone();
    let is_folder = body.r#type == "folder" || body.r#type == "dir";
    let content = body.content;

    let outcome = state
        .run_system_blocking(move || {
            if is_folder {
                root.mkdir(&rel).map(|_| 0u64)
            } else {
                root.write(&rel, &content)
            }
        })
        .await;

    match outcome {
        Ok(Ok(bytes)) => axum::Json(ApiResponse::ok(
            serde_json::json!({ "path": created, "bytes": bytes }),
            "Elemento creado",
        ))
        .into_response(),
        Ok(Err(error)) => error_response(error),
        Err(_) => server_error("creación interrumpida"),
    }
}

/// Mueve un elemento entre directorios del explorador.
pub async fn files_move(
    State(state): State<AppState>,
    _user: AuthenticatedUser,
    axum::Json(body): axum::Json<MoveBody>,
) -> Response {
    let root = match file_root_for_async(&state, body.root.as_deref()).await {
        Ok(root) => root,
        Err(response) => return *response,
    };

    let (from, to) = {
        let source = if !body.from.trim().is_empty() {
            body.from.trim().trim_matches('/').to_string()
        } else if !body.path.trim().is_empty() {
            body.path.trim().trim_matches('/').to_string()
        } else {
            String::new()
        };

        if source.is_empty() {
            return bad_request("debe indicar origen y destino");
        }

        let dest = if !body.to.trim().is_empty() {
            body.to.trim().trim_matches('/').to_string()
        } else if !body.target_dir.trim().is_empty() {
            let Some(basename) = source.rsplit('/').next().filter(|s| !s.is_empty()) else {
                return bad_request("ruta de origen inválida");
            };
            let target_dir = body.target_dir.trim().trim_matches('/');
            if target_dir.is_empty() {
                basename.to_string()
            } else {
                format!("{target_dir}/{basename}")
            }
        } else {
            return bad_request("debe indicar origen y destino");
        };

        (source, dest)
    };

    match state
        .run_system_blocking(move || root.rename(&from, &to))
        .await
    {
        Ok(Ok(_)) => axum::Json(ApiResponse::<serde_json::Value>::ok_empty(
            "Elemento movido exitosamente",
        ))
        .into_response(),
        Ok(Err(error)) => error_response(error),
        Err(_) => server_error("movimiento interrumpido"),
    }
}
