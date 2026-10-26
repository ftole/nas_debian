//! Pruebas de paridad del panel portado (fase F5).
//!
//! Verifican que el HTML servido por `nas-web` conserva el contrato extraído del
//! panel PHP original (`tests/parity/contract.json`): identificadores, clases,
//! atributos `data-*`, globales de `window`, formularios y endpoints.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use axum::body::Body;
use axum::http::{header::SET_COOKIE, Request, StatusCode};
use axum::Router;
use http_body_util::BodyExt;
use nas_core::{NasPaths, ServerRole};
use nas_web::{router, AppState, Auth, Db};
use serde_json::Value;
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
";

/// IDs que `app.js` crea en tiempo de ejecución o resuelve por alias de modal.
const DYNAMIC_IDS: [&str; 3] = [
    "users-no-results",
    "modal-format-disk",
    "notifications-badge",
];

fn parity_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("tests")
        .join("parity")
}

fn contract() -> Value {
    let text = std::fs::read_to_string(parity_dir().join("contract.json")).expect("contrato");
    serde_json::from_str(&text).expect("json")
}

fn app() -> Router {
    let dir = std::env::temp_dir().join(format!(
        "nas-parity-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    std::fs::create_dir_all(&dir).expect("dir");
    let mut paths = NasPaths::development();
    paths.state_dir = dir.clone();
    paths.samba_conf = dir.join("smb.conf");
    paths.role_file = dir.join("role");
    paths.data_root = dir.join("data");
    std::fs::write(&paths.samba_conf, SMB_CONF).expect("smb.conf");
    std::fs::write(&paths.role_file, "ARCHIVOS_BACKUP\n").expect("role");

    let db = Db::in_memory().expect("db");
    db.upsert_user("admin", &hex("Secreta123"), "superadmin")
        .expect("usuario");
    router(AppState::new(
        Auth::new(db, 3600, false),
        paths,
        ServerRole::ArquivosBackup,
    ))
}

fn hex(value: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(value.as_bytes());
    format!("{:x}", hasher.finalize())
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

async fn cookie(app: &Router) -> String {
    let request = Request::builder()
        .method("POST")
        .uri("/login")
        .header("content-type", "application/x-www-form-urlencoded")
        .body(Body::from("username=admin&password=Secreta123"))
        .expect("petición");
    let response = app.clone().oneshot(request).await.expect("login");
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    response
        .headers()
        .get(SET_COOKIE)
        .and_then(|value| value.to_str().ok())
        .expect("cookie")
        .split(';')
        .next()
        .expect("par")
        .to_string()
}

async fn get(app: &Router, uri: &str, cookie: &str) -> axum::response::Response {
    let request = Request::builder()
        .uri(uri)
        .header("cookie", cookie)
        .body(Body::empty())
        .expect("petición");
    app.clone().oneshot(request).await.expect("respuesta")
}

fn strings(value: &Value, key: &str) -> Vec<String> {
    value
        .get(key)
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

fn class_tokens(html: &str) -> BTreeSet<String> {
    let mut tokens = BTreeSet::new();
    for chunk in html.split("class=\"").skip(1) {
        let value = chunk.split('"').next().unwrap_or_default();
        for token in value.split_whitespace() {
            tokens.insert(token.to_string());
        }
    }
    tokens
}

fn assert_ids(html: &str, section: &Value, label: &str) {
    let mut failures = Vec::new();
    for id in strings(section, "ids") {
        if DYNAMIC_IDS.contains(&id.as_str()) {
            continue;
        }
        if !html.contains(&format!("id=\"{id}\"")) {
            failures.push(id);
        }
    }
    assert!(failures.is_empty(), "[{label}] faltan IDs: {failures:?}");
}

#[tokio::test]
async fn dashboard_preserva_ids_clases_y_globales() {
    let contract = contract();
    let layout = &contract["layout"];
    let app = app();
    let session = cookie(&app).await;
    let response = get(&app, "/dashboard", &session).await;
    assert_eq!(response.status(), StatusCode::OK);
    let html = body_text(response).await;

    assert_ids(&html, layout, "layout");

    let tokens = class_tokens(&html);
    let expected = strings(layout, "classes");
    let missing: Vec<_> = expected
        .iter()
        .filter(|class| !tokens.contains(class.as_str()))
        .cloned()
        .collect();
    assert!(missing.is_empty(), "[layout] faltan clases: {missing:?}");

    for attribute in strings(layout, "data_attributes") {
        assert!(
            html.contains(&format!("data-{attribute}")),
            "[layout] falta data-{attribute}"
        );
    }
    for global in strings(layout, "window_globals") {
        assert!(html.contains(&global), "[layout] falta la global {global}");
    }
    assert!(html.contains("name=\"csrf_token\"") || html.contains("csrf"));

    let views: Vec<String> = [
        "dashboard",
        "files",
        "shares",
        "users",
        "backups",
        "storage",
        "terminal",
        "logs",
        "domain",
        "services",
        "networking",
        "applications",
        "updates",
        "diagnostics",
    ]
    .iter()
    .map(|view| format!("view-{view}"))
    .collect();
    for view in views {
        assert!(html.contains(&format!("id=\"{view}\"")), "falta {view}");
    }
}

#[tokio::test]
async fn login_preserva_ids_y_formulario() {
    let contract = contract();
    let app = app();
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/login")
                .body(Body::empty())
                .expect("petición"),
        )
        .await
        .expect("respuesta");
    assert_eq!(response.status(), StatusCode::OK);
    let html = body_text(response).await;

    assert_ids(&html, &contract["login"], "login");
    assert!(html.contains("csrf_token"));
    assert!(contract["login"]["has_csrf_field"]
        .as_bool()
        .unwrap_or(true));
}

#[tokio::test]
async fn todos_los_identificadores_de_app_js_existen() {
    let contract = contract();
    let app = app();
    let session = cookie(&app).await;
    let html = body_text(get(&app, "/dashboard", &session).await).await;

    let mut failures = Vec::new();
    for id in strings(&contract["app_js"], "ids") {
        if DYNAMIC_IDS.contains(&id.as_str()) {
            continue;
        }
        if !html.contains(&format!("id=\"{id}\"")) {
            failures.push(id);
        }
    }
    assert!(
        failures.is_empty(),
        "faltan IDs usados por app.js: {failures:?}"
    );
}

#[tokio::test]
async fn endpoints_del_contrato_existen_y_no_son_501() {
    let list = std::fs::read_to_string(parity_dir().join("endpoints.txt")).expect("endpoints");
    let app = app();
    let session = cookie(&app).await;

    async fn call(app: &Router, method: &str, path: &str, session: &str) -> (StatusCode, String) {
        let builder = Request::builder()
            .method(method)
            .uri(path)
            .header("cookie", session)
            .header("content-type", "application/json");
        let request = if method == "POST" {
            builder.body(Body::from("{}")).expect("petición")
        } else {
            builder.body(Body::empty()).expect("petición")
        };
        let response = app.clone().oneshot(request).await.expect("respuesta");
        let status = response.status();
        (status, body_text(response).await)
    }

    let mut failures = Vec::new();
    for path in list.lines().map(str::trim).filter(|line| !line.is_empty()) {
        let path = &path.replace(":id", "zzqa_probe");
        let (mut status, mut body) = call(&app, "GET", path, &session).await;
        if status == StatusCode::METHOD_NOT_ALLOWED || status == StatusCode::NOT_FOUND {
            let (post_status, post_body) = call(&app, "POST", path, &session).await;
            if post_status != StatusCode::NOT_FOUND && post_status != StatusCode::METHOD_NOT_ALLOWED
            {
                status = post_status;
                body = post_body;
            }
        }
        // Un 404 con cuerpo JSON proviene del manejador (p. ej. `lsblk` ausente en
        // el host de pruebas), no de una ruta inexistente.
        let from_handler = body.contains("\"success\"");
        if status == StatusCode::NOT_IMPLEMENTED || status == StatusCode::METHOD_NOT_ALLOWED {
            failures.push(format!("{path} → {status}"));
        } else if status == StatusCode::NOT_FOUND && !from_handler {
            failures.push(format!("{path} → ruta inexistente"));
        }
    }

    assert!(
        failures.is_empty(),
        "endpoints del contrato con problemas: {failures:#?}"
    );
}
