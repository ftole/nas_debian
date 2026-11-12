use axum::{
    extract::{Query, State},
    http::HeaderMap,
    response::{IntoResponse, Response},
    Router,
};
use serde::Deserialize;

use crate::{
    extractors::AuthenticatedUser,
    routes::{
        common::{error_response, server_error, ApiResponse, ExtQuery},
        files::{file_root_for_async, stream_resolved_file},
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
    _user: AuthenticatedUser,
    Query(query): Query<ExtQuery>,
) -> Response {
    let root = match file_root_for_async(&state, query.root.as_deref()).await {
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
    _user: AuthenticatedUser,
    axum::Json(body): axum::Json<SaveBody>,
) -> Response {
    let root = match file_root_for_async(&state, body.root.as_deref()).await {
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
    _user: AuthenticatedUser,
    headers: HeaderMap,
    Query(query): Query<crate::routes::files::FilesQuery>,
) -> Response {
    let root = match file_root_for_async(&state, query.root.as_deref()).await {
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

    stream_resolved_file(root, path, &headers, content_type, disposition, &name).await
}

/// Router del dominio de previsualización de archivos.
pub fn router() -> Router<crate::AppState> {
    use axum::routing::{get, post};
    Router::new()
        .route("/api/files/raw", get(files_raw))
        .route("/api/files/content", get(files_content))
        .route("/api/files/save", post(files_save))
}
