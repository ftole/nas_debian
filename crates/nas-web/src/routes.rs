//! Manejadores HTTP del panel.

use axum::extract::State;
use axum::http::header::{COOKIE, SET_COOKIE};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::{Html, IntoResponse, Redirect, Response};
use axum::Form;
use chrono::Utc;
use nas_core::ApiResponse;
use nas_engine::parsers::smb::parse_shares;
use nas_engine::{collect_network, collect_status};
use nas_exec::Runner;
use serde::Deserialize;

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
        Ok(Some(session)) => session,
        Ok(None) => return (StatusCode::UNAUTHORIZED, Html(LOGIN_ERROR_HTML)).into_response(),
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
    let cookies = cookie_header(&headers);
    let _ = state.auth.logout(cookies);

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
</style></head>
<body>
<header><h1>NAS · {hostname} · <small>{role}</small></h1>
<form method="post" action="/logout"><button type="submit">Salir ({user})</button></form></header>
<main>
  <div class="card"><h2>Hostname</h2><b>{hostname}</b></div>
  <div class="card"><h2>Grupo de trabajo</h2><b>{workgroup}</b></div>
  <div class="card"><h2>IPv4 primaria</h2><b>{ip}</b></div>
  <div class="card"><h2>API</h2><code>/api/metrics</code> · <code>/api/network</code> · <code>/api/shares</code> · <code>/api/users</code></div>
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
