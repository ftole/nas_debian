//! Manejadores HTTP del panel.

use axum::extract::{Multipart, Query, State};
use axum::http::header::{COOKIE, SET_COOKIE};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::{Html, IntoResponse, Redirect, Response};
use axum::Form;
use chrono::Utc;
use nas_core::{ApiResponse, NasError};
use nas_engine::parsers::smb::parse_shares;
use nas_engine::{collect_network, collect_status};
use nas_exec::Runner;
use serde::Deserialize;
use tokio::io::AsyncWriteExt;

use crate::files::{sanitize_filename, FileRoot};
use crate::views;
use crate::AppState;

/// Sonda de vida usada por systemd/nginx.
pub async fn healthz() -> &'static str {
    "ok"
}

fn hostname() -> String {
    std::fs::read_to_string("/etc/hostname")
        .map(|value| value.trim().to_string())
        .ok()
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "SRV-NAS".to_string())
}

/// Formulario de acceso.
pub async fn login_form() -> Html<String> {
    Html(views::login(&hostname(), None))
}

/// Credenciales enviadas por el formulario.
#[derive(Debug, Deserialize)]
pub struct LoginForm {
    username: String,
    password: String,
}

/// Procesa el acceso y emite la cookie de sesión.
pub async fn login_submit(State(state): State<AppState>, Form(form): Form<LoginForm>) -> Response {
    let now = Utc::now().timestamp();
    let session = match state.auth.login(&form.username, &form.password, now) {
        Ok(Some(session)) => {
            let _ = state
                .auth
                .db()
                .append_audit(&form.username, "login", "web", "SUCCESS", "");
            session
        }
        Ok(None) => {
            let _ = state
                .auth
                .db()
                .append_audit(&form.username, "login", "web", "FAILED", "");
            return (
                StatusCode::UNAUTHORIZED,
                Html(views::login(
                    &hostname(),
                    Some("Usuario o contraseña incorrectos."),
                )),
            )
                .into_response();
        }
        Err(error) => return server_error(&error.to_string()),
    };

    let mut response = Redirect::to("/").into_response();
    if let Ok(header) = HeaderValue::from_str(&state.auth.set_cookie_header(&session)) {
        response.headers_mut().insert(SET_COOKIE, header);
    }
    response
}

/// Cierra la sesión eliminando la cookie y el registro en la base de datos.
pub async fn logout(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let _ = state.auth.logout(cookie_header(&headers));

    let mut response = Redirect::to("/login").into_response();
    if let Ok(header) = HeaderValue::from_str(&state.auth.clear_cookie_header()) {
        response.headers_mut().insert(SET_COOKIE, header);
    }
    response
}

/// Raíz: redirige al dashboard autenticado.
pub async fn dashboard(State(state): State<AppState>, headers: HeaderMap) -> Response {
    match require_auth(&state, &headers) {
        Ok(_) => Redirect::to("/dashboard").into_response(),
        Err(_) => Redirect::to("/login").into_response(),
    }
}

/// Métricas del sistema en JSON (contrato compartido).
pub async fn metrics(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if let Err(response) = require_auth(&state, &headers) {
        return *response;
    }

    match tokio::task::spawn_blocking(|| collect_status(&Runner::new())).await {
        Ok(metrics) => axum::Json(ApiResponse::ok(metrics, "Métricas del sistema")).into_response(),
        Err(_) => server_error("no se pudieron recolectar las métricas"),
    }
}

/// Inventario de red en JSON.
pub async fn network(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if let Err(response) = require_auth(&state, &headers) {
        return *response;
    }

    let paths = state.paths.clone();
    match tokio::task::spawn_blocking(move || collect_network(&Runner::new(), &paths)).await {
        Ok(overview) => axum::Json(ApiResponse::ok(overview, "Inventario de red")).into_response(),
        Err(_) => server_error("no se pudo recolectar el inventario de red"),
    }
}

/// Recursos compartidos declarados en `smb.conf`.
pub async fn shares(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if let Err(response) = require_auth(&state, &headers) {
        return *response;
    }

    if !state.role.has_shares() {
        return (
            StatusCode::FORBIDDEN,
            axum::Json(ApiResponse::<serde_json::Value>::error(
                "el rol BACKUP no expone recursos compartidos",
            )),
        )
            .into_response();
    }

    let path = state.paths.samba_conf.clone();
    match tokio::task::spawn_blocking(move || std::fs::read_to_string(&path)).await {
        Ok(Ok(text)) => {
            axum::Json(ApiResponse::ok(parse_shares(&text), "Recursos compartidos")).into_response()
        }
        Ok(Err(error)) => server_error(&format!("no se pudo leer smb.conf: {error}")),
        Err(_) => server_error("lectura de smb.conf interrumpida"),
    }
}

/// Usuarios administrables y grupos corporativos.
pub async fn users(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if let Err(response) = require_auth(&state, &headers) {
        return *response;
    }

    match tokio::task::spawn_blocking(|| {
        let (mut users, mut groups) = nas_engine::users::load_local();
        users.retain(|user| !user.system);
        groups.retain(|group| group.name.starts_with("grp_"));
        serde_json::json!({ "users": users, "groups": groups })
    })
    .await
    {
        Ok(payload) => axum::Json(ApiResponse::ok(payload, "Usuarios y grupos")).into_response(),
        Err(_) => server_error("no se pudo leer el inventario de usuarios"),
    }
}

/// Parámetro `?path=` del explorador.
#[derive(Debug, Deserialize)]
pub struct FilesQuery {
    #[serde(default)]
    path: String,
}

/// Cuerpo con una única ruta.
#[derive(Debug, Deserialize)]
pub struct PathBody {
    path: String,
}

/// Cuerpo para renombrar o mover.
#[derive(Debug, Deserialize)]
pub struct RenameBody {
    from: String,
    to: String,
}

fn file_root(state: &AppState) -> Result<FileRoot, Box<Response>> {
    FileRoot::new(state.paths.data_root.clone())
        .map_err(|error| Box::new(server_error(&error.to_string())))
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
    let root = match file_root(&state) {
        Ok(root) => root,
        Err(response) => return *response,
    };
    let path = query.path;

    match tokio::task::spawn_blocking(move || root.list(&path)).await {
        Ok(Ok(entries)) => {
            axum::Json(ApiResponse::ok(entries, "Contenido del directorio")).into_response()
        }
        Ok(Err(error)) => error_response(error),
        Err(_) => server_error("listado interrumpido"),
    }
}

