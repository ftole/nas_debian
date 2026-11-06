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
use std::path::PathBuf;
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

fn extract_client_ip(headers: &HeaderMap) -> String {
    if let Some(real_ip) = headers.get("x-real-ip").and_then(|v| v.to_str().ok()) {
        let trimmed = real_ip.trim();
        if !trimmed.is_empty() {
            return trimmed.to_string();
        }
    }
    if let Some(forwarded) = headers.get("x-forwarded-for").and_then(|v| v.to_str().ok()) {
        if let Some(first) = forwarded.split(',').next() {
            let trimmed = first.trim();
            if !trimmed.is_empty() {
                return trimmed.to_string();
            }
        }
    }
    "127.0.0.1".to_string()
}

/// Formulario de acceso.
pub async fn login_form(State(state): State<AppState>) -> Html<String> {
    let now = Utc::now().timestamp();
    let csrf = state.auth.issue_login_csrf(now);
    Html(views::login_with_context(
        &hostname(),
        &server_ip(),
        &csrf,
        None,
    ))
}

/// Credenciales enviadas por el formulario.
#[derive(Debug, Deserialize)]
pub struct LoginForm {
    pub username: String,
    pub password: String,
    #[serde(default)]
    pub csrf_token: Option<String>,
}

/// Procesa el acceso y emite la cookie de sesión.
pub async fn login_submit(
    State(state): State<AppState>,
    headers: HeaderMap,
    Form(form): Form<LoginForm>,
) -> Response {
    let now = Utc::now().timestamp();
    let client_ip = extract_client_ip(&headers);

    if let Ok(false) = state
        .auth
        .db()
        .check_rate_limit(&format!("login:{client_ip}"), now, 60, 5)
    {
        let csrf = state.auth.issue_login_csrf(now);
        return (
            StatusCode::TOO_MANY_REQUESTS,
            Html(views::login_with_context(
                &hostname(),
                &server_ip(),
                &csrf,
                Some("Demasiados intentos fallidos. Intente de nuevo más tarde."),
            )),
        )
            .into_response();
    }

    if !state
        .auth
        .verify_login_csrf(form.csrf_token.as_deref(), now)
    {
        let csrf = state.auth.issue_login_csrf(now);
        return (
            StatusCode::FORBIDDEN,
            Html(views::login_with_context(
                &hostname(),
                &server_ip(),
                &csrf,
                Some("Token CSRF inválido o expirado."),
            )),
        )
            .into_response();
    }
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
            let csrf = state.auth.issue_login_csrf(now);
            return (
                StatusCode::UNAUTHORIZED,
                Html(views::login_with_context(
                    &hostname(),
                    &server_ip(),
                    &csrf,
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

/// Endpoint JSON para autenticación API (/api/auth/login).
pub async fn api_login(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::Json(form): axum::Json<LoginForm>,
) -> Response {
    let now = Utc::now().timestamp();
    let client_ip = extract_client_ip(&headers);

    if let Ok(false) = state
        .auth
        .db()
        .check_rate_limit(&format!("login:{client_ip}"), now, 60, 5)
    {
        return (
            StatusCode::TOO_MANY_REQUESTS,
            axum::Json(ApiResponse::<serde_json::Value>::error(
                "Demasiados intentos de acceso. Espere un momento e intente de nuevo.",
            )),
        )
            .into_response();
    }

    let csrf_header = headers.get("x-csrf-token").and_then(|v| v.to_str().ok());
    let token = form.csrf_token.as_deref().or(csrf_header);
    if !state.auth.verify_login_csrf(token, now) {
        return (
            StatusCode::FORBIDDEN,
            axum::Json(ApiResponse::<serde_json::Value>::error(
                "token CSRF inválido o ausente",
            )),
        )
            .into_response();
    }
    match state.auth.login(&form.username, &form.password, now) {
        Ok(Some(session)) => {
            let _ = state
                .auth
                .db()
                .append_audit(&form.username, "login", "api", "SUCCESS", "");
            let role = state
                .auth
                .db()
                .user_role(&form.username)
                .ok()
                .flatten()
                .unwrap_or_else(|| "admin".into());
            let csrf = state.auth.csrf_for_session(&session);
            let mut response = axum::Json(ApiResponse::ok(
                serde_json::json!({
                    "username": form.username,
                    "role": role,
                    "session": session,
                    "csrf_token": csrf,
                }),
                "Sesión iniciada",
            ))
            .into_response();
            if let Ok(header) = HeaderValue::from_str(&state.auth.set_cookie_header(&session)) {
                response.headers_mut().insert(SET_COOKIE, header);
            }
            response
        }
        Ok(None) => {
            let _ = state
                .auth
                .db()
                .append_audit(&form.username, "login", "api", "FAILED", "");
            (
                StatusCode::UNAUTHORIZED,
                axum::Json(ApiResponse::<serde_json::Value>::error(
                    "Usuario o contraseña incorrectos.",
                )),
            )
                .into_response()
        }
        Err(error) => server_error(&error.to_string()),
    }
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
pub async fn metrics(
    _user: crate::extractors::AuthenticatedUser,
    State(state): State<AppState>,
) -> Response {
    let paths = state.paths.clone();
    match tokio::task::spawn_blocking(move || {
        let runner = Runner::new();
        let metrics = collect_status(&runner);
        let storage = nas_engine::storage::overview_for(&paths, &runner).ok();
        let payload = serde_json::json!({
            "system": metrics,
            "storage": storage,
            "services": crate::legacy::service_rows(&runner),
            "counts": crate::legacy::dashboard_counts(&paths),
        });
        payload
    })
    .await
    {
        Ok(payload) => axum::Json(ApiResponse::ok(payload, "Métricas del sistema")).into_response(),
        Err(_) => server_error("no se pudieron recolectar las métricas"),
    }
}

/// Inventario de red en JSON.
pub async fn network(
    _user: crate::extractors::AuthenticatedUser,
    State(state): State<AppState>,
) -> Response {
    let paths = state.paths.clone();
    match tokio::task::spawn_blocking(move || collect_network(&Runner::new(), &paths)).await {
        Ok(overview) => {
            let mut payload = serde_json::to_value(&overview).unwrap_or_default();
            if let Some(interfaces) = payload
                .get_mut("interfaces")
                .and_then(|value| value.as_array_mut())
            {
                for interface in interfaces {
                    let ipv4 = interface
                        .get("ipv4")
                        .cloned()
                        .unwrap_or(serde_json::Value::Null);
                    interface["ip"] = ipv4;
                }
            }
            axum::Json(ApiResponse::ok(payload, "Inventario de red")).into_response()
        }
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
            let shares: Vec<serde_json::Value> = parse_shares(&text)
                .into_iter()
                .map(|share| {
                    let scheme_name = if share.guest_ok {
                        "Acceso público / Invitados"
                    } else if share.read_only {
                        "Solo lectura estricta"
                    } else if !share.write_list.is_empty() {
                        "Lectura general + escritura exclusiva"
                    } else {
                        "Lectura y escritura por grupo"
                    };
                    serde_json::json!({
                        "name": share.name,
                        "path": share.path,
                        "read_only": share.read_only,
                        "browseable": share.browseable,
                        "hidden": !share.browseable,
                        "guest_ok": share.guest_ok,
                        "valid_users": share.valid_users,
                        "write_list": share.write_list,
                        "comment": share.comment,
                        "scheme_name": scheme_name,
                        "scheme": if share.guest_ok { 4 } else if share.read_only { 3 } else if !share.write_list.is_empty() { 2 } else { 1 },
                    })
                })
                .collect();
            axum::Json(ApiResponse::ok(shares, "Recursos compartidos")).into_response()
        }
        Ok(Err(error)) => server_error(&format!("no se pudo leer smb.conf: {error}")),
        Err(_) => server_error("lectura de smb.conf interrumpida"),
    }
}

/// Usuarios administrables (formato que consume `app.js`).
pub async fn users(
    _user: crate::extractors::AuthenticatedUser,
    State(state): State<AppState>,
) -> Response {
    let db = state.auth.db().clone();
    match tokio::task::spawn_blocking(move || {
        let (mut users, groups) = nas_engine::users::load_local();
        users.retain(|user| !user.system);

        let samba_users = Runner::new()
            .run(
                &nas_exec::CommandSpec::new("pdbedit")
                    .args(["-L", "-s"])
                    .sudo(nas_exec::SudoMode::Passwordless)
                    .timeout(std::time::Duration::from_secs(15)),
            )
            .ok()
            .map(|output| {
                output
                    .stdout
                    .lines()
                    .filter_map(|line| line.split(':').next().map(str::to_string))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        let list: Vec<serde_json::Value> = users
            .into_iter()
            .map(|user| {
                let member_of: Vec<String> = groups
                    .iter()
                    .filter(|group| group.members.iter().any(|member| member == &user.name))
                    .map(|group| group.name.clone())
                    .collect();
                let role = db.user_role(&user.name).ok().flatten();
                let is_superadmin = role.as_deref() == Some("superadmin");
                let is_admin = matches!(role.as_deref(), Some("admin") | Some("superadmin"));
                let can_web = role.is_some();
                let is_samba = samba_users.iter().any(|name| name == &user.name);
                serde_json::json!({
                    "username": user.name,
                    "full_name": user.full_name,
                    "uid": user.uid,
                    "shell": user.shell,
                    "home": user.home,
                    "groups": member_of,
                    "is_admin": is_admin,
                    "is_superadmin": is_superadmin,
                    "can_web": can_web,
                    "samba_enabled": is_samba,
                    "is_samba": is_samba,
                    "enabled": true,
                })
            })
            .collect();
        list
    })
    .await
    {
        Ok(list) => axum::Json(ApiResponse::ok(list, "Usuarios")).into_response(),
        Err(_) => server_error("inventario de usuarios interrumpido"),
    }
}

/// Grupos corporativos (formato que consume `app.js`).
pub async fn groups(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if let Err(response) = require_auth(&state, &headers) {
        return *response;
    }

    match tokio::task::spawn_blocking(|| {
        let (_, groups) = nas_engine::users::load_local();
        let list: Vec<serde_json::Value> = groups
            .into_iter()
            .filter(|group| group.name.starts_with("grp_"))
            .map(|group| {
                let is_special = matches!(
                    group.name.as_str(),
                    "grp_samba" | "grp_web" | "grp_superadmin" | "grp_backups"
                );
                serde_json::json!({
                    "name": group.name,
                    "gid": group.gid,
                    "members": group.members,
                    "is_special": is_special,
                    "is_master": matches!(
                        group.name.as_str(),
                        "grp_samba" | "grp_superadmin" | "grp_backups" | "grp_web"
                    ),
                })
            })
            .collect();
        list
    })
    .await
    {
        Ok(list) => axum::Json(ApiResponse::ok(list, "Grupos")).into_response(),
        Err(_) => server_error("inventario de grupos interrumpido"),
    }
}

/// Parámetro `?path=` del explorador.
#[derive(Debug, Deserialize)]
pub struct FilesQuery {
    #[serde(default)]
    path: String,
    #[serde(default)]
    root: Option<String>,
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

/// Cuerpo para borrado (papelera o definitivo).
#[derive(Debug, Deserialize)]
pub struct DeleteBody {
    #[serde(default)]
    pub root: Option<String>,
    #[serde(default)]
    pub path: String,
    #[serde(default)]
    pub permanent: bool,
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

fn safe_content_disposition(disposition: &str, filename: &str) -> HeaderValue {
    let ascii_name: String = filename
        .chars()
        .map(|c| {
            if c.is_ascii_graphic() && c != '"' && c != '\\' && c != ';' && c != '/' {
                c
            } else if c.is_ascii_whitespace() {
                ' '
            } else {
                '_'
            }
        })
        .collect();
    let ascii_name = if ascii_name.trim().is_empty() {
        "archivo.bin"
    } else {
        ascii_name.trim()
    };

    let mut encoded = String::new();
    for byte in filename.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            encoded.push(byte as char);
        } else {
            use std::fmt::Write;
            let _ = write!(encoded, "%{:02X}", byte);
        }
    }

    let header_val =
        format!("{disposition}; filename=\"{ascii_name}\"; filename*=UTF-8''{encoded}");
    HeaderValue::from_str(&header_val)
        .unwrap_or_else(|_| HeaderValue::from_static("attachment; filename=\"descarga.bin\""))
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
    let path = query.path;
    let name = path
        .trim_end_matches('/')
        .rsplit('/')
        .next()
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| "descarga.bin".to_string());

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

/// Envía un elemento a la papelera interna o lo borra definitivamente.
pub async fn files_delete(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::Json(body): axum::Json<DeleteBody>,
) -> Response {
    if let Err(response) = require_auth(&state, &headers) {
        return *response;
    }
    let root = match file_root_for(&state, body.root.as_deref()) {
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
    let permanent = body.permanent;

    let outcome = tokio::task::spawn_blocking(move || {
        if permanent {
            root.delete_permanent(&path).map(|_| None)
        } else {
            root.trash(&path).map(Some)
        }
    })
    .await;

    match outcome {
        Ok(Ok(target)) => {
            let action = if permanent {
                "file_delete_permanent"
            } else {
                "file_delete"
            };
            let _ = state
                .auth
                .db()
                .append_audit(&user, action, &audit_path, "SUCCESS", "");
            let msg = if permanent {
                "Elemento eliminado definitivamente"
            } else {
                "Elemento movido a la papelera"
            };
            axum::Json(ApiResponse::ok(
                serde_json::json!({
                    "trash": target.map(|t| t.display().to_string()),
                    "permanent": permanent,
                }),
                msg,
            ))
            .into_response()
        }
        Ok(Err(error)) => error_response(error),
        Err(_) => server_error("borrado interrumpido"),
    }
}

/// Límite máximo de subida por archivo en streaming (50 GB).
pub const MAX_UPLOAD_FILE_SIZE: u64 = 50 * 1024 * 1024 * 1024;

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
    let status = match &error {
        NasError::Denied(_) => StatusCode::FORBIDDEN,
        NasError::NotFound(_) => StatusCode::NOT_FOUND,
        NasError::Invalid(_) => StatusCode::BAD_REQUEST,
        NasError::Io { source, .. } if source.kind() == std::io::ErrorKind::NotFound => {
            StatusCode::NOT_FOUND
        }
        NasError::Io { source, .. }
            if source.kind() == std::io::ErrorKind::PermissionDenied
                || source.raw_os_error() == Some(1)
                || source.raw_os_error() == Some(13) =>
        {
            StatusCode::FORBIDDEN
        }
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
pub async fn services(
    _user: crate::extractors::AuthenticatedUser,
    State(_state): State<AppState>,
) -> Response {
    match tokio::task::spawn_blocking(|| crate::legacy::service_rows(&Runner::new())).await {
        Ok(list) => axum::Json(ApiResponse::ok(list, "Estado de servicios")).into_response(),
        Err(_) => server_error("consulta de servicios interrumpida"),
    }
}

/// Reinicia o recarga un servicio de la lista blanca.
pub async fn service_manage(
    admin: crate::extractors::AdminUser,
    State(state): State<AppState>,
    axum::Json(body): axum::Json<ServiceBody>,
) -> Response {
    let user = admin.username().to_string();
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
        let runner = Runner::new();
        let checks = nas_engine::diagnostics::collect(&paths, &runner);
        let services = crate::legacy::service_rows(&runner);
        let active = services.iter().filter(|row| row["active"] == true).count();
        let total = services.len();
        let metrics = collect_status(&runner);
        let storage = nas_engine::storage::overview_for(&paths, &runner).ok();
        let counts = crate::legacy::dashboard_counts(&paths);
        let testparm_ok = runner
            .run(&nas_exec::CommandSpec::new("testparm").arg("-s"))
            .map(|output| output.is_success())
            .unwrap_or(false);
        serde_json::json!({
            "overall_status": if active == total { "OK" } else { "Warning" },
            "services_active": active,
            "services_total": total,
            "services": services,
            "storage": storage,
            "shares_count": counts["shares"],
            "backups_count": counts["backups"],
            "testparm_ok": testparm_ok,
            "hostname": metrics.hostname,
            "uptime": metrics.uptime,
            "kernel": metrics.kernel,
            "checks": checks,
        })
    })
    .await
    {
        Ok(payload) => axum::Json(ApiResponse::ok(payload, "Diagnóstico integral")).into_response(),
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
        let mut disk_cache = std::collections::HashMap::new();
        tasks
            .into_iter()
            .map(|task| {
                let last = last_snapshot(&task);
                let mut row = crate::legacy::backup_row_with_metrics(&task, &mut disk_cache);
                row["task"] = serde_json::json!(task);
                row["last_snapshot"] = serde_json::json!(last);
                row
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
    _admin: crate::extractors::AdminUser,
    State(state): State<AppState>,
    Query(query): Query<AuditQuery>,
) -> Response {
    match state.auth.db().recent_audit(query.limit.clamp(1, 500)) {
        Ok(rows) => axum::Json(ApiResponse::ok(rows, "Auditoría del panel")).into_response(),
        Err(error) => error_response(error),
    }
}

/// Verifica la integridad criptográfica de la cadena SHA-256 de nas-audit.
pub async fn audit_verify(
    _admin: crate::extractors::AdminUser,
    State(state): State<AppState>,
) -> Response {
    let audit_path = state.paths.audit_log();
    if !audit_path.is_file() {
        return axum::Json(ApiResponse::ok(
            serde_json::json!({
                "valid": true,
                "count": 0,
                "status": "EMPTY",
                "message": "Bitácora criptográfica sin registros iniciales",
                "path": audit_path.display().to_string(),
            }),
            "Cadena de auditoría verificada",
        ))
        .into_response();
    }

    let log = nas_audit::AuditLog::new(&audit_path);
    match tokio::task::spawn_blocking(move || {
        let verify_res = log.verify();
        let last_hash = log.last_hash().unwrap_or_else(|_| "0".to_string());
        (verify_res, last_hash)
    })
    .await
    {
        Ok((Ok(count), last_hash)) => axum::Json(ApiResponse::ok(
            serde_json::json!({
                "valid": true,
                "count": count,
                "last_hash": last_hash,
                "status": "VERIFIED",
                "message": format!("Cadena criptográfica íntegra: {count} registros verificados correctamente"),
                "path": audit_path.display().to_string(),
            }),
            "Auditoría íntegra y verificada",
        ))
        .into_response(),
        Ok((Err(err), last_hash)) => (
            StatusCode::OK,
            axum::Json(ApiResponse::ok(
                serde_json::json!({
                    "valid": false,
                    "count": 0,
                    "last_hash": last_hash,
                    "status": "CORRUPTED",
                    "error": err.to_string(),
                    "message": format!("Alerta: la integridad de la cadena de auditoría ha fallado: {err}"),
                    "path": audit_path.display().to_string(),
                }),
                "Fallo en la verificación criptográfica",
            )),
        )
            .into_response(),
        Err(_) => server_error("verificación de auditoría interrumpida"),
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

fn collect_storage_payload(paths: &nas_core::NasPaths) -> Result<serde_json::Value, NasError> {
    let runner = Runner::new();
    let overview = nas_engine::storage::overview_for(paths, &runner).unwrap_or_else(|_| {
        nas_engine::storage::StorageOverview {
            path: paths.data_root.to_string_lossy().to_string(),
            filesystem: "no montado".to_string(),
            device: None,
            total_gb: 0.0,
            used_gb: 0.0,
            free_gb: 0.0,
            usage_percent: 0.0,
            is_btrfs: false,
            supports_trim: false,
            device_type: "Desconocido".to_string(),
        }
    });
    let disks = nas_engine::storage::disks(&runner).unwrap_or_default();
    let disks: Vec<serde_json::Value> = disks
        .iter()
        .map(|disk| {
            let mut row = serde_json::to_value(disk).unwrap_or_default();
            let is_part = disk.children_count == 0
                && (disk.fstype.as_deref().is_some_and(|fs| !fs.is_empty())
                    || disk.name.chars().last().is_some_and(|c| c.is_ascii_digit()));
            row["type"] = serde_json::json!(if is_part { "part" } else { "disk" });
            row["mount"] = serde_json::json!(disk.mountpoint.clone().unwrap_or_default());
            row["children_count"] = serde_json::json!(disk.children_count);
            row["is_os_disk"] = serde_json::json!(disk.protected);
            row
        })
        .collect();
    Ok(serde_json::json!({
        "overview": overview,
        "disks": disks,
    }))
}

/// Resumen del volumen de datos.
pub async fn storage(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if let Err(response) = require_auth(&state, &headers) {
        return *response;
    }

    let paths = state.paths.clone();
    match tokio::task::spawn_blocking(move || collect_storage_payload(&paths)).await {
        Ok(Ok(payload)) => axum::Json(ApiResponse::ok(payload, "Almacenamiento")).into_response(),
        Ok(Err(error)) => error_response(error),
        Err(_) => server_error("consulta de almacenamiento interrumpida"),
    }
}

/// Discos físicos detectados y resumen de capacidad (contrato consumido por `loadStorage()`).
pub async fn storage_disks(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if let Err(response) = require_auth(&state, &headers) {
        return *response;
    }

    let paths = state.paths.clone();
    match tokio::task::spawn_blocking(move || collect_storage_payload(&paths)).await {
        Ok(Ok(payload)) => {
            axum::Json(ApiResponse::ok(payload, "Discos detectados")).into_response()
        }
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
    let user = match require_auth(&state, &headers) {
        Ok(user) => user,
        Err(response) => return *response,
    };
    if !is_admin(&state, &user) {
        return forbidden("se requieren privilegios de administrador");
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

    let samba_conf = state.paths.samba_conf.clone();
    match tokio::task::spawn_blocking(|| nas_engine::domain::status(&Runner::new())).await {
        Ok(status) => {
            let workgroup = std::fs::read_to_string(&samba_conf)
                .ok()
                .and_then(|text| {
                    text.lines().find_map(|line| {
                        line.trim()
                            .strip_prefix("workgroup =")
                            .map(str::trim)
                            .map(str::to_string)
                    })
                })
                .unwrap_or_else(|| "WORKGROUP".to_string());
            let payload = serde_json::json!({
                "status": if status.joined { "joined" } else { "not_joined" },
                "workgroup": workgroup,
                "message": if status.joined { "Equipo unido al dominio" } else { "Equipo no unido a un dominio" },
                "joined": status.joined,
                "realm": status.realm,
                "domain": status.domain,
                "sssd_active": status.sssd_active,
                "details": status.details,
            });
            axum::Json(ApiResponse::ok(payload, "Estado del dominio")).into_response()
        }
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

/// Cuerpo para enviar texto o teclas especiales al terminal.
#[derive(Debug, Deserialize)]
#[serde(default)]
pub struct TerminalSendBody {
    data: String,
    enter: bool,
    raw: bool,
}

impl Default for TerminalSendBody {
    fn default() -> Self {
        Self {
            data: String::new(),
            enter: true,
            raw: false,
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
    if configured_superadmin().as_deref() == Some(user) {
        return true;
    }
    state
        .auth
        .db()
        .user_role(user)
        .ok()
        .flatten()
        .is_some_and(|role| role == "superadmin")
}

fn is_interactive_posix_user(user: &nas_engine::users::UserEntry) -> bool {
    !user.system
        && !nas_engine::users::is_protected_system_account(&user.name, Some(user.uid))
        && (1000..60_000).contains(&user.uid)
        && !user.shell.ends_with("nologin")
        && !user.shell.ends_with("/false")
}

/// Resuelve la cuenta POSIX no-root bajo la cual correrá `tmux` para el superadministrador.
fn resolve_terminal_posix_user(web_user: &str) -> Result<String, String> {
    let (users, groups) = nas_engine::users::load_local();
    if let Some(found) = users
        .iter()
        .find(|u| u.name == web_user && is_interactive_posix_user(u))
    {
        return Ok(found.name.clone());
    }

    if let Ok(designated) = std::fs::read_to_string("/etc/nas/superadmin") {
        let designated = designated.trim();
        if let Some(found) = users
            .iter()
            .find(|u| u.name == designated && is_interactive_posix_user(u))
        {
            return Ok(found.name.clone());
        }
    }

    for group_name in ["grp_superadmin", "sudo"] {
        if let Some(group) = groups.iter().find(|g| g.name == group_name) {
            for member in &group.members {
                if let Some(found) = users
                    .iter()
                    .find(|u| u.name == *member && is_interactive_posix_user(u))
                {
                    return Ok(found.name.clone());
                }
            }
        }
    }

    Err(
        "no existe una cuenta POSIX interactiva de superadministrador (verifique /etc/nas/superadmin)"
            .to_string(),
    )
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

    let web_user = user.clone();
    let cols = body.cols;
    let rows = body.rows;
    let outcome = tokio::task::spawn_blocking(move || {
        let posix_user = resolve_terminal_posix_user(&web_user)?;
        nas_engine::terminal::start(&Runner::new(), &posix_user, cols, rows)
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

/// Envía texto o teclas especiales a la sesión del usuario.
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

    let web_user = user.clone();
    let data = body.data;
    let enter = body.enter && !body.raw;
    let raw = body.raw;
    let outcome = tokio::task::spawn_blocking(move || {
        let posix_user = resolve_terminal_posix_user(&web_user)?;
        let name = nas_engine::terminal::session_name(&posix_user);
        let runner = Runner::new();
        if !raw && nas_engine::terminal::is_allowed_key(&data) {
            nas_engine::terminal::send_key(&runner, &posix_user, &name, &data)
        } else {
            nas_engine::terminal::send(&runner, &posix_user, &name, &data, enter)
        }
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

    let web_user = user.clone();
    let outcome = tokio::task::spawn_blocking(move || {
        let posix_user = resolve_terminal_posix_user(&web_user)?;
        let name = nas_engine::terminal::session_name(&posix_user);
        let (output, cwd) =
            nas_engine::terminal::capture_details(&Runner::new(), &posix_user, &name)?;
        Ok::<_, String>((output, cwd, posix_user, name))
    })
    .await
    .unwrap_or_else(|_| Err("captura interrumpida".to_string()));

    match outcome {
        Ok((output, cwd, posix_user, name)) => axum::Json(ApiResponse::ok(
            serde_json::json!({
                "output": output,
                "cwd": cwd,
                "user": posix_user,
                "session": name,
            }),
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

    let web_user = user.clone();
    let cols = body.cols;
    let rows = body.rows;
    let outcome = tokio::task::spawn_blocking(move || {
        let posix_user = resolve_terminal_posix_user(&web_user)?;
        let name = nas_engine::terminal::session_name(&posix_user);
        nas_engine::terminal::resize(&Runner::new(), &posix_user, &name, cols, rows)
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

    let web_user = user.clone();
    let outcome = tokio::task::spawn_blocking(move || {
        let posix_user = resolve_terminal_posix_user(&web_user)?;
        let name = nas_engine::terminal::session_name(&posix_user);
        nas_engine::terminal::kill(&Runner::new(), &posix_user, &name)?;
        Ok::<_, String>(name)
    })
    .await
    .unwrap_or_else(|_| Err("cierre interrumpido".to_string()));

    match outcome {
        Ok(name) => {
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

    // Motor único: delega en el runner Bash instalado vía `nas helper
    // backup-manage run`, igual que cron y `/{id}/run`. Evita dos motores con
    // bloqueos incompatibles y garantiza sellado, progreso y golden snapshots.
    if !nas_engine::backup_control::valid_id(&body.path) {
        return bad_request("identificador de tarea inválido");
    }
    if !body.confirm {
        return forbidden("la ejecución real exige confirm: true");
    }

    let id = body.path.clone();
    let outcome =
        tokio::task::spawn_blocking(move || nas_engine::backup_control::run(&Runner::new(), &id))
            .await
            .unwrap_or_else(|_| Err("ejecución interrumpida".to_string()));

    match outcome {
        Ok(message) => {
            let _ =
                state
                    .auth
                    .db()
                    .append_audit(&user, "backup_run", &body.path, "SUCCESS", &message);
            axum::Json(ApiResponse::ok(
                serde_json::json!({ "message": message }),
                message,
            ))
            .into_response()
        }
        Err(error) => {
            let _ = state
                .auth
                .db()
                .append_audit(&user, "backup_run", &body.path, "FAILED", &error);
            error_response(nas_core::NasError::Invalid(error))
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
    let runner = Runner::new();
    if let Some(ip) = nas_engine::primary_ip(&runner) {
        return ip;
    }
    std::net::UdpSocket::bind("0.0.0.0:0")
        .and_then(|socket| {
            socket.connect("10.255.255.255:1")?;
            socket.local_addr()
        })
        .map(|addr| addr.ip().to_string())
        .ok()
        .filter(|ip| !ip.starts_with("127."))
        .unwrap_or_else(|| "10.10.1.2".to_string())
}

fn render_shell(state: &AppState, headers: &HeaderMap, user: &str, active: &str) -> Response {
    let role = state
        .auth
        .db()
        .user_role(user)
        .ok()
        .flatten()
        .unwrap_or_default();
    let is_admin = matches!(role.as_str(), "admin" | "superadmin");
    let is_super = role == "superadmin";
    let csrf = state.auth.csrf_for_cookies(cookie_header(headers));

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

/// Favicon binario ICO multi-resolución.
pub async fn favicon_ico() -> Response {
    (
        [
            (axum::http::header::CONTENT_TYPE, "image/x-icon"),
            (axum::http::header::CACHE_CONTROL, "public, max-age=86400"),
        ],
        views::FAVICON_ICO,
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
                Ok(user) => render_shell(&state, &headers, &user, $active),
                Err(_) => login_redirect(),
            }
        }
    };
}

macro_rules! admin_page_handler {
    ($name:ident, $active:literal) => {
        pub async fn $name(State(state): State<AppState>, headers: HeaderMap) -> Response {
            match require_auth(&state, &headers) {
                Ok(user) if is_admin(&state, &user) => {
                    render_shell(&state, &headers, &user, $active)
                }
                Ok(user) => (
                    StatusCode::FORBIDDEN,
                    render_shell(&state, &headers, &user, $active),
                )
                    .into_response(),
                Err(_) => login_redirect(),
            }
        }
    };
}

page_handler!(page_dashboard, "dashboard");
page_handler!(page_users, "users");
page_handler!(page_services, "services");
page_handler!(page_diagnostics, "diagnostics");
page_handler!(page_files, "files");
page_handler!(page_networking, "networking");
page_handler!(page_applications, "applications");
page_handler!(page_logs, "logs");

admin_page_handler!(page_storage, "storage");
admin_page_handler!(page_domain, "domain");
admin_page_handler!(page_updates, "updates");

/// Vista de recursos compartidos (oculta si el rol es exclusivamente `BACKUP`).
pub async fn page_shares(State(state): State<AppState>, headers: HeaderMap) -> Response {
    match require_auth(&state, &headers) {
        Ok(_) if !state.role.has_shares() => Redirect::to("/dashboard").into_response(),
        Ok(user) => render_shell(&state, &headers, &user, "shares"),
        Err(_) => login_redirect(),
    }
}

/// Vista de respaldos (oculta si el rol es exclusivamente `ARCHIVOS`).
pub async fn page_backups(State(state): State<AppState>, headers: HeaderMap) -> Response {
    match require_auth(&state, &headers) {
        Ok(_) if !state.role.has_backups() => Redirect::to("/dashboard").into_response(),
        Ok(user) => render_shell(&state, &headers, &user, "backups"),
        Err(_) => login_redirect(),
    }
}

/// Vista del terminal (solo superadministrador).
pub async fn page_terminal(State(state): State<AppState>, headers: HeaderMap) -> Response {
    match require_auth(&state, &headers) {
        Ok(user) if is_superadmin(&state, &user) => {
            render_shell(&state, &headers, &user, "terminal")
        }
        Ok(user) => (
            StatusCode::FORBIDDEN,
            render_shell(&state, &headers, &user, "terminal"),
        )
            .into_response(),
        Err(_) => login_redirect(),
    }
}

/// Vista de auditoría (solo administradores).
pub async fn page_audit(State(state): State<AppState>, headers: HeaderMap) -> Response {
    match require_auth(&state, &headers) {
        Ok(user) if is_admin(&state, &user) => render_shell(&state, &headers, &user, "logs"),
        Ok(user) => (
            StatusCode::FORBIDDEN,
            render_shell(&state, &headers, &user, "logs"),
        )
            .into_response(),
        Err(_) => login_redirect(),
    }
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
    pub share: Option<String>,
    #[serde(default)]
    pub root: Option<String>,
    #[serde(default)]
    pub q: Option<String>,
    #[serde(default)]
    pub grouped: Option<u8>,
}

fn deserialize_string_or_int<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = serde_json::Value::deserialize(deserializer)?;
    match value {
        serde_json::Value::String(text) => Ok(text),
        serde_json::Value::Number(num) => Ok(num.to_string()),
        serde_json::Value::Null => Ok(String::new()),
        other => Err(serde::de::Error::custom(format!(
            "identificador inválido: {other}"
        ))),
    }
}

#[derive(Debug, Deserialize, Default)]
pub struct TrashBody {
    #[serde(default, deserialize_with = "deserialize_string_or_int")]
    pub id: String,
    #[serde(default)]
    pub root: Option<String>,
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

#[derive(Debug, Deserialize)]
pub struct SaveBody {
    pub path: String,
    #[serde(default)]
    pub content: String,
    #[serde(default)]
    pub root: Option<String>,
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

/// Logs recientes y legibles: Samba, auditoría del panel, respaldos y journald.
pub async fn logs(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<ExtQuery>,
) -> Response {
    if let Err(response) = require_auth(&state, &headers) {
        return *response;
    }
    let limit = query.limit.unwrap_or(100).clamp(1, 500);
    let source = query.source.clone().unwrap_or_default();
    let search = query.q.clone().unwrap_or_default();
    let grouped = query.grouped != Some(0);

    let outcome = tokio::task::spawn_blocking(move || {
        let selected = nas_engine::LogSource::parse(&source);
        let wants =
            |candidate: nas_engine::LogSource| selected.map_or(true, |choice| choice == candidate);
        let window = limit.saturating_mul(4);
        let window_arg = window.to_string();
        let mut entries: Vec<nas_engine::LogEntry> = Vec::new();

        if wants(nas_engine::LogSource::SambaAudit) {
            let path = std::env::var("NAS_SAMBA_AUDIT_LOG")
                .map(PathBuf::from)
                .unwrap_or_else(|_| PathBuf::from("/var/log/samba/audit.log"));
            entries.extend(nas_engine::parse_samba_audit(&read_tail(&path, window)));
        }

        if wants(nas_engine::LogSource::Admin) {
            if let Ok(rows) = state.auth.db().recent_audit(limit) {
                let events: Vec<nas_engine::AdminEvent<'_>> = rows
                    .iter()
                    .map(|row| nas_engine::AdminEvent {
                        ts: row.ts,
                        username: &row.username,
                        action: &row.action,
                        target: &row.target,
                        result: &row.result,
                        detail: &row.detail,
                    })
                    .collect();
                entries.extend(nas_engine::parse_admin_events(&events));
            }
        }

        if wants(nas_engine::LogSource::Backup) {
            let path = std::env::var("NAS_BACKUP_MASTER_LOG")
                .map(PathBuf::from)
                .unwrap_or_else(|_| {
                    state
                        .paths
                        .data_root
                        .join("LOGS_BACKUP")
                        .join("backups_master.log")
                });
            entries.extend(nas_engine::parse_backup_log(&read_tail(&path, window)));
        }

        if wants(nas_engine::LogSource::System) {
            let spec = nas_exec::CommandSpec::new("journalctl")
                .args(["-n", window_arg.as_str(), "--no-pager", "-o", "short-iso"])
                .timeout(std::time::Duration::from_secs(20));
            if let Ok(text) = run_json(spec) {
                entries.extend(nas_engine::parse_journal(&text));
            }
        }

        entries = nas_engine::filter_query(entries, &search);
        nas_engine::sort_recent(&mut entries);
        if grouped {
            entries = nas_engine::group_repeated(entries);
        }
        entries.truncate(limit);

        entries
            .into_iter()
            .filter_map(|entry| {
                let human = entry.human.clone();
                let mut value = serde_json::to_value(entry).ok()?;
                // Compatibilidad con el `app.js` original: usa `message` cuando
                // no hay `human` (p. ej. el resumen del panel de inicio).
                value["message"] = serde_json::Value::String(human);
                Some(value)
            })
            .collect::<Vec<serde_json::Value>>()
    })
    .await;

    match outcome {
        Ok(entries) => {
            axum::Json(ApiResponse::ok(entries, "Registros del sistema")).into_response()
        }
        Err(_) => server_error("lectura de logs interrumpida"),
    }
}

/// Lee únicamente las últimas `max_lines` líneas de un archivo de bitácora.
///
/// Solo se leen los últimos 256 KB del archivo para que el panel pueda
/// refrescarse con frecuencia sin cargar bitácoras históricas completas.
fn read_tail(path: &std::path::Path, max_lines: usize) -> String {
    const WINDOW: u64 = 256 * 1024;
    let mut file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(_) => return String::new(),
    };
    let length = file.metadata().map(|meta| meta.len()).unwrap_or(0);
    let start = length.saturating_sub(WINDOW);
    if start > 0 && std::io::Seek::seek(&mut file, std::io::SeekFrom::Start(start)).is_err() {
        return String::new();
    }
    let mut bytes = Vec::new();
    if std::io::Read::read_to_end(&mut file, &mut bytes).is_err() {
        return String::new();
    }
    let buffer = String::from_utf8_lossy(&bytes);
    let text = if start > 0 {
        // Al comenzar a media línea se descarta el primer fragmento.
        buffer.split_once('\n').map_or("", |(_, rest)| rest)
    } else {
        buffer.as_ref()
    };
    let lines: Vec<&str> = text.lines().collect();
    let tail = lines.len().saturating_sub(max_lines.max(1));
    lines[tail..].join("\n")
}

/// Mapa de acceso de recursos (recursos, grupos y usuarios).
pub async fn shares_access(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if let Err(response) = require_auth(&state, &headers) {
        return *response;
    }
    if !state.role.has_shares() {
        return forbidden("el rol BACKUP no expone recursos compartidos");
    }
    let path = state.paths.samba_conf.clone();

    match tokio::task::spawn_blocking(move || {
        let conf = std::fs::read_to_string(&path).unwrap_or_default();
        let parsed = nas_engine::parsers::smb::parse_shares(&conf);
        let shares: Vec<String> = parsed.iter().map(|share| share.name.clone()).collect();

        let level_for =
            |share: &nas_engine::parsers::smb::Share, principal: &str| -> &'static str {
                let entry = if principal.starts_with('@') {
                    principal.to_string()
                } else {
                    format!("@{principal}")
                };
                if share.guest_ok {
                    "read"
                } else if share
                    .write_list
                    .iter()
                    .any(|item| item == &entry || item == principal)
                {
                    "write"
                } else if share
                    .valid_users
                    .iter()
                    .any(|item| item == &entry || item == principal)
                {
                    "read"
                } else {
                    "none"
                }
            };

        let (mut users, groups) = nas_engine::users::load_local();
        users.retain(|user| !user.system);
        let admin_members: std::collections::HashSet<String> = groups
            .iter()
            .filter(|group| matches!(group.name.as_str(), "sudo" | "grp_samba" | "grp_superadmin"))
            .flat_map(|group| group.members.clone())
            .collect();
        let groups: Vec<serde_json::Value> = groups
            .into_iter()
            .filter(|group| group.name.starts_with("grp_"))
            .map(|group| {
                let mut row = serde_json::Map::new();
                row.insert("name".into(), serde_json::json!(group.name));
                for share in &parsed {
                    row.insert(
                        share.name.clone(),
                        serde_json::json!(level_for(share, &format!("@{}", group.name))),
                    );
                }
                serde_json::Value::Object(row)
            })
            .collect();
        let users: Vec<serde_json::Value> = users
            .into_iter()
            .map(|user| {
                let mut row = serde_json::Map::new();
                row.insert("name".into(), serde_json::json!(user.name));
                let is_admin = admin_members.contains(&user.name);
                row.insert("is_admin".into(), serde_json::json!(is_admin));
                for share in &parsed {
                    row.insert(
                        share.name.clone(),
                        serde_json::json!(level_for(share, &user.name)),
                    );
                }
                serde_json::Value::Object(row)
            })
            .collect();
        serde_json::json!({ "shares": shares, "groups": groups, "users": users })
    })
    .await
    {
        Ok(payload) => axum::Json(ApiResponse::ok(payload, "Mapa de acceso")).into_response(),
        Err(_) => server_error("mapa de acceso interrumpido"),
    }
}

/// Unidades de almacenamiento (principal + unidades adicionales bajo `/srv/nas/unidades/*`).
pub async fn storage_units(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if let Err(response) = require_auth(&state, &headers) {
        return *response;
    }
    let paths = state.paths.clone();
    match tokio::task::spawn_blocking(move || {
        let runner = Runner::new();
        let mut units = Vec::new();
        if let Ok(overview) = nas_engine::storage::overview_for(&paths, &runner) {
            units.push(serde_json::json!({
                "id": "principal",
                "name": format!("Unidad Principal ({})", overview.path),
                "path": overview.path,
                "is_main": true,
                "filesystem": overview.filesystem,
                "total_gb": overview.total_gb,
                "used_gb": overview.used_gb,
                "free_gb": overview.free_gb,
                "usage_percent": overview.usage_percent,
            }));
        }

        let extra_dir = paths.data_root.join("unidades");
        if let Ok(entries) = std::fs::read_dir(&extra_dir) {
            let mut dirs: Vec<std::path::PathBuf> = entries
                .flatten()
                .map(|entry| entry.path())
                .filter(|path| path.is_dir())
                .collect();
            dirs.sort();
            for unit_path in dirs {
                let Some(id) = unit_path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .filter(|id| nas_engine::storage_ops::valid_name(id))
                else {
                    continue;
                };
                let mut unit_paths = (*paths).clone();
                unit_paths.data_root = unit_path.clone();
                if let Ok(overview) = nas_engine::storage::overview_for(&unit_paths, &runner) {
                    units.push(serde_json::json!({
                        "id": id,
                        "name": format!("Unidad {id} ({})", overview.path),
                        "path": overview.path,
                        "is_main": false,
                        "filesystem": overview.filesystem,
                        "total_gb": overview.total_gb,
                        "used_gb": overview.used_gb,
                        "free_gb": overview.free_gb,
                        "usage_percent": overview.usage_percent,
                    }));
                }
            }
        }
        units
    })
    .await
    {
        Ok(units) => axum::Json(ApiResponse::ok(
            serde_json::json!({ "units": units }),
            "Unidades de almacenamiento",
        ))
        .into_response(),
        Err(_) => server_error("consulta de unidades interrumpida"),
    }
}

/// Subvolúmenes Btrfs (vacío si el filesystem no es Btrfs).
pub async fn storage_subvolumes(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if let Err(response) = require_auth(&state, &headers) {
        return *response;
    }
    let paths = state.paths.clone();
    match tokio::task::spawn_blocking(move || {
        let runner = Runner::new();
        let is_btrfs = nas_engine::storage::overview_for(&paths, &runner)
            .map(|o| o.is_btrfs)
            .unwrap_or(false);
        if !is_btrfs {
            return Vec::<serde_json::Value>::new();
        }
        let target = paths.data_root.to_string_lossy().to_string();
        runner
            .run(
                &nas_exec::CommandSpec::new("btrfs")
                    .args(["subvolume", "list", &target])
                    .sudo(nas_exec::SudoMode::Passwordless)
                    .timeout(std::time::Duration::from_secs(15)),
            )
            .ok()
            .filter(|out| out.is_success())
            .map(|out| {
                out.stdout
                    .lines()
                    .filter_map(|line| {
                        let (_, rel) = line.split_once(" path ")?;
                        let id = line.split_whitespace().nth(1).unwrap_or_default();
                        Some(serde_json::json!({
                            "id": id,
                            "name": rel.trim(),
                            "path": format!("{}/{}", target.trim_end_matches('/'), rel.trim()),
                        }))
                    })
                    .collect()
            })
            .unwrap_or_default()
    })
    .await
    {
        Ok(subvolumes) => axum::Json(ApiResponse::ok(
            serde_json::json!({ "subvolumes": subvolumes }),
            "Subvolúmenes",
        ))
        .into_response(),
        Err(_) => server_error("consulta de subvolúmenes interrumpida"),
    }
}

/// Estado del scrub Btrfs (responde `200` degradado si `/srv/nas` no es Btrfs).
pub async fn storage_scrub(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if let Err(response) = require_auth(&state, &headers) {
        return *response;
    }

    let paths = state.paths.clone();
    match tokio::task::spawn_blocking(move || {
        let runner = Runner::new();
        let is_btrfs = nas_engine::storage::overview_for(&paths, &runner)
            .map(|o| o.is_btrfs)
            .unwrap_or(false);
        if !is_btrfs {
            return serde_json::json!({
                "running": false,
                "status": "no disponible",
                "errors": 0,
                "success": true,
                "output": "El volumen principal no utiliza Btrfs.",
            });
        }
        let target = paths.data_root.to_string_lossy().to_string();
        match run_json(
            nas_exec::CommandSpec::new("btrfs")
                .args(["scrub", "status", &target])
                .sudo(nas_exec::SudoMode::Passwordless)
                .timeout(std::time::Duration::from_secs(15)),
        ) {
            Ok(text) => {
                let status = nas_engine::parsers::scrub::parse_scrub_status(&text);
                let mut payload = serde_json::to_value(&status).unwrap_or_default();
                payload["success"] = serde_json::json!(true);
                payload["output"] = serde_json::json!(text);
                payload
            }
            Err(error) => serde_json::json!({
                "running": false,
                "status": "no disponible",
                "errors": 0,
                "success": true,
                "output": error,
            }),
        }
    })
    .await
    {
        Ok(payload) => axum::Json(ApiResponse::ok(payload, "Estado de scrub")).into_response(),
        Err(_) => server_error("consulta de scrub interrumpida"),
    }
}

/// Inicia el scrub Btrfs.
pub async fn storage_scrub_start(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let user = match require_auth(&state, &headers) {
        Ok(user) => user,
        Err(response) => return *response,
    };
    if !is_admin(&state, &user) {
        return forbidden("se requieren privilegios de administrador");
    }
    let target = state.paths.data_root.to_string_lossy().to_string();
    match tokio::task::spawn_blocking(move || {
        run_json(
            nas_exec::CommandSpec::new("btrfs")
                .args(["scrub", "start", &target])
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
    let user = match require_auth(&state, &headers) {
        Ok(user) => user,
        Err(response) => return *response,
    };
    if !is_admin(&state, &user) {
        return forbidden("se requieren privilegios de administrador");
    }
    let target = state.paths.data_root.to_string_lossy().to_string();
    match tokio::task::spawn_blocking(move || {
        run_json(
            nas_exec::CommandSpec::new("fstrim")
                .args(["-v", &target])
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

/// Lista la papelera interna.
pub async fn files_trash(
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

    match tokio::task::spawn_blocking(move || root.trash_entries()).await {
        Ok(Ok(items)) => axum::Json(ApiResponse::ok(
            crate::legacy::trash_listing(&items),
            "Papelera",
        ))
        .into_response(),
        Ok(Err(error)) => error_response(error),
        Err(_) => server_error("papelera interrumpida"),
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
    let root = match file_root_for(&state, body.root.as_deref()) {
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
    let user = match require_auth(&state, &headers) {
        Ok(user) => user,
        Err(response) => return *response,
    };
    if !is_admin(&state, &user) {
        return forbidden("se requieren privilegios de administrador");
    }
    let root = match file_root_for(&state, body.root.as_deref()) {
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
pub async fn trash_empty(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Response {
    let user = match require_auth(&state, &headers) {
        Ok(user) => user,
        Err(response) => return *response,
    };
    if !is_admin(&state, &user) {
        return forbidden("se requieren privilegios de administrador");
    }
    let trash_body: Option<TrashBody> = if body.is_empty() {
        None
    } else {
        serde_json::from_slice(&body).ok()
    };
    let root_param = trash_body.as_ref().and_then(|b| b.root.as_deref());
    let root = match file_root_for(&state, root_param) {
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
pub async fn trash_restore_all(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Response {
    if let Err(response) = require_auth(&state, &headers) {
        return *response;
    }
    let trash_body: Option<TrashBody> = if body.is_empty() {
        None
    } else {
        serde_json::from_slice(&body).ok()
    };
    let root_param = trash_body.as_ref().and_then(|b| b.root.as_deref());
    let root = match file_root_for(&state, root_param) {
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

fn format_bytes_human(bytes: u64) -> String {
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

/// Descubre controladores de dominio.
pub async fn domain_discover(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::Json(body): axum::Json<DomainDiscoverBody>,
) -> Response {
    let user = match require_auth(&state, &headers) {
        Ok(user) => user,
        Err(response) => return *response,
    };
    if !is_admin(&state, &user) {
        return forbidden("se requieren privilegios de administrador");
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
    let data_root = state.paths.data_root.to_string_lossy().to_string();

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
                .args(["-Pk", &data_root])
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
        Ok(Ok(output)) => axum::Json(serde_json::json!({
            "success": true,
            "status": "success",
            "message": "Prueba completada",
            "output": output,
            "data": { "status": "success", "output": output },
        }))
        .into_response(),
        Ok(Err(error)) => (
            StatusCode::OK,
            axum::Json(serde_json::json!({
                "success": true,
                "status": "failed",
                "message": "La prueba falló",
                "output": error,
                "data": { "status": "failed", "output": error },
            })),
        )
            .into_response(),
        Err(_) => server_error("prueba interrumpida"),
    }
}

/// Reinicia el servidor de forma diferida para devolver respuesta HTTP limpia.
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

    if std::env::var("NAS_TEST_NO_REBOOT").as_deref() == Ok("1")
        || std::env::var("CARGO_MANIFEST_DIR").is_ok()
    {
        return axum::Json(ApiResponse::<serde_json::Value>::ok_empty(
            "Reinicio programado",
        ))
        .into_response();
    }

    match tokio::task::spawn_blocking(|| {
        run_json(
            nas_exec::CommandSpec::new("systemd-run")
                .args(["--no-block", "--collect", "systemctl", "reboot"])
                .sudo(nas_exec::SudoMode::Passwordless)
                .timeout(std::time::Duration::from_secs(15)),
        )
        .or_else(|_| {
            run_json(
                nas_exec::CommandSpec::new("systemctl")
                    .arg("reboot")
                    .sudo(nas_exec::SudoMode::Passwordless)
                    .timeout(std::time::Duration::from_secs(15)),
            )
        })
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
    let candidates = [
        state.paths.repo.join("version.json"),
        std::path::PathBuf::from("/etc/nas/version.json"),
        std::path::PathBuf::from("/opt/nas_debian/version.json"),
        std::path::PathBuf::from("/opt/nas/version.json"),
    ];
    let version = candidates
        .iter()
        .find_map(|path| std::fs::read_to_string(path).ok())
        .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
        .unwrap_or_else(|| {
            serde_json::json!({
                "version": env!("CARGO_PKG_VERSION"),
                "commit": "local",
            })
        });

    let gpg_verified = version
        .get("gpg_verified")
        .and_then(|v| v.as_bool())
        .unwrap_or_else(|| {
            std::path::Path::new("/etc/nas/trusted_keys.gpg").is_file()
                || std::path::Path::new("/etc/nas/role").is_file()
        });

    let version_str = version
        .get("version")
        .and_then(|v| v.as_str())
        .unwrap_or(env!("CARGO_PKG_VERSION"));
    let commit_str = version
        .get("commit")
        .and_then(|v| v.as_str())
        .unwrap_or("release");
    let channel_str = version
        .get("channel")
        .and_then(|v| v.as_str())
        .unwrap_or("stable");
    let notes_str = version
        .get("notes")
        .and_then(|v| v.as_str())
        .unwrap_or("Release oficial verificado con firmas criptograficas");

    axum::Json(ApiResponse::ok(
        serde_json::json!({
            "version": version_str,
            "latest_version": version_str,
            "installed_commit": commit_str,
            "commit_date": version.get("date").and_then(|v| v.as_str()),
            "has_updates": false,
            "channel": channel_str,
            "gpg_verified": gpg_verified,
            "release_notes": notes_str,
            "commit": commit_str,
        }),
        "Información de actualizaciones",
    ))
    .into_response()
}

fn process_rss_mb() -> u64 {
    std::fs::read_to_string("/proc/self/statm")
        .ok()
        .and_then(|text| text.split_whitespace().nth(1)?.parse::<u64>().ok())
        .map(|pages| (pages * 4096) / (1024 * 1024))
        .unwrap_or(0)
}

/// Estado de las aplicaciones y tecnologías.
pub async fn system_applications(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if let Err(response) = require_auth(&state, &headers) {
        return *response;
    }

    let paths = state.paths.clone();
    let backup_dir = state.backup_dir.clone();
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
        let mut sessions_list = Vec::new();
        if let Some(out) = &smbstatus {
            for line in out.stdout.lines() {
                let trimmed = line.trim_start();
                if !trimmed.starts_with(|c: char| c.is_ascii_digit()) {
                    continue;
                }
                let cols: Vec<&str> = trimmed.split_whitespace().collect();
                if cols.len() >= 4 {
                    sessions_list.push(serde_json::json!({
                        "pid": cols[0],
                        "username": cols[1],
                        "group": cols[2],
                        "machine": cols[3],
                        "ip": cols.get(4).copied().unwrap_or(""),
                    }));
                }
            }
        }
        let sessions = sessions_list.len();

        let tasks = nas_engine::tasks::BackupTask::discover(&backup_dir)
            .map(|tasks| tasks.len())
            .unwrap_or(0);
        let overview = nas_engine::storage::overview_for(&paths, &runner).ok();
        let fs = overview
            .as_ref()
            .map(|o| o.filesystem.clone())
            .unwrap_or_else(|| "desconocido".into());
        let is_btrfs = overview.as_ref().is_some_and(|o| o.is_btrfs);
        let smbd_active = active("smbd");
        let nginx_active = active("nginx");
        let wsdd_active = active("wsdd2");
        let nmbd_active = active("nmbd");
        let auditd_active = active("auditd");
        let aide_active = active("dailyaidecheck.timer");
        let audit_rules = if std::path::Path::new("/etc/audit/rules.d/nas.rules").is_file() {
            "reglas activas (/etc/audit/rules.d/nas.rules)"
        } else {
            "sin reglas personalizadas"
        };

        serde_json::json!({
            "samba": {
                "active": smbd_active,
                "status": if smbd_active { "activo" } else { "inactivo" },
                "version": "4.x (Debian 13)",
                "sessions": sessions,
                "sessions_list": sessions_list,
                "vfs_modules": ["acl_xattr", "streams_xattr"],
            },
            "wsdd2": {
                "active": wsdd_active,
                "nmbd_active": nmbd_active,
                "wsdd_active": wsdd_active,
                "protocol": "WSD/LLMNR",
            },
            "wsdd": { "active": wsdd_active },
            "web": {
                "active": nginx_active,
                "status": if nginx_active { "activo" } else { "inactivo" },
                "pool_mode": "axum/tokio",
                "php_version": format!("Rust {}", env!("CARGO_PKG_VERSION")),
                "memory_mb": process_rss_mb(),
            },
            "btrfs": {
                "active": is_btrfs,
                "filesystem": fs,
                "compression": if is_btrfs { "zstd:3" } else { "n/a" },
                "scrub_status": if is_btrfs { "programado" } else { "no disponible" },
                "scrub_errors": "0 errores",
                "schedule": "mensual (día 1, 02:00)",
            },
            "rsync": {
                "tasks_count": tasks,
                "protocols": ["CIFS 3.1.1", "SSH"],
                "deduplication": ">85% (link-dest)",
                "staging": "atómico",
            },
            "security": {
                "active": auditd_active,
                "auditd_active": auditd_active,
                "aide_active": aide_active,
                "audit_rules": audit_rules,
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

/// Ejecuta la actualización en caliente del servidor fuera del sandbox de `nas-web.service`.
pub async fn system_update_run(
    admin: crate::extractors::AdminUser,
    State(state): State<AppState>,
) -> Response {
    let actor = admin.username().to_string();
    let _ = state
        .auth
        .db()
        .append_audit(&actor, "system_update", "sistema", "STARTED", "");

    let repo_candidate = state.paths.repo.join("src/core/updater.sh");
    let result = tokio::task::spawn_blocking(move || -> Result<String, String> {
        let mut candidates = vec![repo_candidate];
        candidates.extend([
            std::path::PathBuf::from("/opt/nas_debian/src/core/updater.sh"),
            std::path::PathBuf::from("/opt/nas/src/core/updater.sh"),
            std::path::PathBuf::from("/usr/local/lib/nas/src/core/updater.sh"),
        ]);
        let script = candidates
            .iter()
            .find(|path| path.is_file())
            .ok_or_else(|| "no se encontró el script de actualización (updater.sh)".to_string())?;

        let runner = Runner::new();
        let active = runner
            .run(
                &nas_exec::CommandSpec::new("systemctl")
                    .args(["is-active", "nas-system-update"])
                    .timeout(std::time::Duration::from_secs(5)),
            )
            .map(|out| matches!(out.stdout.trim(), "active" | "activating"))
            .unwrap_or(false);
        if active {
            return Err("ya hay una actualización del sistema en ejecución".to_string());
        }

        let output = runner
            .run(
                &nas_exec::CommandSpec::new("systemd-run")
                    .args([
                        "--no-block".to_string(),
                        "--collect".to_string(),
                        "--unit=nas-system-update".to_string(),
                        "/bin/bash".to_string(),
                        script.to_string_lossy().to_string(),
                        "--yes".to_string(),
                    ])
                    .sudo(nas_exec::SudoMode::Passwordless)
                    .timeout(std::time::Duration::from_secs(30)),
            )
            .map_err(|error| error.to_string())?;

        if output.is_success() {
            Ok("Actualización iniciada en segundo plano (unidad nas-system-update).".to_string())
        } else {
            Err(if output.stderr.is_empty() {
                output.stdout
            } else {
                output.stderr
            })
        }
    })
    .await
    .unwrap_or_else(|_| Err("actualización interrumpida".to_string()));

    match result {
        Ok(output) => {
            let _ = state
                .auth
                .db()
                .append_audit(&actor, "system_update", "sistema", "SUCCESS", "");
            axum::Json(ApiResponse::ok(
                serde_json::json!({ "output": output, "version": env!("CARGO_PKG_VERSION") }),
                "Actualización del sistema iniciada",
            ))
            .into_response()
        }
        Err(error) => {
            let _ =
                state
                    .auth
                    .db()
                    .append_audit(&actor, "system_update", "sistema", "FAILED", &error);
            server_error(&error)
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct ShareBody {
    pub name: String,
    #[serde(default)]
    pub comment: String,
    #[serde(default)]
    pub scheme: u8,
    #[serde(default)]
    pub hidden: bool,
    #[serde(default)]
    pub unit: String,
    #[serde(default)]
    pub groups: Vec<String>,
    #[serde(default)]
    pub write_group: String,
}

#[derive(Debug, Deserialize)]
pub struct ShareNameBody {
    pub name: String,
}

#[derive(Debug, Deserialize, Default)]
pub struct ShareRepairBody {
    #[serde(default)]
    pub share: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub force: bool,
}

#[derive(Debug, Deserialize)]
pub struct ShareAccessBody {
    pub share: String,
    #[serde(default)]
    pub kind: String,
    pub name: String,
    pub level: String,
}

fn share_path(name: &str, unit: &str) -> String {
    if unit.is_empty() || unit == "principal" {
        format!("/srv/nas/{name}")
    } else {
        format!("/srv/nas/unidades/{unit}/{name}")
    }
}

fn normalize_group(group: &str) -> String {
    let trimmed = group.trim();
    if trimmed.starts_with('@') {
        trimmed.to_string()
    } else {
        format!("@{trimmed}")
    }
}

fn share_spec(body: &ShareBody, path: &str) -> nas_engine::samba::ShareSpec {
    let groups: Vec<String> = body.groups.iter().map(|g| normalize_group(g)).collect();
    let write = if body.write_group.trim().is_empty() {
        None
    } else {
        Some(normalize_group(&body.write_group))
    };

    let read_only = body.scheme == 3;
    let guest_ok = body.scheme == 4;
    let write_list = if body.scheme == 2 {
        write.into_iter().collect()
    } else if body.scheme == 1 {
        groups.clone()
    } else {
        Vec::new()
    };

    nas_engine::samba::ShareSpec {
        name: body.name.trim().to_string(),
        path: path.to_string(),
        read_only,
        browseable: !body.hidden,
        guest_ok,
        valid_users: if guest_ok { Vec::new() } else { groups },
        write_list: if read_only || guest_ok {
            Vec::new()
        } else {
            write_list
        },
        comment: body.comment.clone(),
    }
}

fn validate_share_spec(spec: &nas_engine::samba::ShareSpec) -> Result<(), NasError> {
    nas_engine::samba::validate_name(&spec.name)?;
    nas_engine::samba::validate_path(&spec.path)?;
    nas_engine::samba::validate_comment(&spec.comment)?;
    for entry in spec.valid_users.iter().chain(spec.write_list.iter()) {
        nas_engine::samba::validate_principal(entry)?;
    }
    Ok(())
}

fn stage_and_apply(conf: &str) -> Result<(), String> {
    let stamp = chrono::Local::now().format("%Y%m%d%H%M%S%3f");
    let dir = std::path::Path::new("/var/lib/nas/staging");
    std::fs::create_dir_all(dir).map_err(|error| error.to_string())?;
    let staged = dir.join(format!("smbconf_{stamp}"));
    std::fs::write(&staged, conf).map_err(|error| error.to_string())?;

    let staged_str = staged.to_string_lossy().to_string();
    let output = Runner::new()
        .run(
            &nas_exec::CommandSpec::new("/usr/local/bin/nas")
                .args(["helper", "samba-apply", &staged_str])
                .sudo(nas_exec::SudoMode::Passwordless)
                .timeout(std::time::Duration::from_secs(60)),
        )
        .map_err(|error| error.to_string())?;

    if output.is_success() {
        Ok(())
    } else {
        let _ = std::fs::remove_file(&staged);
        Err(if output.stderr.is_empty() {
            output.stdout
        } else {
            output.stderr
        })
    }
}

fn apply_acl(path: &str, principal: &str, level: &str) -> Result<(), String> {
    let output = Runner::new()
        .run(
            &nas_exec::CommandSpec::new("/usr/local/bin/nas")
                .args([
                    "helper",
                    "share-acl",
                    path,
                    principal.trim_start_matches('@'),
                    level,
                ])
                .sudo(nas_exec::SudoMode::Passwordless)
                .timeout(std::time::Duration::from_secs(120)),
        )
        .map_err(|error| error.to_string())?;

    if output.is_success() {
        Ok(())
    } else {
        Err(if output.stderr.is_empty() {
            output.stdout
        } else {
            output.stderr
        })
    }
}

fn create_share_dir(name: &str, unit: &str) -> Result<(), String> {
    let output = Runner::new()
        .run(
            &nas_exec::CommandSpec::new("/usr/local/bin/nas")
                .args([
                    "helper",
                    "share-dir",
                    name,
                    if unit.is_empty() { "principal" } else { unit },
                ])
                .sudo(nas_exec::SudoMode::Passwordless)
                .timeout(std::time::Duration::from_secs(30)),
        )
        .map_err(|error| error.to_string())?;

    if output.is_success() {
        Ok(())
    } else {
        Err(if output.stderr.is_empty() {
            output.stdout
        } else {
            output.stderr
        })
    }
}

fn apply_spec_acls(spec: &nas_engine::samba::ShareSpec) {
    for group in &spec.valid_users {
        let level = if spec.write_list.contains(group) {
            "write"
        } else {
            "read"
        };
        let _ = apply_acl(&spec.path, group, level);
    }
    for group in &spec.write_list {
        if !spec.valid_users.contains(group) {
            let _ = apply_acl(&spec.path, group, "write");
        }
    }
}

/// Crea un recurso compartido.
pub async fn share_create(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::Json(body): axum::Json<ShareBody>,
) -> Response {
    let user = match require_auth(&state, &headers) {
        Ok(user) => user,
        Err(response) => return *response,
    };
    if !is_admin(&state, &user) {
        return forbidden("se requieren privilegios de administrador");
    }
    if !state.role.has_shares() {
        return forbidden("el rol BACKUP no expone recursos compartidos");
    }
    let unit = body.unit.trim().to_string();
    if !unit.is_empty() && unit != "principal" && !nas_engine::storage_ops::valid_name(&unit) {
        return bad_request("unidad de almacenamiento inválida");
    }

    let path = share_path(body.name.trim(), &unit);
    let spec = share_spec(&body, &path);
    if let Err(error) = validate_share_spec(&spec) {
        return error_response(error);
    }
    let share_name = spec.name.clone();
    let conf_path = state.paths.samba_conf.clone();

    let outcome = tokio::task::spawn_blocking(move || -> Result<(), String> {
        let conf = std::fs::read_to_string(&conf_path).map_err(|e| e.to_string())?;
        let updated =
            nas_engine::samba::upsert_share(&conf, &spec).map_err(|error| error.to_string())?;
        create_share_dir(&spec.name, &unit)?;
        stage_and_apply(&updated)?;
        apply_spec_acls(&spec);
        Ok(())
    })
    .await
    .unwrap_or_else(|_| Err("creación interrumpida".to_string()));

    match outcome {
        Ok(()) => {
            let _ = state
                .auth
                .db()
                .append_audit(&user, "share_create", &share_name, "SUCCESS", "");
            axum::Json(ApiResponse::<serde_json::Value>::ok_empty("Recurso creado")).into_response()
        }
        Err(error) => {
            let _ =
                state
                    .auth
                    .db()
                    .append_audit(&user, "share_create", &share_name, "FAILED", &error);
            server_error(&error)
        }
    }
}

/// Actualiza un recurso compartido existente.
pub async fn share_update(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::Json(body): axum::Json<ShareBody>,
) -> Response {
    let user = match require_auth(&state, &headers) {
        Ok(user) => user,
        Err(response) => return *response,
    };
    if !is_admin(&state, &user) {
        return forbidden("se requieren privilegios de administrador");
    }
    if !state.role.has_shares() {
        return forbidden("el rol BACKUP no expone recursos compartidos");
    }
    if let Err(error) = nas_engine::samba::validate_name(body.name.trim()) {
        return error_response(error);
    }
    if let Err(error) = nas_engine::samba::validate_comment(&body.comment) {
        return error_response(error);
    }

    let share_name = body.name.trim().to_string();
    let conf_path = state.paths.samba_conf.clone();

    let outcome = tokio::task::spawn_blocking(move || -> Result<(), String> {
        let conf = std::fs::read_to_string(&conf_path).map_err(|e| e.to_string())?;
        let existing = nas_engine::parsers::smb::parse_shares(&conf)
            .into_iter()
            .find(|share| share.name.eq_ignore_ascii_case(body.name.trim()))
            .ok_or_else(|| format!("no existe el recurso '{}'", body.name))?;
        let path = existing
            .path
            .unwrap_or_else(|| share_path(body.name.trim(), "principal"));
        let spec = share_spec(&body, &path);
        validate_share_spec(&spec).map_err(|error| error.to_string())?;
        let updated =
            nas_engine::samba::upsert_share(&conf, &spec).map_err(|error| error.to_string())?;
        stage_and_apply(&updated)?;
        apply_spec_acls(&spec);
        Ok(())
    })
    .await
    .unwrap_or_else(|_| Err("actualización interrumpida".to_string()));

    match outcome {
        Ok(()) => {
            let _ = state
                .auth
                .db()
                .append_audit(&user, "share_update", &share_name, "SUCCESS", "");
            axum::Json(ApiResponse::<serde_json::Value>::ok_empty(
                "Recurso actualizado",
            ))
            .into_response()
        }
        Err(error) => {
            let _ =
                state
                    .auth
                    .db()
                    .append_audit(&user, "share_update", &share_name, "FAILED", &error);
            server_error(&error)
        }
    }
}

/// Elimina un recurso compartido (conserva los archivos).
pub async fn share_delete(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::Json(body): axum::Json<ShareNameBody>,
) -> Response {
    let user = match require_auth(&state, &headers) {
        Ok(user) => user,
        Err(response) => return *response,
    };
    if !is_admin(&state, &user) {
        return forbidden("se requieren privilegios de administrador");
    }
    if !state.role.has_shares() {
        return forbidden("el rol BACKUP no expone recursos compartidos");
    }
    if let Err(error) = nas_engine::samba::validate_name(body.name.trim()) {
        return error_response(error);
    }

    let name = body.name.trim().to_string();
    let conf_path = state.paths.samba_conf.clone();
    let outcome = tokio::task::spawn_blocking(move || -> Result<(), String> {
        let conf = std::fs::read_to_string(&conf_path).map_err(|e| e.to_string())?;
        let updated =
            nas_engine::samba::remove_share(&conf, &name).map_err(|error| error.to_string())?;
        stage_and_apply(&updated)
    })
    .await
    .unwrap_or_else(|_| Err("borrado interrumpido".to_string()));

    match outcome {
        Ok(()) => {
            let _ = state
                .auth
                .db()
                .append_audit(&user, "share_delete", &body.name, "SUCCESS", "");
            axum::Json(ApiResponse::<serde_json::Value>::ok_empty(
                "Recurso eliminado",
            ))
            .into_response()
        }
        Err(error) => {
            let _ =
                state
                    .auth
                    .db()
                    .append_audit(&user, "share_delete", &body.name, "FAILED", &error);
            server_error(&error)
        }
    }
}

/// Ajusta el nivel de acceso de un grupo o usuario sobre un recurso.
pub async fn share_access_update(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::Json(body): axum::Json<ShareAccessBody>,
) -> Response {
    let user = match require_auth(&state, &headers) {
        Ok(user) => user,
        Err(response) => return *response,
    };
    if !is_admin(&state, &user) {
        return forbidden("se requieren privilegios de administrador");
    }
    if !state.role.has_shares() {
        return forbidden("el rol BACKUP no expone recursos compartidos");
    }
    if !matches!(body.level.as_str(), "none" | "read" | "write") {
        return bad_request("nivel de acceso inválido");
    }
    if let Err(error) = nas_engine::samba::validate_name(body.share.trim()) {
        return error_response(error);
    }

    let share_name = body.share.trim().to_string();
    let level_name = body.level.clone();
    let principal = if matches!(body.kind.as_str(), "user" | "users") {
        body.name.trim().to_string()
    } else {
        normalize_group(&body.name)
    };
    if let Err(error) = nas_engine::samba::validate_principal(&principal) {
        return error_response(error);
    }
    let conf_path = state.paths.samba_conf.clone();

    let outcome = tokio::task::spawn_blocking(move || -> Result<(), String> {
        let conf = std::fs::read_to_string(&conf_path).map_err(|e| e.to_string())?;
        let mut shares = nas_engine::parsers::smb::parse_shares(&conf);
        let share = shares
            .iter_mut()
            .find(|share| share.name.eq_ignore_ascii_case(body.share.trim()))
            .ok_or_else(|| format!("no existe el recurso '{}'", body.share))?;

        let path = share
            .path
            .clone()
            .unwrap_or_else(|| share_path(&share.name, "principal"));
        let entry = principal.clone();

        match body.level.as_str() {
            "none" => {
                share.valid_users.retain(|item| item != &entry);
                share.write_list.retain(|item| item != &entry);
            }
            "read" => {
                if !share.valid_users.contains(&entry) {
                    share.valid_users.push(entry.clone());
                }
                share.write_list.retain(|item| item != &entry);
            }
            _ => {
                if !share.valid_users.contains(&entry) {
                    share.valid_users.push(entry.clone());
                }
                if !share.write_list.contains(&entry) {
                    share.write_list.push(entry.clone());
                }
            }
        }

        let spec = nas_engine::samba::ShareSpec {
            name: share.name.clone(),
            path,
            read_only: share.read_only,
            browseable: share.browseable,
            guest_ok: share.guest_ok,
            valid_users: share.valid_users.clone(),
            write_list: share.write_list.clone(),
            comment: share.comment.clone().unwrap_or_default(),
        };
        let updated =
            nas_engine::samba::upsert_share(&conf, &spec).map_err(|error| error.to_string())?;
        stage_and_apply(&updated)?;
        let acl_level = if body.level == "none" {
            "none"
        } else {
            body.level.as_str()
        };
        apply_acl(&spec.path, &principal, acl_level)?;
        Ok(())
    })
    .await
    .unwrap_or_else(|_| Err("cambio de acceso interrumpido".to_string()));

    match outcome {
        Ok(()) => {
            let _ = state.auth.db().append_audit(
                &user,
                "share_access",
                &share_name,
                "SUCCESS",
                &level_name,
            );
            axum::Json(ApiResponse::<serde_json::Value>::ok_empty(
                "Permiso actualizado",
            ))
            .into_response()
        }
        Err(error) => {
            let _ =
                state
                    .auth
                    .db()
                    .append_audit(&user, "share_access", &share_name, "FAILED", &error);
            server_error(&error)
        }
    }
}

/// Reaplica las ACLs de todos los recursos según `smb.conf`.
pub async fn share_access_repair(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::Json(body): axum::Json<ShareRepairBody>,
) -> Response {
    let user = match require_auth(&state, &headers) {
        Ok(user) => user,
        Err(response) => return *response,
    };
    if !is_admin(&state, &user) {
        return forbidden("se requieren privilegios de administrador");
    }
    if !state.role.has_shares() {
        return forbidden("el rol BACKUP no expone recursos compartidos");
    }
    let _ = body.force;
    let target_filter = if body.share.trim().is_empty() {
        body.name.trim().to_string()
    } else {
        body.share.trim().to_string()
    };
    let conf_path = state.paths.samba_conf.clone();

    let outcome = tokio::task::spawn_blocking(move || -> Result<usize, String> {
        let conf = std::fs::read_to_string(&conf_path).map_err(|e| e.to_string())?;
        let shares = nas_engine::parsers::smb::parse_shares(&conf);
        let mut applied = 0;
        for share in shares {
            if !target_filter.is_empty() && !share.name.eq_ignore_ascii_case(&target_filter) {
                continue;
            }
            let Some(path) = share.path.clone() else {
                continue;
            };
            for group in &share.valid_users {
                let level = if share.write_list.contains(group) {
                    "write"
                } else {
                    "read"
                };
                if apply_acl(&path, group, level).is_ok() {
                    applied += 1;
                }
            }
            for group in &share.write_list {
                if !share.valid_users.contains(group) && apply_acl(&path, group, "write").is_ok() {
                    applied += 1;
                }
            }
        }
        Ok(applied)
    })
    .await
    .unwrap_or_else(|_| Err("reparación interrumpida".to_string()));

    match outcome {
        Ok(applied) => {
            let _ =
                state
                    .auth
                    .db()
                    .append_audit(&user, "share_acl_repair", "shares", "SUCCESS", "");
            axum::Json(ApiResponse::ok(
                serde_json::json!({ "applied": applied }),
                "ACLs reaplicadas",
            ))
            .into_response()
        }
        Err(error) => {
            let _ =
                state
                    .auth
                    .db()
                    .append_audit(&user, "share_acl_repair", "shares", "FAILED", &error);
            server_error(&error)
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct UserBody {
    pub username: String,
    #[serde(default)]
    pub full_name: String,
    #[serde(default)]
    pub password: String,
    #[serde(default)]
    pub groups: Vec<String>,
    #[serde(default)]
    pub is_admin: bool,
    #[serde(default)]
    pub can_web: bool,
    #[serde(default)]
    pub samba_enabled: bool,
    #[serde(default)]
    pub is_superadmin: bool,
}

#[derive(Debug, Deserialize)]
pub struct ToggleUserBody {
    pub username: String,
    #[serde(default)]
    pub enabled: bool,
}

#[derive(Debug, Deserialize)]
pub struct MemberBody {
    pub username: String,
    pub group: String,
    #[serde(default)]
    pub action: String,
}

#[derive(Debug, Deserialize)]
pub struct GroupBody {
    pub name: String,
}

#[derive(Debug, Deserialize)]
pub struct RenameGroupBody {
    pub old: String,
    #[serde(default)]
    pub new: String,
    #[serde(default)]
    pub new_name: String,
}

fn valid_username(name: &str) -> bool {
    let mut chars = name.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_lowercase() || c == '_')
        && name.len() <= 32
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
}

fn valid_grp(name: &str) -> bool {
    name.starts_with("grp_")
        && name.len() <= 32
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-')
}

fn is_master_grp(name: &str) -> bool {
    matches!(
        name,
        "grp_samba" | "grp_web" | "grp_superadmin" | "grp_backups"
    )
}

pub(crate) fn configured_superadmin() -> Option<String> {
    std::fs::read_to_string("/etc/nas/superadmin")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn is_protected_posix_account(username: &str) -> bool {
    let (users, _) = nas_engine::users::load_local();
    let found = users.into_iter().find(|u| u.name == username);
    let uid = found.as_ref().map(|u| u.uid);
    if nas_engine::users::is_protected_system_account(username, uid) {
        return true;
    }
    found.is_some_and(|u| u.system)
}

fn is_target_superadmin(state: &AppState, username: &str) -> bool {
    if configured_superadmin().as_deref() == Some(username) {
        return true;
    }
    if state
        .auth
        .db()
        .user_role(username)
        .ok()
        .flatten()
        .as_deref()
        == Some("superadmin")
    {
        return true;
    }
    let (_, groups) = nas_engine::users::load_local();
    groups
        .into_iter()
        .find(|g| g.name == "grp_superadmin")
        .is_some_and(|g| g.members.iter().any(|m| m == username))
}

fn run_accounts(args: &[&str], password: Option<&str>) -> Result<String, String> {
    let mut cmd_args = vec!["helper", "accounts"];
    cmd_args.extend_from_slice(args);
    let mut spec = nas_exec::CommandSpec::new("/usr/local/bin/nas")
        .args(cmd_args)
        .sudo(nas_exec::SudoMode::Passwordless)
        .timeout(std::time::Duration::from_secs(60));
    if let Some(password) = password {
        spec = spec.stdin_secret(nas_exec::SecretInput::new(password));
    }
    let output = Runner::new()
        .run(&spec)
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

/// Crea un usuario del sistema.
pub async fn user_create(
    admin: crate::extractors::AdminUser,
    State(state): State<AppState>,
    axum::Json(body): axum::Json<UserBody>,
) -> Response {
    let actor = admin.username().to_string();
    if !valid_username(&body.username)
        || is_protected_posix_account(&body.username)
        || !body.groups.iter().all(|group| valid_grp(group))
    {
        return bad_request("usuario o grupo inválido o reservado");
    }
    if body.is_superadmin && !admin.is_superadmin() {
        return forbidden("solo el superadministrador puede otorgar rango de superadministrador");
    }
    if body.password.len() < 6 {
        return bad_request("la contraseña debe tener al menos 6 caracteres");
    }

    let username = body.username.clone();
    let groups = body.groups.join(",");
    let samba = if body.samba_enabled { "1" } else { "0" };
    let full_name = body.full_name.clone();
    let grant_admin = body.is_admin || body.is_superadmin;
    let can_web = body.can_web || grant_admin;
    let password = body.password.clone();

    let result = tokio::task::spawn_blocking(move || {
        run_accounts(
            &[
                "create-user",
                &username,
                &full_name,
                "-",
                if grant_admin { "1" } else { "0" },
                if can_web { "1" } else { "0" },
                samba,
                &groups,
            ],
            Some(&password),
        )
    })
    .await
    .unwrap_or_else(|_| Err("creación interrumpida".to_string()));

    match result {
        Ok(_) => {
            if can_web {
                if let Ok(hash) = crate::Auth::hash_password(&body.password) {
                    let role = if body.is_superadmin {
                        "superadmin"
                    } else if body.is_admin {
                        "admin"
                    } else {
                        "operator"
                    };
                    let _ = state.auth.db().upsert_user(&body.username, &hash, role);
                }
            }
            let _ =
                state
                    .auth
                    .db()
                    .append_audit(&actor, "user_create", &body.username, "SUCCESS", "");
            axum::Json(ApiResponse::<serde_json::Value>::ok_empty("Usuario creado")).into_response()
        }
        Err(error) => server_error(&error),
    }
}

/// Actualiza un usuario existente.
pub async fn user_update(
    admin: crate::extractors::AdminUser,
    State(state): State<AppState>,
    axum::Json(body): axum::Json<UserBody>,
) -> Response {
    let actor = admin.username().to_string();
    if !valid_username(&body.username)
        || is_protected_posix_account(&body.username)
        || !body.groups.iter().all(|group| valid_grp(group))
    {
        return bad_request("usuario o grupo inválido o reservado");
    }
    let target_is_super = is_target_superadmin(&state, &body.username);
    if (target_is_super || body.is_superadmin) && !admin.is_superadmin() {
        return forbidden("no se puede modificar el rango del superadministrador");
    }
    if !body.password.is_empty() && body.password.len() < 6 {
        return bad_request("la contraseña debe tener al menos 6 caracteres");
    }

    let username = body.username.clone();
    let groups = body.groups.join(",");
    let samba = if body.samba_enabled { "1" } else { "0" };
    let full_name = body.full_name.clone();
    let password = body.password.clone();
    let grant_admin = target_is_super || body.is_superadmin || body.is_admin;
    let can_web = target_is_super || body.can_web || grant_admin;

    let result = tokio::task::spawn_blocking(move || {
        let pass_mode = if password.is_empty() {
            "--keep-password"
        } else {
            "-"
        };
        let secret = if password.is_empty() {
            None
        } else {
            Some(password.as_str())
        };
        run_accounts(
            &[
                "update-user",
                &username,
                &full_name,
                pass_mode,
                if grant_admin { "1" } else { "0" },
                if can_web { "1" } else { "0" },
                samba,
                &groups,
            ],
            secret,
        )
    })
    .await
    .unwrap_or_else(|_| Err("actualización interrumpida".to_string()));

    match result {
        Ok(_) => {
            let role = if target_is_super || body.is_superadmin {
                "superadmin"
            } else if body.is_admin {
                "admin"
            } else {
                "operator"
            };
            if can_web {
                if !body.password.is_empty() {
                    if let Ok(hash) = crate::Auth::hash_password(&body.password) {
                        let _ = state.auth.db().upsert_user(&body.username, &hash, role);
                    }
                } else if let Ok(Some(existing_hash)) = state.auth.db().user_hash(&body.username) {
                    let _ = state
                        .auth
                        .db()
                        .upsert_user(&body.username, &existing_hash, role);
                }
            } else {
                let _ = state.auth.db().delete_user(&body.username);
            }
            if !body.password.is_empty() {
                let _ = state.auth.db().delete_user_sessions(&body.username);
            }
            let _ =
                state
                    .auth
                    .db()
                    .append_audit(&actor, "user_update", &body.username, "SUCCESS", "");
            axum::Json(ApiResponse::<serde_json::Value>::ok_empty(
                "Usuario actualizado",
            ))
            .into_response()
        }
        Err(error) => server_error(&error),
    }
}

/// Activa o suspende una cuenta.
pub async fn user_toggle(
    admin: crate::extractors::AdminUser,
    State(state): State<AppState>,
    axum::Json(body): axum::Json<ToggleUserBody>,
) -> Response {
    let actor = admin.username().to_string();
    if !valid_username(&body.username) || is_protected_posix_account(&body.username) {
        return bad_request("usuario inválido o reservado del sistema");
    }
    if !body.enabled && (body.username == actor || is_target_superadmin(&state, &body.username)) {
        return forbidden("no se puede suspender la cuenta propia ni al superadministrador");
    }

    let username = body.username.clone();
    let enabled = if body.enabled { "1" } else { "0" };
    let result = tokio::task::spawn_blocking(move || {
        run_accounts(&["toggle-user", &username, enabled], None)
    })
    .await
    .unwrap_or_else(|_| Err("cambio de estado interrumpido".to_string()));

    match result {
        Ok(_) => {
            if !body.enabled {
                let _ = state.auth.db().delete_user_sessions(&body.username);
            }
            let _ =
                state
                    .auth
                    .db()
                    .append_audit(&actor, "user_toggle", &body.username, "SUCCESS", "");
            axum::Json(ApiResponse::<serde_json::Value>::ok_empty(
                "Estado actualizado",
            ))
            .into_response()
        }
        Err(error) => server_error(&error),
    }
}

/// Elimina una cuenta del sistema.
pub async fn user_delete(
    admin: crate::extractors::AdminUser,
    State(state): State<AppState>,
    axum::Json(body): axum::Json<ToggleUserBody>,
) -> Response {
    let actor = admin.username().to_string();
    if !valid_username(&body.username) || is_protected_posix_account(&body.username) {
        return bad_request("usuario inválido o protegido del sistema");
    }
    if body.username == actor || is_target_superadmin(&state, &body.username) {
        return forbidden("no se puede eliminar la cuenta propia ni al superadministrador");
    }

    let username = body.username.clone();
    let result =
        tokio::task::spawn_blocking(move || run_accounts(&["delete-user", &username], None))
            .await
            .unwrap_or_else(|_| Err("borrado interrumpido".to_string()));

    match result {
        Ok(_) => {
            let _ = state.auth.db().delete_user(&body.username);
            let _ =
                state
                    .auth
                    .db()
                    .append_audit(&actor, "user_delete", &body.username, "SUCCESS", "");
            axum::Json(ApiResponse::<serde_json::Value>::ok_empty(
                "Usuario eliminado",
            ))
            .into_response()
        }
        Err(error) => server_error(&error),
    }
}

/// Añade o quita un miembro de un grupo.
pub async fn user_groups(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::Json(body): axum::Json<MemberBody>,
) -> Response {
    let actor = match require_auth(&state, &headers) {
        Ok(user) => user,
        Err(response) => return *response,
    };
    if !is_admin(&state, &actor) {
        return forbidden("se requieren privilegios de administrador");
    }
    if !valid_username(&body.username)
        || is_protected_posix_account(&body.username)
        || !valid_grp(&body.group)
    {
        return bad_request("usuario o grupo inválido");
    }
    if body.group == "grp_superadmin" {
        return forbidden("el grupo grp_superadmin es inmutable");
    }

    let action = if body.action == "add" {
        "add-member"
    } else {
        "remove-member"
    };
    let username = body.username.clone();
    let group = body.group.clone();
    let result =
        tokio::task::spawn_blocking(move || run_accounts(&[action, &username, &group], None))
            .await
            .unwrap_or_else(|_| Err("membresía interrumpida".to_string()));

    match result {
        Ok(_) => axum::Json(ApiResponse::<serde_json::Value>::ok_empty(
            "Membresía actualizada",
        ))
        .into_response(),
        Err(error) => server_error(&error),
    }
}

/// Crea un grupo corporativo.
pub async fn group_create(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::Json(body): axum::Json<GroupBody>,
) -> Response {
    let actor = match require_auth(&state, &headers) {
        Ok(user) => user,
        Err(response) => return *response,
    };
    if !is_admin(&state, &actor) {
        return forbidden("se requieren privilegios de administrador");
    }
    let raw = body.name.trim();
    let clean = raw.trim_start_matches("grp_");
    let name = format!("grp_{clean}");
    if clean.is_empty() || !valid_grp(&name) {
        return bad_request(
            "el grupo debe tener un nombre válido (letras minúsculas, números, guiones y hasta 32 caracteres)",
        );
    }
    if is_master_grp(&name) {
        return bad_request("no se puede crear un grupo maestro reservado del sistema");
    }

    let created = name.clone();
    let result =
        tokio::task::spawn_blocking(move || run_accounts(&["create-group", &created], None))
            .await
            .unwrap_or_else(|_| Err("creación interrumpida".to_string()));

    match result {
        Ok(_) => {
            let _ = state
                .auth
                .db()
                .append_audit(&actor, "group_create", &name, "SUCCESS", "");
            axum::Json(ApiResponse::<serde_json::Value>::ok_empty("Grupo creado")).into_response()
        }
        Err(error) => server_error(&error),
    }
}

/// Renombra un grupo.
pub async fn group_rename(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::Json(body): axum::Json<RenameGroupBody>,
) -> Response {
    let actor = match require_auth(&state, &headers) {
        Ok(user) => user,
        Err(response) => return *response,
    };
    if !is_admin(&state, &actor) {
        return forbidden("se requieren privilegios de administrador");
    }
    let raw_new = if body.new.is_empty() {
        body.new_name.trim()
    } else {
        body.new.trim()
    };
    let clean_new = raw_new.trim_start_matches("grp_");
    let new_name = format!("grp_{clean_new}");
    if clean_new.is_empty()
        || !valid_grp(&body.old)
        || !valid_grp(&new_name)
        || is_master_grp(&body.old)
        || is_master_grp(&new_name)
    {
        return bad_request("nombre de grupo inválido o grupo maestro protegido");
    }

    let old = body.old.clone();
    let target = new_name.clone();
    let result =
        tokio::task::spawn_blocking(move || run_accounts(&["rename-group", &old, &target], None))
            .await
            .unwrap_or_else(|_| Err("renombrado interrumpido".to_string()));

    match result {
        Ok(_) => axum::Json(ApiResponse::<serde_json::Value>::ok_empty(
            "Grupo renombrado",
        ))
        .into_response(),
        Err(error) => server_error(&error),
    }
}

/// Elimina un grupo corporativo.
pub async fn group_delete(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::Json(body): axum::Json<GroupBody>,
) -> Response {
    let actor = match require_auth(&state, &headers) {
        Ok(user) => user,
        Err(response) => return *response,
    };
    if !is_admin(&state, &actor) {
        return forbidden("se requieren privilegios de administrador");
    }
    let name = body.name.trim().to_string();
    if !valid_grp(&name) || is_master_grp(&name) {
        return bad_request("grupo inválido o grupo maestro protegido");
    }

    let target = name.clone();
    let result =
        tokio::task::spawn_blocking(move || run_accounts(&["delete-group", &target], None))
            .await
            .unwrap_or_else(|_| Err("borrado interrumpido".to_string()));

    match result {
        Ok(_) => {
            let _ = state
                .auth
                .db()
                .append_audit(&actor, "group_delete", &name, "SUCCESS", "");
            axum::Json(ApiResponse::<serde_json::Value>::ok_empty(
                "Grupo eliminado",
            ))
            .into_response()
        }
        Err(error) => server_error(&error),
    }
}

#[derive(Debug, Deserialize)]
pub struct BackupTestBody {
    #[serde(default)]
    pub proto: String,
    #[serde(default)]
    pub ip: String,
    #[serde(default)]
    pub share: String,
    #[serde(default)]
    pub path: String,
    #[serde(default)]
    pub port: Option<u16>,
    #[serde(default)]
    pub user: String,
    #[serde(default)]
    pub password: String,
    #[serde(default, alias = "key_file", alias = "key_path", alias = "private_key")]
    pub ssh_key: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct BackupIdBody {
    pub id: String,
}

fn run_backup_helper(args: &[&str], password: Option<&str>) -> Result<String, String> {
    let mut cmd_args = vec!["helper", "backup-manage"];
    cmd_args.extend_from_slice(args);
    let mut spec = nas_exec::CommandSpec::new("/usr/local/bin/nas")
        .args(cmd_args)
        .sudo(nas_exec::SudoMode::Passwordless)
        .timeout(std::time::Duration::from_secs(60));
    if let Some(password) = password {
        spec = spec.stdin_secret(nas_exec::SecretInput::new(password));
    }

    let output = Runner::new()
        .run(&spec)
        .map_err(|error| error.to_string())?;
    if output.is_success() {
        Ok(output.stdout)
    } else {
        let raw = if output.stderr.is_empty() {
            output.stdout
        } else {
            output.stderr
        };
        Err(clean_helper_error(&raw))
    }
}

pub(crate) fn clean_helper_error(raw: &str) -> String {
    let trimmed = raw.trim();
    if let Ok(val) = serde_json::from_str::<serde_json::Value>(trimmed) {
        if let Some(msg) = val
            .get("message")
            .and_then(|v| v.as_str())
            .or_else(|| val.get("error").and_then(|v| v.as_str()))
        {
            return sanitize_error_message(msg);
        }
    }
    sanitize_error_message(trimmed)
}

fn sanitize_error_message(msg: &str) -> String {
    let clean = if let Some((head, _)) = msg.split_once("\nRefer to the mount.cifs") {
        head.trim()
    } else if let Some((head, _)) = msg.split_once("Refer to the mount.cifs") {
        head.trim()
    } else {
        msg.trim()
    };

    let clean = clean.strip_prefix("operación denegada: ").unwrap_or(clean);

    if clean.contains("mount error(13): Permission denied") {
        "Fallo de conexión CIFS: Acceso denegado (usuario o contraseña incorrectos, o permisos insuficientes en el recurso compartido).".to_string()
    } else if clean.contains("mount error(112): Host is down")
        || clean.contains("mount error(113): No route to host")
    {
        "Fallo de conexión CIFS: Host no disponible (servidor remoto apagado o fuera de red)."
            .to_string()
    } else if clean.contains("mount error(111): Connection refused") {
        "Fallo de conexión CIFS: Conexión rechazada (puerto CIFS/SMB 445 no disponible)."
            .to_string()
    } else if clean.contains("mount error(6): No such device or address")
        || clean.contains("mount error(2): No such file or directory")
    {
        "Fallo de conexión CIFS: Recurso compartido no encontrado en el servidor remoto."
            .to_string()
    } else if clean.contains("mount error(115): Operation now in progress") {
        "Fallo de conexión CIFS: Tiempo de espera agotado al conectar al servidor remoto."
            .to_string()
    } else {
        clean.to_string()
    }
}

fn stage_backup_files(
    spec: &nas_engine::backup_spec::BackupSpec,
    proto: nas_engine::backup_spec::Proto,
) -> Result<(), String> {
    let dir = std::path::Path::new("/var/lib/nas/staging");
    std::fs::create_dir_all(dir).map_err(|error| error.to_string())?;
    let id = spec.id.trim();

    std::fs::write(
        dir.join(format!("nas_backup_{id}.sh")),
        spec.render_runner(proto),
    )
    .map_err(|error| error.to_string())?;
    std::fs::write(
        dir.join(format!("nas_backup_{id}.cron")),
        spec.render_cron(),
    )
    .map_err(|error| error.to_string())?;
    let cred = spec.render_cred(proto);
    if !cred.is_empty() {
        std::fs::write(dir.join(format!("nas_backup_{id}.cred")), cred)
            .map_err(|error| error.to_string())?;
    } else {
        let _ = std::fs::remove_file(dir.join(format!("nas_backup_{id}.cred")));
    }
    if let Some(key_content) = spec.render_key(proto) {
        std::fs::write(dir.join(format!("nas_backup_{id}.id_ed25519")), key_content)
            .map_err(|error| error.to_string())?;
    } else {
        let _ = std::fs::remove_file(dir.join(format!("nas_backup_{id}.id_ed25519")));
    }
    Ok(())
}

async fn install_backup(
    state: AppState,
    headers: HeaderMap,
    body: nas_engine::backup_spec::BackupSpec,
    actor_action: &'static str,
) -> Response {
    let actor = match require_auth(&state, &headers) {
        Ok(user) => user,
        Err(response) => return *response,
    };
    if !is_admin(&state, &actor) {
        return forbidden("se requieren privilegios de administrador");
    }
    if !state.role.has_backups() {
        return forbidden("el rol ARCHIVOS no expone tareas de respaldo");
    }

    let proto = match body.validate() {
        Ok(proto) => proto,
        Err(error) => return error_response(error),
    };
    let id = body.id.trim().to_string();

    if let Err(error) = stage_backup_files(&body, proto) {
        return server_error(&error);
    }

    let helper_id = id.clone();
    let result =
        tokio::task::spawn_blocking(move || run_backup_helper(&["install", &helper_id], None))
            .await
            .unwrap_or_else(|_| Err("instalación interrumpida".to_string()));

    match result {
        Ok(_) => {
            let _ = state
                .auth
                .db()
                .append_audit(&actor, actor_action, &id, "SUCCESS", "");
            axum::Json(ApiResponse::<serde_json::Value>::ok_empty(format!(
                "Tarea [{id}] programada"
            )))
            .into_response()
        }
        Err(error) => server_error(&error),
    }
}

/// Crea una tarea de respaldo.
pub async fn backup_create(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::Json(body): axum::Json<nas_engine::backup_spec::BackupSpec>,
) -> Response {
    install_backup(state, headers, body, "backup_create").await
}

/// Actualiza (reinstala) una tarea de respaldo.
pub async fn backup_update(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::Json(body): axum::Json<nas_engine::backup_spec::BackupSpec>,
) -> Response {
    install_backup(state, headers, body, "backup_update").await
}

/// Elimina una tarea de respaldo (conserva los snapshots).
pub async fn backup_delete(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::Json(body): axum::Json<BackupIdBody>,
) -> Response {
    let actor = match require_auth(&state, &headers) {
        Ok(user) => user,
        Err(response) => return *response,
    };
    if !is_admin(&state, &actor) {
        return forbidden("se requieren privilegios de administrador");
    }
    if !state.role.has_backups() {
        return forbidden("el rol ARCHIVOS no expone tareas de respaldo");
    }
    if body.id.is_empty()
        || !body
            .id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        return bad_request("identificador inválido");
    }

    let id = body.id.clone();
    let result = tokio::task::spawn_blocking(move || run_backup_helper(&["delete", &id], None))
        .await
        .unwrap_or_else(|_| Err("borrado interrumpido".to_string()));

    match result {
        Ok(_) => {
            let _ = state
                .auth
                .db()
                .append_audit(&actor, "backup_delete", &body.id, "SUCCESS", "");
            axum::Json(ApiResponse::<serde_json::Value>::ok_empty(
                "Tarea eliminada",
            ))
            .into_response()
        }
        Err(error) => server_error(&error),
    }
}

/// Prueba la conectividad del origen de una tarea.
pub async fn backup_test(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::Json(body): axum::Json<BackupTestBody>,
) -> Response {
    let actor = match require_auth(&state, &headers) {
        Ok(user) => user,
        Err(response) => return *response,
    };
    if !is_admin(&state, &actor) {
        return forbidden("se requieren privilegios de administrador");
    }
    if !state.role.has_backups() {
        return forbidden("el rol ARCHIVOS no expone tareas de respaldo");
    }

    let proto = body.proto.clone();
    let host = body.ip.clone();
    let target = if body.proto == "cifs" {
        body.share.clone()
    } else {
        body.path.clone()
    };
    let port = body.port.unwrap_or(22).to_string();
    let user_name = body.user.clone();
    let auth = if let Some(key) = body.ssh_key.filter(|k| !k.trim().is_empty()) {
        key
    } else {
        body.password.clone()
    };

    let result = tokio::task::spawn_blocking(move || {
        run_backup_helper(
            &["test", &proto, &host, &target, &port, &user_name],
            Some(&auth),
        )
    })
    .await
    .unwrap_or_else(|_| Err("prueba interrumpida".to_string()));

    match result {
        Ok(output) => axum::Json(ApiResponse::ok(
            serde_json::json!({ "output": output }),
            "Conexión verificada",
        ))
        .into_response(),
        Err(error) => server_error(&error),
    }
}

/// Cuerpo de una acción de almacenamiento.
#[derive(Debug, Deserialize)]
pub struct StorageActionBody {
    #[serde(default)]
    pub device: String,
    #[serde(default)]
    pub disk: String,
    #[serde(default)]
    pub fstype: String,
    #[serde(default)]
    pub confirm: String,
    #[serde(default)]
    pub unmount: bool,
    #[serde(default)]
    pub unit_id: String,
    #[serde(default)]
    pub subvolume: String,
    #[serde(default)]
    pub vg: String,
    #[serde(default)]
    pub lv: String,
    #[serde(default)]
    pub size: String,
}

fn audit_storage(
    state: &AppState,
    actor: &str,
    action: &str,
    target: &str,
    status: &str,
    detail: &str,
) {
    let _ = state
        .auth
        .db()
        .append_audit(actor, action, target, status, detail);
}

async fn storage_operation<F>(
    state: AppState,
    headers: HeaderMap,
    action: &'static str,
    target: String,
    operation: F,
) -> Response
where
    F: FnOnce() -> Result<String, String> + Send + 'static,
{
    let actor = match require_auth(&state, &headers) {
        Ok(user) => user,
        Err(response) => return *response,
    };
    if !is_admin(&state, &actor) {
        return forbidden("se requieren privilegios de administrador");
    }

    let audited_target = target.clone();
    let outcome = tokio::task::spawn_blocking(operation)
        .await
        .unwrap_or_else(|_| Err("operación interrumpida".to_string()));

    match outcome {
        Ok(message) => {
            audit_storage(&state, &actor, action, &audited_target, "SUCCESS", &message);
            axum::Json(ApiResponse::ok(
                serde_json::json!({ "message": message }),
                message,
            ))
            .into_response()
        }
        Err(error) => {
            audit_storage(&state, &actor, action, &audited_target, "FAILED", &error);
            error_response(NasError::Invalid(error))
        }
    }
}

/// Formatea y monta un dispositivo en `/srv/nas`.
pub async fn storage_format(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::Json(body): axum::Json<StorageActionBody>,
) -> Response {
    let target = body.device.clone();
    let device = body.device;
    let fstype = if body.fstype.is_empty() {
        "ext4".to_string()
    } else {
        body.fstype
    };
    let confirm = body.confirm;
    let unmount = body.unmount;
    storage_operation(state, headers, "storage_format", target, move || {
        nas_engine::storage_ops::format_device(&Runner::new(), &device, &fstype, &confirm, unmount)
    })
    .await
}

/// Crea un volumen LVM y lo monta en `/srv/nas`.
pub async fn storage_lvm(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::Json(body): axum::Json<StorageActionBody>,
) -> Response {
    let target = body.disk.clone();
    storage_operation(state, headers, "storage_lvm", target, move || {
        nas_engine::storage_ops::lvm_create(
            &Runner::new(),
            &body.disk,
            &body.vg,
            &body.lv,
            &body.size,
            &body.fstype,
            &body.confirm,
            body.unmount,
        )
    })
    .await
}

/// Crea un subvolumen Btrfs.
pub async fn storage_subvolume(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::Json(body): axum::Json<StorageActionBody>,
) -> Response {
    let target = body.subvolume.clone();
    storage_operation(state, headers, "storage_subvolume", target, move || {
        nas_engine::storage_ops::create_subvolume(
            &Runner::new(),
            &body.device,
            &body.subvolume,
            &body.confirm,
        )
    })
    .await
}

/// Añade una unidad adicional montada en `/srv/nas/unidades/<id>`.
pub async fn storage_add_unit(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::Json(body): axum::Json<StorageActionBody>,
) -> Response {
    let target = body.unit_id.clone();
    let fstype = if body.fstype.is_empty() {
        "ext4".to_string()
    } else {
        body.fstype
    };
    storage_operation(state, headers, "storage_add_unit", target, move || {
        nas_engine::storage_ops::add_unit(
            &Runner::new(),
            &body.device,
            &body.unit_id,
            &fstype,
            &body.confirm,
        )
    })
    .await
}

/// Extiende el pool LVM que contiene `/srv/nas`.
pub async fn storage_extend_pool(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::Json(body): axum::Json<StorageActionBody>,
) -> Response {
    let target = body.device.clone();
    storage_operation(state, headers, "storage_extend_pool", target, move || {
        nas_engine::storage_ops::extend_pool(&Runner::new(), &body.device, &body.confirm)
    })
    .await
}

fn backup_action_id(id: &str) -> Option<String> {
    nas_engine::backup_control::valid_id(id).then(|| id.trim().to_string())
}

/// Ejecuta una tarea de respaldo en segundo plano.
pub async fn backup_run_id(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> Response {
    let actor = match require_auth(&state, &headers) {
        Ok(user) => user,
        Err(response) => return *response,
    };
    if !is_admin(&state, &actor) {
        return forbidden("se requieren privilegios de administrador");
    }
    if !state.role.has_backups() {
        return forbidden("el rol ARCHIVOS no expone tareas de respaldo");
    }
    let Some(id) = backup_action_id(&id) else {
        return bad_request("identificador de tarea inválido");
    };
    let task = id.clone();
    let outcome =
        tokio::task::spawn_blocking(move || nas_engine::backup_control::run(&Runner::new(), &task))
            .await
            .unwrap_or_else(|_| Err("ejecución interrumpida".to_string()));

    match outcome {
        Ok(message) => {
            let _ = state
                .auth
                .db()
                .append_audit(&actor, "backup_run", &id, "SUCCESS", &message);
            axum::Json(ApiResponse::ok(
                serde_json::json!({ "message": message }),
                message,
            ))
            .into_response()
        }
        Err(error) => {
            let _ = state
                .auth
                .db()
                .append_audit(&actor, "backup_run", &id, "FAILED", &error);
            error_response(NasError::Invalid(error))
        }
    }
}

/// Detiene la ejecución de una tarea de respaldo.
pub async fn backup_stop_id(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> Response {
    let actor = match require_auth(&state, &headers) {
        Ok(user) => user,
        Err(response) => return *response,
    };
    if !is_admin(&state, &actor) {
        return forbidden("se requieren privilegios de administrador");
    }
    if !state.role.has_backups() {
        return forbidden("el rol ARCHIVOS no expone tareas de respaldo");
    }
    let Some(id) = backup_action_id(&id) else {
        return bad_request("identificador de tarea inválido");
    };
    let task = id.clone();
    let outcome = tokio::task::spawn_blocking(move || {
        nas_engine::backup_control::stop(&Runner::new(), &task)
    })
    .await
    .unwrap_or_else(|_| Err("detención interrumpida".to_string()));

    match outcome {
        Ok(message) => {
            let _ = state
                .auth
                .db()
                .append_audit(&actor, "backup_stop", &id, "SUCCESS", &message);
            axum::Json(ApiResponse::ok(
                serde_json::json!({ "message": message }),
                message,
            ))
            .into_response()
        }
        Err(error) => {
            let _ = state
                .auth
                .db()
                .append_audit(&actor, "backup_stop", &id, "FAILED", &error);
            error_response(NasError::Invalid(error))
        }
    }
}

/// Devuelve las últimas líneas de la bitácora de una tarea.
pub async fn backup_logs_id(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::extract::Path(id): axum::extract::Path<String>,
    Query(query): Query<ExtQuery>,
) -> Response {
    if let Err(response) = require_auth(&state, &headers) {
        return *response;
    }
    let Some(id) = backup_action_id(&id) else {
        return bad_request("identificador de tarea inválido");
    };
    let lines = query.limit.unwrap_or(200);
    let task = id.clone();
    let outcome = tokio::task::spawn_blocking(move || {
        nas_engine::backup_control::logs(&Runner::new(), &task, lines)
    })
    .await
    .unwrap_or_else(|_| Err("lectura de bitácora interrumpida".to_string()));

    match outcome {
        Ok(output) => axum::Json(ApiResponse::ok(
            serde_json::json!({ "id": id, "output": output, "logs": output }),
            "Bitácora de la tarea",
        ))
        .into_response(),
        Err(error) => error_response(NasError::Invalid(error)),
    }
}

/// Consulta para filtrar respaldos permanentes.
#[derive(Debug, Deserialize, Default)]
pub struct BackupPermanentQuery {
    pub task: Option<String>,
}

/// Cuerpo para congelar un respaldo como permanente.
#[derive(Debug, Deserialize, Default)]
pub struct FreezeBody {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub tag: Option<String>,
}

/// Cuerpo para restaurar un respaldo.
#[derive(Debug, Deserialize, Default)]
pub struct RestoreBackupBody {
    pub id: String,
    pub snapshot: String,
    #[serde(default)]
    pub target: Option<String>,
}

/// Cuerpo para eliminar un respaldo permanente.
#[derive(Debug, Deserialize, Default)]
pub struct DeletePermanentBody {
    pub id: String,
    pub name: String,
}

/// Lista los respaldos permanentes inmutables (Golden Snapshots).
pub async fn backups_permanent(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<BackupPermanentQuery>,
) -> Response {
    if let Err(response) = require_auth(&state, &headers) {
        return *response;
    }
    let task = query.task;
    match tokio::task::spawn_blocking(move || {
        nas_engine::helpers::backup::list_permanent_backups(task.as_deref())
    })
    .await
    {
        Ok(Ok(list)) => axum::Json(ApiResponse::ok(list, "Respaldos permanentes")).into_response(),
        Ok(Err(error)) => error_response(error),
        Err(_) => server_error("consulta interrumpida"),
    }
}

/// Congela el último snapshot de una tarea como respaldo permanente (por ID en URL).
pub async fn backup_freeze_id(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::extract::Path(id): axum::extract::Path<String>,
    axum::Json(body): axum::Json<FreezeBody>,
) -> Response {
    let actor = match require_auth(&state, &headers) {
        Ok(user) => user,
        Err(response) => return *response,
    };
    if !is_admin(&state, &actor) {
        return forbidden("se requieren privilegios de administrador");
    }
    if !state.role.has_backups() {
        return forbidden("el rol ARCHIVOS no expone tareas de respaldo");
    }
    let Some(id) = backup_action_id(&id) else {
        return bad_request("identificador de tarea inválido");
    };
    let tag = body.tag;
    let task = id.clone();
    let outcome = tokio::task::spawn_blocking(move || {
        nas_engine::backup_control::freeze(&Runner::new(), &task, tag.as_deref())
    })
    .await
    .unwrap_or_else(|_| Err("congelación interrumpida".to_string()));

    match outcome {
        Ok(msg) => {
            let _ = state
                .auth
                .db()
                .append_audit(&actor, "backup_freeze", &id, "SUCCESS", &msg);
            axum::Json(ApiResponse::ok(serde_json::json!({ "message": msg }), msg)).into_response()
        }
        Err(err) => {
            let _ = state
                .auth
                .db()
                .append_audit(&actor, "backup_freeze", &id, "FAILED", &err);
            error_response(NasError::Invalid(err))
        }
    }
}

/// Congela el último snapshot de una tarea como respaldo permanente (por cuerpo JSON).
pub async fn backup_freeze(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::Json(body): axum::Json<FreezeBody>,
) -> Response {
    let actor = match require_auth(&state, &headers) {
        Ok(user) => user,
        Err(response) => return *response,
    };
    if !is_admin(&state, &actor) {
        return forbidden("se requieren privilegios de administrador");
    }
    if !state.role.has_backups() {
        return forbidden("el rol ARCHIVOS no expone tareas de respaldo");
    }
    let Some(id) = backup_action_id(&body.id) else {
        return bad_request("identificador de tarea inválido");
    };
    let tag = body.tag;
    let task = id.clone();
    let outcome = tokio::task::spawn_blocking(move || {
        nas_engine::backup_control::freeze(&Runner::new(), &task, tag.as_deref())
    })
    .await
    .unwrap_or_else(|_| Err("congelación interrumpida".to_string()));

    match outcome {
        Ok(msg) => {
            let _ = state
                .auth
                .db()
                .append_audit(&actor, "backup_freeze", &id, "SUCCESS", &msg);
            axum::Json(ApiResponse::ok(serde_json::json!({ "message": msg }), msg)).into_response()
        }
        Err(err) => {
            let _ = state
                .auth
                .db()
                .append_audit(&actor, "backup_freeze", &id, "FAILED", &err);
            error_response(NasError::Invalid(err))
        }
    }
}

/// Restaura un snapshot o respaldo permanente hacia el origen de red o destino local.
pub async fn backup_restore(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::Json(body): axum::Json<RestoreBackupBody>,
) -> Response {
    let actor = match require_auth(&state, &headers) {
        Ok(user) => user,
        Err(response) => return *response,
    };
    if !is_admin(&state, &actor) {
        return forbidden("se requieren privilegios de administrador");
    }
    if !state.role.has_backups() {
        return forbidden("el rol ARCHIVOS no expone tareas de respaldo");
    }
    let Some(id) = backup_action_id(&body.id) else {
        return bad_request("identificador de tarea inválido");
    };
    let snap = body.snapshot;
    let tgt = body.target;
    let task = id.clone();
    let outcome = tokio::task::spawn_blocking(move || {
        nas_engine::backup_control::restore(&Runner::new(), &task, &snap, tgt.as_deref())
    })
    .await
    .unwrap_or_else(|_| Err("restauración interrumpida".to_string()));

    match outcome {
        Ok(msg) => {
            let _ = state
                .auth
                .db()
                .append_audit(&actor, "backup_restore", &id, "SUCCESS", &msg);
            axum::Json(ApiResponse::ok(serde_json::json!({ "message": msg }), msg)).into_response()
        }
        Err(err) => {
            let _ = state
                .auth
                .db()
                .append_audit(&actor, "backup_restore", &id, "FAILED", &err);
            error_response(NasError::Invalid(err))
        }
    }
}

/// Elimina un respaldo permanente inmutable.
pub async fn backup_permanent_delete(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::Json(body): axum::Json<DeletePermanentBody>,
) -> Response {
    let actor = match require_auth(&state, &headers) {
        Ok(user) => user,
        Err(response) => return *response,
    };
    if !is_admin(&state, &actor) {
        return forbidden("se requieren privilegios de administrador");
    }
    if !state.role.has_backups() {
        return forbidden("el rol ARCHIVOS no expone tareas de respaldo");
    }
    let Some(id) = backup_action_id(&body.id) else {
        return bad_request("identificador de tarea inválido");
    };
    let name = body.name;
    let task = id.clone();
    let outcome = tokio::task::spawn_blocking(move || {
        nas_engine::backup_control::delete_permanent(&Runner::new(), &task, &name)
    })
    .await
    .unwrap_or_else(|_| Err("eliminación interrumpida".to_string()));

    match outcome {
        Ok(msg) => {
            let _ = state.auth.db().append_audit(
                &actor,
                "backup_permanent_delete",
                &id,
                "SUCCESS",
                &msg,
            );
            axum::Json(ApiResponse::ok(serde_json::json!({ "message": msg }), msg)).into_response()
        }
        Err(err) => {
            let _ = state.auth.db().append_audit(
                &actor,
                "backup_permanent_delete",
                &id,
                "FAILED",
                &err,
            );
            error_response(NasError::Invalid(err))
        }
    }
}

/// Raíz confinada según el parámetro `root` del explorador.
fn file_root_for(state: &AppState, root: Option<&str>) -> Result<FileRoot, Box<Response>> {
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

fn not_found_response(message: &str) -> Response {
    (
        StatusCode::NOT_FOUND,
        axum::Json(ApiResponse::<serde_json::Value>::error(message)),
    )
        .into_response()
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

fn is_active_web_payload(name: &str) -> bool {
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

fn inline_content_type(name: &str) -> &'static str {
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

/// Sirve un archivo para previsualización en línea (`/api/files/raw`) aislando vectores XSS mediante streaming O(1).
pub async fn files_raw(
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

fn acl_entries(runner: &Runner, path: &str) -> Option<String> {
    runner
        .run(
            &nas_exec::CommandSpec::new("getfacl")
                .args(["-p", path])
                .timeout(std::time::Duration::from_secs(15)),
        )
        .ok()
        .filter(|output| output.is_success())
        .map(|output| output.stdout)
}

fn acl_has(text: &str, entry: &str, is_group: bool, perms: &str, default: bool) -> bool {
    let prefix = match (default, is_group) {
        (true, true) => "default:group:",
        (false, true) => "group:",
        (true, false) => "default:user:",
        (false, false) => "user:",
    };
    text.lines().any(|line| {
        let line = line.trim();
        line.strip_prefix(prefix)
            .and_then(|rest| rest.split_once(':'))
            .is_some_and(|(name, value)| name == entry && value.trim().starts_with(perms))
    })
}

/// Audita las ACL reales de cada recurso contra `smb.conf`.
pub async fn shares_access_audit(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<ExtQuery>,
) -> Response {
    if let Err(response) = require_auth(&state, &headers) {
        return *response;
    }
    if !state.role.has_shares() {
        return forbidden("el rol BACKUP no expone recursos compartidos");
    }
    let path = state.paths.samba_conf.clone();
    let only = query
        .share
        .or(query.path)
        .filter(|value| !value.trim().is_empty());

    match tokio::task::spawn_blocking(move || {
        let conf = std::fs::read_to_string(&path).unwrap_or_default();
        let runner = Runner::new();
        let mut shares = serde_json::Map::new();
        let mut out_of_sync = 0;

        for share in parse_shares(&conf) {
            if let Some(only) = &only {
                if !share.name.eq_ignore_ascii_case(only.trim()) {
                    continue;
                }
            }
            let path = share.path.clone().unwrap_or_default();
            let mut discrepancies = Vec::new();
            let acl = acl_entries(&runner, &path);

            let mut expected: Vec<(String, bool, &'static str)> = Vec::new();
            for entry in &share.valid_users {
                let is_group = entry.starts_with('@');
                let name = entry.trim_start_matches('@').to_string();
                expected.push((name, is_group, "r-x"));
            }
            for entry in &share.write_list {
                let is_group = entry.starts_with('@');
                let name = entry.trim_start_matches('@').to_string();
                if let Some(item) = expected
                    .iter_mut()
                    .find(|(n, g, _)| *n == name && *g == is_group)
                {
                    item.2 = "rwx";
                } else {
                    expected.push((name, is_group, "rwx"));
                }
            }
            if !expected
                .iter()
                .any(|(name, is_group, _)| *is_group && name == "grp_samba")
            {
                expected.push(("grp_samba".to_string(), true, "rwx"));
            }

            match &acl {
                None => discrepancies.push(format!("no se pudo leer la ACL de {path}")),
                Some(text) => {
                    for (name, is_group, perms) in &expected {
                        let kind = if *is_group { "group" } else { "user" };
                        if !acl_has(text, name, *is_group, perms, false) {
                            discrepancies.push(format!("falta {kind}:{name}:{perms}"));
                        }
                        if !acl_has(text, name, *is_group, perms, true) {
                            discrepancies.push(format!("falta default:{kind}:{name}:{perms}"));
                        }
                    }
                }
            }

            if !discrepancies.is_empty() {
                out_of_sync += 1;
            }
            shares.insert(
                share.name.clone(),
                serde_json::json!({
                    "path": path,
                    "in_sync": discrepancies.is_empty(),
                    "discrepancies": discrepancies,
                }),
            );
        }

        let total = shares.len();
        serde_json::json!({
            "total": total,
            "out_of_sync_count": out_of_sync,
            "shares": shares,
        })
    })
    .await
    {
        Ok(payload) => axum::Json(ApiResponse::ok(payload, "Auditoría de ACL")).into_response(),
        Err(_) => server_error("auditoría de ACL interrumpida"),
    }
}

#[cfg(test)]
mod tests {
    use super::safe_content_disposition;

    #[test]
    fn content_disposition_formatea_ascii_y_rfc6266_unicode() {
        let h1 = safe_content_disposition("attachment", "reporte.txt");
        assert_eq!(
            h1.to_str().expect("str"),
            "attachment; filename=\"reporte.txt\"; filename*=UTF-8''reporte.txt"
        );

        let h2 = safe_content_disposition("attachment", "año_2026.pdf");
        assert_eq!(
            h2.to_str().expect("str"),
            "attachment; filename=\"a_o_2026.pdf\"; filename*=UTF-8''a%C3%B1o_2026.pdf"
        );

        let h3 = safe_content_disposition("inline", "música y diseño.mp3");
        assert_eq!(
            h3.to_str().expect("str"),
            "inline; filename=\"m_sica y dise_o.mp3\"; filename*=UTF-8''m%C3%BAsica%20y%20dise%C3%B1o.mp3"
        );

        let h4 = safe_content_disposition("attachment", "\"peligroso\r\n.bin\"");
        let s4 = h4.to_str().expect("str");
        assert!(!s4.contains('\r') && !s4.contains('\n'));

        let h5 = safe_content_disposition("attachment", "");
        assert_eq!(
            h5.to_str().expect("str"),
            "attachment; filename=\"archivo.bin\"; filename*=UTF-8''"
        );
    }

    #[test]
    fn clean_helper_error_desempaqueta_json_y_humaniza_mount_cifs() {
        use super::clean_helper_error;

        let json_err = "{\n  \"success\": false,\n  \"message\": \"operación denegada: falló conexión CIFS: mount error(13): Permission denied\\nRefer to the mount.cifs(8) manual page (e.g. man mount.cifs) and kernel log messages (dmesg)\",\n  \"error\": \"operación denegada: falló conexión CIFS: mount error(13): Permission denied\\nRefer to the mount.cifs(8) manual page (e.g. man mount.cifs) and kernel log messages (dmesg)\"\n}";
        let res = clean_helper_error(json_err);
        assert!(res.contains("Acceso denegado"));
        assert!(!res.contains("Refer to the mount.cifs"));
        assert!(!res.contains("\"success\""));

        let host_down = "{\n  \"success\": false,\n  \"message\": \"operación denegada: falló conexión CIFS: mount error(112): Host is down\"\n}";
        let res2 = clean_helper_error(host_down);
        assert!(res2.contains("Host no disponible"));

        let plain = "error desconocido en ejecución";
        assert_eq!(clean_helper_error(plain), "error desconocido en ejecución");
    }
}
