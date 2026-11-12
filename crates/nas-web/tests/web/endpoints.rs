use super::*;

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
    assert!(services.len() >= 4);
    assert!(services.iter().any(|item| item["service"] == "smbd"));
    assert!(services.iter().all(|item| item["name"].is_string()));

    let response = app
        .oneshot(get("/api/diagnostics", &cookie))
        .await
        .expect("diagnóstico");
    assert_eq!(response.status(), StatusCode::OK);
    let payload: serde_json::Value =
        serde_json::from_str(&body_text(response).await).expect("json");
    assert!(payload["data"]["overall_status"].is_string());
    let checks = payload["data"]["checks"].as_array().expect("arreglo");
    assert!(checks.iter().any(|check| check["name"] == "rol"));
}

#[tokio::test]
async fn backups_lista_y_planifica() {
    let (app, _dir) = app_with_fixtures(ServerRole::ArquivosBackup, Some(("demo", LOCAL_RUNNER)));
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
        .expect("id inválido");
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    let response = app
        .clone()
        .oneshot(post_json(
            "/api/backups/run",
            &cookie,
            r#"{"path":">a b<","confirm":true}"#,
        ))
        .await
        .expect("id con espacios");
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

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

#[tokio::test]
async fn vistas_renderizan_datos_reales() {
    let (app, _dir) = app_with_fixtures(ServerRole::ArquivosBackup, Some(("demo", LOCAL_RUNNER)));
    let cookie = cookie_for(&app).await;

    let cases = [
        ("/dashboard", "view-dashboard"),
        ("/shares", "view-shares"),
        ("/users", "view-users"),
        ("/backups", "view-backups"),
        ("/services", "view-services"),
        ("/diagnostics", "view-services"),
        ("/storage", "view-storage"),
        ("/files", "view-files"),
        ("/domain", "view-domain"),
    ];

    for (uri, view_id) in cases {
        let response = app
            .clone()
            .oneshot(get(uri, &cookie))
            .await
            .expect("respuesta");
        assert_eq!(response.status(), StatusCode::OK, "fallo en {uri}");
        let body = body_text(response).await;
        assert!(
            body.contains(&format!("id=\"{view_id}\"")),
            "{uri} no contiene la vista '{view_id}'"
        );
        assert!(body.contains("sidebar"), "{uri} sin layout");
    }
}

#[tokio::test]
async fn zip_descarga_una_carpeta() {
    let (app, dir) = app_with_fixtures(ServerRole::Arquivos, None);
    let cookie = cookie_for(&app).await;

    let root = dir.join("data");
    std::fs::create_dir_all(root.join("SISTEMAS/2026")).expect("dirs");
    std::fs::write(root.join("SISTEMAS/2026/a.txt"), b"hola").expect("a");

    let response = app
        .clone()
        .oneshot(get("/api/files/zip?path=SISTEMAS", &cookie))
        .await
        .expect("zip");
    assert_eq!(response.status(), StatusCode::OK);
    assert!(response
        .headers()
        .get("content-disposition")
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .contains(".zip"));

    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("cuerpo")
        .to_bytes();
    assert!(bytes.starts_with(b"PK"));

    let response = app
        .oneshot(get("/api/files/zip?path=../etc", &cookie))
        .await
        .expect("escape");
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn vistas_terminal_y_auditoria() {
    let (app, _dir) = app_with_fixtures(ServerRole::Arquivos, None);
    let cookie = cookie_for(&app).await;

    let response = app
        .clone()
        .oneshot(get("/terminal", &cookie))
        .await
        .expect("terminal");
    assert_eq!(response.status(), StatusCode::OK);
    assert!(body_text(response).await.contains("id=\"view-terminal\""));

    let response = app
        .oneshot(get("/audit", &cookie))
        .await
        .expect("auditoría");
    assert_eq!(response.status(), StatusCode::OK);
    assert!(body_text(response).await.contains("id=\"view-logs\""));
}

#[tokio::test]
async fn contrato_de_paridad_completo() {
    let (app, _dir) = app_with_fixtures(ServerRole::ArquivosBackup, None);
    let cookie = cookie_for(&app).await;

    let response = app
        .oneshot(get("/dashboard", &cookie))
        .await
        .expect("panel");
    assert_eq!(response.status(), StatusCode::OK);
    let body = body_text(response).await;

    let contract: serde_json::Value =
        serde_json::from_str(include_str!("../../../../tests/parity/contract.json"))
            .expect("contrato");
    let ids = contract["layout"]["ids"].as_array().expect("ids");
    assert!(ids.len() > 500, "contrato incompleto");

    let missing: Vec<&str> = ids
        .iter()
        .filter_map(|value| value.as_str())
        .filter(|id| !body.contains(&format!("id=\"{id}\"")))
        .collect();

    assert!(
        missing.is_empty(),
        "faltan {} IDs del contrato: {:?}",
        missing.len(),
        &missing[..missing.len().min(15)]
    );
}

#[tokio::test]
async fn modulos_js_compuestos_se_sirven_completos() {
    let (app, _dir) = app_with_fixtures(ServerRole::ArquivosBackup, None);
    let cookie = cookie_for(&app).await;

    let cases: [(&str, usize, &[&str]); 5] = [
        (
            "/js/modules/storage.js",
            41365,
            &[
                "renderStorageOverview",
                "onStorageDeviceChange",
                "loadStorageUnits",
            ],
        ),
        (
            "/js/modules/users.js",
            51912,
            &[
                "renderUsersTableBody",
                "loadUsersAndGroups",
                "loadAccessMatrix",
            ],
        ),
        (
            "/js/modules/backups.js",
            30901,
            &[
                "renderBackupsTable",
                "runBackupTask",
                "loadPermanentBackups",
            ],
        ),
        (
            "/js/modules/system.js",
            23339,
            &["loadUpdates", "loadApplications", "confirmRebootServer"],
        ),
        (
            "/js/modules/files.js",
            88786,
            &[
                "getFileTypeMeta",
                "loadFiles",
                "uploadFiles",
                "loadTrash",
                "previewFile",
                "saveCurrentPreviewFile",
            ],
        ),
    ];

    for (uri, expected_len, symbols) in cases {
        let response = app
            .clone()
            .oneshot(get(uri, &cookie))
            .await
            .expect("respuesta js");
        assert_eq!(response.status(), StatusCode::OK, "fallo en {uri}");
        let body = body_text(response).await;
        assert_eq!(body.len(), expected_len, "tamaño inesperado en {uri}");
        for symbol in symbols {
            assert!(body.contains(symbol), "{uri} no contiene {symbol}");
        }
    }
}