/// Descarga un archivo con cabeceras seguras.
pub async fn files_download(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<FilesQuery>,
) -> Response {
    if let Err(response) = require_auth(&state, &headers) {
        return *response;
    }
    let root = match file_root(&state) {
        Ok(root) => root,
        Err(response) => return *response,
    };
    let path = query.path;
    let name = path
        .rsplit('/')
        .next()
        .map(str::to_string)
        .unwrap_or_else(|| "descarga.bin".to_string());

    match tokio::task::spawn_blocking(move || root.read(&path, 64 * 1024 * 1024)).await {
        Ok(Ok(bytes)) => {
            let mut response = Response::new(axum::body::Body::from(bytes));
            let response_headers = response.headers_mut();
            response_headers.insert(
                axum::http::header::CONTENT_TYPE,
                HeaderValue::from_static("application/octet-stream"),
            );
            if let Ok(value) = HeaderValue::from_str(&format!(
                "attachment; filename=\"{}\"",
                name.replace('"', "")
            )) {
                response_headers.insert(axum::http::header::CONTENT_DISPOSITION, value);
            }
            response_headers.insert(
                "content-security-policy",
                HeaderValue::from_static("default-src 'none'; sandbox"),
            );
            response
        }
        Ok(Err(error)) => error_response(error),
        Err(_) => server_error("descarga interrumpida"),
    }
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
    let root = match file_root(&state) {
        Ok(root) => root,
        Err(response) => return *response,
    };
    let path = body.path;

    match tokio::task::spawn_blocking(move || root.mkdir(&path)).await {
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
    let root = match file_root(&state) {
        Ok(root) => root,
        Err(response) => return *response,
    };

    match tokio::task::spawn_blocking(move || root.rename(&body.from, &body.to)).await {
        Ok(Ok(_)) => axum::Json(ApiResponse::<serde_json::Value>::ok_empty(
            "Elemento renombrado",
        ))
        .into_response(),
        Ok(Err(error)) => error_response(error),
        Err(_) => server_error("renombrado interrumpido"),
    }
}

/// Envía un elemento a la papelera interna.
pub async fn files_delete(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::Json(body): axum::Json<PathBody>,
) -> Response {
    if let Err(response) = require_auth(&state, &headers) {
        return *response;
    }
    let root = match file_root(&state) {
        Ok(root) => root,
        Err(response) => return *response,
    };
    let path = body.path;

    let user = state
        .auth
        .authenticate(cookie_header(&headers), Utc::now().timestamp())
        .ok()
        .flatten()
        .unwrap_or_default();
    let audit_path = path.clone();

    match tokio::task::spawn_blocking(move || root.trash(&path)).await {
        Ok(Ok(target)) => {
            let _ = state
                .auth
                .db()
                .append_audit(&user, "file_delete", &audit_path, "SUCCESS", "");
            axum::Json(ApiResponse::ok(
                serde_json::json!({ "trash": target.display().to_string() }),
                "Elemento movido a la papelera",
            ))
            .into_response()
        }
        Ok(Err(error)) => error_response(error),
        Err(_) => server_error("borrado interrumpido"),
    }
}

/// Sube uno o varios archivos a un directorio, escribiendo por streaming.
pub async fn files_upload(
    State(state): State<AppState>,
    headers: HeaderMap,
    mut multipart: Multipart,
) -> Response {
    if let Err(response) = require_auth(&state, &headers) {
        return *response;
    }
    let root = match file_root(&state) {
        Ok(root) => root,
        Err(response) => return *response,
    };

    let mut directory = String::new();
    let mut saved: Vec<String> = Vec::new();

    loop {
        let field = match multipart.next_field().await {
            Ok(Some(field)) => field,
            Ok(None) => break,
            Err(error) => return bad_request(&format!("multipart inválido: {error}")),
        };

        let name = field.name().unwrap_or_default().to_string();
        if name == "path" {
            match field.text().await {
                Ok(text) => directory = text,
                Err(error) => return bad_request(&format!("campo path inválido: {error}")),
            }
            continue;
        }
        if name != "file" {
            continue;
        }

        let filename = match sanitize_filename(field.file_name().unwrap_or_default()) {
            Ok(name) => name,
            Err(error) => return error_response(error),
        };
        let target_dir = match root.resolve_dir(&directory) {
            Ok(dir) => dir,
            Err(error) => return error_response(error),
        };
        let destination = target_dir.join(&filename);
        if destination.exists() {
            return (
                StatusCode::CONFLICT,
                axum::Json(ApiResponse::<serde_json::Value>::error(format!(
                    "el archivo '{filename}' ya existe"
                ))),
            )
                .into_response();
        }

        let temporary = target_dir.join(format!(".upload-{}", random_hex()));
        let mut file = match tokio::fs::File::create(&temporary).await {
            Ok(file) => file,
            Err(error) => return server_error(&format!("no se pudo crear el temporal: {error}")),
        };

        let mut field = field;
        let mut total: u64 = 0;
        loop {
            match field.chunk().await {
                Ok(Some(chunk)) => {
                    total += chunk.len() as u64;
                    if total > 512 * 1024 * 1024 {
                        let _ = tokio::fs::remove_file(&temporary).await;
                        return (
                            StatusCode::PAYLOAD_TOO_LARGE,
                            axum::Json(ApiResponse::<serde_json::Value>::error(
                                "el archivo supera el límite de 512 MB",
                            )),
                        )
                            .into_response();
                    }
                    if let Err(error) = file.write_all(&chunk).await {
                        let _ = tokio::fs::remove_file(&temporary).await;
                        return server_error(&format!("fallo al escribir: {error}"));
                    }
                }
                Ok(None) => break,
                Err(error) => {
                    let _ = tokio::fs::remove_file(&temporary).await;
                    return bad_request(&format!("carga interrumpida: {error}"));
                }
            }
        }

        if let Err(error) = file.flush().await {
            let _ = tokio::fs::remove_file(&temporary).await;
            return server_error(&format!("fallo al sincronizar: {error}"));
        }
        drop(file);

        if let Err(error) = tokio::fs::rename(&temporary, &destination).await {
            let _ = tokio::fs::remove_file(&temporary).await;
            return server_error(&format!("no se pudo promover la carga: {error}"));
        }

        saved.push(if directory.is_empty() {
            filename
        } else {
            format!("{directory}/{filename}")
        });
    }

    axum::Json(ApiResponse::ok(
        serde_json::json!({ "uploaded": saved }),
        "Carga completada",
    ))
    .into_response()
}

fn random_hex() -> String {
    let mut buffer = [0u8; 8];
    let _ = getrandom::getrandom(&mut buffer);
    hex::encode(buffer)
}

fn bad_request(message: &str) -> Response {
    (
        StatusCode::BAD_REQUEST,
        axum::Json(ApiResponse::<serde_json::Value>::error(message)),
    )
        .into_response()
}

fn error_response(error: NasError) -> Response {
    let status = match error {
        NasError::Denied(_) => StatusCode::FORBIDDEN,
        NasError::NotFound(_) => StatusCode::NOT_FOUND,
        NasError::Invalid(_) => StatusCode::BAD_REQUEST,
        _ => StatusCode::INTERNAL_SERVER_ERROR,
    };

    (
        status,
        axum::Json(ApiResponse::<serde_json::Value>::error(error.to_string())),
    )
        .into_response()
}

fn cookie_header(headers: &HeaderMap) -> &str {
    headers
        .get(COOKIE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
}

fn require_auth(state: &AppState, headers: &HeaderMap) -> Result<String, Box<Response>> {
    match state
        .auth
        .authenticate(cookie_header(headers), Utc::now().timestamp())
    {
        Ok(Some(user)) => Ok(user),
        Ok(None) => Err(Box::new(unauthorized())),
        Err(error) => Err(Box::new(server_error(&error.to_string()))),
    }
}

fn unauthorized() -> Response {
    (
        StatusCode::UNAUTHORIZED,
        axum::Json(ApiResponse::<serde_json::Value>::error(
            "sesión no válida o expirada",
        )),
    )
        .into_response()
}

fn server_error(message: &str) -> Response {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        axum::Json(ApiResponse::<serde_json::Value>::error(message)),
    )
        .into_response()
}

/// Cuerpo para gestionar un servicio.
#[derive(Debug, Deserialize)]
pub struct ServiceBody {
    service: String,
    action: String,
}

/// Parámetro `?limit=` de la auditoría.
#[derive(Debug, Deserialize)]
pub struct AuditQuery {
    #[serde(default = "default_audit_limit")]
    limit: usize,
}

fn default_audit_limit() -> usize {
    50
}

/// Estado de los demonios clave.
pub async fn services(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if let Err(response) = require_auth(&state, &headers) {
        return *response;
    }

    match tokio::task::spawn_blocking(|| {
        nas_engine::services::collect(&Runner::new(), nas_engine::services::DEFAULT_SERVICES)
    })
    .await
    {
        Ok(list) => axum::Json(ApiResponse::ok(list, "Estado de servicios")).into_response(),
        Err(_) => server_error("consulta de servicios interrumpida"),
    }
}

/// Reinicia o recarga un servicio de la lista blanca.
pub async fn service_manage(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::Json(body): axum::Json<ServiceBody>,
) -> Response {
    let user = match require_auth(&state, &headers) {
        Ok(user) => user,
        Err(response) => return *response,
    };
    if !is_admin(&state, &user) {
        return (
            StatusCode::FORBIDDEN,
            axum::Json(ApiResponse::<serde_json::Value>::error(
                "se requieren privilegios de administrador",
            )),
        )
            .into_response();
    }
    if !nas_engine::services::is_allowed_service(&body.service)
        || !nas_engine::services::is_allowed_action(&body.action)
    {
        let _ = state.auth.db().append_audit(
            &user,
            "service_manage",
            &body.service,
            "DENIED",
            &body.action,
        );
        return bad_request("servicio o acción no permitidos");
    }

    let service = body.service.clone();
    let action = body.action.clone();
    let outcome = tokio::task::spawn_blocking(move || {
        nas_engine::services::manage(&Runner::new(), &service, &action, true)
    })
    .await
    .unwrap_or_else(|_| Err("ejecución interrumpida".to_string()));

    match outcome {
        Ok(()) => {
            let _ = state.auth.db().append_audit(
                &user,
                "service_manage",
                &body.service,
                "SUCCESS",
                &body.action,
            );
            axum::Json(ApiResponse::<serde_json::Value>::ok_empty(format!(
                "{} {} ejecutado",
                body.action, body.service
            )))
            .into_response()
        }
        Err(error) => {
            let _ = state.auth.db().append_audit(
                &user,
                "service_manage",
                &body.service,
                "FAILED",
                &error,
            );
            server_error(&error)
        }
    }
}

/// Diagnóstico integral del servidor.
pub async fn diagnostics(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if let Err(response) = require_auth(&state, &headers) {
        return *response;
    }

    let paths = state.paths.clone();
    match tokio::task::spawn_blocking(move || {
        nas_engine::diagnostics::collect(&paths, &Runner::new())
    })
    .await
    {
        Ok(checks) => axum::Json(ApiResponse::ok(checks, "Diagnóstico integral")).into_response(),
        Err(_) => server_error("diagnóstico interrumpido"),
    }
}

/// Lista las tareas de respaldo instaladas con su último snapshot.
pub async fn backups(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if let Err(response) = require_auth(&state, &headers) {
        return *response;
    }

    let dir = state.backup_dir.clone();
    match tokio::task::spawn_blocking(move || {
        let tasks = nas_engine::tasks::BackupTask::discover(&dir).unwrap_or_default();
        tasks
            .into_iter()
            .map(|task| {
                let last = last_snapshot(&task);
                serde_json::json!({
                    "task": task,
                    "last_snapshot": last,
                })
            })
            .collect::<Vec<_>>()
    })
    .await
    {
        Ok(list) => axum::Json(ApiResponse::ok(list, "Tareas de respaldo")).into_response(),
        Err(_) => server_error("consulta de respaldos interrumpida"),
    }
}

/// Plan de ejecución de una tarea de respaldo.
pub async fn backup_plan(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::Json(body): axum::Json<PathBody>,
) -> Response {
    if let Err(response) = require_auth(&state, &headers) {
        return *response;
    }

    let dir = state.backup_dir.clone();
    let id = body.path;
    match tokio::task::spawn_blocking(move || {
        let tasks = nas_engine::tasks::BackupTask::discover(&dir)?;
        let task = tasks
            .into_iter()
            .find(|task| task.id == id)
            .ok_or_else(|| nas_core::NasError::NotFound(format!("tarea '{id}' no encontrada")))?;

        let problems = task.validate();
        if !problems.is_empty() {
            return Err(nas_core::NasError::Invalid(problems.join("; ")));
        }

        let previous = last_snapshot_path(&task);
        let plan = task.plan(previous.as_deref(), &chrono::Local::now());
        let steps: Vec<String> = plan
            .iter()
            .map(|step| nas_core::shell::quote_all(step))
            .collect();

        Ok::<_, nas_core::NasError>(serde_json::json!({
            "task": task,
            "last_snapshot": previous.map(|path| path.display().to_string()),
            "steps": steps,
        }))
    })
    .await
    {
        Ok(Ok(payload)) => axum::Json(ApiResponse::ok(payload, "Plan de respaldo")).into_response(),
        Ok(Err(error)) => error_response(error),
        Err(_) => server_error("planificación interrumpida"),
    }
}

/// Últimas entradas de auditoría del panel.
pub async fn audit(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<AuditQuery>,
) -> Response {
    let user = match require_auth(&state, &headers) {
        Ok(user) => user,
        Err(response) => return *response,
    };
    if !is_admin(&state, &user) {
        return (
            StatusCode::FORBIDDEN,
            axum::Json(ApiResponse::<serde_json::Value>::error(
                "se requieren privilegios de administrador",
            )),
        )
            .into_response();
    }

    match state.auth.db().recent_audit(query.limit.clamp(1, 500)) {
        Ok(rows) => axum::Json(ApiResponse::ok(rows, "Auditoría del panel")).into_response(),
        Err(error) => error_response(error),
    }
}

fn is_admin(state: &AppState, user: &str) -> bool {
    state
        .auth
        .db()
        .user_role(user)
        .ok()
        .flatten()
        .is_some_and(|role| matches!(role.as_str(), "admin" | "superadmin"))
}

fn last_snapshot(task: &nas_engine::tasks::BackupTask) -> Option<String> {
    last_snapshot_path(task).map(|path| path.display().to_string())
}

fn last_snapshot_path(task: &nas_engine::tasks::BackupTask) -> Option<std::path::PathBuf> {
    let entries = std::fs::read_dir(task.task_dir()).ok()?;
    let mut snapshots: Vec<std::path::PathBuf> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("snapshot_"))
        })
        .collect();
    snapshots.sort();
    snapshots.pop()
}

