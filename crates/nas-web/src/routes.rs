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
use crate::AppState;

/// Sonda de vida usada por systemd/nginx.
pub async fn healthz() -> &'static str {
    "ok"
}

/// Formulario de acceso.
pub async fn login_form() -> Html<&'static str> {
    Html(LOGIN_HTML)
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
            return (StatusCode::UNAUTHORIZED, Html(LOGIN_ERROR_HTML)).into_response();
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

/// Dashboard mínimo con datos de red en vivo.
pub async fn dashboard(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let user = match require_auth(&state, &headers) {
        Ok(user) => user,
        Err(response) if response.status() == StatusCode::UNAUTHORIZED => {
            return Redirect::to("/login").into_response()
        }
        Err(response) => return *response,
    };

    let paths = state.paths.clone();
    let metrics = tokio::task::spawn_blocking(move || {
        let runner = Runner::new();
        let overview = collect_network(&runner, &paths);
        (overview.hostname, overview.workgroup, overview.primary_ip)
    })
    .await
    .unwrap_or_else(|_| ("desconocido".to_string(), None, None));

    let hostname = html_escape(&metrics.0);
    let workgroup = html_escape(metrics.1.as_deref().unwrap_or("N/D"));
    let ip = html_escape(metrics.2.as_deref().unwrap_or("N/D"));
    let role = state.role.label();

    Html(format!(
        r#"<!DOCTYPE html>
<html lang="es"><head><meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Panel NAS · {hostname}</title>
<style>
body{{margin:0;font-family:system-ui,sans-serif;background:#0f172a;color:#e2e8f0}}
header{{display:flex;justify-content:space-between;align-items:center;padding:14px 22px;background:#1e293b;border-bottom:1px solid #334155}}
main{{max-width:960px;margin:24px auto;padding:0 22px;display:grid;gap:16px}}
.card{{background:#1e293b;border:1px solid #334155;border-radius:10px;padding:16px}}
h1{{font-size:18px;margin:0}} h2{{font-size:13px;color:#94a3b8;margin:0 0 8px}}
b{{color:#38bdf8;font-size:15px}}
button{{background:#334155;color:#e2e8f0;border:1px solid #475569;border-radius:8px;padding:6px 12px;cursor:pointer}}
code{{color:#7dd3fc}}
</style></head>
<body>
<header><h1>NAS · {hostname} · <small>{role}</small></h1>
<form method="post" action="/logout"><button type="submit">Salir ({user})</button></form></header>
<main>
  <div class="card"><h2>Hostname</h2><b>{hostname}</b></div>
  <div class="card"><h2>Grupo de trabajo</h2><b>{workgroup}</b></div>
  <div class="card"><h2>IPv4 primaria</h2><b>{ip}</b></div>
  <div class="card"><h2>API</h2><code>/api/metrics</code> · <code>/api/network</code> · <code>/api/shares</code> · <code>/api/users</code> · <code>/api/files</code></div>
</main>
</body></html>"#,
        user = html_escape(&user)
    ))
    .into_response()
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

fn html_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

const LOGIN_HTML: &str = concat!(
    r#"<!DOCTYPE html>
<html lang="es"><head><meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Acceso · Panel NAS</title><style>"#,
    "body{margin:0;min-height:100vh;display:grid;place-items:center;background:#0f172a;color:#e2e8f0;font-family:system-ui,sans-serif}form{background:#1e293b;border:1px solid #334155;border-radius:12px;padding:26px;width:320px;display:grid;gap:12px}h1{font-size:18px;margin:0 0 6px}label{font-size:12px;color:#94a3b8}input{background:#0f172a;border:1px solid #334155;border-radius:8px;padding:9px;color:#e2e8f0}button{background:#38bdf8;color:#0f172a;font-weight:600;border:0;border-radius:8px;padding:10px;cursor:pointer}",
    r#"</style></head>
<body><form method="post" action="/login">
<h1>Panel NAS</h1>
<div><label for="username">Usuario</label><input id="username" name="username" autocomplete="username" required autofocus></div>
<div><label for="password">Contraseña</label><input id="password" name="password" type="password" autocomplete="current-password" required></div>
<button type="submit">Entrar</button>
</form></body></html>"#
);

const LOGIN_ERROR_HTML: &str = concat!(
    r#"<!DOCTYPE html>
<html lang="es"><head><meta charset="utf-8"><title>Acceso denegado</title><style>"#,
    "body{margin:0;min-height:100vh;display:grid;place-items:center;background:#0f172a;color:#e2e8f0;font-family:system-ui,sans-serif}form{background:#1e293b;border:1px solid #334155;border-radius:12px;padding:26px;width:320px;display:grid;gap:12px}h1{font-size:18px;margin:0}label{font-size:12px;color:#94a3b8}input{background:#0f172a;border:1px solid #334155;border-radius:8px;padding:9px;color:#e2e8f0}button{background:#38bdf8;color:#0f172a;font-weight:600;border:0;border-radius:8px;padding:10px;cursor:pointer}.err{background:#7f1d1d;border:1px solid #b91c1c;padding:8px;border-radius:8px;font-size:13px}",
    r#"</style></head>
<body><form method="post" action="/login">
<h1>Acceso denegado</h1><div class="err">Usuario o contraseña incorrectos.</div>
<div><label for="username">Usuario</label><input id="username" name="username" autocomplete="username" required autofocus></div>
<div><label for="password">Contraseña</label><input id="password" name="password" type="password" autocomplete="current-password" required></div>
<button type="submit">Reintentar</button>
</form></body></html>"#
);

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
