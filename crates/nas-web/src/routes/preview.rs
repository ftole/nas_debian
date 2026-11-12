use axum::{
    extract::{Query, State},
    http::HeaderMap,
    response::{IntoResponse, Response},
    Router,
};
use nas_core::NasError;
use serde::Deserialize;

use crate::{
    routes::{
        common::{
            error_response, require_auth, safe_content_disposition, server_error, ApiResponse,
            ExtQuery,
        },
        files::file_root_for,
    },
    AppState,
};

#[derive(Debug, Deserialize)]
pub struct SaveBody {
    pub path: String,
    #[serde(default)]
    pub content: String,
    #[serde(default)]
    pub root: Option<String>,
}

pub fn format_bytes_human(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut size = bytes as f64;
    let mut idx = 0usize;
    while size >= 1024.0 && idx + 1 < UNITS.len() {
        size /= 1024.0;
        idx += 1;
    }
    if idx == 0 {
        format!("{bytes} B")
    } else {
        format!("{size:.1} {}", UNITS[idx])
    }
}

pub fn is_active_web_payload(name: &str) -> bool {
    let extension = name
        .rsplit('.')
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    matches!(
        extension.as_str(),
        "svg" | "svgz" | "html" | "htm" | "xhtml" | "xml" | "mhtml" | "xsl" | "xslt"
    )
}

pub fn inline_content_type(name: &str) -> &'static str {
    let extension = name
        .rsplit('.')
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    match extension.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "bmp" => "image/bmp",
        "ico" => "image/x-icon",
        "pdf" => "application/pdf",
        "mp4" => "video/mp4",
        "webm" => "video/webm",
        "mp3" => "audio/mpeg",
        "wav" => "audio/wav",
        "ogg" => "audio/ogg",
        "txt" => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}

/// Contenido de un archivo de texto.
pub async fn files_content(
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
    let path = query.path.unwrap_or_default();

    match tokio::task::spawn_blocking(move || root.read(&path, 2 * 1024 * 1024)).await {
        Ok(Ok(bytes)) => {
            let content = String::from_utf8_lossy(&bytes).to_string();
            let lines = content.lines().count();
            axum::Json(ApiResponse::ok(
                serde_json::json!({ "content": content, "lines_count": lines }),
                "Contenido del archivo",
            ))
            .into_response()
        }
        Ok(Err(error)) => error_response(error),
        Err(_) => server_error("lectura interrumpida"),
    }
}

/// Guarda el contenido de un archivo.
pub async fn files_save(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::Json(body): axum::Json<SaveBody>,
) -> Response {
    if let Err(response) = require_auth(&state, &headers) {
        return *response;
    }
    let root = match file_root_for(&state, body.root.as_deref()) {
        Ok(root) => root,
        Err(response) => return *response,
    };
    match tokio::task::spawn_blocking(move || root.write(&body.path, &body.content)).await {
        Ok(Ok(bytes)) => {
            let formatted = format_bytes_human(bytes);
            axum::Json(serde_json::json!({
                "success": true,
                "message": "Archivo guardado",
                "size_formatted": formatted,
                "data": {
                    "bytes": bytes,
                    "size_formatted": formatted,
                }
            }))
            .into_response()
        }
        Ok(Err(error)) => error_response(error),
        Err(_) => server_error("guardado interrumpido"),
    }
}

/// Sirve un archivo para previsualización en línea (`/api/files/raw`) aislando vectores XSS mediante streaming O(1).
pub async fn files_raw(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<crate::routes::files::FilesQuery>,
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
        .rsplit('/')
        .next()
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| "archivo.bin".to_string());

    let (file_path, file_size) = match root.resolve_file(&path) {
        Ok(pair) => pair,
        Err(error) => return error_response(error),
    };

    let mut file = match tokio::fs::File::open(&file_path).await {
        Ok(file) => file,
        Err(e) => return error_response(NasError::io(&file_path, e)),
    };

    let force_attachment = is_active_web_payload(&name);
    let content_type = if force_attachment {
        "application/octet-stream"
    } else {
        inline_content_type(&name)
    };
    let disposition = if force_attachment {
        "attachment"
    } else {
        "inline"
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
        .header(axum::http::header::CONTENT_TYPE, content_type)
        .header(axum::http::header::CONTENT_LENGTH, content_length)
        .header(axum::http::header::ACCEPT_RANGES, "bytes")
        .header(
            axum::http::header::CONTENT_DISPOSITION,
            safe_content_disposition(disposition, &name),
        )
        .header("content-security-policy", "default-src 'none'; sandbox")
        .header("x-content-type-options", "nosniff");

    if let Some(cr) = content_range {
        builder = builder.header(axum::http::header::CONTENT_RANGE, cr);
    }

    builder
        .body(body)
        .unwrap_or_else(|_| server_error("error al crear respuesta raw"))
}

/// Router del dominio de previsualización de archivos.
pub fn router() -> Router<crate::AppState> {
    use axum::routing::{get, post};
    Router::new()
        .route("/api/files/raw", get(files_raw))
        .route("/api/files/content", get(files_content))
        .route("/api/files/save", post(files_save))
}