/// Cuerpo para inspeccionar un dispositivo.
#[derive(Debug, Deserialize)]
pub struct DeviceBody {
    device: String,
}

/// Cuerpo para unirse a un dominio.
#[derive(Debug, Deserialize)]
pub struct DomainJoinBody {
    domain: String,
    user: String,
    password: String,
}

/// Resumen del volumen de datos.
pub async fn storage(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if let Err(response) = require_auth(&state, &headers) {
        return *response;
    }

    let paths = state.paths.clone();
    match tokio::task::spawn_blocking(move || {
        nas_engine::storage::overview_for(&paths, &Runner::new())
    })
    .await
    {
        Ok(Ok(overview)) => axum::Json(ApiResponse::ok(overview, "Almacenamiento")).into_response(),
        Ok(Err(error)) => error_response(error),
        Err(_) => server_error("consulta de almacenamiento interrumpida"),
    }
}

/// Discos físicos detectados.
pub async fn storage_disks(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if let Err(response) = require_auth(&state, &headers) {
        return *response;
    }

    match tokio::task::spawn_blocking(|| nas_engine::storage::disks(&Runner::new())).await {
        Ok(Ok(disks)) => axum::Json(ApiResponse::ok(disks, "Discos detectados")).into_response(),
        Ok(Err(error)) => error_response(error),
        Err(_) => server_error("inventario de discos interrumpido"),
    }
}

/// Inspección de un dispositivo concreto.
pub async fn storage_inspect(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::Json(body): axum::Json<DeviceBody>,
) -> Response {
    if let Err(response) = require_auth(&state, &headers) {
        return *response;
    }

    let device = body.device;
    match tokio::task::spawn_blocking(move || nas_engine::storage::inspect(&Runner::new(), &device))
        .await
    {
        Ok(Ok(payload)) => {
            axum::Json(ApiResponse::ok(payload, "Inspección de disco")).into_response()
        }
        Ok(Err(error)) => error_response(error),
        Err(_) => server_error("inspección interrumpida"),
    }
}

