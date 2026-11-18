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
use nas_core::{NasError, NasPaths, NasResult, ResourceBudget, ServerRole, SystemResources};

/// Error devuelto al ejecutar una operación criptográfica de autenticación acotada.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthBlockingError {
    /// Cola de inicios de sesión simultáneos saturada (aplicar backpressure HTTP 503).
    Busy,
    /// La tarea bloqueante fue interrumpida antes de finalizar.
    Failed,
}

fn auth_semaphores(
    budget: &ResourceBudget,
) -> (Arc<tokio::sync::Semaphore>, Arc<tokio::sync::Semaphore>) {
    let auth_permits = budget.max_concurrent_auth_ops.max(1);
    let queue_permits = auth_permits.saturating_mul(4).max(4);
    (
        Arc::new(tokio::sync::Semaphore::new(auth_permits)),
        Arc::new(tokio::sync::Semaphore::new(queue_permits)),
    )
}

fn upload_semaphores(
    budget: &ResourceBudget,
) -> (Arc<tokio::sync::Semaphore>, Arc<tokio::sync::Semaphore>) {
    let upload_permits = budget.max_concurrent_upload_ops.max(1);
    let queue_permits = upload_permits.saturating_mul(3).max(3);
    (
        Arc::new(tokio::sync::Semaphore::new(upload_permits)),
        Arc::new(tokio::sync::Semaphore::new(queue_permits)),
    )
}

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
    /// Presupuesto dinámico de recursos del sistema.
    pub budget: ResourceBudget,
    /// Semáforo que acota operaciones bloqueantes de sistema (`systemctl`, `lsblk`, `df`, etc.).
    pub system_ops: Arc<tokio::sync::Semaphore>,
    /// Semáforo que acota operaciones pesadas de CPU/E/S (`zip`, `audit_verify`, `du`, discos).
    pub heavy_ops: Arc<tokio::sync::Semaphore>,
    /// Semáforo que acota verificaciones criptográficas simultáneas de login (PBKDF2).
    pub auth_ops: Arc<tokio::sync::Semaphore>,
    /// Semáforo que acota la cola total de inicios de sesión en espera para aplicar backpressure.
    pub auth_queue: Arc<tokio::sync::Semaphore>,
    /// Semáforo que acota subidas simultáneas de archivos en streaming.
    pub upload_ops: Arc<tokio::sync::Semaphore>,
    /// Semáforo que acota la cola de subidas en espera para aplicar backpressure.
    pub upload_queue: Arc<tokio::sync::Semaphore>,
}

