//! Panel web del servidor NAS sobre axum.
//!
//! F3 (rebanada 2): servidor HTTP con sesiones persistentes en SQLite,
//! dashboard, recursos compartidos y usuarios, con el mismo contrato JSON que
//! el panel PHP.

#![forbid(unsafe_code)]
#![deny(missing_docs)]

pub mod auth;
pub mod db;
pub mod files;
mod routes;

use std::path::PathBuf;
use std::sync::Arc;

use axum::extract::{DefaultBodyLimit, Request};
use axum::http::{header::HeaderName, HeaderValue};
use axum::middleware::Next;
use axum::response::Response;
use axum::routing::{get, post};
use axum::Router;
use nas_core::{NasPaths, ServerRole};

pub use auth::Auth;
pub use db::Db;

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
        .route("/login", get(routes::login_form).post(routes::login_submit))
        .route("/logout", post(routes::logout))
        .route("/", get(routes::dashboard))
        .route("/api/metrics", get(routes::metrics))
        .route("/api/network", get(routes::network))
        .route("/api/shares", get(routes::shares))
        .route("/api/users", get(routes::users))
        .route("/api/services", get(routes::services))
        .route("/api/services/manage", post(routes::service_manage))
        .route("/api/diagnostics", get(routes::diagnostics))
        .route("/api/backups", get(routes::backups))
        .route("/api/backups/plan", post(routes::backup_plan))
        .route("/api/backups/run", post(routes::backup_run))
        .route("/api/audit", get(routes::audit))
        .route("/api/storage", get(routes::storage))
        .route("/api/storage/disks", get(routes::storage_disks))
        .route("/api/storage/inspect", post(routes::storage_inspect))
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
        .route("/api/files/mkdir", post(routes::files_mkdir))
        .route("/api/files/rename", post(routes::files_rename))
        .route("/api/files/delete", post(routes::files_delete))
        .route(
            "/api/files/upload",
            post(routes::files_upload).layer(DefaultBodyLimit::max(512 * 1024 * 1024)),
        )
        .layer(axum::middleware::from_fn(security_headers))
        .with_state(state)
}

async fn security_headers(request: Request, next: Next) -> Response {
    let mut response = next.run(request).await;
    let headers = response.headers_mut();

    let mut set = |name: &'static str, value: &'static str| {
        headers.insert(
            HeaderName::from_static(name),
            HeaderValue::from_static(value),
        );
    };

    set(
        "content-security-policy",
        "default-src 'self'; img-src 'self' data:; style-src 'self' 'unsafe-inline'; script-src 'self' 'unsafe-inline'; frame-ancestors 'self'",
    );
    set("x-frame-options", "SAMEORIGIN");
    set("x-content-type-options", "nosniff");
    set("referrer-policy", "strict-origin-when-cross-origin");
    set(
        "permissions-policy",
        "camera=(), microphone=(), geolocation=(), payment=()",
    );

    response
}