/// Estado de la integración con Active Directory.
pub async fn domain_status(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if let Err(response) = require_auth(&state, &headers) {
        return *response;
    }

    match tokio::task::spawn_blocking(|| nas_engine::domain::status(&Runner::new())).await {
        Ok(status) => axum::Json(ApiResponse::ok(status, "Estado del dominio")).into_response(),
        Err(_) => server_error("consulta de dominio interrumpida"),
    }
}

/// Une el servidor al dominio (requiere administrador).
pub async fn domain_join(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::Json(body): axum::Json<DomainJoinBody>,
) -> Response {
    let user = match require_auth(&state, &headers) {
        Ok(user) => user,
        Err(response) => return *response,
    };
    if !is_admin(&state, &user) {
        return forbidden("se requieren privilegios de administrador");
    }
    if !nas_engine::domain::is_valid_domain(&body.domain)
        || !nas_engine::domain::is_valid_user(&body.user)
    {
        let _ = state.auth.db().append_audit(
            &user,
            "domain_join",
            &body.domain,
            "DENIED",
            "parámetros inválidos",
        );
        return bad_request("dominio o usuario inválidos");
    }

    let domain = body.domain.clone();
    let domain_user = body.user.clone();
    let password = body.password.clone();
    let outcome = tokio::task::spawn_blocking(move || {
        nas_engine::domain::join(&Runner::new(), &domain, &domain_user, &password)
    })
    .await
    .unwrap_or_else(|_| Err("unión interrumpida".to_string()));

    match outcome {
        Ok(()) => {
            let _ = state
                .auth
                .db()
                .append_audit(&user, "domain_join", &body.domain, "SUCCESS", "");
            axum::Json(ApiResponse::<serde_json::Value>::ok_empty(
                "Equipo unido al dominio",
            ))
            .into_response()
        }
        Err(error) => {
            let _ =
                state
                    .auth
                    .db()
                    .append_audit(&user, "domain_join", &body.domain, "FAILED", &error);
            server_error(&error)
        }
    }
}

/// Saca el servidor del dominio (requiere administrador).
pub async fn domain_leave(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let user = match require_auth(&state, &headers) {
        Ok(user) => user,
        Err(response) => return *response,
    };
    if !is_admin(&state, &user) {
        return forbidden("se requieren privilegios de administrador");
    }

    let outcome = tokio::task::spawn_blocking(|| nas_engine::domain::leave(&Runner::new()))
        .await
        .unwrap_or_else(|_| Err("salida interrumpida".to_string()));

    match outcome {
        Ok(()) => {
            let _ = state
                .auth
                .db()
                .append_audit(&user, "domain_leave", "domain", "SUCCESS", "");
            axum::Json(ApiResponse::<serde_json::Value>::ok_empty(
                "Equipo retirado del dominio",
            ))
            .into_response()
        }
        Err(error) => {
            let _ = state
                .auth
                .db()
                .append_audit(&user, "domain_leave", "domain", "FAILED", &error);
            server_error(&error)
        }
    }
}

fn forbidden(message: &str) -> Response {
    (
        StatusCode::FORBIDDEN,
        axum::Json(ApiResponse::<serde_json::Value>::error(message)),
    )
        .into_response()
}

/// Cuerpo para iniciar la sesión de terminal.
#[derive(Debug, Deserialize)]
#[serde(default)]
pub struct TerminalSessionBody {
    cols: u16,
    rows: u16,
}

impl Default for TerminalSessionBody {
    fn default() -> Self {
        Self {
            cols: 200,
            rows: 50,
        }
    }
}

/// Cuerpo para enviar texto al terminal.
#[derive(Debug, Deserialize)]
#[serde(default)]
pub struct TerminalSendBody {
    data: String,
    enter: bool,
}

impl Default for TerminalSendBody {
    fn default() -> Self {
        Self {
            data: String::new(),
            enter: true,
        }
    }
}

/// Cuerpo para redimensionar el terminal.
#[derive(Debug, Deserialize)]
pub struct TerminalResizeBody {
    cols: u16,
    rows: u16,
}

fn is_superadmin(state: &AppState, user: &str) -> bool {
    state
        .auth
        .db()
        .user_role(user)
        .ok()
        .flatten()
        .is_some_and(|role| role == "superadmin")
}

/// Inicia (o reutiliza) la sesión de terminal del usuario.
pub async fn terminal_session(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::Json(body): axum::Json<TerminalSessionBody>,
) -> Response {
    let user = match require_auth(&state, &headers) {
        Ok(user) => user,
        Err(response) => return *response,
    };
    if !is_superadmin(&state, &user) {
        return forbidden("el terminal está reservado al superadministrador");
    }
    if !nas_engine::terminal::is_valid_geometry(body.cols, body.rows) {
        return bad_request("geometría del terminal fuera de rango");
    }

    let session_user = user.clone();
    let cols = body.cols;
    let rows = body.rows;
    let outcome = tokio::task::spawn_blocking(move || {
        nas_engine::terminal::start(&Runner::new(), &session_user, cols, rows)
    })
    .await
    .unwrap_or_else(|_| Err("apertura de terminal interrumpida".to_string()));

    match outcome {
        Ok(session) => {
            let _ =
                state
                    .auth
                    .db()
                    .append_audit(&user, "terminal_start", &session.name, "SUCCESS", "");
            axum::Json(ApiResponse::ok(session, "Sesión de terminal lista")).into_response()
        }
        Err(error) => {
            let _ =
                state
                    .auth
                    .db()
                    .append_audit(&user, "terminal_start", "terminal", "FAILED", &error);
            server_error(&error)
        }
    }
}

/// Envía texto a la sesión del usuario.
pub async fn terminal_send(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::Json(body): axum::Json<TerminalSendBody>,
) -> Response {
    let user = match require_auth(&state, &headers) {
        Ok(user) => user,
        Err(response) => return *response,
    };
    if !is_superadmin(&state, &user) {
        return forbidden("el terminal está reservado al superadministrador");
    }
    if !nas_engine::terminal::is_valid_input(&body.data) {
        return bad_request("entrada inválida para el terminal");
    }

    let session_user = user.clone();
    let name = nas_engine::terminal::session_name(&user);
    let data = body.data;
    let enter = body.enter;
    let outcome = tokio::task::spawn_blocking(move || {
        nas_engine::terminal::send(&Runner::new(), &session_user, &name, &data, enter)
    })
    .await
    .unwrap_or_else(|_| Err("envío interrumpido".to_string()));

    match outcome {
        Ok(()) => axum::Json(ApiResponse::<serde_json::Value>::ok_empty(
            "Entrada enviada",
        ))
        .into_response(),
        Err(error) => server_error(&error),
    }
}

/// Captura el panel visible del terminal.
pub async fn terminal_capture(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let user = match require_auth(&state, &headers) {
        Ok(user) => user,
        Err(response) => return *response,
    };
    if !is_superadmin(&state, &user) {
        return forbidden("el terminal está reservado al superadministrador");
    }

    let session_user = user.clone();
    let name = nas_engine::terminal::session_name(&user);
    let captured_name = name.clone();
    let outcome = tokio::task::spawn_blocking(move || {
        nas_engine::terminal::capture(&Runner::new(), &session_user, &captured_name)
    })
    .await
    .unwrap_or_else(|_| Err("captura interrumpida".to_string()));

    match outcome {
        Ok(output) => axum::Json(ApiResponse::ok(
            serde_json::json!({ "output": output, "session": name }),
            "Captura del terminal",
        ))
        .into_response(),
        Err(error) => server_error(&error),
    }
}

