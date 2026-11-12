mod mutations;

use axum::{
    extract::{Query, State},
    http::{HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    Router,
};
use nas_core::NasError;
use serde::Deserialize;

use crate::{
    files::FileRoot,
    routes::common::{
        bad_request, error_response, forbidden, require_auth, safe_content_disposition,
        server_error, ApiResponse,
    },
    AppState,
};

pub use mutations::{files_create, files_mkdir, files_move, files_rename, files_upload};

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
    if is_folder_flow {
        let check_root = root.clone();
        let check_path = path.clone();
        let is_dir = tokio::task::spawn_blocking(move || {
            check_root
                .resolve(&check_path)
                .map(|resolved| resolved.is_dir())
                .unwrap_or(false)
        })
        .await
        .unwrap_or(false);
        if is_dir {
            return files_zip(State(state), headers, Query(query)).await;
        }
    }

    let resolve_root = root;
    let resolve_path = path;
    let (file_path, file_size) =
        match tokio::task::spawn_blocking(move || resolve_root.resolve_file(&resolve_path)).await {
            Ok(Ok(pair)) => pair,
            Ok(Err(error)) => return error_response(error),
            Err(_) => return server_error("resolución de archivo interrumpida"),
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

/// Router del dominio de archivos (parte 1).
pub fn router() -> Router<crate::AppState> {
    use axum::routing::{get, post};
    Router::new()
        .route("/api/files", get(files_list))
        .route("/api/files/download", get(files_download))
        .route("/api/files/zip", get(files_zip))
        .route("/api/files/list", get(files_list_rooted))
        .route("/api/files/create", post(files_create))
        .route("/api/files/move", post(files_move))
        .route("/api/files/mkdir", post(files_mkdir))
        .route("/api/files/rename", post(files_rename))
}
