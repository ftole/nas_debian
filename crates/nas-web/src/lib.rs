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
pub use extractors::{AdminUser, AuthenticatedUser, SuperAdminUser};

use std::path::PathBuf;
use std::sync::Arc;

use axum::extract::{DefaultBodyLimit, Request};
use axum::http::{header::HeaderName, HeaderValue};
use axum::middleware::Next;
use axum::response::Response;
use axum::routing::{get, post};
use axum::Router;
use nas_core::{NasPaths, ServerRole};

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
    Router::new()
        .route("/healthz", get(routes::healthz))
        .route("/css/app.css", get(routes::css))
        .route("/css/login.css", get(routes::login_css))
        .route("/js/app.js", get(routes::app_js))
        .route("/js/lib/api.js", get(routes::lib_api_js))
        .route("/js/lib/modal.js", get(routes::lib_modal_js))
        .route("/js/lib/utils.js", get(routes::lib_utils_js))
        .route("/js/lib/toast.js", get(routes::lib_toast_js))
        .route("/js/modules/storage.js", get(routes::module_storage_js))
        .route("/js/modules/users.js", get(routes::module_users_js))
        .route("/js/modules/terminal.js", get(routes::module_terminal_js))
        .route("/js/modules/system.js", get(routes::module_system_js))
        .route("/js/modules/shares.js", get(routes::module_shares_js))
        .route("/js/modules/services.js", get(routes::module_services_js))
        .route("/js/modules/misc.js", get(routes::module_misc_js))
        .route("/js/modules/logs.js", get(routes::module_logs_js))
        .route("/js/modules/files.js", get(routes::module_files_js))
        .route("/js/modules/domain.js", get(routes::module_domain_js))
        .route("/js/modules/dashboard.js", get(routes::module_dashboard_js))
        .route("/js/modules/backups.js", get(routes::module_backups_js))
        .route("/js/vendor/xlsx.full.min.js", get(routes::vendor_xlsx))
        .route("/js/vendor/jszip.min.js", get(routes::vendor_jszip))
        .route("/js/vendor/docx-preview.min.js", get(routes::vendor_docx))
        .route("/favicon.svg", get(routes::favicon))
        .route("/favicon.ico", get(routes::favicon_ico))
        .route("/img/logo-iintec.svg", get(routes::logo_iintec))
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
        .merge(routes::system::router())
        .merge(routes::shares::router())
        .merge(routes::users::router())
        .merge(routes::services::router())
        .merge(routes::diagnostics::router())
        .merge(routes::backups::router())
        .merge(routes::audit::router())
        .merge(routes::storage::router())
        .merge(routes::domain::router())
        .merge(routes::terminal::router())
        .merge(routes::files::router())
        .merge(routes::preview::router())
        .merge(routes::trash::router())
        .merge(routes::logs::router())
        .route(
            "/api/files/upload",
            post(routes::files::files_upload).layer(DefaultBodyLimit::max(
                usize::try_from(50u64 * 1024 * 1024 * 1024).unwrap_or(usize::MAX),
            )),
        )
        .layer(axum::middleware::from_fn(security_headers))
        .with_state(state)
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
