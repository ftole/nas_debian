use axum::extract::State;
use axum::http::header::{RETRY_AFTER, SET_COOKIE};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::{Html, IntoResponse, Redirect, Response};
use axum::Form;
use chrono::Utc;
use nas_core::ApiResponse;
use serde::Deserialize;

use super::common::{
    audit_ip, cached_host_ip, cached_host_ip_nonblocking, cookie_header, extract_client_ip,
    server_error,
};
use crate::extractors::AuthenticatedUser;
use crate::views;
use crate::{AppState, AuthBlockingError};

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
    let (host, ip) = cached_host_ip().await;
    Html(views::login_with_context(&host, &ip, &csrf, None))
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
        let (host, ip) = cached_host_ip().await;
        return (
            StatusCode::FORBIDDEN,
            Html(views::login_with_context(
                &host,
                &ip,
                &csrf,
                Some("Token CSRF inválido o expirado."),
            )),
        )
            .into_response();
    }
    let rate_key = format!("login_fail:{client_ip}");
    let rate_key_check = rate_key.clone();
    if matches!(
        state
            .run_db(move |db| db.is_rate_limited(&rate_key_check, now, 60, 5))
            .await,
        Ok(true)
    ) {
        audit_ip(
            &state,
            &headers,
            &form.username,
            "login",
            "web",
            "DENIED",
            "rate_limit",
        )
        .await;
        let csrf = state.auth.issue_login_csrf(now);
        let (host, ip) = cached_host_ip().await;
        return (
            StatusCode::TOO_MANY_REQUESTS,
            Html(views::login_with_context(
                &host,
                &ip,
                &csrf,
                Some("Demasiados intentos fallidos. Intente de nuevo más tarde."),
            )),
        )
            .into_response();
    }
    let auth = state.auth.clone();
    let login_user = form.username.clone();
    let login_pass = form.password.clone();
    let login_res = match state
        .run_auth_blocking(move || auth.login(&login_user, &login_pass, now))
        .await
    {
        Ok(res) => res,
        Err(AuthBlockingError::Busy) => {
            let csrf = state.auth.issue_login_csrf(now);
            let (host, ip) = cached_host_ip_nonblocking();
            let mut response = (
                StatusCode::SERVICE_UNAVAILABLE,
                Html(views::login_with_context(
                    &host,
                    &ip,
                    &csrf,
                    Some("Demasiados inicios de sesión simultáneos. Intente en unos segundos."),
                )),
            )
                .into_response();
            response
                .headers_mut()
                .insert(RETRY_AFTER, HeaderValue::from_static("2"));
            return response;
        }
        Err(AuthBlockingError::Failed) => return server_error("autenticación interrumpida"),
    };
    let session = match login_res {
        Ok(Some(session)) => {
            audit_ip(
                &state,
                &headers,
                &form.username,
                "login",
                "web",
                "SUCCESS",
                "",
            )
            .await;
            session
        }
        Ok(None) => {
            let rate_key_hit = rate_key.clone();
            let hits = state
                .run_db(move |db| db.check_rate_limit(&rate_key_hit, now, 60, 5))
                .await;
            audit_ip(
                &state,
                &headers,
                &form.username,
                "login",
                "web",
                "FAILED",
                "",
            )
            .await;
            let blocked = matches!(hits, Ok(false));
            let csrf = state.auth.issue_login_csrf(now);
            let (host, ip) = cached_host_ip().await;
            if blocked {
                return (
                    StatusCode::TOO_MANY_REQUESTS,
                    Html(views::login_with_context(
                        &host,
                        &ip,
                        &csrf,
                        Some("Demasiados intentos fallidos. Intente de nuevo más tarde."),
                    )),
                )
                    .into_response();
            }
            return (
                StatusCode::UNAUTHORIZED,
                Html(views::login_with_context(
                    &host,
                    &ip,
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
    let rate_key = format!("login_fail:{client_ip}");
    let rate_key_check = rate_key.clone();
    if matches!(
        state
            .run_db(move |db| db.is_rate_limited(&rate_key_check, now, 60, 5))
            .await,
        Ok(true)
    ) {
        audit_ip(
            &state,
            &headers,
            &form.username,
            "login",
            "api",
            "DENIED",
            "rate_limit",
        )
        .await;
        return (
            StatusCode::TOO_MANY_REQUESTS,
            axum::Json(ApiResponse::<serde_json::Value>::error(
                "Demasiados intentos de acceso. Espere un momento e intente de nuevo.",
            )),
        )
            .into_response();
    }
    let auth = state.auth.clone();
    let login_user = form.username.clone();
    let login_pass = form.password.clone();
    let login_res = match state
        .run_auth_blocking(move || auth.login(&login_user, &login_pass, now))
        .await
    {
        Ok(res) => res,
        Err(AuthBlockingError::Busy) => {
            let mut response = (
                StatusCode::SERVICE_UNAVAILABLE,
                axum::Json(ApiResponse::<serde_json::Value>::error(
                    "Demasiados inicios de sesión simultáneos. Espere un momento.",
                )),
            )
                .into_response();
            response
                .headers_mut()
                .insert(RETRY_AFTER, HeaderValue::from_static("2"));
            return response;
        }
        Err(AuthBlockingError::Failed) => return server_error("autenticación interrumpida"),
    };
    match login_res {
        Ok(Some(session)) => {
            audit_ip(
                &state,
                &headers,
                &form.username,
                "login",
                "api",
                "SUCCESS",
                "",
            )
            .await;
            let uname = form.username.clone();
            let sess_lookup = session.clone();
            let now = chrono::Utc::now().timestamp();
            let (canonical_user, role) = state
                .run_db(move |db| {
                    let canonical = db
                        .session_user(&sess_lookup, now)
                        .ok()
                        .flatten()
                        .unwrap_or(uname);
                    let role = super::common::resolve_effective_role(db, &canonical);
                    Ok((canonical, role))
                })
                .await
                .unwrap_or_else(|_| (form.username.clone(), "operator".into()));
            let csrf = state.auth.csrf_for_session(&session);
            let mut response = axum::Json(ApiResponse::ok(
                serde_json::json!({
                    "username": canonical_user,
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
            let rate_key_hit = rate_key.clone();
            let hits = state
                .run_db(move |db| db.check_rate_limit(&rate_key_hit, now, 60, 5))
                .await;
            audit_ip(
                &state,
                &headers,
                &form.username,
                "login",
                "api",
                "FAILED",
                "",
            )
            .await;
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
pub async fn logout(
    State(state): State<AppState>,
    _user: AuthenticatedUser,
    headers: HeaderMap,
) -> Response {
    let cookies = cookie_header(&headers).to_string();
    if let Some(session_id) = crate::auth::cookie_value(&cookies).map(str::to_string) {
        let _ = state.run_db(move |db| db.delete_session(&session_id)).await;
    }

    let mut response = Redirect::to("/login").into_response();
    if let Ok(header) = HeaderValue::from_str(&state.auth.clear_cookie_header()) {
        response.headers_mut().insert(SET_COOKIE, header);
    }
    response
}

/// Raíz: redirige al dashboard autenticado.
pub async fn dashboard(_user: AuthenticatedUser) -> Response {
    Redirect::to("/dashboard").into_response()
}