/// Redimensiona el terminal.
pub async fn terminal_resize(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::Json(body): axum::Json<TerminalResizeBody>,
) -> Response {
    let user = match require_auth(&state, &headers) {
        Ok(user) => user,
        Err(response) => return *response,
    };
    if !is_superadmin(&state, &user) {
        return forbidden("el terminal está reservado al superadministrador");
    }
    if !nas_engine::terminal::is_valid_geometry(body.cols, body.rows) {
        return bad_request("geometría del terminal fuera de rango");
    }

    let session_user = user.clone();
    let name = nas_engine::terminal::session_name(&user);
    let cols = body.cols;
    let rows = body.rows;
    let outcome = tokio::task::spawn_blocking(move || {
        nas_engine::terminal::resize(&Runner::new(), &session_user, &name, cols, rows)
    })
    .await
    .unwrap_or_else(|_| Err("redimensionado interrumpido".to_string()));

    match outcome {
        Ok(()) => axum::Json(ApiResponse::<serde_json::Value>::ok_empty(
            "Terminal redimensionado",
        ))
        .into_response(),
        Err(error) => server_error(&error),
    }
}

/// Termina la sesión de terminal del usuario.
pub async fn terminal_kill(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let user = match require_auth(&state, &headers) {
        Ok(user) => user,
        Err(response) => return *response,
    };
    if !is_superadmin(&state, &user) {
        return forbidden("el terminal está reservado al superadministrador");
    }

    let session_user = user.clone();
    let name = nas_engine::terminal::session_name(&user);
    let killed_name = name.clone();
    let outcome = tokio::task::spawn_blocking(move || {
        nas_engine::terminal::kill(&Runner::new(), &session_user, &killed_name)
    })
    .await
    .unwrap_or_else(|_| Err("cierre interrumpido".to_string()));

    match outcome {
        Ok(()) => {
            let _ = state
                .auth
                .db()
                .append_audit(&user, "terminal_kill", &name, "SUCCESS", "");
            axum::Json(ApiResponse::<serde_json::Value>::ok_empty("Sesión cerrada")).into_response()
        }
        Err(error) => server_error(&error),
    }
}

/// Cuerpo para ejecutar una tarea de respaldo.
#[derive(Debug, Deserialize)]
pub struct BackupRunBody {
    path: String,
    #[serde(default)]
    confirm: bool,
}

/// Ejecuta una tarea de respaldo con las guardas del motor.
pub async fn backup_run(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::Json(body): axum::Json<BackupRunBody>,
) -> Response {
    let user = match require_auth(&state, &headers) {
        Ok(user) => user,
        Err(response) => return *response,
    };
    if !is_admin(&state, &user) {
        return forbidden("se requieren privilegios de administrador");
    }

    let dir = state.backup_dir.clone();
    let id = body.path.clone();
    let confirm = body.confirm;

    let outcome = tokio::task::spawn_blocking(move || {
        let tasks = nas_engine::tasks::BackupTask::discover(&dir)?;
        let task = tasks
            .into_iter()
            .find(|task| task.id == id)
            .ok_or_else(|| nas_core::NasError::NotFound(format!("tarea '{id}' no encontrada")))?;

        let options = nas_engine::RunOptions {
            dry_run: false,
            confirmed: confirm,
            sudo: true,
            lock_dir: std::path::PathBuf::from("/var/lock"),
        };

        nas_engine::run_backup(&Runner::new(), &task, &options, &chrono::Local::now())
    })
    .await
    .unwrap_or_else(|_| Err(nas_core::NasError::Invalid("ejecución interrumpida".into())));

    match outcome {
        Ok(report) => {
            let _ = state.auth.db().append_audit(
                &user,
                "backup_run",
                &report.task,
                "SUCCESS",
                &report.snapshot,
            );
            axum::Json(ApiResponse::ok(report, "Respaldo completado")).into_response()
        }
        Err(error) => {
            let _ = state.auth.db().append_audit(
                &user,
                "backup_run",
                &body.path,
                "FAILED",
                &error.to_string(),
            );
            error_response(error)
        }
    }
}

fn hostname_value() -> String {
    std::fs::read_to_string("/etc/hostname")
        .map(|value| value.trim().to_string())
        .ok()
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "SRV-NAS".to_string())
}

fn workgroup_value(state: &AppState) -> String {
    std::fs::read_to_string(&state.paths.samba_conf)
        .ok()
        .and_then(|text| nas_core::config::parse_workgroup(&text))
        .unwrap_or_else(|| "N/D".to_string())
}

fn server_ip() -> String {
    std::net::UdpSocket::bind("0.0.0.0:0")
        .and_then(|socket| {
            socket.connect("1.1.1.1:53")?;
            socket.local_addr()
        })
        .map(|addr| addr.ip().to_string())
        .unwrap_or_else(|_| "127.0.0.1".to_string())
}

fn render_shell(state: &AppState, user: &str, active: &str) -> Response {
    let role = state
        .auth
        .db()
        .user_role(user)
        .ok()
        .flatten()
        .unwrap_or_default();
    let is_admin = matches!(role.as_str(), "admin" | "superadmin");
    let is_super = role == "superadmin";
    let csrf = views::csrf_token();

    let shell = views::Shell {
        active,
        user,
        hostname: &hostname_value(),
        workgroup: &workgroup_value(state),
        role_label: if is_super {
            "Superadministrador"
        } else if is_admin {
            "Administrador"
        } else {
            "Operador Web"
        },
        server_role: state.role.as_str(),
        is_admin,
        is_super,
        ip: &server_ip(),
    };

    Html(views::shell(&shell, &csrf)).into_response()
}

fn login_redirect() -> Response {
    Redirect::to("/login").into_response()
}

/// Hoja de estilos del panel.
pub async fn css() -> Response {
    (
        [
            (axum::http::header::CONTENT_TYPE, "text/css; charset=utf-8"),
            (axum::http::header::CACHE_CONTROL, "public, max-age=300"),
        ],
        views::APP_CSS,
    )
        .into_response()
}

/// Hoja de estilos de la pantalla de acceso.
pub async fn login_css() -> Response {
    (
        [
            (axum::http::header::CONTENT_TYPE, "text/css; charset=utf-8"),
            (axum::http::header::CACHE_CONTROL, "public, max-age=300"),
        ],
        views::LOGIN_CSS,
    )
        .into_response()
}

/// Favicon.
pub async fn favicon() -> Response {
    (
        [
            (axum::http::header::CONTENT_TYPE, "image/svg+xml"),
            (axum::http::header::CACHE_CONTROL, "public, max-age=86400"),
        ],
        views::FAVICON,
    )
        .into_response()
}

/// Comportamiento original del panel.
pub async fn app_js() -> Response {
    js_response(crate::assets::APP_JS)
}

/// Visor de hojas de cálculo.
pub async fn vendor_xlsx() -> Response {
    js_response(crate::assets::XLSX_JS)
}

/// Compresión ZIP en el navegador.
pub async fn vendor_jszip() -> Response {
    js_response(crate::assets::JSZIP_JS)
}

/// Vista previa de documentos Word.
pub async fn vendor_docx() -> Response {
    js_response(crate::assets::DOCX_JS)
}

fn js_response(bytes: &'static [u8]) -> Response {
    (
        [
            (
                axum::http::header::CONTENT_TYPE,
                "application/javascript; charset=utf-8",
            ),
            (axum::http::header::CACHE_CONTROL, "public, max-age=300"),
        ],
        bytes,
    )
        .into_response()
}

