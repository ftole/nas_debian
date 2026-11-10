use std::path::PathBuf;

use axum::{
    extract::{Multipart, Query, State},
    http::{HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
};
use nas_core::NasError;
use serde::Deserialize;
use tokio::io::AsyncWriteExt;

use crate::{
    AppState,
    files::{sanitize_filename, FileRoot},
    routes::common::{
        random_hex, safe_content_disposition, ApiResponse, bad_request, error_response, forbidden,
        require_auth, server_error,
    },
};

/// Parámetro `?path=` del explorador.
#[derive(Debug, Deserialize)]
pub struct FilesQuery {
    #[serde(default)]
    pub path: String,
    #[serde(default)]
    pub root: Option<String>,
    /// Solicitud de preparación previa (informativo; el flujo ZIP pre-comprime).
    #[serde(default)]
    pub prepare: Option<String>,
    /// Token asociado a un ZIP ya comprimido (informativo para la UI).
    #[serde(default)]
    #[allow(dead_code)]
    pub token: Option<String>,
}

/// Cuerpo con una única ruta o con nombre.
#[derive(Debug, Deserialize)]
pub struct PathBody {
    #[serde(default)]
    pub root: Option<String>,
    #[serde(default)]
    pub path: String,
    #[serde(default)]
    pub name: Option<String>,
}

/// Cuerpo para renombrar o mover.
#[derive(Debug, Deserialize)]
pub struct RenameBody {
    #[serde(default)]
    pub root: Option<String>,
    #[serde(default)]
    pub from: String,
    #[serde(default)]
    pub to: String,
    #[serde(default)]
    pub old_path: String,
    #[serde(default)]
    pub new_name: String,
    #[serde(default)]
    pub path: String,
}

#[derive(Debug, Deserialize, Default)]
pub struct CreateFileBody {
    #[serde(default)]
    pub root: Option<String>,
    #[serde(default)]
    pub path: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub content: String,
    #[serde(default)]
    pub r#type: String,
}

#[derive(Debug, Deserialize, Default)]
pub struct MoveBody {
    #[serde(default, alias = "source")]
    pub from: String,
    #[serde(default, alias = "destination", alias = "target")]
    pub to: String,
    #[serde(default)]
    pub path: String,
    #[serde(default, alias = "destination_dir")]
    pub target_dir: String,
    #[serde(default)]
    pub root: Option<String>,
}

/// Límite máximo de subida por archivo en streaming (50 GB).
pub const MAX_UPLOAD_FILE_SIZE: u64 = 50 * 1024 * 1024 * 1024;

pub fn file_root_for(state: &AppState, root: Option<&str>) -> Result<FileRoot, Box<Response>> {
    let requested = root.unwrap_or("").trim();
    let base = if requested.is_empty() || requested == "nas" || requested == "principal" {
        state.paths.data_root.clone()
    } else if requested == "backups" {
        if !state.role.has_backups() {
            return Err(Box::new(forbidden(
                "el rol ARCHIVOS no expone respaldos históricos",
            )));
        }
        state.paths.data_root.join("BACKUPS_HISTORICOS")
    } else {
        if !nas_engine::storage_ops::valid_name(requested) {
            return Err(Box::new(bad_request("unidad de almacenamiento inválida")));
        }
        let unit = state.paths.data_root.join("unidades").join(requested);
        if !unit.is_dir() {
            return Err(Box::new(not_found_response(
                "la unidad solicitada no existe",
            )));
        }
        unit
    };
    FileRoot::new(base).map_err(|error| Box::new(server_error(&error.to_string())))
}

pub fn not_found_response(message: &str) -> Response {
    (
        StatusCode::NOT_FOUND,
        axum::Json(ApiResponse::<serde_json::Value>::error(message)),
    )
        .into_response()
}

/// Lista el contenido de un directorio.
pub async fn files_list(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<FilesQuery>,
) -> Response {
    if let Err(response) = require_auth(&state, &headers) {
        return *response;
    }
    let root = match file_root_for(&state, query.root.as_deref()) {
        Ok(root) => root,
        Err(response) => return *response,
    };
    let path = query.path;

    match tokio::task::spawn_blocking(move || {
        let entries = root.list(&path)?;
        Ok::<_, nas_core::NasError>(crate::legacy::file_listing(&root, &path, &entries))
    })
    .await
    {
        Ok(Ok(payload)) => {
            axum::Json(ApiResponse::ok(payload, "Contenido del directorio")).into_response()
        }
        Ok(Err(error)) => error_response(error),
        Err(_) => server_error("listado interrumpido"),
    }
}

/// Descarga un archivo con cabeceras seguras mediante streaming asíncrono O(1).
pub async fn files_download(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<FilesQuery>,
) -> Response {
    if let Err(response) = require_auth(&state, &headers) {
        return *response;
    }
    let root = match file_root_for(&state, query.root.as_deref()) {
        Ok(root) => root,
        Err(response) => return *response,
    };
    let path = query.path.clone();
    let name = path
        .trim_end_matches('/')
        .rsplit('/')
        .next()
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| "descarga.bin".to_string());

    // Solicitud ZIP de una carpeta (el front-end manda `prepare=1` cuando es
    // carpeta): resolvemos con el zipper ya existente en `/api/files/zip`.
    let is_folder_flow = query
        .prepare
        .as_deref()
        .map(|value| !value.is_empty() && value != "0")
        .unwrap_or(false);
    if is_folder_flow
        && std::fs::metadata(root.root().join(&path))
            .map(|m| m.is_dir())
            .unwrap_or(false)
    {
        return files_zip(State(state), headers, Query(query)).await;
    }

    let (file_path, file_size) = match root.resolve_file(&path) {
        Ok(pair) => pair,
        Err(error) => return error_response(error),
    };

    let mut file = match tokio::fs::File::open(&file_path).await {
        Ok(file) => file,
        Err(e) => return error_response(NasError::io(&file_path, e)),
    };

    let range_header = headers
        .get(axum::http::header::RANGE)
        .and_then(|v| v.to_str().ok());

    let byte_range = if let Some(raw_range) = range_header {
        match crate::files::parse_range(raw_range, file_size) {
            Ok(Some(range)) => Some(range),
            Ok(None) => None,
            Err(crate::files::RangeError::Malformed) => None,
            Err(crate::files::RangeError::Unsatisfiable) => {
                let resp = Response::builder()
                    .status(axum::http::StatusCode::RANGE_NOT_SATISFIABLE)
                    .header(
                        axum::http::header::CONTENT_RANGE,
                        format!("bytes */{file_size}"),
                    )
                    .header(axum::http::header::ACCEPT_RANGES, "bytes")
                    .body(axum::body::Body::empty())
                    .unwrap_or_else(|_| server_error("error al responder 416"));
                return resp;
            }
        }
    } else {
        None
    };

    let (status, content_length, content_range, body) = if let Some(range) = byte_range {
        use std::io::SeekFrom;
        use tokio::io::{AsyncReadExt, AsyncSeekExt};
        if let Err(e) = file.seek(SeekFrom::Start(range.start)).await {
            return error_response(NasError::io(&file_path, e));
        }
        let length = range.length();
        let stream = crate::files::ReaderStream::new(file.take(length));
        (
            axum::http::StatusCode::PARTIAL_CONTENT,
            length,
            Some(range.content_range_header()),
            axum::body::Body::from_stream(stream),
        )
    } else {
        let stream = crate::files::ReaderStream::new(file);
        (
            axum::http::StatusCode::OK,
            file_size,
            None,
            axum::body::Body::from_stream(stream),
        )
    };

    let mut builder = Response::builder()
        .status(status)
        .header(axum::http::header::CONTENT_TYPE, "application/octet-stream")
        .header(axum::http::header::CONTENT_LENGTH, content_length)
        .header(axum::http::header::ACCEPT_RANGES, "bytes")
        .header(
            axum::http::header::CONTENT_DISPOSITION,
            safe_content_disposition("attachment", &name),
        )
        .header("content-security-policy", "default-src 'none'; sandbox")
        .header("x-content-type-options", "nosniff");

    if let Some(cr) = content_range {
        builder = builder.header(axum::http::header::CONTENT_RANGE, cr);
    }

    builder
        .body(body)
        .unwrap_or_else(|_| server_error("error al crear respuesta de descarga"))
}

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

