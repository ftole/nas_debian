use super::*;

#[tokio::test]
async fn shares_valida_nombres_y_sesion() {
    let (app, _dir) = app_with_fixtures(ServerRole::ArquivosBackup, None);

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/shares")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"name":"mal nombre"}"#))
                .expect("petición"),
        )
        .await
        .expect("respuesta");
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

    let cookie = cookie_for(&app).await;
    let response = app
        .oneshot(post_json(
            "/api/shares",
            &cookie,
            r#"{"name":"mal nombre","scheme":1}"#,
        ))
        .await
        .expect("respuesta");
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn usuarios_valida_nombres_y_privilegios() {
    let (app, _dir) = app_with_fixtures(ServerRole::Arquivos, None);
    let cookie = cookie_for(&app).await;

    let response = app
        .clone()
        .oneshot(post_json(
            "/api/users",
            &cookie,
            r#"{"username":"Usuario Malo","password":"123456","groups":[]}"#,
        ))
        .await
        .expect("respuesta");
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    let response = app
        .clone()
        .oneshot(post_json(
            "/api/groups",
            &cookie,
            r#"{"name":"grupo invalido con espacios"}"#,
        ))
        .await
        .expect("respuesta");
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    let response = app
        .clone()
        .oneshot(post_json("/api/groups", &cookie, r#"{"name":"grp_samba"}"#))
        .await
        .expect("respuesta");
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    let response = app
        .clone()
        .oneshot(post_json("/api/groups", &cookie, r#"{"name":""}"#))
        .await
        .expect("respuesta");
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    let response = app
        .clone()
        .oneshot(post_json("/api/groups", &cookie, r#"{"name":"ventas"}"#))
        .await
        .expect("respuesta");
    assert_ne!(response.status(), StatusCode::BAD_REQUEST);

    let response = app
        .oneshot(post_json(
            "/api/groups",
            &cookie,
            r#"{"name":"grp_ventas"}"#,
        ))
        .await
        .expect("respuesta");
    assert_ne!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn backups_valida_tareas() {
    let (app, _dir) = app_with_fixtures(ServerRole::ArquivosBackup, None);
    let cookie = cookie_for(&app).await;

    let response = app
        .clone()
        .oneshot(post_json(
            "/api/backups",
            &cookie,
            r#"{"id":"a b","proto":"local","path":"/srv/nas","cron":"0 2 * * *","retention":3}"#,
        ))
        .await
        .expect("respuesta");
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    let response = app
        .oneshot(post_json(
            "/api/backups",
            &cookie,
            r#"{"id":"ok","proto":"ftp","path":"/srv/nas","cron":"0 2 * * *","retention":3}"#,
        ))
        .await
        .expect("respuesta");
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn almacenamiento_valida_confirmacion_y_dispositivo() {
    let (app, _dir) = app_with_fixtures(ServerRole::ArquivosBackup, None);
    let cookie = cookie_for(&app).await;

    let response = app
        .clone()
        .oneshot(post_json(
            "/api/storage/format",
            &cookie,
            r#"{"device":"/dev/sdz","fstype":"ext4","confirm":""}"#,
        ))
        .await
        .expect("respuesta");
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    let response = app
        .clone()
        .oneshot(post_json(
            "/api/storage/format",
            &cookie,
            r#"{"device":"../etc/passwd","fstype":"ext4","confirm":"SI-FORMATEAR"}"#,
        ))
        .await
        .expect("respuesta");
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    let response = app
        .clone()
        .oneshot(post_json(
            "/api/storage/lvm",
            &cookie,
            r#"{"disk":"/dev/sdz","vg":"vg","lv":"lv","fstype":"xfs","confirm":"SI-FORMATEAR"}"#,
        ))
        .await
        .expect("respuesta");
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    let response = app
        .oneshot(post_json(
            "/api/storage/inspect",
            &cookie,
            r#"{"device":"/dev/../etc/passwd"}"#,
        ))
        .await
        .expect("respuesta");
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn respaldos_rechazan_identificadores_invalidos() {
    let (app, _dir) = app_with_fixtures(ServerRole::ArquivosBackup, None);
    let cookie = cookie_for(&app).await;

    for path in [
        "/api/backups/a%20b/run",
        "/api/backups/..%2Fetc%2Fpasswd/stop",
    ] {
        let response = app
            .clone()
            .oneshot(post_json(path, &cookie, "{}"))
            .await
            .expect("respuesta");
        assert!(
            response.status() == StatusCode::BAD_REQUEST
                || response.status() == StatusCode::NOT_FOUND,
            "{path} devolvió {}",
            response.status()
        );
    }
}

#[tokio::test]
async fn auditoria_de_acl_responde() {
    let (app, _dir) = app_with_fixtures(ServerRole::ArquivosBackup, None);
    let cookie = cookie_for(&app).await;

    let response = app
        .oneshot(get("/api/shares/access/audit", &cookie))
        .await
        .expect("respuesta");
    assert_eq!(response.status(), StatusCode::OK);
    let payload: serde_json::Value =
        serde_json::from_str(&body_text(response).await).expect("json");
    assert!(payload["data"]["total"].is_number());
    assert!(payload["data"]["shares"].is_object());
}

#[tokio::test]
async fn audit_verify_requiere_admin_y_valida_cadena() {
    let (app, dir) = app_with_fixtures(ServerRole::ArquivosBackup, None);

    // 1. Sin sesión devuelve 401
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/system/audit/verify")
                .body(Body::empty())
                .expect("petición"),
        )
        .await
        .expect("respuesta");
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

    // 2. Con sesión de administrador devuelve 200 y valida la bitácora vacía
    let cookie = cookie_for(&app).await;
    let response = app
        .clone()
        .oneshot(get("/api/system/audit/verify", &cookie))
        .await
        .expect("respuesta");
    assert_eq!(response.status(), StatusCode::OK);
    let payload: serde_json::Value =
        serde_json::from_str(&body_text(response).await).expect("json");
    assert_eq!(payload["success"], true);
    assert_eq!(payload["data"]["valid"], true);

    // 3. Escribir entradas en la cadena criptográfica y volver a verificar
    let audit_log = nas_audit::AuditLog::new(dir.join("audit").join("nas-audit.jsonl"));
    let entry1 = nas_audit::AuditEntry::new(
        "admin",
        "local",
        "login",
        "web",
        "SUCCESS",
        "inicio de sesión",
    );
    audit_log.append(entry1).expect("append 1");

    let entry2 = nas_audit::AuditEntry::new(
        "admin",
        "local",
        "storage-inspect",
        "/dev/sdb",
        "SUCCESS",
        "inspección",
    );
    audit_log.append(entry2).expect("append 2");

    let response = app
        .clone()
        .oneshot(get("/api/audit/verify", &cookie))
        .await
        .expect("respuesta");
    assert_eq!(response.status(), StatusCode::OK);
    let payload: serde_json::Value =
        serde_json::from_str(&body_text(response).await).expect("json");
    assert_eq!(payload["success"], true);
    assert_eq!(payload["data"]["valid"], true);
    assert_eq!(payload["data"]["count"], 2);
    assert_eq!(payload["data"]["status"], "VERIFIED");
}
