use axum::extract::State;
use axum::http::header::SET_COOKIE;
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::{Html, IntoResponse, Redirect, Response};
use axum::Form;
use chrono::Utc;
use nas_core::ApiResponse;
use serde::Deserialize;

use super::common::{
    audit_ip, cookie_header, extract_client_ip, hostname, require_auth, server_error, server_ip,
};
use crate::views;
use crate::AppState;

/// Credenciales enviadas por el formulario.
#[derive(Debug, Deserialize)]
pub struct LoginForm {
    pub username: String,
    pub password: String,
    #[serde(default)]
    pub csrf_token: Option<String>,
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

// Procesa el acceso y emite la cookie de sesión.
pub async fn login_submit(
    State(state): State<AppState>,
    headers: HeaderMap,
    Form(form): Form<LoginForm>,
) -> Response {
    let now = Utc::now().timestamp();
    let client_ip = extract_client_ip(&headers);

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
            audit_ip(
                &state,
                &headers,
                &form.username,
                "login",
                "web",
                "SUCCESS",
                "",
            );
            session
        }
        Ok(None) => {
            let hits =
                state
                    .auth
                    .db()
                    .check_rate_limit(&format!("login_fail:{client_ip}"), now, 60, 5);
            audit_ip(
                &state,
                &headers,
                &form.username,
                "login",
                "web",
                "FAILED",
                "",
            );
            let blocked = matches!(hits, Ok(false));
            let csrf = state.auth.issue_login_csrf(now);
            if blocked {
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
            audit_ip(
                &state,
                &headers,
                &form.username,
                "login",
                "api",
                "SUCCESS",
                "",
            );
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
            let hits =
                state
                    .auth
                    .db()
                    .check_rate_limit(&format!("login_fail:{client_ip}"), now, 60, 5);
            audit_ip(
                &state,
                &headers,
                &form.username,
                "login",
                "api",
                "FAILED",
                "",
            );
            let blocked = matches!(hits, Ok(false));
            if blocked {
                return (
                    StatusCode::TOO_MANY_REQUESTS,
                    axum::Json(ApiResponse::<serde_json::Value>::error(
                        "Demasiados intentos de acceso. Espere un momento e intente de nuevo.",
                    )),
                )
                    .into_response();
            }
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
