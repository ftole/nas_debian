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
pub async fn network(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if let Err(response) = require_auth(&state, &headers) {
        return *response;
    }

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
pub async fn users(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if let Err(response) = require_auth(&state, &headers) {
        return *response;
    }

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
    let status = match &error {
        NasError::Denied(_) => StatusCode::FORBIDDEN,
        NasError::NotFound(_) => StatusCode::NOT_FOUND,
        NasError::Invalid(_) => StatusCode::BAD_REQUEST,
        NasError::Io { source, .. } if source.kind() == std::io::ErrorKind::NotFound => {
            StatusCode::NOT_FOUND
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
pub async fn services(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if let Err(response) = require_auth(&state, &headers) {
        return *response;
    }

    match tokio::task::spawn_blocking(|| crate::legacy::service_rows(&Runner::new())).await {
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
        tasks
            .into_iter()
            .map(|task| {
                let last = last_snapshot(&task);
                let mut row = crate::legacy::backup_row(&task);
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
        let runner = Runner::new();
        let overview = nas_engine::storage::overview_for(&paths, &runner)?;
        let disks = nas_engine::storage::disks(&runner).unwrap_or_default();
        let disks: Vec<serde_json::Value> = disks
            .iter()
            .map(|disk| {
                let mut row = serde_json::to_value(disk).unwrap_or_default();
                let is_part = disk.fstype.as_deref().is_some_and(|fs| !fs.is_empty())
                    || disk.name.chars().last().is_some_and(|c| c.is_ascii_digit());
                row["type"] = serde_json::json!(if is_part { "part" } else { "disk" });
                row["mount"] = serde_json::json!(disk.mountpoint.clone().unwrap_or_default());
                row["children_count"] = serde_json::json!(0);
                row
            })
            .collect();
        Ok::<_, nas_core::NasError>(serde_json::json!({
            "overview": overview,
            "disks": disks,
        }))
    })
    .await
    {
        Ok(Ok(payload)) => axum::Json(ApiResponse::ok(payload, "Almacenamiento")).into_response(),
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
        Ok(status) => {
            let workgroup = std::fs::read_to_string("/etc/samba/smb.conf")
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
                    let status = if message.to_ascii_lowercase().contains("error")
                        || message.to_ascii_lowercase().contains("fail")
                    {
                        "FAILED"
                    } else {
                        "SUCCESS"
                    };
                    serde_json::json!({
                        "source": "system",
                        "unit": unit,
                        "timestamp": timestamp,
                        "message": message,
                        "action": message,
                        "action_label": unit,
                        "badge": if status == "FAILED" { "red" } else { "blue" },
                        "target": unit,
                        "status": status,
                        "user": "sistema",
                        "ip": "",
                        "raw": line,
                        "count": 1,
                        "human": message,
                        "level": if status == "FAILED" { "error" } else { "info" },
                    })
                })
                .collect();
            axum::Json(ApiResponse::ok(entries, "Logs del sistema")).into_response()
        }
        Ok(Err(error)) => server_error(&error),
        Err(_) => server_error("lectura de logs interrumpida"),
    }
}

/// Mapa de acceso de recursos (recursos, grupos y usuarios).
pub async fn shares_access(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if let Err(response) = require_auth(&state, &headers) {
        return *response;
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

/// Unidades de almacenamiento adicionales (aún sin montajes extra).
pub async fn storage_units(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if let Err(response) = require_auth(&state, &headers) {
        return *response;
    }
    let paths = state.paths.clone();
    match tokio::task::spawn_blocking(move || {
        nas_engine::storage::overview_for(&paths, &Runner::new())
    })
    .await
    {
        Ok(Ok(overview)) => {
            let unit = serde_json::json!({
                "id": "principal",
                "name": format!("Unidad Principal ({})", overview.path),
                "path": overview.path,
                "is_main": true,
                "filesystem": overview.filesystem,
                "total_gb": overview.total_gb,
                "used_gb": overview.used_gb,
                "free_gb": overview.free_gb,
                "usage_percent": overview.usage_percent,
            });
            axum::Json(ApiResponse::ok(
                serde_json::json!({ "units": [unit] }),
                "Unidades de almacenamiento",
            ))
            .into_response()
        }
        Ok(Err(_)) => axum::Json(ApiResponse::ok(
            serde_json::json!({ "units": [] }),
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
            let mut payload = serde_json::to_value(&status).unwrap_or_default();
            payload["success"] = serde_json::json!(true);
            payload["output"] = serde_json::json!(text);
            axum::Json(ApiResponse::ok(payload, "Estado de scrub")).into_response()
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
            "installed_commit": version["commit"],
            "commit_date": version["date"],
            "has_updates": false,
            "channel": "dev",
            "gpg_verified": version["gpg_verified"],
            "release_notes": version["notes"],
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
                "status": if active("smbd") { "activo" } else { "inactivo" },
                "version": "4.x (Debian 13)",
                "sessions": sessions,
                "sessions_list": [],
                "vfs_modules": "acl_xattr, streams_xattr",
            },
            "wsdd2": {
                "active": active("wsdd2"),
                "nmbd_active": active("nmbd"),
                "wsdd_active": active("wsdd2"),
                "protocol": "WSD/LLMNR",
            },
            "wsdd": { "active": active("wsdd2") },
            "web": {
                "active": true,
                "status": "activo",
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

/// Ejecuta la actualización en caliente del servidor desde el panel.
pub async fn system_update_run(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let actor = match require_auth(&state, &headers) {
        Ok(user) => user,
        Err(response) => return *response,
    };
    if !is_admin(&state, &actor) {
        return forbidden("se requieren privilegios de administrador");
    }
    let _ = state
        .auth
        .db()
        .append_audit(&actor, "system_update", "sistema", "STARTED", "");

    let result = tokio::task::spawn_blocking(move || -> Result<String, String> {
        let candidates = [
            "/opt/nas_debian/src/core/updater.sh",
            "/opt/nas/src/core/updater.sh",
            "/usr/local/lib/nas/src/core/updater.sh",
        ];
        let script = candidates
            .iter()
            .find(|path| std::path::Path::new(path).is_file())
            .ok_or_else(|| "no se encontró el script de actualización (updater.sh)".to_string())?;

        let output = Runner::new()
            .run(
                &nas_exec::CommandSpec::new("/bin/bash")
                    .args([(*script).to_string(), "--yes".to_string()])
                    .sudo(nas_exec::SudoMode::Passwordless)
                    .timeout(std::time::Duration::from_secs(300)),
            )
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
                "Actualización del sistema completada",
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
    if group.starts_with('@') {
        group.to_string()
    } else {
        format!("@{group}")
    }
}

fn share_spec(body: &ShareBody, path: &str) -> nas_engine::samba::ShareSpec {
    let groups: Vec<String> = body.groups.iter().map(|g| normalize_group(g)).collect();
    let write = if body.write_group.is_empty() {
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

fn stage_and_apply(conf: &str) -> Result<(), String> {
    let stamp = chrono::Local::now().format("%Y%m%d%H%M%S%3f");
    let dir = std::path::Path::new("/var/lib/nas/staging");
    std::fs::create_dir_all(dir).map_err(|error| error.to_string())?;
    let staged = dir.join(format!("smbconf_{stamp}"));
    std::fs::write(&staged, conf).map_err(|error| error.to_string())?;

    let output = Runner::new()
        .run(
            &nas_exec::CommandSpec::new("/usr/local/sbin/nas-samba-apply")
                .arg(staged.to_string_lossy().to_string())
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
            &nas_exec::CommandSpec::new("/usr/local/sbin/nas-share-acl")
                .args([path, principal.trim_start_matches('@'), level])
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
            &nas_exec::CommandSpec::new("/usr/local/sbin/nas-share-dir")
                .args([name, if unit.is_empty() { "principal" } else { unit }])
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
    if nas_engine::samba::validate_name(&body.name).is_err() {
        return bad_request("nombre de recurso inválido");
    }

    let path = share_path(body.name.trim(), body.unit.trim());
    if let Err(error) = nas_engine::samba::validate_path(&path) {
        return error_response(error);
    }
    let spec = share_spec(&body, &path);
    let share_name = body.name.clone();

    let outcome = tokio::task::spawn_blocking(move || -> Result<(), String> {
        let conf = std::fs::read_to_string("/etc/samba/smb.conf").map_err(|e| e.to_string())?;
        let updated =
            nas_engine::samba::upsert_share(&conf, &spec).map_err(|error| error.to_string())?;
        create_share_dir(&spec.name, "principal")?;
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

    let share_name = body.name.clone();

    let outcome = tokio::task::spawn_blocking(move || -> Result<(), String> {
        let conf = std::fs::read_to_string("/etc/samba/smb.conf").map_err(|e| e.to_string())?;
        let existing = nas_engine::parsers::smb::parse_shares(&conf)
            .into_iter()
            .find(|share| share.name.eq_ignore_ascii_case(body.name.trim()))
            .ok_or_else(|| format!("no existe el recurso '{}'", body.name))?;
        let path = existing
            .path
            .unwrap_or_else(|| share_path(body.name.trim(), "principal"));
        let spec = share_spec(&body, &path);
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

    let name = body.name.clone();
    let outcome = tokio::task::spawn_blocking(move || -> Result<(), String> {
        let conf = std::fs::read_to_string("/etc/samba/smb.conf").map_err(|e| e.to_string())?;
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
    if !matches!(body.level.as_str(), "none" | "read" | "write") {
        return bad_request("nivel de acceso inválido");
    }

    let share_name = body.share.clone();
    let level_name = body.level.clone();
    let principal = if body.kind == "users" {
        body.name.clone()
    } else {
        normalize_group(&body.name)
    };

    let outcome = tokio::task::spawn_blocking(move || -> Result<(), String> {
        let conf = std::fs::read_to_string("/etc/samba/smb.conf").map_err(|e| e.to_string())?;
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
    axum::Json(body): axum::Json<ShareNameBody>,
) -> Response {
    let user = match require_auth(&state, &headers) {
        Ok(user) => user,
        Err(response) => return *response,
    };
    if !is_admin(&state, &user) {
        return forbidden("se requieren privilegios de administrador");
    }

    let outcome = tokio::task::spawn_blocking(move || -> Result<usize, String> {
        let conf = std::fs::read_to_string("/etc/samba/smb.conf").map_err(|e| e.to_string())?;
        let shares = nas_engine::parsers::smb::parse_shares(&conf);
        let mut applied = 0;
        for share in shares {
            if !body.name.is_empty() && !share.name.eq_ignore_ascii_case(body.name.trim()) {
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

fn run_accounts(args: &[&str], password: Option<&str>) -> Result<String, String> {
    let mut spec = nas_exec::CommandSpec::new("/usr/local/sbin/nas-accounts")
        .args(args.iter().map(|arg| (*arg).to_string()))
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
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::Json(body): axum::Json<UserBody>,
) -> Response {
    let actor = match require_auth(&state, &headers) {
        Ok(user) => user,
        Err(response) => return *response,
    };
    if !is_admin(&state, &actor) {
        return forbidden("se requieren privilegios de administrador");
    }
    if !valid_username(&body.username) || !body.groups.iter().all(|group| valid_grp(group)) {
        return bad_request("usuario o grupo inválido");
    }
    if body.password.len() < 6 {
        return bad_request("la contraseña debe tener al menos 6 caracteres");
    }

    let username = body.username.clone();
    let groups = body.groups.join(",");
    let samba = if body.samba_enabled { "1" } else { "0" };
    let full_name = body.full_name.clone();
    let can_web = body.can_web;
    let password = body.password.clone();

    let result = tokio::task::spawn_blocking(move || {
        run_accounts(
            &[
                "create-user",
                &username,
                &full_name,
                "-",
                "0",
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
                use sha2::{Digest, Sha256};
                let mut hasher = Sha256::new();
                hasher.update(body.password.as_bytes());
                let hash = hex::encode(hasher.finalize());
                let role = if body.is_superadmin {
                    "superadmin"
                } else if body.is_admin {
                    "admin"
                } else {
                    "operator"
                };
                let _ = state.auth.db().upsert_user(&body.username, &hash, role);
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
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::Json(body): axum::Json<UserBody>,
) -> Response {
    let actor = match require_auth(&state, &headers) {
        Ok(user) => user,
        Err(response) => return *response,
    };
    if !is_admin(&state, &actor) {
        return forbidden("se requieren privilegios de administrador");
    }
    if !valid_username(&body.username) {
        return bad_request("usuario inválido");
    }

    let username = body.username.clone();
    let groups = body.groups.join(",");
    let samba = if body.samba_enabled { "1" } else { "0" };
    let full_name = body.full_name.clone();
    let password = body.password.clone();
    let can_web = body.can_web;

    let result = tokio::task::spawn_blocking(move || {
        let password = if password.is_empty() {
            "cambiar123".to_string()
        } else {
            password
        };
        run_accounts(
            &[
                "update-user",
                &username,
                &full_name,
                "-",
                "0",
                "1",
                samba,
                &groups,
            ],
            Some(&password),
        )
    })
    .await
    .unwrap_or_else(|_| Err("actualización interrumpida".to_string()));

    match result {
        Ok(_) => {
            if can_web && !body.password.is_empty() {
                use sha2::{Digest, Sha256};
                let mut hasher = Sha256::new();
                hasher.update(body.password.as_bytes());
                let hash = hex::encode(hasher.finalize());
                let _ = state
                    .auth
                    .db()
                    .upsert_user(&body.username, &hash, "operator");
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
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::Json(body): axum::Json<ToggleUserBody>,
) -> Response {
    let actor = match require_auth(&state, &headers) {
        Ok(user) => user,
        Err(response) => return *response,
    };
    if !is_admin(&state, &actor) {
        return forbidden("se requieren privilegios de administrador");
    }
    if !valid_username(&body.username) {
        return bad_request("usuario inválido");
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
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::Json(body): axum::Json<ToggleUserBody>,
) -> Response {
    let actor = match require_auth(&state, &headers) {
        Ok(user) => user,
        Err(response) => return *response,
    };
    if !is_admin(&state, &actor) {
        return forbidden("se requieren privilegios de administrador");
    }
    if !valid_username(&body.username) || body.username == "www-data" {
        return bad_request("usuario inválido o protegido");
    }

    let username = body.username.clone();
    let result =
        tokio::task::spawn_blocking(move || run_accounts(&["delete-user", &username], None))
            .await
            .unwrap_or_else(|_| Err("borrado interrumpido".to_string()));

    match result {
        Ok(_) => {
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
    if !valid_username(&body.username) || !valid_grp(&body.group) {
        return bad_request("usuario o grupo inválido");
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
    let name = body.name.trim().to_string();
    if !valid_grp(&name) {
        return bad_request("el grupo debe llamarse grp_<nombre>");
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
    let new_name = if body.new.is_empty() {
        body.new_name.clone()
    } else {
        body.new.clone()
    };
    if !valid_grp(&body.old) || !valid_grp(&new_name) {
        return bad_request("nombre de grupo inválido");
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
    if !valid_grp(&name) {
        return bad_request("grupo inválido");
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
}

#[derive(Debug, Deserialize)]
pub struct BackupIdBody {
    pub id: String,
}

fn run_backup_helper(args: &[&str], password: Option<&str>) -> Result<String, String> {
    let mut spec = nas_exec::CommandSpec::new("/usr/local/sbin/nas-backup-manage")
        .args(args.iter().map(|arg| (*arg).to_string()))
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
    if let Err(response) = require_auth(&state, &headers) {
        return *response;
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
    let password = body.password.clone();

    let result = tokio::task::spawn_blocking(move || {
        run_backup_helper(
            &["test", &proto, &host, &target, &port, &user_name],
            Some(&password),
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
