use std::path::PathBuf;

use axum::{
    extract::{Multipart, Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use tokio::io::AsyncWriteExt;

use super::{
    file_root_for, file_root_for_async, CreateFileBody, MoveBody, PathBody, RenameBody,
    MAX_UPLOAD_FILE_SIZE,
};
use crate::{
    extractors::AuthenticatedUser,
    files::{free_and_total_bytes_at, sanitize_filename},
    routes::common::{bad_request, error_response, random_hex, server_error, ApiResponse},
    AppState,
};

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

const UPLOAD_SPACE_CHECK_INTERVAL_BYTES: u64 = 16 * 1024 * 1024;

fn upload_reserve_bytes(total_fs_bytes: u64) -> u64 {
    (total_fs_bytes / 50).clamp(16 * 1024 * 1024, 512 * 1024 * 1024)
}

fn insufficient_storage_response(message: String) -> Response {
    (
        StatusCode::INSUFFICIENT_STORAGE,
        axum::Json(ApiResponse::<serde_json::Value>::error(message)),
    )
        .into_response()
}

fn prepare_upload_staging_dir(state: &AppState, requested_root: Option<&str>) -> PathBuf {
    let base_root = if let Some(req) = requested_root.map(str::trim).filter(|s| !s.is_empty()) {
        file_root_for(state, Some(req))
            .ok()
            .filter(|r| r.is_protected_path("").is_none())
            .map(|r| r.root().to_path_buf())
            .unwrap_or_else(|| state.paths.data_root.clone())
    } else {
        state.paths.data_root.clone()
    };

    let staging = base_root.join(".nas_staging");
    if std::fs::create_dir_all(&staging).is_ok() && staging.is_dir() {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&staging, std::fs::Permissions::from_mode(0o700));
        }
        return staging;
    }
    state.paths.data_root.clone()
}

async fn check_staging_space(
    state: &AppState,
    dir: &std::path::Path,
    content_len: Option<u64>,
) -> Option<Response> {
    let check_dir = dir.to_path_buf();
    if let Ok(Some((free, total_fs))) = state
        .run_system_blocking(move || free_and_total_bytes_at(&check_dir))
        .await
    {
        let reserve = upload_reserve_bytes(total_fs);
        if free <= reserve {
            return Some(insufficient_storage_response(format!(
                "espacio libre insuficiente en el volumen de destino ({} MiB disponibles, reserva mínima {} MiB)",
                free / 1024 / 1024,
                reserve / 1024 / 1024
            )));
        }
        if let Some(expected_bytes) = content_len {
            if expected_bytes.saturating_add(reserve) > free {
                return Some(insufficient_storage_response(format!(
                    "espacio insuficiente para la carga ({} MiB requeridos con reserva, {} MiB disponibles)",
                    expected_bytes.saturating_add(reserve) / 1024 / 1024,
                    free / 1024 / 1024
                )));
            }
        }
    }
    None
}

async fn resolve_staging_dir_async(state: &AppState, root_name: Option<&str>) -> PathBuf {
    let state_for_staging = state.clone();
    let req_root_clone = root_name.map(str::to_string);
    match state
        .run_system_blocking(move || {
            prepare_upload_staging_dir(&state_for_staging, req_root_clone.as_deref())
        })
        .await
    {
        Ok(dir) => dir,
        Err(_) => state.paths.data_root.clone(),
    }
}