impl AppState {
    /// Crea el estado del panel.
    pub fn new(auth: Auth, paths: NasPaths, role: ServerRole) -> Self {
        let backup_dir = std::env::var("NAS_BACKUP_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("/usr/local/bin"));
        let budget = ResourceBudget::detect();
        let system_ops = Arc::new(tokio::sync::Semaphore::new(
            budget.max_concurrent_system_ops.max(1),
        ));
        let heavy_ops = Arc::new(tokio::sync::Semaphore::new(
            budget.max_concurrent_heavy_ops.max(1),
        ));
        let (auth_ops, auth_queue) = auth_semaphores(&budget);
        let (upload_ops, upload_queue) = upload_semaphores(&budget);

        Self {
            auth: Arc::new(auth),
            paths: Arc::new(paths),
            role,
            backup_dir,
            budget,
            system_ops,
            heavy_ops,
            auth_ops,
            auth_queue,
            upload_ops,
            upload_queue,
        }
    }

    /// Sobrescribe el presupuesto dinámico y reconfigura los semáforos de concurrencia.
    pub fn with_budget(mut self, budget: ResourceBudget) -> Self {
        self.system_ops = Arc::new(tokio::sync::Semaphore::new(
            budget.max_concurrent_system_ops.max(1),
        ));
        self.heavy_ops = Arc::new(tokio::sync::Semaphore::new(
            budget.max_concurrent_heavy_ops.max(1),
        ));
        let (auth_ops, auth_queue) = auth_semaphores(&budget);
        self.auth_ops = auth_ops;
        self.auth_queue = auth_queue;
        let (upload_ops, upload_queue) = upload_semaphores(&budget);
        self.upload_ops = upload_ops;
        self.upload_queue = upload_queue;
        self.budget = budget;
        self
    }

    /// Fija el directorio donde se descubren las tareas de respaldo.
    pub fn with_backup_dir(mut self, dir: impl Into<PathBuf>) -> Self {
        self.backup_dir = dir.into();
        self
    }

    /// Ejecuta una operación de sistema bloqueante acotada por el semáforo `system_ops`.
    pub async fn run_system_blocking<F, T>(&self, task: F) -> Result<T, tokio::task::JoinError>
    where
        F: FnOnce() -> T + Send + 'static,
        T: Send + 'static,
    {
        let _permit = self.system_ops.acquire().await.ok();
        tokio::task::spawn_blocking(task).await
    }

    /// Ejecuta una operación pesada de CPU/E/S acotada por `heavy_ops` con regulación
    /// reactiva en vivo ante presión crítica de memoria o E/S.
    pub async fn run_heavy_blocking<F, T>(&self, task: F) -> Result<T, tokio::task::JoinError>
    where
        F: FnOnce() -> T + Send + 'static,
        T: Send + 'static,
    {
        let live = SystemResources::detect();
        self.run_heavy_blocking_with_resources(live, task).await
    }

    /// Ejecuta una operación pesada aplicando regulación reactiva según `live`.
    pub async fn run_heavy_blocking_with_resources<F, T>(
        &self,
        live: SystemResources,
        task: F,
    ) -> Result<T, tokio::task::JoinError>
    where
        F: FnOnce() -> T + Send + 'static,
        T: Send + 'static,
    {
        let (permits, backoff) = self.budget.heavy_ops_throttle_plan(&live);
        if !backoff.is_zero() {
            tokio::time::sleep(backoff).await;
        }
        let _permit = self.heavy_ops.acquire_many(permits).await.ok();
        tokio::task::spawn_blocking(task).await
    }

    /// Ejecuta una operación criptográfica de autenticación (PBKDF2) con concurrencia
    /// y cola de espera acotadas para evitar saturación de CPU y del pool bloqueante.
    pub async fn run_auth_blocking<F, T>(&self, task: F) -> Result<T, AuthBlockingError>
    where
        F: FnOnce() -> T + Send + 'static,
        T: Send + 'static,
    {
        let queue_permit = self
            .auth_queue
            .try_acquire()
            .map_err(|_| AuthBlockingError::Busy)?;
        let exec_permit = self
            .auth_ops
            .acquire()
            .await
            .map_err(|_| AuthBlockingError::Failed)?;
        let result = tokio::task::spawn_blocking(task)
            .await
            .map_err(|_| AuthBlockingError::Failed);
        drop(exec_permit);
        drop(queue_permit);
        result
    }

    /// Ejecuta una operación SQLite fuera del reactor asíncrono sin competir con `system_ops`.
    pub async fn run_db<F, T>(&self, task: F) -> NasResult<T>
    where
        F: FnOnce(&Db) -> NasResult<T> + Send + 'static,
        T: Send + 'static,
    {
        let db = self.auth.db().clone();
        tokio::task::spawn_blocking(move || task(&db))
            .await
            .map_err(|e| {
                NasError::Database(format!("operación de base de datos interrumpida: {e}"))
            })?
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

#[cfg(test)]
mod tests {
    use super::*;
    use nas_core::SystemResources;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[tokio::test]
    async fn run_heavy_blocking_serializa_bajo_presion_critica() {
        let dir = std::env::temp_dir().join(format!("nas-web-heavy-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let db = Db::in_memory().expect("db");
        let auth = Auth::new(db, 3600, false);
        let mut paths = NasPaths::development();
        paths.state_dir = dir.clone();
        paths.data_root = dir.clone();

        let idle_res =
            SystemResources::from_snapshot(12, None, 16384, 12288, None, 0.3, 0.0, 0.0, 0.0);
        let budget = ResourceBudget::from_resources(idle_res);
        assert_eq!(budget.max_concurrent_heavy_ops, 3);
        let state = AppState::new(auth, paths, ServerRole::Arquivos).with_budget(budget);

        let critical_res =
            SystemResources::from_snapshot(12, None, 16384, 128, None, 1.5, 10.0, 40.0, 60.0);

        let active = Arc::new(AtomicUsize::new(0));
        let max_seen = Arc::new(AtomicUsize::new(0));
        let mut handles = Vec::new();
        for _ in 0..3 {
            let st = state.clone();
            let live = critical_res.clone();
            let act = Arc::clone(&active);
            let mx = Arc::clone(&max_seen);
            handles.push(tokio::spawn(async move {
                st.run_heavy_blocking_with_resources(live, move || {
                    let now = act.fetch_add(1, Ordering::SeqCst) + 1;
                    mx.fetch_max(now, Ordering::SeqCst);
                    std::thread::sleep(std::time::Duration::from_millis(30));
                    act.fetch_sub(1, Ordering::SeqCst);
                })
                .await
                .expect("join");
            }));
        }
        for h in handles {
            h.await.expect("task");
        }
        assert_eq!(max_seen.load(Ordering::SeqCst), 1);

        let _ = std::fs::remove_dir_all(&dir);
    }
}
