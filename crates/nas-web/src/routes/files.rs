mod mutations;

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use axum::{
    extract::{Query, State},
    http::{HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    Router,
};
use nas_core::NasError;
use serde::Deserialize;

use crate::{
    extractors::AuthenticatedUser,
    files::{FileRoot, TempFile},
    routes::common::{
        bad_request, error_response, forbidden, random_hex, safe_content_disposition, server_error,
        ApiResponse,
    },
    AppState,
};

struct PreparedZip {
    temp_file: TempFile,
    filename: String,
    size: u64,
    owner: String,
    created_at: Instant,
}

const PREPARED_ZIP_TTL: Duration = Duration::from_secs(600);
const PREPARED_ZIP_MAX_ENTRIES: usize = 8;

fn prepared_zips() -> &'static Mutex<HashMap<String, PreparedZip>> {
    static STORE: OnceLock<Mutex<HashMap<String, PreparedZip>>> = OnceLock::new();
    STORE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn store_prepared_zip(owner: &str, temp_file: TempFile, filename: String, size: u64) -> String {
    let token = random_hex();
    let now = Instant::now();
    if let Ok(mut guard) = prepared_zips().lock() {
        guard.retain(|_, entry| now.duration_since(entry.created_at) < PREPARED_ZIP_TTL);
        while guard.len() >= PREPARED_ZIP_MAX_ENTRIES {
            let oldest_key = guard
                .iter()
                .min_by_key(|(_, entry)| entry.created_at)
                .map(|(k, _)| k.clone());
            if let Some(k) = oldest_key {
                guard.remove(&k);
            } else {
                break;
            }
        }
        guard.insert(
            token.clone(),
            PreparedZip {
                temp_file,
                filename,
                size,
                owner: owner.to_string(),
                created_at: now,
            },
        );
    }
    token
}

fn take_prepared_zip(token: &str, owner: &str) -> Option<PreparedZip> {
    let now = Instant::now();
    let mut guard = prepared_zips().lock().ok()?;
    guard.retain(|_, entry| now.duration_since(entry.created_at) < PREPARED_ZIP_TTL);
    let matches_owner = guard.get(token).is_some_and(|entry| entry.owner == owner);
    if matches_owner {
        guard.remove(token)
    } else {
        None
    }
}

async fn stream_zip_temp(temp_file: TempFile, file_size: u64, zip_filename: &str) -> Response {
    let tokio_file = match tokio::fs::File::open(temp_file.path()).await {
        Ok(f) => f,
        Err(e) => return error_response(NasError::io(temp_file.path(), e)),
    };

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
        safe_content_disposition("attachment", zip_filename),
    );
    response_headers.insert(
        "content-security-policy",
        HeaderValue::from_static("default-src 'none'; sandbox"),
    );
    response
}

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
    /// Token asociado a un ZIP ya comprimido en staging (descarga directa sin `res.blob()`).
    #[serde(default)]
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
        let unit_id = requested.strip_prefix("unidades/").unwrap_or(requested);
        if !nas_engine::storage_ops::valid_name(unit_id) {
            return Err(Box::new(bad_request("unidad de almacenamiento inválida")));
        }
        let unit = state.paths.data_root.join("unidades").join(unit_id);
        if !unit.is_dir() {
            return Err(Box::new(not_found_response(
                "la unidad solicitada no existe",
            )));
        }
        unit
    };
    FileRoot::new(base).map_err(|error| Box::new(server_error(&error.to_string())))
}

pub async fn file_root_for_async(
    state: &AppState,
    root: Option<&str>,
) -> Result<FileRoot, Box<Response>> {
    let state_clone = state.clone();
    let root = root.map(str::to_string);
    match state
        .run_system_blocking(move || file_root_for(&state_clone, root.as_deref()))
        .await
    {
        Ok(res) => res,
        Err(_) => Err(Box::new(server_error("resolución de raíz interrumpida"))),
    }
}

