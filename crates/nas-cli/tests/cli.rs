//! Pruebas de integración de la CLI `nas`.

use std::process::Command;

fn nas() -> Command {
    Command::new(env!("CARGO_BIN_EXE_nas"))
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
    let output = nas().args(["--json", "network"]).output().expect("ejecuta");
    let payload = json_of(&output);

    assert_eq!(payload["success"], true);
    assert!(payload["data"]["interfaces"].is_array());
    assert!(payload["data"]["listening"].is_array());
    assert!(payload["data"]["samba"]["min_protocol"].is_string());
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
    assert!(payload["data"]["steps"].is_array());
    assert_eq!(payload["data"]["retention"]["keep"], 2);

    let _ = std::fs::remove_dir_all(&dir);
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