macro_rules! page_handler {
    ($name:ident, $active:literal) => {
        pub async fn $name(State(state): State<AppState>, headers: HeaderMap) -> Response {
            match require_auth(&state, &headers) {
                Ok(user) => render_shell(&state, &user, $active),
                Err(_) => login_redirect(),
            }
        }
    };
}

page_handler!(page_dashboard, "dashboard");
page_handler!(page_shares, "shares");
page_handler!(page_users, "users");
page_handler!(page_backups, "backups");
page_handler!(page_services, "services");
page_handler!(page_diagnostics, "diagnostics");
page_handler!(page_storage, "storage");
page_handler!(page_files, "files");
page_handler!(page_domain, "domain");

/// Vista del terminal (solo superadministrador).
pub async fn page_terminal(State(state): State<AppState>, headers: HeaderMap) -> Response {
    match require_auth(&state, &headers) {
        Ok(user) if is_superadmin(&state, &user) => render_shell(&state, &user, "terminal"),
        Ok(user) => (
            StatusCode::FORBIDDEN,
            render_shell(&state, &user, "terminal"),
        )
            .into_response(),
        Err(_) => login_redirect(),
    }
}

/// Vista de auditoría (solo administradores).
pub async fn page_audit(State(state): State<AppState>, headers: HeaderMap) -> Response {
    match require_auth(&state, &headers) {
        Ok(user) if is_admin(&state, &user) => render_shell(&state, &user, "logs"),
        Ok(user) => (StatusCode::FORBIDDEN, render_shell(&state, &user, "logs")).into_response(),
        Err(_) => login_redirect(),
    }
}

