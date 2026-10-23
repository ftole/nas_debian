//! Panel web del servidor NAS sobre axum.
//!
//! Primera rebanada de F3: servidor HTTP, sesión firmada, dashboard mínimo y
//! endpoints de estado/red con el mismo contrato JSON que el panel PHP.

#![forbid(unsafe_code)]
#![deny(missing_docs)]

pub mod auth;
mod routes;

use std::sync::Arc;

use axum::extract::Request;
use axum::http::{header::HeaderName, HeaderValue};
use axum::middleware::Next;
use axum::response::Response;
use axum::routing::{get, post};
use axum::Router;
use nas_core::NasPaths;

pub use auth::Auth;

/// Estado compartido por los manejadores.
#[derive(Clone, Debug)]
pub struct AppState {
    /// Configuración de autenticación.
    pub auth: Arc<Auth>,
    /// Rutas del despliegue.
    pub paths: Arc<NasPaths>,
}

impl AppState {
    /// Crea el estado a partir de autenticación y rutas.
    pub fn new(auth: Auth, paths: NasPaths) -> Self {
        Self {
            auth: Arc::new(auth),
            paths: Arc::new(paths),
        }
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
