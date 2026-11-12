use std::path::PathBuf;

use axum::{
    extract::{Multipart, Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use tokio::io::AsyncWriteExt;

use super::{file_root_for, CreateFileBody, MoveBody, PathBody, RenameBody, MAX_UPLOAD_FILE_SIZE};
use crate::{
    files::sanitize_filename,
    routes::common::{
        bad_request, error_response, random_hex, require_auth, server_error, ApiResponse,
    },
    AppState,
};

/// Crea un directorio.
pub async fn files_mkdir(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::Json(body): axum::Json<PathBody>,
) -> Response {
    if let Err(response) = require_auth(&state, &headers) {
        return *response;
    }
    let root = match file_root_for(&state, body.root.as_deref()) {
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

    match tokio::task::spawn_blocking(move || root.mkdir(&target)).await {
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
    headers: HeaderMap,
    axum::Json(body): axum::Json<RenameBody>,
) -> Response {
    if let Err(response) = require_auth(&state, &headers) {
        return *response;
    }
    let root = match file_root_for(&state, body.root.as_deref()) {
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

    match tokio::task::spawn_blocking(move || root.rename(&from, &to)).await {
        Ok(Ok(_)) => axum::Json(ApiResponse::<serde_json::Value>::ok_empty(
            "Elemento renombrado",
        ))
        .into_response(),
        Ok(Err(error)) => error_response(error),
        Err(_) => server_error("renombrado interrumpido"),
    }
}

/// Sube uno o varios archivos a un directorio, escribiendo por streaming.
pub async fn files_upload(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<std::collections::HashMap<String, String>>,
    mut multipart: Multipart,
) -> Response {
    if let Err(response) = require_auth(&state, &headers) {
        return *response;
    }

    let mut root_name: Option<String> = query.get("root").cloned();
    let mut directory: Option<String> = query.get("path").cloned();
    let mut staged_files: Vec<(PathBuf, String)> = Vec::new();
    let mut saved: Vec<String> = Vec::new();

    loop {
        let field = match multipart.next_field().await {
            Ok(Some(field)) => field,
            Ok(None) => break,
            Err(error) => {
                for (temp, _) in &staged_files {
                    let _ = tokio::fs::remove_file(temp).await;
                }
                return bad_request(&format!("multipart inválido: {error}"));
            }
        };

        let name = field.name().unwrap_or_default().to_string();
        if name == "root" {
            match field.text().await {
                Ok(text) => root_name = Some(text),
                Err(error) => {
                    for (temp, _) in &staged_files {
                        let _ = tokio::fs::remove_file(temp).await;
                    }
                    return bad_request(&format!("campo root inválido: {error}"));
                }
            }
            continue;
        }
        if name == "path" {
            match field.text().await {
                Ok(text) => directory = Some(text),
                Err(error) => {
                    for (temp, _) in &staged_files {
                        let _ = tokio::fs::remove_file(temp).await;
                    }
                    return bad_request(&format!("campo path inválido: {error}"));
                }
            }
            continue;
        }
        if name != "file" {
            continue;
        }

        let filename = match sanitize_filename(field.file_name().unwrap_or_default()) {
            Ok(name) => name,
            Err(error) => {
                for (temp, _) in &staged_files {
                    let _ = tokio::fs::remove_file(temp).await;
                }
                return error_response(error);
            }
        };

        let temporary = state
            .paths
            .data_root
            .join(format!(".upload-{}", random_hex()));
        let mut file = match tokio::fs::File::create(&temporary).await {
            Ok(file) => file,
            Err(error) => {
                for (temp, _) in &staged_files {
                    let _ = tokio::fs::remove_file(temp).await;
                }
                return server_error(&format!("no se pudo crear el temporal: {error}"));
            }
        };

        let mut field = field;
        let mut total: u64 = 0;
        let mut read_error = None;
        loop {
            match field.chunk().await {
                Ok(Some(chunk)) => {
                    total += chunk.len() as u64;
                    if total > MAX_UPLOAD_FILE_SIZE {
                        let _ = tokio::fs::remove_file(&temporary).await;
                        for (temp, _) in &staged_files {
                            let _ = tokio::fs::remove_file(temp).await;
                        }
                        return (
                            StatusCode::PAYLOAD_TOO_LARGE,
                            axum::Json(ApiResponse::<serde_json::Value>::error(
                                "el archivo supera el límite de 50 GB",
                            )),
                        )
                            .into_response();
                    }
                    if let Err(error) = file.write_all(&chunk).await {
                        let _ = tokio::fs::remove_file(&temporary).await;
                        for (temp, _) in &staged_files {
                            let _ = tokio::fs::remove_file(temp).await;
                        }
                        return server_error(&format!("fallo al escribir: {error}"));
                    }
                }
                Ok(None) => break,
                Err(error) => {
                    read_error = Some(error);
                    break;
                }
            }
        }

        if let Some(error) = read_error {
            let _ = tokio::fs::remove_file(&temporary).await;
            for (temp, _) in &staged_files {
                let _ = tokio::fs::remove_file(temp).await;
            }
            return bad_request(&format!("carga interrumpida: {error}"));
        }

        if let Err(error) = file.flush().await {
            let _ = tokio::fs::remove_file(&temporary).await;
            for (temp, _) in &staged_files {
                let _ = tokio::fs::remove_file(temp).await;
            }
            return server_error(&format!("fallo al sincronizar: {error}"));
        }
        drop(file);

        staged_files.push((temporary, filename));
    }

    let root = match file_root_for(&state, root_name.as_deref()) {
        Ok(root) => root,
        Err(response) => {
            for (temp, _) in &staged_files {
                let _ = tokio::fs::remove_file(temp).await;
            }
            return *response;
        }
    };

    let target_dir_str = directory.unwrap_or_default();
    let target_dir = match root.resolve_dir(&target_dir_str) {
        Ok(dir) => dir,
        Err(error) => {
            for (temp, _) in &staged_files {
                let _ = tokio::fs::remove_file(temp).await;
            }
            return error_response(error);
        }
    };

    let mut seen = std::collections::HashSet::new();
    for (_, filename) in &staged_files {
        if !seen.insert(filename) {
            for (temp, _) in &staged_files {
                let _ = tokio::fs::remove_file(temp).await;
            }
            return (
                StatusCode::CONFLICT,
                axum::Json(ApiResponse::<serde_json::Value>::error(format!(
                    "el archivo '{filename}' está duplicado en la carga"
                ))),
            )
                .into_response();
        }
        let destination = target_dir.join(filename);
        if destination.exists() {
            for (temp, _) in &staged_files {
                let _ = tokio::fs::remove_file(temp).await;
            }
            return (
                StatusCode::CONFLICT,
                axum::Json(ApiResponse::<serde_json::Value>::error(format!(
                    "el archivo '{filename}' ya existe"
                ))),
            )
                .into_response();
        }
    }

    for (temporary, filename) in staged_files {
        let destination = target_dir.join(&filename);
        if let Err(_error) = tokio::fs::rename(&temporary, &destination).await {
            if let Err(cp_err) = tokio::fs::copy(&temporary, &destination).await {
                let _ = tokio::fs::remove_file(&temporary).await;
                return server_error(&format!("no se pudo promover la carga: {cp_err}"));
            }
            let _ = tokio::fs::remove_file(&temporary).await;
        }
        crate::files::set_file_permissions(&destination);

        saved.push(if target_dir_str.is_empty() {
            filename
        } else {
            format!("{target_dir_str}/{filename}")
        });
    }

    axum::Json(ApiResponse::ok(
        serde_json::json!({ "uploaded": saved }),
        "Carga completada",
    ))
    .into_response()
}

/// Crea un archivo vacío/inicial o carpeta según el payload del explorador.
pub async fn files_create(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::Json(body): axum::Json<CreateFileBody>,
) -> Response {
    if let Err(response) = require_auth(&state, &headers) {
        return *response;
    }
    let root = match file_root_for(&state, body.root.as_deref()) {
        Ok(root) => root,
        Err(response) => return *response,
    };

    if body.name.trim().is_empty() {
        let path = body.path.trim().to_string();
        return match tokio::task::spawn_blocking(move || root.mkdir(&path)).await {
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

    let outcome = tokio::task::spawn_blocking(move || {
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
    headers: HeaderMap,
    axum::Json(body): axum::Json<MoveBody>,
) -> Response {
    if let Err(response) = require_auth(&state, &headers) {
        return *response;
    }
    let root = match file_root_for(&state, body.root.as_deref()) {
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

    match tokio::task::spawn_blocking(move || root.rename(&from, &to)).await {
        Ok(Ok(_)) => axum::Json(ApiResponse::<serde_json::Value>::ok_empty(
            "Elemento movido exitosamente",
        ))
        .into_response(),
        Ok(Err(error)) => error_response(error),
        Err(_) => server_error("movimiento interrumpido"),
    }
}
