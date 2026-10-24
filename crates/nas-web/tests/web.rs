//! Pruebas de integración del panel web.

use std::path::{Path, PathBuf};

use axum::body::Body;
use axum::http::{header::SET_COOKIE, Request, StatusCode};
use axum::Router;
use http_body_util::BodyExt;
use nas_core::{NasPaths, ServerRole};
use nas_web::{router, AppState, Auth, Db};
use sha2::{Digest, Sha256};
use tower::ServiceExt;

const SMB_CONF: &str = "\
[global]
   workgroup = TEAM-JOFRATO
   server min protocol = SMB2_02

[SISTEMAS]
   path = /srv/nas/SISTEMAS
   read only = no
   valid users = @grp_samba

[CONTABILIDAD$]
   path = /srv/nas/CONTABILIDAD
   read only = yes
   browseable = no
";

fn sha256_hex(value: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(value.as_bytes());
    hex::encode(hasher.finalize())
}

fn temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "nas-web-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    std::fs::create_dir_all(&dir).expect("dir");
    dir
}

fn paths_for(dir: &Path) -> NasPaths {
    let mut paths = NasPaths::development();
    paths.state_dir = dir.to_path_buf();
    paths.samba_conf = dir.join("smb.conf");
    paths.role_file = dir.join("role");
    std::fs::write(&paths.samba_conf, SMB_CONF).expect("smb.conf");
    std::fs::write(&paths.role_file, "ARCHIVOS\n").expect("role");
    paths
}

fn app(role: ServerRole) -> Router {
    let dir = temp_dir("app");
    let db = Db::in_memory().expect("db");
    db.upsert_user("admin", &sha256_hex("Secreta123"), "superadmin")
        .expect("usuario");
    let auth = Auth::new(db, 3600, false);
    router(AppState::new(auth, paths_for(&dir), role))
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

async fn cookie_for(app: &Router) -> String {
    let response = app
        .clone()
        .oneshot(login_request("admin", "Secreta123"))
        .await
        .expect("login");
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    response
        .headers()
        .get(SET_COOKIE)
        .and_then(|value| value.to_str().ok())
        .expect("cookie")
        .split(';')
        .next()
        .expect("par cookie")
        .to_string()
}

fn get(uri: &str, cookie: &str) -> Request<Body> {
    Request::builder()
        .uri(uri)
        .header("cookie", cookie)
        .body(Body::empty())
        .expect("petición")
}

#[tokio::test]
async fn healthz_responde_sin_sesion() {
    let response = app(ServerRole::Arquivos)
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
    let response = app(ServerRole::Arquivos)
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
    let response = app(ServerRole::Arquivos)
        .oneshot(login_request("admin", "incorrecta"))
        .await
        .expect("respuesta");

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn sesion_permite_metricas_y_red() {
    let app = app(ServerRole::Arquivos);
    let cookie = cookie_for(&app).await;

    for uri in ["/api/metrics", "/api/network", "/api/users"] {
        let response = app
            .clone()
            .oneshot(get(uri, &cookie))
            .await
            .expect("respuesta");
        assert_eq!(response.status(), StatusCode::OK, "fallo en {uri}");
        let payload: serde_json::Value =
            serde_json::from_str(&body_text(response).await).expect("json");
        assert_eq!(payload["success"], true);
    }
}

#[tokio::test]
async fn shares_lista_los_recursos_del_smb_conf() {
    let app = app(ServerRole::Arquivos);
    let cookie = cookie_for(&app).await;

    let response = app
        .oneshot(get("/api/shares", &cookie))
        .await
        .expect("respuesta");
    assert_eq!(response.status(), StatusCode::OK);

    let payload: serde_json::Value =
        serde_json::from_str(&body_text(response).await).expect("json");
    let shares = payload["data"].as_array().expect("arreglo");
    assert_eq!(shares.len(), 2);
    assert_eq!(shares[0]["name"], "SISTEMAS");
    assert_eq!(shares[0]["read_only"], false);
    assert_eq!(shares[1]["name"], "CONTABILIDAD$");
    assert_eq!(shares[1]["browseable"], false);
}

#[tokio::test]
async fn shares_rechaza_el_rol_backup() {
    let app = app(ServerRole::Backup);
    let cookie = cookie_for(&app).await;

    let response = app
        .oneshot(get("/api/shares", &cookie))
        .await
        .expect("respuesta");
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn dashboard_redirige_sin_sesion_y_carga_con_sesion() {
    let app = app(ServerRole::Arquivos);

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

    let cookie = cookie_for(&app).await;
    let response = app.oneshot(get("/", &cookie)).await.expect("respuesta");
    assert_eq!(response.status(), StatusCode::OK);
    let body = body_text(response).await;
    assert!(body.contains("Panel NAS"));
    assert!(body.contains("Salir"));
}

#[tokio::test]
async fn la_sesion_persiste_entre_reinicios() {
    let dir = temp_dir("persistencia");
    let db_path = dir.join("nas.sqlite");

    let db = Db::open(&db_path).expect("db");
    db.upsert_user("admin", &sha256_hex("Secreta123"), "superadmin")
        .expect("usuario");
    let app = router(AppState::new(
        Auth::new(db.clone(), 3600, false),
        paths_for(&dir),
        ServerRole::Arquivos,
    ));
    let cookie = cookie_for(&app).await;
    drop(app);
    drop(db);

    // "Reinicio": nueva conexión al mismo archivo y nuevo router.
    let db = Db::open(&db_path).expect("reabrir");
    let app = router(AppState::new(
        Auth::new(db, 3600, false),
        paths_for(&dir),
        ServerRole::Arquivos,
    ));

    let response = app
        .oneshot(get("/api/metrics", &cookie))
        .await
        .expect("respuesta");
    assert_eq!(response.status(), StatusCode::OK);

    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn cabeceras_de_seguridad_presentes() {
    let response = app(ServerRole::Arquivos)
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
