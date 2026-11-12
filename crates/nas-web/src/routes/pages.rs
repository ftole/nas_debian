use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{Html, IntoResponse, Redirect, Response};

use super::common::{cookie_header, hostname, server_ip};
use crate::extractors::AuthenticatedUser;
use crate::views;
use crate::AppState;

/// Sonda de vida usada por systemd/nginx.
pub async fn healthz() -> &'static str {
    "ok"
}

pub async fn render_shell(
    state: &AppState,
    headers: &HeaderMap,
    user: &AuthenticatedUser,
    active: &str,
) -> Response {
    let is_admin = user.is_admin();
    let is_super = user.is_superadmin();
    let csrf = state.auth.csrf_for_cookies(cookie_header(headers));
    let samba_conf = state.paths.samba_conf.clone();
    let (host, workgroup, ip) = tokio::task::spawn_blocking(move || {
        let workgroup = std::fs::read_to_string(&samba_conf)
            .ok()
            .and_then(|text| nas_core::config::parse_workgroup(&text))
            .unwrap_or_else(|| "N/D".to_string());
        (hostname(), workgroup, server_ip())
    })
    .await
    .unwrap_or_else(|_| {
        (
            "SRV-NAS".to_string(),
            "N/D".to_string(),
            "127.0.0.1".to_string(),
        )
    });

    let shell = views::Shell {
        active,
        user: user.username(),
        hostname: &host,
        workgroup: &workgroup,
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
        ip: &ip,
    };

    Html(views::shell(&shell, &csrf)).into_response()
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

/// Helpers compartidos (lib).
pub async fn lib_api_js() -> Response {
    js_response(crate::assets::LIB_API_JS)
}
pub async fn lib_modal_js() -> Response {
    js_response(crate::assets::LIB_MODAL_JS)
}
pub async fn lib_utils_js() -> Response {
    js_response(crate::assets::LIB_UTILS_JS)
}
pub async fn lib_toast_js() -> Response {
    js_response(crate::assets::LIB_TOAST_JS)
}

/// Módulos por dominio.
pub async fn module_storage_js() -> Response {
    js_response(crate::assets::MODULE_STORAGE_JS)
}

/// Módulos por dominio: usuarios.
pub async fn module_users_js() -> Response {
    js_response(crate::assets::MODULE_USERS_JS)
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
        pub async fn $name(
            State(state): State<AppState>,
            user: AuthenticatedUser,
            headers: HeaderMap,
        ) -> Response {
            render_shell(&state, &headers, &user, $active).await
        }
    };
}

macro_rules! admin_page_handler {
    ($name:ident, $active:literal) => {
        pub async fn $name(
            State(state): State<AppState>,
            user: AuthenticatedUser,
            headers: HeaderMap,
        ) -> Response {
            let page = render_shell(&state, &headers, &user, $active).await;
            if user.is_admin() {
                page
            } else {
                (StatusCode::FORBIDDEN, page).into_response()
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
pub async fn page_shares(
    State(state): State<AppState>,
    user: AuthenticatedUser,
    headers: HeaderMap,
) -> Response {
    if !state.role.has_shares() {
        return Redirect::to("/dashboard").into_response();
    }
    render_shell(&state, &headers, &user, "shares").await
}

/// Vista de respaldos (oculta si el rol es exclusivamente `ARCHIVOS`).
pub async fn page_backups(
    State(state): State<AppState>,
    user: AuthenticatedUser,
    headers: HeaderMap,
) -> Response {
    if !state.role.has_backups() {
        return Redirect::to("/dashboard").into_response();
    }
    render_shell(&state, &headers, &user, "backups").await
}

/// Vista del terminal (solo superadministrador).
pub async fn page_terminal(
    State(state): State<AppState>,
    user: AuthenticatedUser,
    headers: HeaderMap,
) -> Response {
    let page = render_shell(&state, &headers, &user, "terminal").await;
    if user.is_superadmin() {
        page
    } else {
        (StatusCode::FORBIDDEN, page).into_response()
    }
}

/// Vista de auditoría (solo administradores).
pub async fn page_audit(
    State(state): State<AppState>,
    user: AuthenticatedUser,
    headers: HeaderMap,
) -> Response {
    let page = render_shell(&state, &headers, &user, "logs").await;
    if user.is_admin() {
        page
    } else {
        (StatusCode::FORBIDDEN, page).into_response()
    }
}

/// Módulos por dominio: backups.
pub async fn module_backups_js() -> Response {
    js_response(crate::assets::MODULE_BACKUPS_JS)
}

/// Módulos por dominio: dashboard.
pub async fn module_dashboard_js() -> Response {
    js_response(crate::assets::MODULE_DASHBOARD_JS)
}

/// Módulos por dominio: domain.
pub async fn module_domain_js() -> Response {
    js_response(crate::assets::MODULE_DOMAIN_JS)
}

/// Módulos por dominio: files.
pub async fn module_files_js() -> Response {
    js_response(crate::assets::MODULE_FILES_JS)
}

/// Módulos por dominio: logs.
pub async fn module_logs_js() -> Response {
    js_response(crate::assets::MODULE_LOGS_JS)
}

/// Módulos por dominio: misc.
pub async fn module_misc_js() -> Response {
    js_response(crate::assets::MODULE_MISC_JS)
}

/// Módulos por dominio: services.
pub async fn module_services_js() -> Response {
    js_response(crate::assets::MODULE_SERVICES_JS)
}

/// Módulos por dominio: shares.
pub async fn module_shares_js() -> Response {
    js_response(crate::assets::MODULE_SHARES_JS)
}

/// Módulos por dominio: system.
pub async fn module_system_js() -> Response {
    js_response(crate::assets::MODULE_SYSTEM_JS)
}

/// Módulos por dominio: terminal.
pub async fn module_terminal_js() -> Response {
    js_response(crate::assets::MODULE_TERMINAL_JS)
}
