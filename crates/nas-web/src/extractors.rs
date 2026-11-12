//! Extractores tipados de Axum 0.8 para autenticación, roles y validación CSRF.

use std::ops::Deref;

use axum::extract::{FromRef, FromRequestParts};
use axum::http::header::COOKIE;
use axum::http::request::Parts;
use axum::http::{Method, StatusCode};
use axum::response::{IntoResponse, Redirect, Response};
use nas_core::ApiResponse;

use crate::AppState;

/// Usuario autenticado en el panel web con su rol y validación CSRF previa.
#[derive(Debug, Clone)]
pub struct AuthenticatedUser {
    /// Nombre del usuario autenticado.
    pub username: String,
    /// Rol del usuario (`superadmin`, `admin`, `operator`).
    pub role: String,
    /// Identificador de la sesión activa en SQLite.
    pub session_id: String,
}

impl AuthenticatedUser {
    /// Devuelve el nombre del usuario autenticado.
    pub fn username(&self) -> &str {
        &self.username
    }

    /// Devuelve el rol asignado al usuario.
    pub fn role(&self) -> &str {
        &self.role
    }

    /// Indica si el usuario tiene privilegios de administrador (`admin` o `superadmin`).
    pub fn is_admin(&self) -> bool {
        matches!(self.role.as_str(), "admin" | "superadmin")
    }

    /// Indica si el usuario es superadministrador inmutable.
    pub fn is_superadmin(&self) -> bool {
        self.role == "superadmin"
    }

    /// Comprueba si el rol del usuario cumple una condición requerida, o devuelve error FORBIDDEN.
    pub fn require_admin(&self) -> Result<(), Box<Response>> {
        if self.is_admin() {
            Ok(())
        } else {
            Err(Box::new(forbidden_response(
                "se requieren privilegios de administrador",
            )))
        }
    }

    /// Comprueba si el usuario es superadministrador, o devuelve error FORBIDDEN.
    pub fn require_superadmin(&self) -> Result<(), Box<Response>> {
        if self.is_superadmin() {
            Ok(())
        } else {
            Err(Box::new(forbidden_response(
                "se requieren privilegios de superadministrador",
            )))
        }
    }
}

impl Deref for AuthenticatedUser {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        &self.username
    }
}

impl AsRef<str> for AuthenticatedUser {
    fn as_ref(&self) -> &str {
        &self.username
    }
}

impl PartialEq<str> for AuthenticatedUser {
    fn eq(&self, other: &str) -> bool {
        self.username == other
    }
}

impl PartialEq<&str> for AuthenticatedUser {
    fn eq(&self, other: &&str) -> bool {
        &self.username == other
    }
}

/// Extractor tipado que exige autenticación válida y privilegios de administrador.
#[derive(Debug, Clone)]
pub struct AdminUser(pub AuthenticatedUser);

impl AdminUser {
    /// Devuelve una referencia al usuario autenticado subyacente.
    pub fn user(&self) -> &AuthenticatedUser {
        &self.0
    }
}

impl Deref for AdminUser {
    type Target = AuthenticatedUser;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl AsRef<str> for AdminUser {
    fn as_ref(&self) -> &str {
        self.0.as_ref()
    }
}

impl PartialEq<str> for AdminUser {
    fn eq(&self, other: &str) -> bool {
        self.0.username == other
    }
}

impl PartialEq<&str> for AdminUser {
    fn eq(&self, other: &&str) -> bool {
        &self.0.username == other
    }
}

impl<S> FromRequestParts<S> for AuthenticatedUser
where
    AppState: FromRef<S>,
    S: Send + Sync,
{
    type Rejection = Response;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let app_state = AppState::from_ref(state);
        let path = parts.uri.path();
        let is_api = path.starts_with("/api/");

        // 1. Extraer cookie de sesión
        let cookie_str = parts
            .headers
            .get(COOKIE)
            .and_then(|val| val.to_str().ok())
            .unwrap_or_default();

        let session_id = match crate::auth::cookie_value(cookie_str) {
            Some(id) => id,
            None => {
                return Err(if is_api {
                    unauthorized_response()
                } else {
                    Redirect::to("/login").into_response()
                });
            }
        };

        // 2. Validar sesión en SQLite
        let now = chrono::Utc::now().timestamp();
        let username = match app_state.auth.authenticate(cookie_str, now) {
            Ok(Some(user)) => user,
            Ok(None) => {
                return Err(if is_api {
                    unauthorized_response()
                } else {
                    Redirect::to("/login").into_response()
                });
            }
            Err(e) => return Err(server_error_response(&e.to_string())),
        };

        // 3. Validación CSRF para métodos mutantes (POST, PUT, DELETE, PATCH)
        let is_mutating = matches!(
            parts.method,
            Method::POST | Method::PUT | Method::DELETE | Method::PATCH
        );
        if is_mutating && path != "/login" && path != "/api/auth/login" {
            let csrf_token = parts
                .headers
                .get("x-csrf-token")
                .and_then(|val| val.to_str().ok());

            if !app_state.auth.verify_session_csrf(cookie_str, csrf_token) {
                return Err(csrf_forbidden_response());
            }
        }

        // 4. Resolver rol del usuario en SQLite (priorizando superadministrador configurado)
        let role = if crate::routes::configured_superadmin().as_deref() == Some(&username) {
            "superadmin".to_string()
        } else {
            app_state
                .auth
                .db()
                .user_role(&username)
                .ok()
                .flatten()
                .unwrap_or_else(|| "operator".to_string())
        };

        Ok(AuthenticatedUser {
            username,
            role,
            session_id: session_id.to_string(),
        })
    }
}

impl<S> FromRequestParts<S> for AdminUser
where
    AppState: FromRef<S>,
    S: Send + Sync,
{
    type Rejection = Response;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let user = AuthenticatedUser::from_request_parts(parts, state).await?;
        if !user.is_admin() {
            return Err(forbidden_response(
                "se requieren privilegios de administrador",
            ));
        }
        Ok(AdminUser(user))
    }
}

/// Extractor de usuario con privilegios de superadministrador.
#[derive(Debug, Clone)]
pub struct SuperAdminUser(pub AuthenticatedUser);

impl SuperAdminUser {
    /// Devuelve la referencia al usuario autenticado subyacente.
    pub fn user(&self) -> &AuthenticatedUser {
        &self.0
    }
}

impl<S> FromRequestParts<S> for SuperAdminUser
where
    AppState: FromRef<S>,
    S: Send + Sync,
{
    type Rejection = Response;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let user = AuthenticatedUser::from_request_parts(parts, state).await?;
        if !user.is_superadmin() {
            return Err(forbidden_response(
                "se requieren privilegios de superadministrador",
            ));
        }
        Ok(SuperAdminUser(user))
    }
}

fn unauthorized_response() -> Response {
    (
        StatusCode::UNAUTHORIZED,
        axum::Json(ApiResponse::<serde_json::Value>::error(
            "sesión no válida o expirada",
        )),
    )
        .into_response()
}

fn csrf_forbidden_response() -> Response {
    (
        StatusCode::FORBIDDEN,
        axum::Json(ApiResponse::<serde_json::Value>::error(
            "token CSRF inválido o ausente",
        )),
    )
        .into_response()
}

fn forbidden_response(message: &str) -> Response {
    (
        StatusCode::FORBIDDEN,
        axum::Json(ApiResponse::<serde_json::Value>::error(message)),
    )
        .into_response()
}

fn server_error_response(message: &str) -> Response {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        axum::Json(ApiResponse::<serde_json::Value>::error(message)),
    )
        .into_response()
}
