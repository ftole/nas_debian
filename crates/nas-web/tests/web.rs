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
    paths.data_root = dir.join("data");
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

fn post_json(uri: &str, cookie: &str, body: &str) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri(uri)
        .header("cookie", cookie)
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .expect("petición")
}

fn upload_body(boundary: &str, dir: &str, filename: &str, content: &str) -> Body {
    let crlf = "\r\n";
    let mut body = String::new();
    body.push_str(&format!("--{boundary}{crlf}"));
    body.push_str(&format!(
        "Content-Disposition: form-data; name=\"path\"{crlf}{crlf}"
    ));
    body.push_str(dir);
    body.push_str(&format!("{crlf}--{boundary}{crlf}"));
    body.push_str(&format!(
        "Content-Disposition: form-data; name=\"file\"; filename=\"{filename}\"{crlf}"
    ));
    body.push_str(&format!("Content-Type: text/plain{crlf}{crlf}"));
    body.push_str(content);
    body.push_str(&format!("{crlf}--{boundary}--{crlf}"));
    Body::from(body)
}

#[tokio::test]
async fn files_crea_lista_renombra_y_borra() {
    let app = app(ServerRole::Arquivos);
    let cookie = cookie_for(&app).await;

    let response = app
        .clone()
        .oneshot(post_json(
            "/api/files/mkdir",
            &cookie,
            r#"{"path":"SISTEMAS"}"#,
        ))
        .await
        .expect("mkdir");
    assert_eq!(response.status(), StatusCode::OK);

    let response = app
        .clone()
        .oneshot(get("/api/files?path=", &cookie))
        .await
        .expect("list");
    assert_eq!(response.status(), StatusCode::OK);
    let payload: serde_json::Value =
        serde_json::from_str(&body_text(response).await).expect("json");
    let entries = payload["data"].as_array().expect("arreglo");
    assert!(entries.iter().any(|entry| entry["name"] == "SISTEMAS"));

    let response = app
        .clone()
        .oneshot(post_json(
            "/api/files/rename",
            &cookie,
            r#"{"from":"SISTEMAS","to":"SISTEMAS_2026"}"#,
        ))
        .await
        .expect("rename");
    assert_eq!(response.status(), StatusCode::OK);

    let response = app
        .clone()
        .oneshot(post_json(
            "/api/files/delete",
            &cookie,
            r#"{"path":"SISTEMAS_2026"}"#,
        ))
        .await
        .expect("delete");
    assert_eq!(response.status(), StatusCode::OK);
    let payload: serde_json::Value =
        serde_json::from_str(&body_text(response).await).expect("json");
    assert!(payload["data"]["trash"]
        .as_str()
        .unwrap_or_default()
        .contains(".trash"));
}

#[tokio::test]
async fn files_rechaza_escape_de_la_raiz() {
    let app = app(ServerRole::Arquivos);
    let cookie = cookie_for(&app).await;

    for uri in [
        "/api/files?path=../etc",
        "/api/files?path=/etc/passwd",
        "/api/files?path=docs/../../etc",
    ] {
        let response = app
            .clone()
            .oneshot(get(uri, &cookie))
            .await
            .expect("respuesta");
        assert_eq!(response.status(), StatusCode::FORBIDDEN, "fallo en {uri}");
    }
}

#[tokio::test]
async fn files_sube_descarga_y_detecta_duplicados() {
    let app = app(ServerRole::Arquivos);
    let cookie = cookie_for(&app).await;
    let boundary = "XBOUNDARYX";

    let request = Request::builder()
        .method("POST")
        .uri("/api/files/upload")
        .header("cookie", &cookie)
        .header(
            "content-type",
            format!("multipart/form-data; boundary={boundary}"),
        )
        .body(upload_body(
            boundary,
            "",
            "informe.txt",
            "contenido de prueba",
        ))
        .expect("petición");

    let response = app.clone().oneshot(request).await.expect("upload");
    assert_eq!(response.status(), StatusCode::OK);
    let payload: serde_json::Value =
        serde_json::from_str(&body_text(response).await).expect("json");
    assert_eq!(payload["data"]["uploaded"][0], "informe.txt");

    let response = app
        .clone()
        .oneshot(get("/api/files/download?path=informe.txt", &cookie))
        .await
        .expect("download");
    assert_eq!(response.status(), StatusCode::OK);
    assert!(response
        .headers()
        .get("content-disposition")
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .contains("informe.txt"));
    assert_eq!(body_text(response).await, "contenido de prueba");

    let request = Request::builder()
        .method("POST")
        .uri("/api/files/upload")
        .header("cookie", &cookie)
        .header(
            "content-type",
            format!("multipart/form-data; boundary={boundary}"),
        )
        .body(upload_body(boundary, "", "informe.txt", "otra vez"))
        .expect("petición");
    let response = app.oneshot(request).await.expect("upload duplicado");
    assert_eq!(response.status(), StatusCode::CONFLICT);
}

