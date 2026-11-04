//! Panel web del servidor NAS sobre axum.
//!
//! F3 (rebanada 2): servidor HTTP con sesiones persistentes en SQLite,
//! dashboard, recursos compartidos y usuarios, con el mismo contrato JSON que
//! el panel PHP.

#![forbid(unsafe_code)]
#![deny(missing_docs)]

pub mod assets;
pub mod auth;
pub mod db;
pub mod extractors;
pub mod files;
pub mod legacy;
mod routes;
pub mod template;
pub mod views;

pub use auth::Auth;
pub use db::Db;
pub use extractors::{AdminUser, AuthenticatedUser};

use std::path::PathBuf;
use std::sync::Arc;

use axum::extract::{DefaultBodyLimit, Request, State};
use axum::http::{header::HeaderName, header::COOKIE, HeaderValue, Method, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::Router;
use nas_core::{ApiResponse, NasPaths, ServerRole};

/// Estado compartido por los manejadores.
#[derive(Clone, Debug)]
pub struct AppState {
    /// Autenticación y sesiones.
    pub auth: Arc<Auth>,
    /// Rutas del despliegue.
    pub paths: Arc<NasPaths>,
    /// Rol del servidor.
    pub role: ServerRole,
    /// Directorio de runners de respaldo.
    pub backup_dir: PathBuf,
}

impl AppState {
    /// Crea el estado del panel.
    pub fn new(auth: Auth, paths: NasPaths, role: ServerRole) -> Self {
        let backup_dir = std::env::var("NAS_BACKUP_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("/usr/local/bin"));

        Self {
            auth: Arc::new(auth),
            paths: Arc::new(paths),
            role,
            backup_dir,
        }
    }

    /// Fija el directorio donde se descubren las tareas de respaldo.
    pub fn with_backup_dir(mut self, dir: impl Into<PathBuf>) -> Self {
        self.backup_dir = dir.into();
        self
    }
}

/// Construye el enrutador con todas las rutas y las cabeceras de seguridad.
pub fn router(state: AppState) -> Router {
    let csrf_state = state.clone();
    Router::new()
        .route("/healthz", get(routes::healthz))
        .route("/css/app.css", get(routes::css))
        .route("/css/login.css", get(routes::login_css))
        .route("/js/app.js", get(routes::app_js))
        .route("/js/vendor/xlsx.full.min.js", get(routes::vendor_xlsx))
        .route("/js/vendor/jszip.min.js", get(routes::vendor_jszip))
        .route("/js/vendor/docx-preview.min.js", get(routes::vendor_docx))
        .route("/favicon.svg", get(routes::favicon))
        .route("/favicon.ico", get(routes::favicon_ico))
        .route("/login", get(routes::login_form).post(routes::login_submit))
        .route("/api/auth/login", post(routes::api_login))
        .route("/logout", post(routes::logout))
        .route("/", get(routes::dashboard))
        .route("/dashboard", get(routes::page_dashboard))
        .route("/shares", get(routes::page_shares))
        .route("/users", get(routes::page_users))
        .route("/backups", get(routes::page_backups))
        .route("/services", get(routes::page_services))
        .route("/diagnostics", get(routes::page_diagnostics))
        .route("/storage", get(routes::page_storage))
        .route("/files", get(routes::page_files))
        .route("/domain", get(routes::page_domain))
        .route("/terminal", get(routes::page_terminal))
        .route("/audit", get(routes::page_audit))
        .route("/logs", get(routes::page_logs))
        .route("/networking", get(routes::page_networking))
        .route("/applications", get(routes::page_applications))
        .route("/updates", get(routes::page_updates))
        .route("/api/metrics", get(routes::metrics))
        .route("/api/network", get(routes::network))
        .route(
            "/api/shares",
            get(routes::shares).post(routes::share_create),
        )
        .route("/api/users", get(routes::users).post(routes::user_create))
        .route("/api/services", get(routes::services))
        .route("/api/services/manage", post(routes::service_manage))
        .route("/api/diagnostics", get(routes::diagnostics))
        .route(
            "/api/backups",
            get(routes::backups).post(routes::backup_create),
        )
        .route("/api/backups/plan", post(routes::backup_plan))
        .route("/api/backups/run", post(routes::backup_run))
        .route("/api/audit", get(routes::audit))
        .route(
            "/api/system/audit/verify",
            get(routes::audit_verify).post(routes::audit_verify),
        )
        .route(
            "/api/audit/verify",
            get(routes::audit_verify).post(routes::audit_verify),
        )
        .route("/api/storage", get(routes::storage))
        .route("/api/storage/disks", get(routes::storage_disks))
        .route("/api/storage/inspect", post(routes::storage_inspect))
        .route("/api/storage/format", post(routes::storage_format))
        .route("/api/storage/lvm", post(routes::storage_lvm))
        .route("/api/storage/subvolume", post(routes::storage_subvolume))
        .route("/api/storage/add-unit", post(routes::storage_add_unit))
        .route(
            "/api/storage/extend-pool",
            post(routes::storage_extend_pool),
        )
        .route("/api/domain", get(routes::domain_status))
        .route("/api/domain/join", post(routes::domain_join))
        .route("/api/domain/leave", post(routes::domain_leave))
        .route("/api/terminal/session", post(routes::terminal_session))
        .route("/api/terminal/send", post(routes::terminal_send))
        .route("/api/terminal/capture", get(routes::terminal_capture))
        .route("/api/terminal/resize", post(routes::terminal_resize))
        .route("/api/terminal/kill", post(routes::terminal_kill))
        .route("/api/files", get(routes::files_list))
        .route("/api/files/download", get(routes::files_download))
        .route("/api/files/zip", get(routes::files_zip))
        .route("/api/files/list", get(routes::files_list_rooted))
        .route("/api/files/raw", get(routes::files_raw))
        .route("/api/logs", get(routes::logs))
        .route(
            "/api/groups",
            get(routes::groups).post(routes::group_create),
        )
        .route("/api/groups/rename", post(routes::group_rename))
        .route("/api/groups/delete", post(routes::group_delete))
        .route("/api/shares/update", post(routes::share_update))
        .route("/api/shares/delete", post(routes::share_delete))
        .route(
            "/api/shares/access",
            get(routes::shares_access).post(routes::share_access_update),
        )
        .route(
            "/api/shares/access/repair",
            post(routes::share_access_repair),
        )
        .route("/api/shares/access/audit", get(routes::shares_access_audit))
        .route("/api/users/update", post(routes::user_update))
        .route("/api/users/toggle", post(routes::user_toggle))
        .route("/api/users/groups", post(routes::user_groups))
        .route("/api/users/delete", post(routes::user_delete))
        .route("/api/backups/running", get(routes::backups_running))
        .route("/api/backups/{id}/run", post(routes::backup_run_id))
        .route("/api/backups/{id}/stop", post(routes::backup_stop_id))
        .route("/api/backups/{id}/logs", get(routes::backup_logs_id))
        .route("/api/backups/test", post(routes::backup_test))
        .route("/api/backups/update", post(routes::backup_update))
        .route("/api/backups/delete", post(routes::backup_delete))
        .route("/api/backups/permanent", get(routes::backups_permanent))
        .route("/api/backups/{id}/freeze", post(routes::backup_freeze_id))
        .route("/api/backups/freeze", post(routes::backup_freeze))
        .route("/api/backups/restore", post(routes::backup_restore))
        .route(
            "/api/backups/permanent/delete",
            post(routes::backup_permanent_delete),
        )
        .route("/api/backups/", post(routes::backup_create))
        .route("/api/storage/units", get(routes::storage_units))
        .route("/api/storage/subvolumes", get(routes::storage_subvolumes))
        .route(
            "/api/storage/scrub",
            get(routes::storage_scrub).post(routes::storage_scrub_start),
        )
        .route("/api/storage/trim", post(routes::storage_trim))
        .route("/api/system/updates", get(routes::system_updates))
        .route("/api/system/applications", get(routes::system_applications))
        .route("/api/system/reboot", post(routes::system_reboot))
        .route("/api/system/update/run", post(routes::system_update_run))
        .route("/api/domain/status", get(routes::domain_status))
        .route("/api/domain/discover", post(routes::domain_discover))
        .route("/api/diagnostics/run", post(routes::diagnostics_run))
        .route("/api/terminal/history", get(routes::terminal_history))
        .route("/api/files/content", get(routes::files_content))
        .route("/api/files/create", post(routes::files_create))
        .route("/api/files/move", post(routes::files_move))
        .route("/api/files/save", post(routes::files_save))
        .route("/api/files/trash", get(routes::files_trash))
        .route("/api/files/trash/restore", post(routes::trash_restore))
        .route(
            "/api/files/trash/restore-all",
            post(routes::trash_restore_all),
        )
        .route("/api/files/trash/delete", post(routes::trash_delete))
        .route("/api/files/trash/empty", post(routes::trash_empty))
        .route("/api/files/mkdir", post(routes::files_mkdir))
        .route("/api/files/rename", post(routes::files_rename))
        .route("/api/files/delete", post(routes::files_delete))
        .route(
            "/api/files/upload",
            post(routes::files_upload).layer(DefaultBodyLimit::max(
                usize::try_from(50u64 * 1024 * 1024 * 1024).unwrap_or(usize::MAX),
            )),
        )
        .layer(axum::middleware::from_fn_with_state(csrf_state, csrf_guard))
        .layer(axum::middleware::from_fn(security_headers))
        .with_state(state)
}

async fn csrf_guard(State(state): State<AppState>, request: Request, next: Next) -> Response {
    let is_mutating = matches!(
        *request.method(),
        Method::POST | Method::PUT | Method::DELETE | Method::PATCH
    );
    let path = request.uri().path();
    if is_mutating && path != "/login" && path != "/api/auth/login" {
        let cookies = request
            .headers()
            .get(COOKIE)
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default();
        let now = chrono::Utc::now().timestamp();
        if let Ok(Some(_)) = state.auth.authenticate(cookies, now) {
            let csrf = request
                .headers()
                .get("x-csrf-token")
                .and_then(|value| value.to_str().ok());
            if !state.auth.verify_session_csrf(cookies, csrf) {
                return (
                    StatusCode::FORBIDDEN,
                    axum::Json(ApiResponse::<serde_json::Value>::error(
                        "token CSRF inválido o ausente",
                    )),
                )
                    .into_response();
            }
        }
    }
    next.run(request).await
}

async fn security_headers(request: Request, next: Next) -> Response {
    let mut response = next.run(request).await;
    let headers = response.headers_mut();

    let mut set_default = |name: &'static str, value: &'static str| {
        headers
            .entry(HeaderName::from_static(name))
            .or_insert_with(|| HeaderValue::from_static(value));
    };

    set_default(
        "content-security-policy",
        "default-src 'self'; img-src 'self' data:; style-src 'self' 'unsafe-inline'; script-src 'self' 'unsafe-inline'; frame-ancestors 'self'",
    );
    set_default("x-frame-options", "SAMEORIGIN");
    set_default("x-content-type-options", "nosniff");
    set_default("referrer-policy", "strict-origin-when-cross-origin");
    set_default(
        "permissions-policy",
        "camera=(), microphone=(), geolocation=(), payment=()",
    );

    response
}
