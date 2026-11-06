//! Pruebas de integración de la CLI `nas`.

use std::process::Command;

fn nas() -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_nas"));
    cmd.env("NAS_ESCAPED", "1");
    cmd
}

fn json_of(output: &std::process::Output) -> serde_json::Value {
    assert!(
        output.status.success(),
        "la CLI falló: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("salida JSON válida")
}

#[test]
fn status_json_respeta_el_contrato() {
    let output = nas().args(["--json", "status"]).output().expect("ejecuta");
    let payload = json_of(&output);

    assert_eq!(payload["success"], true);
    assert_eq!(payload["message"], "Métricas del sistema");
    assert!(payload["data"]["hostname"].is_string());
    assert!(payload["data"]["ram_total_gb"].is_number());
    assert!(payload["data"]["server_time"].is_string());
}

#[test]
fn network_json_expone_puertos_e_interfaces() {
    let dir = fixture_dir("network");
    let smb_conf = dir.join("smb.conf");
    std::fs::write(&smb_conf, "[global]\n   server min protocol = SMB2_02\n").expect("smb.conf");

    let output = nas()
        .env("NAS_SAMBA_CONF", &smb_conf)
        .args(["--json", "network"])
        .output()
        .expect("ejecuta");
    let payload = json_of(&output);

    assert_eq!(payload["success"], true);
    assert!(payload["data"]["interfaces"].is_array());
    assert!(payload["data"]["listening"].is_array());
    assert!(payload["data"]["samba"]["min_protocol"].is_string());

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn services_json_lista_los_demonios_clave() {
    let output = nas()
        .args(["--json", "services"])
        .output()
        .expect("ejecuta");
    let payload = json_of(&output);

    let services = payload["data"].as_array().expect("arreglo de servicios");
    assert_eq!(services.len(), 4);
    let names: Vec<&str> = services
        .iter()
        .filter_map(|service| service["service"].as_str())
        .collect();
    assert!(names.contains(&"smbd"));
    assert!(names.contains(&"nginx"));
}

#[test]
fn version_responde_sin_tocar_el_sistema() {
    let output = nas().arg("--version").output().expect("ejecuta");
    assert!(output.status.success());
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(text.contains("nas"));
}

fn fixture_dir(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "nas-cli-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    std::fs::create_dir_all(&dir).expect("fixture dir");
    dir
}

#[test]
fn backup_run_exige_confirmacion_explicita() {
    let dir = fixture_dir("backup-guard");
    std::fs::write(
        dir.join("backup_demo.sh"),
        "SOURCE_TYPE=\"local\"
SRC_PATH=\"/datos\"
RETENTION=3
",
    )
    .expect("runner");

    let output = nas()
        .args(["backup", "run", "demo", "--dir", &dir.to_string_lossy()])
        .output()
        .expect("ejecuta");

    assert_eq!(output.status.code(), Some(3));
    let payload: serde_json::Value = serde_json::from_slice(&output.stderr).expect("json de error");
    assert_eq!(payload["success"], false);
    assert!(payload["error"].as_str().unwrap_or("").contains("--yes"));

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn backup_run_dry_run_no_modifica_nada() {
    let dir = fixture_dir("backup-dry");
    std::fs::write(
        dir.join("backup_demo.sh"),
        "SOURCE_TYPE=\"local\"
SRC_PATH=\"/datos\"
RETENTION=2
",
    )
    .expect("runner");

    let output = nas()
        .args([
            "--json",
            "backup",
            "run",
            "demo",
            "--dir",
            &dir.to_string_lossy(),
            "--dry-run",
        ])
        .output()
        .expect("ejecuta");

    let payload = json_of(&output);
    assert_eq!(payload["success"], true);
    assert_eq!(payload["data"]["dry_run"], true);
    assert!(payload["data"]["steps"].is_array());
    assert_eq!(payload["data"]["removed"], 0);

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn deploy_plan_con_password_genera_plan_valido() {
    let output = nas()
        .args([
            "deploy",
            "plan",
            "--disk",
            "LOCAL",
            "--workgroup",
            "WORKGROUP",
            "--netbios",
            "SRV-NAS",
            "--admin",
            "administrador",
            "--password",
            "ClaveSegura123!",
            "--role",
            "ARCHIVOS",
        ])
        .output()
        .expect("ejecuta");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("SRV-NAS"));
    assert!(stdout.contains("ClaveSegura123!"));
}

#[test]
fn users_list_json_es_estable() {
    let output = nas()
        .args(["--json", "users", "list"])
        .output()
        .expect("ejecuta");
    let payload = json_of(&output);

    assert_eq!(payload["success"], true);
    assert!(payload["data"]["users"].is_array());
    assert!(payload["data"]["groups"].is_array());
}

#[test]
fn update_apply_sin_pubkey_falla_con_error_claro() {
    let output = nas()
        .args(["update", "apply"])
        .env_remove("NAS_RELEASE_PUBKEY")
        .output()
        .expect("ejecuta");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("clave pública Ed25519 obligatoria"));
}

#[test]
fn update_apply_rechaza_manifiesto_sin_firma() {
    let dir = fixture_dir("update-unsigned");
    let manifest = dir.join("manifest.json");
    std::fs::write(
        &manifest,
        r#"{"version":"1.0.0","commit":"abc","artifact":"a.tar.gz","sha256":"00","published_at":"2026-01-01T00:00:00Z"}"#,
    )
    .expect("manifest");
    let artifact = dir.join("a.tar.gz");
    std::fs::write(&artifact, b"x").expect("artifact");

    let output = nas()
        .args([
            "update",
            "apply",
            &manifest.to_string_lossy(),
            "--artifact",
            &artifact.to_string_lossy(),
            "--pubkey",
            &"11".repeat(32),
        ])
        .output()
        .expect("ejecuta");

    assert_eq!(output.status.code(), Some(3));
    let payload: serde_json::Value = serde_json::from_slice(&output.stderr).expect("json de error");
    assert_eq!(payload["success"], false);

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn migrate_status_lista_componentes_legacy() {
    let output = nas()
        .args(["--json", "migrate", "status"])
        .output()
        .expect("ejecuta");
    let payload = json_of(&output);

    assert_eq!(payload["success"], true);
    let items = payload["data"]["items"].as_array().expect("items");
    assert_eq!(items.len(), 10);
    assert!(items
        .iter()
        .any(|item| item["path"] == "web" && item["wave"] == "A"));
    assert!(items
        .iter()
        .any(|item| item["path"] == "src/core/deploy.sh" && item["wave"] == "B"));
}

#[test]
fn uninstall_sin_confirmacion_solo_muestra_el_plan() {
    let output = nas()
        .args(["--json", "uninstall"])
        .output()
        .expect("ejecuta");
    let payload = json_of(&output);
    assert_eq!(payload["success"], true);
    assert_eq!(payload["data"]["executed"], false);
    assert!(payload["data"]["steps"].as_array().unwrap().len() > 5);
    let steps = serde_json::to_string(&payload["data"]["steps"]).unwrap();
    assert!(!steps.contains("/srv/nas"));
}

#[test]
fn update_pubkey_deriva_la_clave_publica() {
    let dir = std::env::temp_dir().join(format!("nas-key-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("dir");
    let key = dir.join("release.key");
    std::fs::write(
        &key,
        "9d61b19deffd5a60ba844af492ec2cc44449c5697b326919703bac031cae7f60",
    )
    .expect("clave");

    let output = nas()
        .args(["--json", "update", "pubkey", "--key"])
        .arg(&key)
        .output()
        .expect("ejecuta");
    let payload = json_of(&output);
    assert_eq!(
        payload["data"]["pubkey"],
        "d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a"
    );
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn update_status_reporta_version_y_requisito_de_firma() {
    let output = nas()
        .args(["--json", "update", "status"])
        .output()
        .expect("ejecuta update status");
    assert!(output.status.success());
    let payload = json_of(&output);
    assert_eq!(payload["data"]["require_signed_tags"], true);
    assert_eq!(payload["data"]["status"], "up_to_date");
}

#[test]
fn update_sin_subcomando_reporta_status_por_defecto() {
    let output = nas()
        .args(["--json", "update"])
        .output()
        .expect("ejecuta update sin subcomando");
    assert!(output.status.success());
    let payload = json_of(&output);
    assert_eq!(payload["data"]["require_signed_tags"], true);
    assert_eq!(payload["data"]["status"], "up_to_date");
}

#[test]
fn uninstall_alcance_os_planifica_ajustes_del_sistema() {
    let output = nas()
        .args(["--json", "uninstall", "--scope", "os"])
        .output()
        .expect("ejecuta");
    let payload = json_of(&output);
    let steps = serde_json::to_string(&payload["data"]["steps"]).unwrap();
    assert!(steps.contains("99-nas-tuning.conf"));
    assert!(steps.contains("nas.rules"));
    assert_eq!(payload["data"]["executed"], false);

    let bad = nas()
        .args(["uninstall", "--scope", "todo"])
        .output()
        .expect("ejecuta");
    assert!(!bad.status.success());
}

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
