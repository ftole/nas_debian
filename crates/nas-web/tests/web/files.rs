use super::*;

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
    let entries = payload["data"]["items"].as_array().expect("arreglo");
    assert!(entries.iter().any(|entry| entry["name"] == "SISTEMAS"));
    assert_eq!(payload["data"]["current_path"], "");

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

    let response = app
        .clone()
        .oneshot(post_json("/api/files/trash/empty", &cookie, ""))
        .await
        .expect("trash empty");
    assert_eq!(response.status(), StatusCode::OK);
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
    assert_eq!(
        response
            .headers()
            .get("accept-ranges")
            .and_then(|v| v.to_str().ok()),
        Some("bytes")
    );
    assert!(response
        .headers()
        .get("content-disposition")
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .contains("informe.txt"));
    assert_eq!(body_text(response).await, "contenido de prueba");

    // Range request parcial (206 Partial Content)
    let range_req = Request::builder()
        .uri("/api/files/download?path=informe.txt")
        .header("cookie", &cookie)
        .header("range", "bytes=0-8")
        .body(Body::empty())
        .expect("range req");
    let response = app
        .clone()
        .oneshot(range_req)
        .await
        .expect("range download");
    assert_eq!(response.status(), StatusCode::PARTIAL_CONTENT);
    assert_eq!(
        response
            .headers()
            .get("content-range")
            .and_then(|v| v.to_str().ok()),
        Some("bytes 0-8/19")
    );
    assert_eq!(
        response
            .headers()
            .get("accept-ranges")
            .and_then(|v| v.to_str().ok()),
        Some("bytes")
    );
    assert_eq!(body_text(response).await, "contenido");

    // Range request sufijo (últimos 6 bytes)
    let suffix_req = Request::builder()
        .uri("/api/files/download?path=informe.txt")
        .header("cookie", &cookie)
        .header("range", "bytes=-6")
        .body(Body::empty())
        .expect("suffix req");
    let response = app
        .clone()
        .oneshot(suffix_req)
        .await
        .expect("suffix download");
    assert_eq!(response.status(), StatusCode::PARTIAL_CONTENT);
    assert_eq!(
        response
            .headers()
            .get("content-range")
            .and_then(|v| v.to_str().ok()),
        Some("bytes 13-18/19")
    );
    assert_eq!(body_text(response).await, "prueba");

    // Range insatisfactorio (416 Range Not Satisfiable)
    let bad_range = Request::builder()
        .uri("/api/files/download?path=informe.txt")
        .header("cookie", &cookie)
        .header("range", "bytes=100-")
        .body(Body::empty())
        .expect("bad range req");
    let response = app.clone().oneshot(bad_range).await.expect("bad range");
    assert_eq!(response.status(), StatusCode::RANGE_NOT_SATISFIABLE);
    assert_eq!(
        response
            .headers()
            .get("content-range")
            .and_then(|v| v.to_str().ok()),
        Some("bytes */19")
    );

    // Range en previsualización /api/files/raw
    let raw_range = Request::builder()
        .uri("/api/files/raw?path=informe.txt")
        .header("cookie", &cookie)
        .header("range", "bytes=0-3")
        .body(Body::empty())
        .expect("raw range req");
    let response = app.clone().oneshot(raw_range).await.expect("raw range");
    assert_eq!(response.status(), StatusCode::PARTIAL_CONTENT);
    assert_eq!(
        response
            .headers()
            .get("content-range")
            .and_then(|v| v.to_str().ok()),
        Some("bytes 0-3/19")
    );
    assert_eq!(body_text(response).await, "cont");

    // Múltiples rangos deben ignorarse y responder 200 OK completo según RFC 9110
    let multi_range = Request::builder()
        .uri("/api/files/download?path=informe.txt")
        .header("cookie", &cookie)
        .header("range", "bytes=0-3, 5-8")
        .body(Body::empty())
        .expect("multi range req");
    let response = app.clone().oneshot(multi_range).await.expect("multi range");
    assert_eq!(response.status(), StatusCode::OK);
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
async fn files_rechaza_duplicados_en_mismo_lote() {
    let app = app(ServerRole::Arquivos);
    let cookie = cookie_for(&app).await;
    let boundary = "XBOUNDARYMULTI";

    let crlf = "\r\n";
    let mut body = String::new();
    body.push_str(&format!("--{boundary}{crlf}"));
    body.push_str(&format!(
        "Content-Disposition: form-data; name=\"path\"{crlf}{crlf}"
    ));
    body.push_str("");
    body.push_str(&format!("{crlf}--{boundary}{crlf}"));
    body.push_str(&format!(
        "Content-Disposition: form-data; name=\"file\"; filename=\"duplicado.txt\"{crlf}"
    ));
    body.push_str(&format!("Content-Type: text/plain{crlf}{crlf}"));
    body.push_str("primero");
    body.push_str(&format!("{crlf}--{boundary}{crlf}"));
    body.push_str(&format!(
        "Content-Disposition: form-data; name=\"file\"; filename=\"duplicado.txt\"{crlf}"
    ));
    body.push_str(&format!("Content-Type: text/plain{crlf}{crlf}"));
    body.push_str("segundo");
    body.push_str(&format!("{crlf}--{boundary}--{crlf}"));

    let request = Request::builder()
        .method("POST")
        .uri("/api/files/upload")
        .header("cookie", &cookie)
        .header(
            "content-type",
            format!("multipart/form-data; boundary={boundary}"),
        )
        .body(Body::from(body))
        .expect("petición");

    let response = app.oneshot(request).await.expect("upload");
    assert_eq!(response.status(), StatusCode::CONFLICT);
    let payload: serde_json::Value =
        serde_json::from_str(&body_text(response).await).expect("json");
    assert!(payload["message"]
        .as_str()
        .unwrap_or_default()
        .contains("está duplicado"));
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

#[tokio::test]
async fn files_upload_rechaza_content_length_mayor_al_espacio_y_satura_cola() {
    let dir = temp_dir("upload-guards");
    let db = Db::in_memory().expect("db");
    db.upsert_user("admin", &sha256_hex("Secreta123"), "superadmin")
        .expect("usuario");
    let auth = Auth::new(db, 3600, false);
    let mut budget = nas_core::ResourceBudget::from_resources(
        nas_core::SystemResources::from_snapshot(2, None, 2048, 1536, None, 0.1, 0.0, 0.0, 0.0),
    );
    budget.max_concurrent_upload_ops = 1;
    let state = AppState::new(auth, paths_for(&dir), ServerRole::Arquivos).with_budget(budget);
    let held_queue = state
        .upload_queue
        .acquire_many(state.upload_queue.available_permits() as u32)
        .await
        .expect("permits");
    let busy_app = router(state.clone());
    let cookie = cookie_for(&busy_app).await;

    let boundary = "XBOUNDARYBUSY";
    let req_busy = Request::builder()
        .method("POST")
        .uri("/api/files/upload")
        .header("cookie", &cookie)
        .header(
            "content-type",
            format!("multipart/form-data; boundary={boundary}"),
        )
        .body(upload_body(boundary, "", "cola.txt", "hola"))
        .expect("petición");
    let res_busy = busy_app
        .clone()
        .oneshot(req_busy)
        .await
        .expect("upload busy");
    assert_eq!(res_busy.status(), StatusCode::SERVICE_UNAVAILABLE);
    drop(held_queue);

    // Content-Length absurdamente grande (10 PiB) debe fallar con 507 antes de escribir bytes.
    let huge_req = Request::builder()
        .method("POST")
        .uri("/api/files/upload")
        .header("cookie", &cookie)
        .header(
            "content-type",
            format!("multipart/form-data; boundary={boundary}"),
        )
        .header("content-length", "11258999068426240")
        .body(upload_body(boundary, "", "grande.txt", "hola"))
        .expect("petición");
    let res_huge = busy_app.oneshot(huge_req).await.expect("upload huge");
    assert_eq!(res_huge.status(), StatusCode::INSUFFICIENT_STORAGE);

    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn files_upload_usa_staging_de_unidad_cuando_root_viene_en_multipart() {
    let dir = temp_dir("upload-unit-staging");
    let paths = paths_for(&dir);
    let unit_dir = paths.data_root.join("unidades").join("disco2");
    std::fs::create_dir_all(&unit_dir).expect("unit dir");
    let db = Db::in_memory().expect("db");
    db.upsert_user("admin", &sha256_hex("Secreta123"), "superadmin")
        .expect("usuario");
    let auth = Auth::new(db, 3600, false);
    let state = AppState::new(auth, paths.clone(), ServerRole::Arquivos);
    let app = router(state);
    let cookie = cookie_for(&app).await;

    let boundary = "XUNITBOUNDARY";
    let body = format!(
        "--{boundary}\r\n\
         Content-Disposition: form-data; name=\"root\"\r\n\r\n\
         disco2\r\n\
         --{boundary}\r\n\
         Content-Disposition: form-data; name=\"path\"\r\n\r\n\
         \r\n\
         --{boundary}\r\n\
         Content-Disposition: form-data; name=\"file\"; filename=\"dato.txt\"\r\n\
         Content-Type: text/plain\r\n\r\n\
         contenido en disco2\r\n\
         --{boundary}--\r\n"
    );
    let req = Request::builder()
        .method("POST")
        .uri("/api/files/upload")
        .header("cookie", &cookie)
        .header(
            "content-type",
            format!("multipart/form-data; boundary={boundary}"),
        )
        .body(axum::body::Body::from(body))
        .expect("petición");
    let res = app.oneshot(req).await.expect("upload unit");
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(
        std::fs::read_to_string(unit_dir.join("dato.txt")).expect("archivo destino"),
        "contenido en disco2"
    );
    assert!(
        unit_dir.join(".nas_staging").is_dir(),
        "debe crear .nas_staging en la unidad destino"
    );
    assert!(
        !paths.data_root.join(".nas_staging").exists(),
        "no debe crear .nas_staging en data_root cuando root es disco2"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn files_acepta_prefijo_unidades_slash_id() {
    let dir = temp_dir("files-unidades-prefix");
    let paths = paths_for(&dir);
    let unit_dir = paths.data_root.join("unidades").join("pool2");
    std::fs::create_dir_all(&unit_dir).expect("unit dir");
    let db = Db::in_memory().expect("db");
    db.upsert_user("admin", &sha256_hex("Secreta123"), "superadmin")
        .expect("usuario");
    let auth = Auth::new(db, 3600, false);
    let state = AppState::new(auth, paths, ServerRole::Arquivos);
    let app = router(state);
    let cookie = cookie_for(&app).await;

    let res_mkdir = app
        .clone()
        .oneshot(post_json(
            "/api/files/mkdir",
            &cookie,
            r#"{"root":"unidades/pool2","path":"docs"}"#,
        ))
        .await
        .expect("mkdir unidad");
    assert_eq!(res_mkdir.status(), StatusCode::OK);
    assert!(unit_dir.join("docs").is_dir());

    let res_list = app
        .clone()
        .oneshot(get("/api/files/list?root=unidades%2Fpool2&path=", &cookie))
        .await
        .expect("list unidad");
    assert_eq!(res_list.status(), StatusCode::OK);
    let payload: serde_json::Value =
        serde_json::from_str(&body_text(res_list).await).expect("json");
    let items = payload["data"]["items"].as_array().expect("items");
    assert!(items.iter().any(|i| i["name"] == "docs"));

    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn files_upload_bloquea_rutas_protegidas_y_permite_carpetas_legitimas() {
    let dir = temp_dir("upload-protected-paths");
    let paths = paths_for(&dir);
    for protected in [
        ".trash",
        ".trash/.meta",
        ".nas_staging",
        "LOGS_BACKUP",
        "BACKUPS_HISTORICOS",
        "BACKUPS_PERMANENTES",
        "unidades/u1/BACKUPS_HISTORICOS",
    ] {
        std::fs::create_dir_all(paths.data_root.join(protected)).expect("mkdir protected");
    }
    std::fs::create_dir_all(paths.data_root.join("SISTEMAS")).expect("mkdir SISTEMAS");
    let db = Db::in_memory().expect("db");
    db.upsert_user("operador", &sha256_hex("Secreta123"), "operator")
        .expect("operador");
    let auth = Auth::new(db, 3600, false);
    let state = AppState::new(auth, paths.clone(), ServerRole::ArquivosBackup);
    let app = router(state);

    let login = app
        .clone()
        .oneshot(login_request("operador", "Secreta123"))
        .await
        .expect("login");
    let cookie = login
        .headers()
        .get(SET_COOKIE)
        .and_then(|v| v.to_str().ok())
        .expect("cookie")
        .split(';')
        .next()
        .expect("pair")
        .to_string();

    for blocked in [
        ".trash",
        ".trash/.meta",
        "./.trash",
        ".nas_staging",
        "LOGS_BACKUP",
        "BACKUPS_HISTORICOS",
        "BACKUPS_PERMANENTES",
        "unidades/u1/BACKUPS_HISTORICOS",
    ] {
        let boundary = "PROTBOUNDARY";
        let req = Request::builder()
            .method("POST")
            .uri("/api/files/upload")
            .header("cookie", &cookie)
            .header(
                "content-type",
                format!("multipart/form-data; boundary={boundary}"),
            )
            .body(upload_body(boundary, blocked, "evil.txt", "payload"))
            .expect("req");
        let resp = app.clone().oneshot(req).await.expect("upload protected");
        assert_eq!(
            resp.status(),
            StatusCode::FORBIDDEN,
            "subida a '{blocked}' debe bloquearse con 403"
        );
        assert!(
            !paths.data_root.join(blocked).join("evil.txt").exists(),
            "no debe escribir archivo en ruta protegida '{blocked}'"
        );
    }

    // Subida con root=backups también debe rechazarse
    let boundary = "PROTROOT";
    let body_backups = format!(
        "--{boundary}\r\n\
         Content-Disposition: form-data; name=\"root\"\r\n\r\n\
         backups\r\n\
         --{boundary}\r\n\
         Content-Disposition: form-data; name=\"file\"; filename=\"evil.txt\"\r\n\
         Content-Type: text/plain\r\n\r\n\
         payload\r\n\
         --{boundary}--\r\n"
    );
    let req_bkp = Request::builder()
        .method("POST")
        .uri("/api/files/upload")
        .header("cookie", &cookie)
        .header(
            "content-type",
            format!("multipart/form-data; boundary={boundary}"),
        )
        .body(axum::body::Body::from(body_backups))
        .expect("req bkp");
    let resp_bkp = app.clone().oneshot(req_bkp).await.expect("upload bkp");
    assert_eq!(resp_bkp.status(), StatusCode::FORBIDDEN);

    // Subir un archivo llamado literalmente BACKUPS_HISTORICOS o .trash a la raíz también debe dar 403
    for reserved_name in ["BACKUPS_HISTORICOS", ".trash", ".nas_staging"] {
        let boundary = "RESNAMEBOUNDARY";
        let req_res = Request::builder()
            .method("POST")
            .uri("/api/files/upload")
            .header("cookie", &cookie)
            .header(
                "content-type",
                format!("multipart/form-data; boundary={boundary}"),
            )
            .body(upload_body(boundary, "", reserved_name, "evil"))
            .expect("req reserved");
        let resp_res = app.clone().oneshot(req_res).await.expect("upload reserved");
        assert_eq!(
            resp_res.status(),
            StatusCode::FORBIDDEN,
            "archivo llamado '{reserved_name}' debe rechazarse con 403"
        );
    }

    // Flujo legítimo en carpeta compartida sigue funcionando
    let ok_boundary = "OKBOUNDARY";
    let ok_req = Request::builder()
        .method("POST")
        .uri("/api/files/upload")
        .header("cookie", &cookie)
        .header(
            "content-type",
            format!("multipart/form-data; boundary={ok_boundary}"),
        )
        .body(upload_body(ok_boundary, "SISTEMAS", "readme.txt", "ok"))
        .expect("req ok");
    let ok_resp = app.oneshot(ok_req).await.expect("upload ok");
    assert_eq!(ok_resp.status(), StatusCode::OK);
    assert_eq!(
        std::fs::read_to_string(paths.data_root.join("SISTEMAS/readme.txt")).expect("read"),
        "ok"
    );

    let _ = std::fs::remove_dir_all(&dir);
}
