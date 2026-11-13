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
        .env("NAS_RELEASE_PUBKEY_CANDIDATES", "none")
        .output()
        .expect("ejecuta");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("clave pública Ed25519 obligatoria"));
}

#[test]
fn update_apply_con_yes_registra_fallo_en_auditoria() {
    let dir = fixture_dir("update-audit-fail");
    let state_dir = dir.join("state");

    let output = nas()
        .args(["update", "apply", "--yes"])
        .env("NAS_STATE_DIR", &state_dir)
        .env_remove("NAS_RELEASE_PUBKEY")
        .env("NAS_RELEASE_PUBKEY_CANDIDATES", "none")
        .output()
        .expect("ejecuta");

    assert!(!output.status.success());
    let audit_raw =
        std::fs::read_to_string(state_dir.join("audit/nas-audit.jsonl")).expect("bitácora");
    assert!(audit_raw.contains("\"action\":\"update-apply\""));
    assert!(audit_raw.contains("\"result\":\"FAILED\""));

    let _ = std::fs::remove_dir_all(&dir);
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

#[path = "cli/helpers.rs"]
mod helpers;
