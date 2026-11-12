use super::nas;

#[test]
fn helper_samba_apply_rechaza_rutas_invalidas() {
    let output = nas()
        .args(["--json", "helper", "samba-apply", "/etc/passwd"])
        .output()
        .expect("ejecuta");
    assert!(!output.status.success());
    let payload: serde_json::Value = serde_json::from_slice(&output.stderr).expect("json");
    assert_eq!(payload["success"], false);
    assert!(payload["error"]
        .as_str()
        .unwrap()
        .contains("ruta no permitida"));
}

#[test]
fn helper_share_dir_rechaza_nombres_invalidos() {
    let output = nas()
        .args(["--json", "helper", "share-dir", "--name", "../escape"])
        .output()
        .expect("ejecuta");
    assert!(!output.status.success());
    let payload: serde_json::Value = serde_json::from_slice(&output.stderr).expect("json");
    assert_eq!(payload["success"], false);
    assert!(payload["error"].as_str().unwrap().contains("inválido"));
}

#[test]
fn helper_share_acl_rechaza_rutas_invalidas() {
    let output = nas()
        .args([
            "--json",
            "helper",
            "share-acl",
            "--path",
            "/etc/samba",
            "--principal",
            "grp_samba",
            "--level",
            "read",
        ])
        .output()
        .expect("ejecuta");
    assert!(!output.status.success());
    let payload: serde_json::Value = serde_json::from_slice(&output.stderr).expect("json");
    assert_eq!(payload["success"], false);
    assert!(payload["error"]
        .as_str()
        .unwrap()
        .contains("ruta no permitida"));
}

#[test]
fn helper_accounts_rechaza_usuarios_reservados() {
    let output = nas()
        .args([
            "--json",
            "helper",
            "accounts",
            "create-user",
            "root",
            "Super",
            "123456",
        ])
        .output()
        .expect("ejecuta");
    assert!(!output.status.success());
    let payload: serde_json::Value = serde_json::from_slice(&output.stderr).expect("json");
    assert_eq!(payload["success"], false);
    assert!(payload["error"]
        .as_str()
        .unwrap()
        .contains("inválido o reservado"));
}

#[test]
fn helper_accounts_create_group_valida_prefijo_grp() {
    let output = nas()
        .args(["--json", "helper", "accounts", "create-group", "sinprefijo"])
        .output()
        .expect("ejecuta");
    assert!(!output.status.success());
    let payload: serde_json::Value = serde_json::from_slice(&output.stderr).expect("json");
    assert_eq!(payload["success"], false);
    assert!(payload["error"]
        .as_str()
        .unwrap()
        .contains("grupo inválido"));
}

#[test]
fn helper_backup_manage_rechaza_id_invalido() {
    let output = nas()
        .args(["--json", "helper", "backup-manage", "install", "../escape"])
        .output()
        .expect("ejecuta");
    assert!(!output.status.success());
    let payload: serde_json::Value = serde_json::from_slice(&output.stderr).expect("json");
    assert_eq!(payload["success"], false);
    assert!(payload["error"].as_str().unwrap().contains("inválido"));
}

#[test]
fn helper_storage_apply_exige_confirmacion() {
    let output = nas()
        .args([
            "--json",
            "helper",
            "storage-apply",
            "format",
            "no-confirmo",
            "/dev/sdb",
        ])
        .output()
        .expect("ejecuta");
    assert!(!output.status.success());
    let payload: serde_json::Value = serde_json::from_slice(&output.stderr).expect("json");
    assert_eq!(payload["success"], false);
    assert!(payload["error"]
        .as_str()
        .unwrap()
        .contains("confirmación requerida"));
}

#[test]
fn helper_terminal_rechaza_geometria_invalida() {
    let output = nas()
        .args([
            "--json", "helper", "terminal", "-u", "admin", "start", "5", "5",
        ])
        .output()
        .expect("ejecuta");
    assert!(!output.status.success());
}

#[test]
fn helper_terminal_raw_enter_acepta_alias_y_subcomando() {
    let output = nas()
        .args([
            "--json",
            "helper",
            "terminal",
            "-u",
            "root",
            "raw_enter",
            "echo test",
        ])
        .output()
        .expect("ejecuta");
    assert_ne!(output.status.code(), Some(2));
    assert!(!output.status.success());
    let payload: serde_json::Value = serde_json::from_slice(&output.stderr).expect("json");
    assert!(payload["error"]
        .as_str()
        .unwrap()
        .contains("inválido o reservado"));
}

#[test]
fn helper_storage_apply_aliases_reconocidos() {
    let output = nas()
        .args([
            "--json",
            "helper",
            "storage-apply",
            "wipe_disk",
            "no-confirmo",
            "/dev/sdb",
        ])
        .output()
        .expect("ejecuta");
    assert_ne!(output.status.code(), Some(2));
    assert!(!output.status.success());
    let payload: serde_json::Value = serde_json::from_slice(&output.stderr).expect("json");
    assert!(payload["error"]
        .as_str()
        .unwrap()
        .contains("confirmación textual requerida"));
}

#[test]
fn helper_backup_manage_aliases_reconocidos() {
    let output = nas()
        .args([
            "--json",
            "helper",
            "backup-manage",
            "test_cifs",
            "10.10.1.50",
            "share",
            "user",
        ])
        .output()
        .expect("ejecuta");
    assert_ne!(output.status.code(), Some(2));
}