/// Sube uno o varios archivos a un directorio, escribiendo por streaming.
pub async fn files_upload(
    State(state): State<AppState>,
    _user: AuthenticatedUser,
    Query(query): Query<std::collections::HashMap<String, String>>,
    headers: HeaderMap,
    mut multipart: Multipart,
) -> Response {
    let Ok(_queue_permit) = state.upload_queue.try_acquire() else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            axum::Json(ApiResponse::<serde_json::Value>::error(
                "cola de cargas simultáneas saturada; reintente en unos momentos",
            )),
        )
            .into_response();
    };
    let Ok(_upload_permit) = state.upload_ops.acquire().await else {
        return server_error("semáforo de carga cerrado");
    };

    let mut root_name: Option<String> = query.get("root").cloned();
    let mut directory: Option<String> = query.get("path").cloned();
    let content_len = headers
        .get(axum::http::header::CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.trim().parse::<u64>().ok());

    let mut staging_dir: Option<PathBuf> = None;
    if root_name.is_some() {
        let dir = resolve_staging_dir_async(&state, root_name.as_deref()).await;
        if let Some(resp) = check_staging_space(&state, &dir, content_len).await {
            return resp;
        }
        staging_dir = Some(dir);
    }

    let mut staged_files: Vec<(PathBuf, String)> = Vec::new();
    let mut saved: Vec<String> = Vec::new();
    let mut total_written_all: u64 = 0;
    let mut last_space_check: u64 = 0;

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
                Ok(text) => {
                    if staging_dir.is_some() && root_name.as_deref() != Some(text.as_str()) {
                        staging_dir = None;
                    }
                    root_name = Some(text);
                }
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

        let active_staging_dir = match &staging_dir {
            Some(dir) => dir.clone(),
            None => {
                let dir = resolve_staging_dir_async(&state, root_name.as_deref()).await;
                if let Some(resp) = check_staging_space(&state, &dir, content_len).await {
                    for (temp, _) in &staged_files {
                        let _ = tokio::fs::remove_file(temp).await;
                    }
                    return resp;
                }
                staging_dir = Some(dir.clone());
                dir
            }
        };

        let filename = match sanitize_filename(field.file_name().unwrap_or_default()) {
            Ok(name) => name,
            Err(error) => {
                for (temp, _) in &staged_files {
                    let _ = tokio::fs::remove_file(temp).await;
                }
                return error_response(error);
            }
        };

        let temporary = active_staging_dir.join(format!(".upload-{}", random_hex()));
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
                    total_written_all = total_written_all.saturating_add(chunk.len() as u64);
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
                    if total_written_all.saturating_sub(last_space_check)
                        >= UPLOAD_SPACE_CHECK_INTERVAL_BYTES
                    {
                        last_space_check = total_written_all;
                        let check_dir = active_staging_dir.clone();
                        if let Ok(Some((free_now, total_fs_now))) = state
                            .run_system_blocking(move || free_and_total_bytes_at(&check_dir))
                            .await
                        {
                            let reserve_now = upload_reserve_bytes(total_fs_now);
                            if free_now <= reserve_now {
                                let _ = tokio::fs::remove_file(&temporary).await;
                                for (temp, _) in &staged_files {
                                    let _ = tokio::fs::remove_file(temp).await;
                                }
                                return insufficient_storage_response(
                                    "carga abortada: espacio libre agotándose en el volumen de almacenamiento".to_string(),
                                );
                            }
                        }
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

    let root = match file_root_for_async(&state, root_name.as_deref()).await {
        Ok(root) => root,
        Err(response) => {
            for (temp, _) in &staged_files {
                let _ = tokio::fs::remove_file(temp).await;
            }
            return *response;
        }
    };

    let target_dir_str = directory.unwrap_or_default();
    let target_dir_arg = target_dir_str.clone();
    let target_dir = match state
        .run_system_blocking(move || root.resolve_dir(&target_dir_arg))
        .await
    {
        Ok(Ok(dir)) => dir,
        Ok(Err(error)) => {
            for (temp, _) in &staged_files {
                let _ = tokio::fs::remove_file(temp).await;
            }
            return error_response(error);
        }
        Err(_) => {
            for (temp, _) in &staged_files {
                let _ = tokio::fs::remove_file(temp).await;
            }
            return server_error("resolución de directorio interrumpida");
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
        if tokio::fs::try_exists(&destination).await.unwrap_or(false) {
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

    for (idx, (temporary, filename)) in staged_files.iter().enumerate() {
        let destination = target_dir.join(filename);
        if let Err(_error) = tokio::fs::rename(temporary, &destination).await {
            let staged_size = tokio::fs::metadata(temporary)
                .await
                .map(|m| m.len())
                .unwrap_or(0);
            let check_target = target_dir.clone();
            if let Ok(Some((free_dest, total_dest))) = state
                .run_system_blocking(move || free_and_total_bytes_at(&check_target))
                .await
            {
                let reserve = upload_reserve_bytes(total_dest);
                if staged_size.saturating_add(reserve) > free_dest {
                    for (rem, _) in &staged_files[idx..] {
                        let _ = tokio::fs::remove_file(rem).await;
                    }
                    return insufficient_storage_response(format!(
                        "espacio insuficiente en el volumen destino para '{}'",
                        filename
                    ));
                }
            }
            if let Err(cp_err) = tokio::fs::copy(temporary, &destination).await {
                for (rem, _) in &staged_files[idx..] {
                    let _ = tokio::fs::remove_file(rem).await;
                }
                return server_error(&format!("no se pudo promover la carga: {cp_err}"));
            }
            let _ = tokio::fs::remove_file(temporary).await;
        }
        let dest_for_perms = destination.clone();
        let _ = state
            .run_system_blocking(move || crate::files::set_file_permissions(&dest_for_perms))
            .await;

        saved.push(if target_dir_str.is_empty() {
            filename.clone()
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