pub async fn stream_resolved_file(
    state: &AppState,
    root: FileRoot,
    path: String,
    headers: &HeaderMap,
    content_type: &str,
    disposition: &str,
    name: &str,
) -> Response {
    let (file_path, file_size) = match state
        .run_system_blocking(move || root.resolve_file(&path))
        .await
    {
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
                return Response::builder()
                    .status(axum::http::StatusCode::RANGE_NOT_SATISFIABLE)
                    .header(
                        axum::http::header::CONTENT_RANGE,
                        format!("bytes */{file_size}"),
                    )
                    .header(axum::http::header::ACCEPT_RANGES, "bytes")
                    .body(axum::body::Body::empty())
                    .unwrap_or_else(|_| server_error("error al responder 416"));
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
            safe_content_disposition(disposition, name),
        )
        .header("content-security-policy", "default-src 'none'; sandbox")
        .header("x-content-type-options", "nosniff");

    if let Some(cr) = content_range {
        builder = builder.header(axum::http::header::CONTENT_RANGE, cr);
    }

    builder
        .body(body)
        .unwrap_or_else(|_| server_error("error al crear respuesta de archivo"))
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
    _user: AuthenticatedUser,
    Query(query): Query<FilesQuery>,
) -> Response {
    let root = match file_root_for_async(&state, query.root.as_deref()).await {
        Ok(root) => root,
        Err(response) => return *response,
    };
    let path = query.path;

    match state
        .run_system_blocking(move || {
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
    user: AuthenticatedUser,
    headers: HeaderMap,
    Query(query): Query<FilesQuery>,
) -> Response {
    // 1. Entrega en streaming directo de un ZIP previamente preparado por token.
    if let Some(token) = query
        .token
        .as_deref()
        .map(str::trim)
        .filter(|t| !t.is_empty())
    {
        if let Some(prepared) = take_prepared_zip(token, user.username()) {
            return stream_zip_temp(prepared.temp_file, prepared.size, &prepared.filename).await;
        }
        return not_found_response("el enlace temporal de descarga ZIP expiró o no es válido");
    }

    let root = match file_root_for_async(&state, query.root.as_deref()).await {
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

    // 2. Solicitud de preparación previa de ZIP (`prepare=1`): genera el ZIP en staging
    //    y devuelve un token JSON de un solo uso para descarga directa en streaming sin RAM.
    let is_folder_flow = query
        .prepare
        .as_deref()
        .map(|value| !value.is_empty() && value != "0")
        .unwrap_or(false);
    if is_folder_flow {
        let check_root = root.clone();
        let check_path = path.clone();
        let is_dir = state
            .run_system_blocking(move || {
                check_root
                    .resolve(&check_path)
                    .map(|resolved| resolved.is_dir())
                    .unwrap_or(false)
            })
            .await
            .unwrap_or(false);
        if is_dir {
            let zip_name = path
                .trim_end_matches('/')
                .rsplit('/')
                .next()
                .filter(|n| !n.is_empty())
                .map(str::to_string)
                .unwrap_or_else(|| "nas".to_string());
            let zip_filename = format!("{zip_name}.zip");
            let state_dir = state.paths.state_dir.clone();
            let data_root = state.paths.data_root.clone();
            let zip_path = path.clone();
            let zip_res = state
                .run_heavy_blocking(move || {
                    root.zip_temp_in(&zip_path, Some(&state_dir), Some(&data_root))
                })
                .await;
            let (temp_file, file_size) = match zip_res {
                Ok(Ok(pair)) => pair,
                Ok(Err(error)) => return error_response(error),
                Err(_) => return server_error("compresión interrumpida"),
            };
            let token =
                store_prepared_zip(user.username(), temp_file, zip_filename.clone(), file_size);
            return axum::Json(serde_json::json!({
                "success": true,
                "token": token,
                "filename": zip_filename,
                "size": file_size,
                "size_formatted": crate::legacy::human_bytes(file_size),
            }))
            .into_response();
        }
    }

    stream_resolved_file(
        &state,
        root,
        path,
        &headers,
        "application/octet-stream",
        "attachment",
        &name,
    )
    .await
}

/// Descarga una carpeta (o archivo) como ZIP en streaming asíncrono con archivo temporal seguro.
pub async fn files_zip(
    State(state): State<AppState>,
    _user: AuthenticatedUser,
    Query(query): Query<FilesQuery>,
) -> Response {
    let root = match file_root_for_async(&state, query.root.as_deref()).await {
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

    let state_dir = state.paths.state_dir.clone();
    let data_root = state.paths.data_root.clone();
    let zip_res = state
        .run_heavy_blocking(move || root.zip_temp_in(&path, Some(&state_dir), Some(&data_root)))
        .await;
    let (temp_file, file_size) = match zip_res {
        Ok(Ok(pair)) => pair,
        Ok(Err(error)) => return error_response(error),
        Err(_) => return server_error("compresión interrumpida"),
    };

    let zip_filename = format!("{name}.zip");
    stream_zip_temp(temp_file, file_size, &zip_filename).await
}

/// Listado del explorador con selección de unidad (`/api/files/list`).
pub async fn files_list_rooted(
    State(state): State<AppState>,
    _user: AuthenticatedUser,
    Query(query): Query<FilesQuery>,
) -> Response {
    let root = match file_root_for_async(&state, query.root.as_deref()).await {
        Ok(root) => root,
        Err(response) => return *response,
    };
    let path = query.path;

    match state
        .run_system_blocking(move || {
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
