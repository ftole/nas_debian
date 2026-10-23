//! Pruebas de integración del panel web.

use axum::body::Body;
use axum::http::{header::SET_COOKIE, Request, StatusCode};
use axum::Router;
use http_body_util::BodyExt;
use nas_core::NasPaths;
use nas_web::{router, AppState, Auth};
use sha2::{Digest, Sha256};
use tower::ServiceExt;

fn sha256_hex(value: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(value.as_bytes());
    hex::encode(hasher.finalize())
}

fn app() -> Router {
    let auth = Auth::new(
        "admin",
        sha256_hex("Secreta123"),
        b"secreto-de-prueba-1234",
        3600,
        false,
    );
    router(AppState::new(auth, NasPaths::development()))
}

async fn body_text(response: axum::response::Response) -> String {
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("cuerpo")
        .to_bytes();
    String::from_utf8_lossy(&bytes).to_string()
}

fn login_request(user: &str, password: &str) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri("/login")
        .header("content-type", "application/x-www-form-urlencoded")
        .body(Body::from(format!("username={user}&password={password}")))
        .expect("petición")
}

#[tokio::test]
async fn healthz_responde_sin_sesion() {
    let response = app()
        .oneshot(
            Request::builder()
                .uri("/healthz")
                .body(Body::empty())
                .expect("petición"),
        )
        .await
        .expect("respuesta");

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(body_text(response).await, "ok");
}

#[tokio::test]
async fn api_sin_sesion_devuelve_401() {
    let response = app()
        .oneshot(
            Request::builder()
                .uri("/api/metrics")
                .body(Body::empty())
                .expect("petición"),
        )
        .await
        .expect("respuesta");

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    let payload: serde_json::Value =
        serde_json::from_str(&body_text(response).await).expect("json");
    assert_eq!(payload["success"], false);
}

#[tokio::test]
async fn login_incorrecto_devuelve_401() {
    let response = app()
        .oneshot(login_request("admin", "incorrecta"))
        .await
        .expect("respuesta");

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert!(body_text(response).await.contains("Acceso denegado"));
}

#[tokio::test]
async fn login_correcto_emite_cookie_y_permite_metricas() {
    let app = app();

    let response = app
        .clone()
        .oneshot(login_request("admin", "Secreta123"))
        .await
        .expect("respuesta");

    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    let set_cookie = response
        .headers()
        .get(SET_COOKIE)
        .and_then(|value| value.to_str().ok())
        .expect("cookie")
        .to_string();
    assert!(set_cookie.contains("HttpOnly"));
    assert!(set_cookie.contains("SameSite=Strict"));

    let cookie = set_cookie
        .split(';')
        .next()
        .expect("par cookie")
        .to_string();

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/metrics")
                .header("cookie", cookie)
                .body(Body::empty())
                .expect("petición"),
        )
        .await
        .expect("respuesta");

    assert_eq!(response.status(), StatusCode::OK);
    let payload: serde_json::Value =
        serde_json::from_str(&body_text(response).await).expect("json");
    assert_eq!(payload["success"], true);
    assert!(payload["data"]["hostname"].is_string());
}

#[tokio::test]
async fn dashboard_redirige_sin_sesion_y_carga_con_sesion() {
    let app = app();

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/")
                .body(Body::empty())
                .expect("petición"),
        )
        .await
        .expect("respuesta");
    assert_eq!(response.status(), StatusCode::SEE_OTHER);

    let login = app
        .clone()
        .oneshot(login_request("admin", "Secreta123"))
        .await
        .expect("login");
    let cookie = login
        .headers()
        .get(SET_COOKIE)
        .and_then(|value| value.to_str().ok())
        .expect("cookie")
        .split(';')
        .next()
        .expect("par cookie")
        .to_string();

    let response = app
        .oneshot(
            Request::builder()
                .uri("/")
                .header("cookie", cookie)
                .body(Body::empty())
                .expect("petición"),
        )
        .await
        .expect("respuesta");

    assert_eq!(response.status(), StatusCode::OK);
    let body = body_text(response).await;
    assert!(body.contains("Panel NAS"));
    assert!(body.contains("Salir"));
}

#[tokio::test]
async fn cabeceras_de_seguridad_presentes() {
    let response = app()
        .oneshot(
            Request::builder()
                .uri("/healthz")
                .body(Body::empty())
                .expect("petición"),
        )
        .await
        .expect("respuesta");

    let headers = response.headers();
    assert!(headers.contains_key("content-security-policy"));
    assert!(headers.contains_key("x-frame-options"));
    assert!(headers.contains_key("x-content-type-options"));
}