/// Descarga una carpeta (o archivo) como ZIP.
pub async fn files_zip(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<FilesQuery>,
) -> Response {
    if let Err(response) = require_auth(&state, &headers) {
        return *response;
    }
    let root = match file_root(&state) {
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

    match tokio::task::spawn_blocking(move || root.zip(&path)).await {
        Ok(Ok(bytes)) => {
            let mut response = Response::new(axum::body::Body::from(bytes));
            let response_headers = response.headers_mut();
            response_headers.insert(
                axum::http::header::CONTENT_TYPE,
                HeaderValue::from_static("application/zip"),
            );
            if let Ok(value) = HeaderValue::from_str(&format!(
                "attachment; filename=\"{}.zip\"",
                name.replace('"', "")
            )) {
                response_headers.insert(axum::http::header::CONTENT_DISPOSITION, value);
            }
            response_headers.insert(
                "content-security-policy",
                HeaderValue::from_static("default-src 'none'; sandbox"),
            );
            response
        }
        Ok(Err(error)) => error_response(error),
        Err(_) => server_error("compresión interrumpida"),
    }
}

#[derive(Debug, Deserialize, Default)]
#[allow(dead_code)]
pub struct ExtQuery {
    #[serde(default)]
    pub limit: Option<usize>,
    #[serde(default)]
    pub source: Option<String>,
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub root: Option<String>,
    #[serde(default)]
    pub q: Option<String>,
    #[serde(default)]
    pub grouped: Option<u8>,
}

#[derive(Debug, Deserialize, Default)]
pub struct TrashBody {
    #[serde(default)]
    pub id: String,
}

#[derive(Debug, Deserialize)]
pub struct MoveBody {
    pub from: String,
    pub to: String,
}

#[derive(Debug, Deserialize)]
pub struct SaveBody {
    pub path: String,
    #[serde(default)]
    pub content: String,
}

#[derive(Debug, Deserialize)]
pub struct DomainDiscoverBody {
    pub domain: String,
}

#[derive(Debug, Deserialize)]
pub struct DiagBody {
    #[serde(default)]
    pub r#type: String,
}

fn not_implemented(feature: &str) -> Response {
    (
        StatusCode::NOT_IMPLEMENTED,
        axum::Json(ApiResponse::<serde_json::Value>::error(format!(
            "{feature} aún no está portado a Rust (F3)"
        ))),
    )
        .into_response()
}

fn run_json(command: nas_exec::CommandSpec) -> Result<String, String> {
    let output = Runner::new()
        .run(&command)
        .map_err(|error| error.to_string())?;
    if output.is_success() {
        Ok(output.stdout)
    } else {
        Err(if output.stderr.is_empty() {
            output.stdout
        } else {
            output.stderr
        })
    }
}

/// Logs recientes del sistema (journalctl).
pub async fn logs(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<ExtQuery>,
) -> Response {
    if let Err(response) = require_auth(&state, &headers) {
        return *response;
    }
    let limit = query.limit.unwrap_or(100).clamp(1, 500);

    match tokio::task::spawn_blocking(move || {
        let spec = nas_exec::CommandSpec::new("journalctl")
            .args(["-n", &limit.to_string(), "--no-pager", "-o", "short-iso"])
            .timeout(std::time::Duration::from_secs(20));
        run_json(spec)
    })
    .await
    {
        Ok(Ok(text)) => {
            let entries: Vec<serde_json::Value> = text
                .lines()
                .filter(|line| !line.trim().is_empty())
                .map(|line| {
                    let mut parts = line.splitn(3, ' ');
                    let timestamp = parts.next().unwrap_or_default();
                    let unit = parts.next().unwrap_or("system");
                    let message = parts.next().unwrap_or(line);
                    serde_json::json!({
                        "source": "system",
                        "unit": unit,
                        "timestamp": timestamp,
                        "message": message,
                        "action": message,
                        "action_label": unit,
                        "badge": "blue",
                    })
                })
                .collect();
            axum::Json(ApiResponse::ok(entries, "Logs del sistema")).into_response()
        }
        Ok(Err(error)) => server_error(&error),
        Err(_) => server_error("lectura de logs interrumpida"),
    }
}

/// Alias de listado de grupos.
pub async fn groups_list(State(state): State<AppState>, headers: HeaderMap) -> Response {
    users(State(state), headers).await
}

/// Mapa de acceso de recursos (recursos, grupos y usuarios).
pub async fn shares_access(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if let Err(response) = require_auth(&state, &headers) {
        return *response;
    }
    let path = state.paths.samba_conf.clone();

    match tokio::task::spawn_blocking(move || {
        let shares: Vec<String> = std::fs::read_to_string(&path)
            .ok()
            .map(|text| {
                nas_engine::parsers::smb::parse_shares(&text)
                    .into_iter()
                    .map(|share| share.name)
                    .collect()
            })
            .unwrap_or_default();
        let (_, groups) = nas_engine::users::load_local();
        let groups: Vec<String> = groups
            .into_iter()
            .filter(|group| group.name.starts_with("grp_"))
            .map(|group| group.name)
            .collect();
        let (mut users, _) = nas_engine::users::load_local();
        users.retain(|user| !user.system);
        let users: Vec<String> = users.into_iter().map(|user| user.name).collect();
        serde_json::json!({ "shares": shares, "groups": groups, "users": users })
    })
    .await
    {
        Ok(payload) => axum::Json(ApiResponse::ok(payload, "Mapa de acceso")).into_response(),
        Err(_) => server_error("mapa de acceso interrumpido"),
    }
}

/// Unidades de almacenamiento adicionales (aún sin montajes extra).
pub async fn storage_units(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if let Err(response) = require_auth(&state, &headers) {
        return *response;
    }
    axum::Json(ApiResponse::ok(
        serde_json::json!({ "units": [] }),
        "Unidades de almacenamiento",
    ))
    .into_response()
}

/// Subvolúmenes Btrfs (vacío si el filesystem no es Btrfs).
pub async fn storage_subvolumes(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if let Err(response) = require_auth(&state, &headers) {
        return *response;
    }
    axum::Json(ApiResponse::ok(
        serde_json::json!({ "subvolumes": [] }),
        "Subvolúmenes",
    ))
    .into_response()
}

/// Estado del scrub Btrfs.
pub async fn storage_scrub(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if let Err(response) = require_auth(&state, &headers) {
        return *response;
    }

    match tokio::task::spawn_blocking(|| {
        run_json(
            nas_exec::CommandSpec::new("btrfs")
                .args(["scrub", "status", "/srv/nas"])
                .sudo(nas_exec::SudoMode::Passwordless)
                .timeout(std::time::Duration::from_secs(15)),
        )
    })
    .await
    {
        Ok(Ok(text)) => {
            let status = nas_engine::parsers::scrub::parse_scrub_status(&text);
            axum::Json(ApiResponse::ok(status, "Estado de scrub")).into_response()
        }
        Ok(Err(error)) => server_error(&error),
        Err(_) => server_error("consulta de scrub interrumpida"),
    }
}

/// Inicia el scrub Btrfs.
pub async fn storage_scrub_start(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if let Err(response) = require_auth(&state, &headers) {
        return *response;
    }
    match tokio::task::spawn_blocking(|| {
        run_json(
            nas_exec::CommandSpec::new("btrfs")
                .args(["scrub", "start", "/srv/nas"])
                .sudo(nas_exec::SudoMode::Passwordless)
                .timeout(std::time::Duration::from_secs(30)),
        )
    })
    .await
    {
        Ok(Ok(text)) => axum::Json(ApiResponse::ok(
            serde_json::json!({ "output": text }),
            "Scrub iniciado",
        ))
        .into_response(),
        Ok(Err(error)) => server_error(&error),
        Err(_) => server_error("inicio de scrub interrumpido"),
    }
}

/// Ejecuta fstrim sobre el volumen.
pub async fn storage_trim(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if let Err(response) = require_auth(&state, &headers) {
        return *response;
    }
    match tokio::task::spawn_blocking(|| {
        run_json(
            nas_exec::CommandSpec::new("fstrim")
                .args(["-v", "/srv/nas"])
                .sudo(nas_exec::SudoMode::Passwordless)
                .timeout(std::time::Duration::from_secs(120)),
        )
    })
    .await
    {
        Ok(Ok(text)) => axum::Json(ApiResponse::ok(
            serde_json::json!({ "output": text }),
            "TRIM ejecutado",
        ))
        .into_response(),
        Ok(Err(error)) => server_error(&error),
        Err(_) => server_error("TRIM interrumpido"),
    }
}

/// Respaldos actualmente en ejecución.
pub async fn backups_running(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if let Err(response) = require_auth(&state, &headers) {
        return *response;
    }

    match tokio::task::spawn_blocking(|| {
        let output = Runner::new().run(
            &nas_exec::CommandSpec::new("pgrep")
                .args(["-af", "backup_|rsync"])
                .timeout(std::time::Duration::from_secs(10)),
        );
        let mut tasks: Vec<String> = Vec::new();
        if let Ok(output) = output {
            for line in output.stdout.lines() {
                if let Some(name) = line.split("backup_").nth(1) {
                    let id = name
                        .split(|c: char| !c.is_ascii_alphanumeric() && c != '-' && c != '_')
                        .next()
                        .unwrap_or("");
                    if !id.is_empty() && !tasks.iter().any(|t| t == id) {
                        tasks.push(id.to_string());
                    }
                }
            }
        }
        let running = !tasks.is_empty();
        serde_json::json!({ "running": running, "tasks": tasks })
    })
    .await
    {
        Ok(payload) => {
            axum::Json(ApiResponse::ok(payload, "Respaldos en ejecución")).into_response()
        }
        Err(_) => server_error("consulta de respaldos interrumpida"),
    }
}

/// Historial de comandos del terminal (aún sin persistencia propia).
pub async fn terminal_history(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if let Err(response) = require_auth(&state, &headers) {
        return *response;
    }
    axum::Json(ApiResponse::ok(
        Vec::<String>::new(),
        "Historial de terminal",
    ))
    .into_response()
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
    let root = match file_root(&state) {
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

/// Lista la papelera interna.
pub async fn files_trash(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if let Err(response) = require_auth(&state, &headers) {
        return *response;
    }
    let root = match file_root(&state) {
        Ok(root) => root,
        Err(response) => return *response,
    };

    match tokio::task::spawn_blocking(move || root.trash_entries()).await {
        Ok(Ok(items)) => {
            let count = items.len();
            let total: u64 = items.iter().map(|item| item.size).sum();
            axum::Json(ApiResponse::ok(
                serde_json::json!({
                    "items": items,
                    "count": count,
                    "total_size": total,
                    "total_size_formatted": format_size(total),
                }),
                "Papelera",
            ))
            .into_response()
        }
        Ok(Err(error)) => error_response(error),
        Err(_) => server_error("papelera interrumpida"),
    }
}

fn format_size(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["B", "KiB", "MiB", "GiB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

/// Restaura un elemento de la papelera.
pub async fn trash_restore(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::Json(body): axum::Json<TrashBody>,
) -> Response {
    if let Err(response) = require_auth(&state, &headers) {
        return *response;
    }
    let root = match file_root(&state) {
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
    headers: HeaderMap,
    axum::Json(body): axum::Json<TrashBody>,
) -> Response {
    if let Err(response) = require_auth(&state, &headers) {
        return *response;
    }
    let root = match file_root(&state) {
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
pub async fn trash_empty(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if let Err(response) = require_auth(&state, &headers) {
        return *response;
    }
    let root = match file_root(&state) {
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
pub async fn trash_restore_all(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if let Err(response) = require_auth(&state, &headers) {
        return *response;
    }
    let root = match file_root(&state) {
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

/// Crea un archivo o carpeta (alias de mkdir para nombres con extensión).
pub async fn files_create(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::Json(body): axum::Json<PathBody>,
) -> Response {
    files_mkdir(State(state), headers, axum::Json(body)).await
}

/// Mueve un elemento.
pub async fn files_move(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::Json(body): axum::Json<MoveBody>,
) -> Response {
    files_rename(
        State(state),
        headers,
        axum::Json(RenameBody {
            from: body.from,
            to: body.to,
        }),
    )
    .await
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
    let root = match file_root(&state) {
        Ok(root) => root,
        Err(response) => return *response,
    };
    match tokio::task::spawn_blocking(move || root.write(&body.path, &body.content)).await {
        Ok(Ok(bytes)) => axum::Json(ApiResponse::ok(
            serde_json::json!({ "bytes": bytes }),
            "Archivo guardado",
        ))
        .into_response(),
        Ok(Err(error)) => error_response(error),
        Err(_) => server_error("guardado interrumpido"),
    }
}

/// Descubre controladores de dominio.
pub async fn domain_discover(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::Json(body): axum::Json<DomainDiscoverBody>,
) -> Response {
    if let Err(response) = require_auth(&state, &headers) {
        return *response;
    }
    if !nas_engine::domain::is_valid_domain(&body.domain) {
        return bad_request("dominio inválido");
    }

    let domain = body.domain.clone();
    match tokio::task::spawn_blocking(move || {
        run_json(
            nas_exec::CommandSpec::new("realm")
                .args(["discover", "--verbose", &domain])
                .sudo(nas_exec::SudoMode::Passwordless)
                .timeout(std::time::Duration::from_secs(30)),
        )
    })
    .await
    {
        Ok(Ok(raw)) => {
            let realm = raw
                .lines()
                .find_map(|line| line.strip_prefix("realm-name:"))
                .map(str::trim)
                .unwrap_or_default()
                .to_string();
            axum::Json(ApiResponse::ok(
                serde_json::json!({
                    "domain_name": body.domain,
                    "realm_name": if realm.is_empty() { body.domain.to_uppercase() } else { realm },
                    "raw": raw,
                }),
                "Dominio descubierto",
            ))
            .into_response()
        }
        Ok(Err(error)) => server_error(&error),
        Err(_) => server_error("descubrimiento interrumpido"),
    }
}

/// Ejecuta una prueba diagnóstica.
pub async fn diagnostics_run(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::Json(body): axum::Json<DiagBody>,
) -> Response {
    if let Err(response) = require_auth(&state, &headers) {
        return *response;
    }
    let kind = if body.r#type.is_empty() {
        "services".to_string()
    } else {
        body.r#type.clone()
    };

    let outcome = tokio::task::spawn_blocking(move || match kind.as_str() {
        "ping" => run_json(
            nas_exec::CommandSpec::new("ping")
                .args(["-c", "1", "-W", "2", "1.1.1.1"])
                .timeout(std::time::Duration::from_secs(10)),
        ),
        "dns" => run_json(
            nas_exec::CommandSpec::new("getent")
                .args(["hosts", "localhost"])
                .timeout(std::time::Duration::from_secs(10)),
        ),
        "samba" => run_json(
            nas_exec::CommandSpec::new("testparm")
                .arg("-s")
                .timeout(std::time::Duration::from_secs(15)),
        ),
        "disk" => run_json(
            nas_exec::CommandSpec::new("df")
                .args(["-Pk", "/srv/nas"])
                .timeout(std::time::Duration::from_secs(10)),
        ),
        _ => run_json(
            nas_exec::CommandSpec::new("systemctl")
                .args(["is-active", "smbd"])
                .timeout(std::time::Duration::from_secs(10)),
        ),
    })
    .await;

    match outcome {
        Ok(Ok(output)) => axum::Json(ApiResponse::ok(
            serde_json::json!({ "status": "success", "output": output }),
            "Prueba completada",
        ))
        .into_response(),
        Ok(Err(error)) => (
            StatusCode::OK,
            axum::Json(ApiResponse::ok(
                serde_json::json!({ "status": "failed", "output": error }),
                "La prueba falló",
            )),
        )
            .into_response(),
        Err(_) => server_error("prueba interrumpida"),
    }
}

/// Reinicia el servidor.
pub async fn system_reboot(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let user = match require_auth(&state, &headers) {
        Ok(user) => user,
        Err(response) => return *response,
    };
    if !is_admin(&state, &user) {
        return forbidden("se requieren privilegios de administrador");
    }
    let _ = state
        .auth
        .db()
        .append_audit(&user, "system_reboot", "host", "REQUESTED", "");

    match tokio::task::spawn_blocking(|| {
        run_json(
            nas_exec::CommandSpec::new("systemctl")
                .arg("reboot")
                .sudo(nas_exec::SudoMode::Passwordless)
                .timeout(std::time::Duration::from_secs(15)),
        )
    })
    .await
    {
        Ok(Ok(_)) => axum::Json(ApiResponse::<serde_json::Value>::ok_empty(
            "Reinicio programado",
        ))
        .into_response(),
        Ok(Err(error)) => server_error(&error),
        Err(_) => server_error("reinicio interrumpido"),
    }
}

/// Información de versión y actualizaciones.
pub async fn system_updates(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if let Err(response) = require_auth(&state, &headers) {
        return *response;
    }
    let version = std::fs::read_to_string(state.paths.repo.join("version.json"))
        .ok()
        .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
        .unwrap_or_else(|| serde_json::json!({ "version": "0.1.0", "commit": "dev" }));

    axum::Json(ApiResponse::ok(
        serde_json::json!({
            "version": version["version"],
            "latest_version": version["version"],
            "has_updates": false,
            "gpg_verified": false,
            "commit": version["commit"],
        }),
        "Información de actualizaciones",
    ))
    .into_response()
}

/// Estado de las aplicaciones y tecnologías.
pub async fn system_applications(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if let Err(response) = require_auth(&state, &headers) {
        return *response;
    }

    let paths = state.paths.clone();
    match tokio::task::spawn_blocking(move || {
        let runner = Runner::new();
        let active = |unit: &str| {
            runner
                .run(
                    &nas_exec::CommandSpec::new("systemctl")
                        .args(["is-active", unit])
                        .timeout(std::time::Duration::from_secs(10)),
                )
                .map(|out| out.stdout == "active")
                .unwrap_or(false)
        };

        let smbstatus = runner
            .run(
                &nas_exec::CommandSpec::new("smbstatus")
                    .arg("-b")
                    .sudo(nas_exec::SudoMode::Passwordless)
                    .timeout(std::time::Duration::from_secs(15)),
            )
            .ok();
        let sessions = smbstatus
            .as_ref()
            .map(|out| {
                out.stdout
                    .lines()
                    .filter(|line| line.trim_start().starts_with(|c: char| c.is_ascii_digit()))
                    .count()
            })
            .unwrap_or(0);

        let tasks = nas_engine::tasks::BackupTask::discover("/usr/local/bin")
            .map(|tasks| tasks.len())
            .unwrap_or(0);
        let overview = nas_engine::storage::overview_for(&paths, &runner).ok();
        let fs = overview
            .as_ref()
            .map(|o| o.filesystem.clone())
            .unwrap_or_else(|| "desconocido".into());
        let is_btrfs = overview.as_ref().is_some_and(|o| o.is_btrfs);

        serde_json::json!({
            "samba": {
                "active": active("smbd"),
                "version": "4.x (Debian 13)",
                "sessions": sessions,
                "sessions_list": [],
                "vfs_modules": "acl_xattr, streams_xattr",
            },
            "wsdd": { "active": active("wsdd2") },
            "web": {
                "active": true,
                "pool_mode": "ondemand",
                "php_version": serde_json::Value::Null,
                "memory_mb": 0,
            },
            "btrfs": {
                "active": is_btrfs,
                "filesystem": fs,
                "compression": "zstd:3",
                "scrub_status": if is_btrfs { "programado" } else { "no disponible" },
                "scrub_errors": "0",
                "schedule": "mensual (día 1, 02:00)",
            },
            "rsync": {
                "tasks_count": tasks,
                "protocols": "SSH + CIFS",
                "deduplication": ">85% (link-dest)",
                "staging": "atómico",
            },
            "security": {
                "active": active("auditd"),
                "auditd_active": active("auditd"),
                "aide_active": active("dailyaidecheck.timer"),
                "audit_rules": 0,
                "fim_status": "AIDE configurado",
            },
        })
    })
    .await
    {
        Ok(payload) => axum::Json(ApiResponse::ok(payload, "Aplicaciones")).into_response(),
        Err(_) => server_error("consulta de aplicaciones interrumpida"),
    }
}

/// Acciones aún no portadas (devuelven 501 con mensaje claro).
pub async fn pending_shares_update() -> Response {
    not_implemented("La edición de recursos compartidos")
}
/// Acción pendiente.
pub async fn pending_shares_delete() -> Response {
    not_implemented("El borrado de recursos compartidos")
}
/// Acción pendiente.
pub async fn pending_shares_access() -> Response {
    not_implemented("La asignación de ACLs de recursos")
}
/// Acción pendiente.
pub async fn pending_shares_repair() -> Response {
    not_implemented("La reparación de ACLs")
}
/// Acción pendiente.
pub async fn pending_users() -> Response {
    not_implemented("La gestión de usuarios")
}
/// Acción pendiente.
pub async fn pending_groups() -> Response {
    not_implemented("La gestión de grupos")
}
/// Acción pendiente.
pub async fn pending_backups_write() -> Response {
    not_implemented("La creación o edición de tareas de respaldo")
}
/// Acción pendiente.
pub async fn pending_system_update() -> Response {
    not_implemented("La actualización del sistema desde el panel")
}
