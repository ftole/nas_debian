use std::path::PathBuf;

use axum::{
    extract::{Multipart, Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use tokio::io::AsyncWriteExt;

use super::super::{file_root_for, file_root_for_async, MAX_UPLOAD_FILE_SIZE};
use crate::{
    extractors::AuthenticatedUser,
    files::{free_and_total_bytes_at, sanitize_filename},
    routes::common::{bad_request, error_response, random_hex, server_error, ApiResponse},
    AppState,
};

const UPLOAD_SPACE_CHECK_INTERVAL_BYTES: u64 = 64 * 1024 * 1024;
const UPLOAD_SPACE_CHECK_MIN_INTERVAL: std::time::Duration = std::time::Duration::from_secs(2);

enum PromoteError {
    AlreadyExists,
    CrossDevice,
    Io(std::io::Error),
}

/// Promueve un archivo temporal a `destination` sin sobrescribir un archivo ya existente
/// (cierra la condición de carrera TOCTOU entre `try_exists` y `rename`).
fn promote_staged_same_fs_exclusive(
    src: &std::path::Path,
    dst: &std::path::Path,
) -> Result<(), PromoteError> {
    match std::fs::hard_link(src, dst) {
        Ok(()) => {
            let _ = std::fs::remove_file(src);
            Ok(())
        }
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Err(PromoteError::AlreadyExists),
        Err(e)
            if e.raw_os_error() == Some(18)
                || e.kind() == std::io::ErrorKind::Unsupported
                || e.kind() == std::io::ErrorKind::PermissionDenied =>
        {
            Err(PromoteError::CrossDevice)
        }
        Err(e) => Err(PromoteError::Io(e)),
    }
}

/// Copia exclusiva (`O_EXCL`) para promoción entre volúmenes distintos (p. ej. cuando `root`
/// llega en el multipart después de `file`).
fn copy_staged_exclusive(src: &std::path::Path, dst: &std::path::Path) -> Result<(), PromoteError> {
    let mut reader = std::fs::File::open(src).map_err(PromoteError::Io)?;
    let mut writer = match std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(dst)
    {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
            return Err(PromoteError::AlreadyExists);
        }
        Err(e) => return Err(PromoteError::Io(e)),
    };
    if let Err(e) = std::io::copy(&mut reader, &mut writer) {
        drop(writer);
        let _ = std::fs::remove_file(dst);
        return Err(PromoteError::Io(e));
    }
    let _ = std::fs::remove_file(src);
    Ok(())
}

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

struct StagedCleanupGuard(Vec<PathBuf>);

impl StagedCleanupGuard {
    fn new() -> Self {
        Self(Vec::new())
    }

    fn push(&mut self, path: PathBuf) {
        self.0.push(path);
    }

    fn cleanup_sync(&mut self) {
        for p in self.0.drain(..) {
            let _ = std::fs::remove_file(p);
        }
    }
}

impl Drop for StagedCleanupGuard {
    fn drop(&mut self) {
        self.cleanup_sync();
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
    let mut staged_guard = StagedCleanupGuard::new();
    let mut saved: Vec<String> = Vec::new();
    let mut total_written_all: u64 = 0;
    let mut last_space_check: u64 = 0;
    let mut last_space_check_at = std::time::Instant::now();

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
        staged_guard.push(temporary.clone());
        let mut file = match tokio::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
            .await
        {
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
                        && last_space_check_at.elapsed() >= UPLOAD_SPACE_CHECK_MIN_INTERVAL
                    {
                        last_space_check = total_written_all;
                        last_space_check_at = std::time::Instant::now();
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
    let root_for_dir = root.clone();
    let target_dir = match state
        .run_system_blocking(move || root_for_dir.resolve_dir(&target_dir_arg))
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
        let destination = target_dir.join(filename);
        let dest_rel = destination
            .strip_prefix(root.root())
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|_| filename.clone());
        if let Some(desc) = root.is_protected_path(&dest_rel) {
            for (temp, _) in &staged_files {
                let _ = tokio::fs::remove_file(temp).await;
            }
            return error_response(nas_core::NasError::Denied(format!(
                "operación denegada: {desc} están protegidos contra escritura y subidas"
            )));
        }
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

    for (temporary, filename) in &staged_files {
        let destination = target_dir.join(filename);
        let src_for_link = temporary.clone();
        let dst_for_link = destination.clone();
        let promote_res = state
            .run_system_blocking(move || {
                promote_staged_same_fs_exclusive(&src_for_link, &dst_for_link)
            })
            .await
            .map_err(|_| PromoteError::Io(std::io::Error::other("promoción interrumpida")))
            .and_then(|r| r);

        match promote_res {
            Ok(()) => {}
            Err(PromoteError::AlreadyExists) => {
                return (
                    StatusCode::CONFLICT,
                    axum::Json(ApiResponse::<serde_json::Value>::error(format!(
                        "el archivo '{filename}' ya existe"
                    ))),
                )
                    .into_response();
            }
            Err(PromoteError::CrossDevice) => {
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
                        return insufficient_storage_response(format!(
                            "espacio insuficiente en el volumen destino para '{}'",
                            filename
                        ));
                    }
                }
                let src_cp = temporary.clone();
                let dst_cp = destination.clone();
                let cp_res = state
                    .run_system_blocking(move || copy_staged_exclusive(&src_cp, &dst_cp))
                    .await
                    .map_err(|_| PromoteError::Io(std::io::Error::other("copia interrumpida")))
                    .and_then(|r| r);
                match cp_res {
                    Ok(()) => {}
                    Err(PromoteError::AlreadyExists) => {
                        return (
                            StatusCode::CONFLICT,
                            axum::Json(ApiResponse::<serde_json::Value>::error(format!(
                                "el archivo '{filename}' ya existe"
                            ))),
                        )
                            .into_response();
                    }
                    Err(PromoteError::Io(cp_err)) => {
                        return server_error(&format!("no se pudo promover la carga: {cp_err}"));
                    }
                    Err(PromoteError::CrossDevice) => {
                        return server_error("no se pudo promover la carga entre volúmenes");
                    }
                }
            }
            Err(PromoteError::Io(err)) => {
                return server_error(&format!("no se pudo promover la carga: {err}"));
            }
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