#[tokio::test]
async fn files_requiere_sesion() {
    let response = app(ServerRole::Arquivos)
        .oneshot(
            Request::builder()
                .uri("/api/files")
                .body(Body::empty())
                .expect("petición"),
        )
        .await
        .expect("respuesta");
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

fn app_with_fixtures(role: ServerRole, backup: Option<(&str, &str)>) -> (Router, PathBuf) {
    let dir = temp_dir("f3d");
    let db = Db::in_memory().expect("db");
    db.upsert_user("admin", &sha256_hex("Secreta123"), "superadmin")
        .expect("usuario");
    let auth = Auth::new(db, 3600, false);
    let paths = paths_for(&dir);

    let backup_dir = dir.join("runners");
    std::fs::create_dir_all(&backup_dir).expect("runners");
    if let Some((id, content)) = backup {
        std::fs::write(backup_dir.join(format!("backup_{id}.sh")), content).expect("runner");
    }

    let state = AppState::new(auth, paths, role).with_backup_dir(backup_dir);
    (router(state), dir)
}

const LOCAL_RUNNER: &str = "SOURCE_TYPE=\"local\"
SRC_PATH=\"/datos\"
RETENTION=3
";

#[tokio::test]
async fn services_y_diagnostics_responden() {
    let (app, _dir) = app_with_fixtures(ServerRole::Arquivos, None);
    let cookie = cookie_for(&app).await;

    let response = app
        .clone()
        .oneshot(get("/api/services", &cookie))
        .await
        .expect("servicios");
    assert_eq!(response.status(), StatusCode::OK);
    let payload: serde_json::Value =
        serde_json::from_str(&body_text(response).await).expect("json");
    let services = payload["data"].as_array().expect("arreglo");
    assert_eq!(services.len(), 4);
    assert!(services.iter().any(|item| item["service"] == "smbd"));

    let response = app
        .oneshot(get("/api/diagnostics", &cookie))
        .await
        .expect("diagnóstico");
    assert_eq!(response.status(), StatusCode::OK);
    let payload: serde_json::Value =
        serde_json::from_str(&body_text(response).await).expect("json");
    let checks = payload["data"].as_array().expect("arreglo");
    assert!(checks.iter().any(|check| check["name"] == "rol"));
}

#[tokio::test]
async fn backups_lista_y_planifica() {
    let (app, _dir) = app_with_fixtures(ServerRole::Arquivos, Some(("demo", LOCAL_RUNNER)));
    let cookie = cookie_for(&app).await;

    let response = app
        .clone()
        .oneshot(get("/api/backups", &cookie))
        .await
        .expect("lista");
    assert_eq!(response.status(), StatusCode::OK);
    let payload: serde_json::Value =
        serde_json::from_str(&body_text(response).await).expect("json");
    let tasks = payload["data"].as_array().expect("arreglo");
    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0]["task"]["id"], "demo");

    let response = app
        .oneshot(post_json(
            "/api/backups/plan",
            &cookie,
            r#"{"path":"demo"}"#,
        ))
        .await
        .expect("plan");
    assert_eq!(response.status(), StatusCode::OK);
    let payload: serde_json::Value =
        serde_json::from_str(&body_text(response).await).expect("json");
    assert!(!payload["data"]["steps"]
        .as_array()
        .expect("pasos")
        .is_empty());
}