#[test]
fn helper_accounts_aliases_reconocidos() {
    let output = nas()
        .args([
            "--json",
            "helper",
            "accounts",
            "create_user",
            "root",
            "Root User",
            "123456",
        ])
        .output()
        .expect("ejecuta");
    assert!(!output.status.success());
    let payload: serde_json::Value = serde_json::from_slice(&output.stderr).expect("json");
    assert_eq!(payload["success"], false);
    assert!(payload["error"]
        .as_str()
        .unwrap()
        .contains("inválido o reservado"));
}

#[test]
fn helper_purge_recycle_valida_o_ejecuta() {
    let output = nas()
        .args(["--json", "helper", "purge-recycle", "--days", "15"])
        .output()
        .expect("ejecuta");
    let text = if !output.stderr.is_empty() {
        String::from_utf8_lossy(&output.stderr).to_string()
    } else {
        String::from_utf8_lossy(&output.stdout).to_string()
    };
    let payload: serde_json::Value = serde_json::from_str(&text).expect("debe ser salida JSON");
    assert!(payload.get("success").is_some());
}

#[test]
fn helper_backup_manage_freeze_rechaza_id_invalido() {
    let output = nas()
        .args(["--json", "helper", "backup-manage", "freeze", "id invalido"])
        .output()
        .expect("ejecuta");
    assert!(!output.status.success());
}

#[test]
fn deploy_rechaza_contrasena_vacia_y_roles_invalidos() {
    let bad_role = nas()
        .args([
            "--json",
            "deploy",
            "plan",
            "--disk",
            "/dev/sdb",
            "--workgroup",
            "TEAM",
            "--netbios",
            "SRV",
            "--admin",
            "admin",
            "--role",
            "INVENTADO",
        ])
        .output()
        .expect("ejecuta");
    assert!(!bad_role.status.success());

    let bad_fs = nas()
        .args([
            "deploy",
            "plan",
            "--disk",
            "/dev/sdb",
            "--workgroup",
            "TEAM",
            "--netbios",
            "SRV",
            "--admin",
            "admin",
            "--ext4",
            "--btrfs",
        ])
        .output()
        .expect("ejecuta");
    assert!(!bad_fs.status.success());
}

#[test]
fn uninstall_rechaza_alcance_desconocido_y_deploy_rechaza_disco_so() {
    let bad_scope = nas()
        .args(["uninstall", "--scope", "raiz"])
        .output()
        .expect("ejecuta");
    assert!(!bad_scope.status.success());

    let os_disk = nas()
        .args([
            "deploy",
            "apply",
            "--disk",
            "/dev/nvme0n1",
            "--workgroup",
            "TEAM",
            "--netbios",
            "SRV",
            "--admin",
            "admin",
            "--yes",
            "--format-token",
            "SI-FORMATEAR",
        ])
        .output()
        .expect("ejecuta");
    // Debe abortar sin ejecutar nada por disco del SO o validación, nunca éxito.
    assert!(!os_disk.status.success());
}

#[test]
fn helper_backup_congela_y_restaura_valida_id() {
    // Validaciones de entrada (sin ejecutar nada): IDs inválidos deben abortar
    // antes de tocar el runner o la base de datos.
    let run_bad = nas()
        .args(["--json", "helper"])
        .args(["backup-manage"])
        .args(["run"])
        .args(["../escape"])
        .output()
        .expect("ejecuta");
    assert!(!run_bad.status.success());

    let freeze_bad = nas()
        .args(["--json", "helper", "backup-manage", "freeze", "/etc", "x"])
        .output()
        .expect("ejecuta");
    assert!(!freeze_bad.status.success());
}

#[test]
fn update_verify_artefacto_relativo_resuelve_contra_el_padre_del_manifiesto() {
    use std::fs;
    use std::process::Command;

    // Base de directorios temporales, cada uno en un cwd distinto.
    let dir = std::env::temp_dir().join(format!(
        "nas-verify-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    fs::create_dir_all(&dir).expect("dir");

    // El artefacto y el manifiesto viven en el mismo subdirectorio; el CLI se
    // ejecuta desde otro working directory (p.ej. el padre).
    let artifact_path = dir.join("nas-probe.tar.gz");
    fs::write(&artifact_path, b"payload").expect("artifact");
    let manifest_path = dir.join("nas-probe.json");
    fs::write(&manifest_path, r#"{"version":"0.1.probe","commit":"x","artifact":"nas-probe.tar.gz","sha256":"AA","published_at":"2026-01-01T00:00:00Z","signature":null}"#).expect("manifest");

    // El directorio de trabajo del CLI será el PADRE del subdirectorio (~/).
    let cwd = dir.parent().unwrap().to_path_buf();
    let output = Command::new(env!("CARGO_BIN_EXE_nas"))
        .arg("--json")
        .arg("update")
        .arg("verify")
        .arg(&manifest_path)
        .arg("--pubkey")
        .arg("220d56af9dad91eb44fb9a6fa175adc96cc9ffb25b405a2db2e867b2a9153335")
        .current_dir(cwd)
        .output()
        .expect("run");
    let stdout = String::from_utf8_lossy(&output.stdout);
    // El error esperado ya no es "No such file or directory" por ruta relativa;
    // es SHA-256 mismatch (esperado porque no pasamos una firma válida), pero
    // importantly: no debe ser "No such file or directory".
    assert!(
        !stdout.contains("No such file or directory"),
        "el CLI debe resolver el nombre del artefacto desde el padre del manifiesto; obtuvo: {stdout}"
    );
    let _ = fs::remove_dir_all(dir);
}