/// Descarga una carpeta (o archivo) como ZIP en streaming asíncrono con archivo temporal seguro.
pub async fn files_zip(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<FilesQuery>,
) -> Response {
    if let Err(response) = require_auth(&state, &headers) {
        return *response;
    }
    let root = match file_root_for(&state, query.root.as_deref()) {
        Ok(root) => root,
        Err(response) => return *response,
    };

    let path = query.path;
    let name = path
        .trim_end_matches('/')
        .rsplit('/')
        .next()
        .filter(|name| !name.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| "nas".to_string());

    let (temp_file, file_size) =
        match tokio::task::spawn_blocking(move || root.zip_temp(&path)).await {
            Ok(Ok(pair)) => pair,
            Ok(Err(error)) => return error_response(error),
            Err(_) => return server_error("compresión interrumpida"),
        };

    let tokio_file = match tokio::fs::File::open(temp_file.path()).await {
        Ok(f) => f,
        Err(e) => return error_response(NasError::io(temp_file.path(), e)),
    };

    let zip_filename = format!("{name}.zip");
    let stream = crate::files::CleaningReaderStream::new(tokio_file, temp_file);
    let mut response = Response::new(axum::body::Body::from_stream(stream));
    let response_headers = response.headers_mut();
    response_headers.insert(
        axum::http::header::CONTENT_TYPE,
        HeaderValue::from_static("application/zip"),
    );
    response_headers.insert(
        axum::http::header::CONTENT_LENGTH,
        HeaderValue::from(file_size),
    );
    response_headers.insert(
        axum::http::header::CONTENT_DISPOSITION,
        safe_content_disposition("attachment", &zip_filename),
    );
    response_headers.insert(
        "content-security-policy",
        HeaderValue::from_static("default-src 'none'; sandbox"),
    );
    response
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

/// Listado del explorador con selección de unidad (`/api/files/list`).
pub async fn files_list_rooted(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<FilesQuery>,
) -> Response {
    if let Err(response) = require_auth(&state, &headers) {
        return *response;
    }
    let root = match file_root_for(&state, query.root.as_deref()) {
        Ok(root) => root,
        Err(response) => return *response,
    };
    let path = query.path;

    match tokio::task::spawn_blocking(move || {
        let entries = root.list(&path)?;
        Ok::<_, nas_core::NasError>(crate::legacy::file_listing(&root, &path, &entries))
    })
    .await
    {
        Ok(Ok(payload)) => {
            axum::Json(ApiResponse::ok(payload, "Contenido del directorio")).into_response()
        }
        Ok(Err(error)) => error_response(error),
        Err(_) => server_error("listado interrumpido"),
    }
}