#[tokio::test]
async fn auditoria_registra_el_login() {
    let (app, _dir) = app_with_fixtures(ServerRole::Arquivos, None);
    let cookie = cookie_for(&app).await;

    let response = app
        .oneshot(get("/api/audit?limit=10", &cookie))
        .await
        .expect("auditoría");
    assert_eq!(response.status(), StatusCode::OK);
    let payload: serde_json::Value =
        serde_json::from_str(&body_text(response).await).expect("json");
    let rows = payload["data"].as_array().expect("arreglo");
    assert!(rows
        .iter()
        .any(|row| row["action"] == "login" && row["result"] == "SUCCESS"));
}

#[tokio::test]
async fn gestion_de_servicio_rechaza_fuera_de_lista_blanca() {
    let (app, _dir) = app_with_fixtures(ServerRole::Arquivos, None);
    let cookie = cookie_for(&app).await;

    let response = app
        .oneshot(post_json(
            "/api/services/manage",
            &cookie,
            r#"{"service":"sshd","action":"restart"}"#,
        ))
        .await
        .expect("gestión");

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn dominio_expone_estado_y_valida_parametros() {
    let (app, _dir) = app_with_fixtures(ServerRole::Arquivos, None);
    let cookie = cookie_for(&app).await;

    let response = app
        .clone()
        .oneshot(get("/api/domain", &cookie))
        .await
        .expect("estado");
    assert_eq!(response.status(), StatusCode::OK);
    let payload: serde_json::Value =
        serde_json::from_str(&body_text(response).await).expect("json");
    assert!(payload["data"]["joined"].is_boolean());

    let response = app
        .oneshot(post_json(
            "/api/domain/join",
            &cookie,
            r#"{"domain":"team..local","user":"admin","password":"x"}"#,
        ))
        .await
        .expect("join inválido");
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn almacenamiento_responde_en_linux() {
    if cfg!(windows) {
        return;
    }

    let (app, _dir) = app_with_fixtures(ServerRole::Arquivos, None);
    let cookie = cookie_for(&app).await;

    let response = app
        .clone()
        .oneshot(get("/api/storage", &cookie))
        .await
        .expect("storage");
    assert_eq!(response.status(), StatusCode::OK);

    let response = app
        .oneshot(get("/api/storage/disks", &cookie))
        .await
        .expect("discos");
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn terminal_exige_superadministrador_y_valida_geometria() {
    let (app, _dir) = app_with_fixtures(ServerRole::Arquivos, None);
    let cookie = cookie_for(&app).await;

    // Geometría inválida con superadmin: 400 antes de ejecutar nada.
    let response = app
        .clone()
        .oneshot(post_json(
            "/api/terminal/session",
            &cookie,
            r#"{"cols":1,"rows":1}"#,
        ))
        .await
        .expect("sesión");
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    // Operador sin privilegios: 403.
    let dir = temp_dir("terminal-op");
    let db = Db::in_memory().expect("db");
    db.upsert_user("operador", &sha256_hex("Secreta123"), "operator")
        .expect("usuario");
    let auth = Auth::new(db, 3600, false);
    let mut paths = paths_for(&dir);
    paths.data_root = dir.join("data");
    let state =
        AppState::new(auth, paths, ServerRole::Arquivos).with_backup_dir(dir.join("runners"));
    let app = router(state);

    let response = app
        .clone()
        .oneshot(login_request("operador", "Secreta123"))
        .await
        .expect("login");
    let cookie = response
        .headers()
        .get(SET_COOKIE)
        .and_then(|value| value.to_str().ok())
        .expect("cookie")
        .split(';')
        .next()
        .expect("par cookie")
        .to_string();

    let response = app
        .oneshot(post_json(
            "/api/terminal/session",
            &cookie,
            r#"{"cols":200,"rows":50}"#,
        ))
        .await
        .expect("sesión");
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn backup_run_exige_confirmacion_y_tarea_valida() {
    let (app, _dir) = app_with_fixtures(ServerRole::Arquivos, Some(("demo", LOCAL_RUNNER)));
    let cookie = cookie_for(&app).await;

    let response = app
        .clone()
        .oneshot(post_json(
            "/api/backups/run",
            &cookie,
            r#"{"path":"fantasma","confirm":true}"#,
        ))
        .await
        .expect("tarea inexistente");
    assert_eq!(response.status(), StatusCode::NOT_FOUND);

    let response = app
        .oneshot(post_json(
            "/api/backups/run",
            &cookie,
            r#"{"path":"demo","confirm":false}"#,
        ))
        .await
        .expect("sin confirmar");
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}
