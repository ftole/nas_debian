use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{Html, IntoResponse, Redirect, Response};

use super::common::{cookie_header, is_admin, is_superadmin, require_auth, server_ip};
use crate::views;
use crate::AppState;

/// Sonda de vida usada por systemd/nginx.
pub async fn healthz() -> &'static str {
    "ok"
}

pub fn hostname_value() -> String {
    std::fs::read_to_string("/etc/hostname")
        .map(|value| value.trim().to_string())
        .ok()
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "SRV-NAS".to_string())
}

pub fn workgroup_value(state: &AppState) -> String {
    std::fs::read_to_string(&state.paths.samba_conf)
        .ok()
        .and_then(|text| nas_core::config::parse_workgroup(&text))
        .unwrap_or_else(|| "N/D".to_string())
}

pub fn render_shell(state: &AppState, headers: &HeaderMap, user: &str, active: &str) -> Response {
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

pub fn login_redirect() -> Response {
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

/// Logotipo iintec (se sirve a color; la pantalla de acceso lo tiñe con CSS mask).
pub async fn logo_iintec() -> Response {
    (
        [
            (axum::http::header::CONTENT_TYPE, "image/svg+xml"),
            (axum::http::header::CACHE_CONTROL, "public, max-age=86400"),
        ],
        views::LOGO_IINTEC,
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

pub fn js_response(bytes: &'static [u8]) -> Response {
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
