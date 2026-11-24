use std::path::Path;

use ed25519_dalek::{Signer, SigningKey};
use rand_core::OsRng;

use super::{
    build_plan, build_rollback_plan, list_installed_releases, ReleaseManifest, RemoteReleaseInfo,
};

fn manifest() -> ReleaseManifest {
    ReleaseManifest {
        version: "1.2.3".to_string(),
        commit: "abc123".to_string(),
        artifact: "nas-1.2.3.tar.gz".to_string(),
        sha256: "00".repeat(32),
        published_at: "2026-10-23T12:00:00Z".to_string(),
        signature: None,
    }
}

fn create_dummy_release(root: &Path, version: &str, with_bins: bool, with_helpers: bool) {
    let dir = root.join("releases").join(version);
    std::fs::create_dir_all(dir.join("bin")).unwrap();
    if with_bins {
        for bin in ["nas", "nas-web", "nas-tui", "nasctl", "nas-dev"] {
            std::fs::write(dir.join("bin").join(bin), b"bin").unwrap();
        }
    }
    if with_helpers {
        std::fs::create_dir_all(dir.join("packaging")).unwrap();
        for helper in crate::deploy::helpers::ROOT_HELPERS {
            std::fs::write(dir.join("packaging").join(helper), b"#!/bin/sh\n").unwrap();
        }
    }
    std::fs::write(
        dir.join("version.json"),
        format!(r#"{{"version":"{version}","commit":"deadbeef","date":"2026-11-01"}}"#),
    )
    .unwrap();
}

#[test]
fn carga_canonica_es_estable() {
    let payload = manifest().canonical_payload();
    assert_eq!(
        payload,
        "1.2.3|abc123|nas-1.2.3.tar.gz|0000000000000000000000000000000000000000000000000000000000000000|2026-10-23T12:00:00Z"
    );
}

#[test]
fn acepta_firma_valida_y_rechaza_alteraciones() {
    let signing = SigningKey::generate(&mut OsRng);
    let mut signed = manifest();
    let signature = signing.sign(signed.canonical_payload().as_bytes());
    signed.signature = Some(hex::encode(signature.to_bytes()));

    let public = hex::encode(signing.verifying_key().to_bytes());
    assert!(signed.verify_signature(&public).is_ok());

    let mut tampered = signed.clone();
    tampered.version = "9.9.9".to_string();
    assert!(tampered.verify_signature(&public).is_err());
}

#[test]
fn rechaza_manifiesto_sin_firma() {
    assert!(manifest().verify_signature(&"11".repeat(32)).is_err());
}

#[test]
fn rechaza_clave_o_firma_con_longitud_incorrecta() {
    let mut signed = manifest();
    signed.signature = Some("ab".repeat(10));
    assert!(signed.verify_signature(&"11".repeat(32)).is_err());
}

#[test]
fn verifica_sha256_del_artefacto() {
    use sha2::{Digest, Sha256};

    let dir = std::env::temp_dir().join(format!("nas-update-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("dir");
    let artifact = dir.join("nas-1.2.3.tar.gz");
    std::fs::write(&artifact, b"contenido").expect("write");

    let mut hasher = Sha256::new();
    hasher.update(b"contenido");
    let digest = hex::encode(hasher.finalize());

    let mut release = manifest();
    release.sha256 = digest.clone();
    assert!(release.verify_artifact(&artifact).is_ok());

    release.sha256 = "ff".repeat(32);
    assert!(release.verify_artifact(&artifact).is_err());

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn verifica_sha256_por_streaming_en_artefacto_multibloque() {
    use sha2::{Digest, Sha256};

    let dir = std::env::temp_dir().join(format!("nas-update-stream-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("dir");
    let artifact = dir.join("nas-stream.tar.gz");
    let payload = vec![0x5Au8; 256 * 1024];
    std::fs::write(&artifact, &payload).expect("write");

    let mut hasher = Sha256::new();
    hasher.update(&payload);
    let digest = hex::encode(hasher.finalize());

    let mut release = manifest();
    release.sha256 = digest;
    assert!(release.verify_artifact(&artifact).is_ok());

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn plan_abi_cambia_enlace_y_revierte() {
    let plan = build_plan(
        &manifest(),
        "/opt/nas_debian",
        "/tmp/nas.tar.gz",
        Some("1.2.2"),
    );

    assert_eq!(plan.release_dir, "/opt/nas_debian/releases/1.2.3");
    assert!(plan
        .steps
        .iter()
        .any(|step| step.first().is_some_and(|arg| arg == "ln")));
    assert!(plan
        .steps
        .iter()
        .any(|step| step.contains(&"--strip-components=1".to_string())));
    assert!(plan
        .steps
        .iter()
        .any(|step| step == &vec!["chmod", "0755", "/opt/nas_debian/releases"]));
    assert!(plan
        .steps
        .iter()
        .any(|step| step == &vec!["chmod", "0755", "/opt/nas_debian/releases/1.2.3"]));
    assert!(plan
        .steps
        .iter()
        .any(|step| step.last().is_some_and(|arg| arg.contains("/login"))));

    assert_eq!(plan.rollback.len(), 18);
    assert!(plan.rollback[0].contains(&"/opt/nas_debian/releases/1.2.2".to_string()));
    assert!(plan.steps.iter().any(|step| {
        step == &vec![
            "install",
            "-m",
            "0755",
            "/opt/nas_debian/current/bin/nas-web",
            "/usr/local/bin/nas-web",
        ]
    }));
    assert!(plan.steps.iter().any(|step| {
        step == &vec![
            "install",
            "-m",
            "0755",
            "-o",
            "root",
            "-g",
            "root",
            "/opt/nas_debian/current/packaging/nas-share-dir",
            "/usr/local/sbin/nas-share-dir",
        ]
    }));
    assert!(plan.steps.iter().any(|step| {
        step == &vec![
            "install",
            "-m",
            "0644",
            "/opt/nas_debian/current/version.json",
            "/etc/nas/version.json",
        ]
    }));
    assert!(plan.steps.iter().any(|step| {
        step == &vec![
            "install",
            "-m",
            "0644",
            "/opt/nas_debian/current/version.json",
            "/opt/nas_debian/version.json",
        ]
    }));
    assert!(plan.rollback.iter().any(|step| {
        step == &vec![
            "install",
            "-m",
            "0644",
            "/opt/nas_debian/current/version.json",
            "/etc/nas/version.json",
        ]
    }));
}

#[test]
fn plan_sin_version_previa_no_tiene_rollback() {
    let plan = build_plan(&manifest(), "/opt/nas_debian", "/tmp/nas.tar.gz", None);
    assert!(plan.rollback.is_empty());
}

#[test]
fn rechaza_version_con_traversal_y_reinicia_nas_web() {
    let bad = r#"{"version":"../../etc","commit":"abc","artifact":"a.tar.gz","sha256":"00","published_at":"2026-01-01T00:00:00Z"}"#;
    assert!(ReleaseManifest::from_json(bad).is_err());

    let plan = build_plan(&manifest(), "/opt/nas_debian", "/tmp/nas.tar.gz", None);
    assert!(plan
        .steps
        .iter()
        .any(|step| step == &vec!["systemctl", "restart", "nas-web"]));
    assert!(!plan
        .steps
        .iter()
        .any(|step| step.iter().any(|arg| arg.contains("php"))));
}

#[test]
fn parsea_respuesta_de_github_releases() {
    let sample = r#"{
        "tag_name": "v1.0.0",
        "name": "Release v1.0.0",
        "body": "Novedades de la versión",
        "assets": [
            {
                "name": "nas-1.0.0.json",
                "browser_download_url": "https://github.com/ftole/nas_debian/releases/download/v1.0.0/nas-1.0.0.json"
            },
            {
                "name": "nas_debian-1.0.0.tar.gz",
                "browser_download_url": "https://github.com/ftole/nas_debian/releases/download/v1.0.0/nas_debian-1.0.0.tar.gz"
            }
        ]
    }"#;

    let info = RemoteReleaseInfo::parse_github_release(sample).expect("parse ok");
    assert_eq!(info.tag_name, "v1.0.0");
    assert_eq!(info.name.as_deref(), Some("Release v1.0.0"));
    assert_eq!(
        info.manifest_url.as_deref(),
        Some("https://github.com/ftole/nas_debian/releases/download/v1.0.0/nas-1.0.0.json")
    );
    assert_eq!(
        info.artifact_url.as_deref(),
        Some(
            "https://github.com/ftole/nas_debian/releases/download/v1.0.0/nas_debian-1.0.0.tar.gz"
        )
    );
}

#[test]
fn install_sh_usa_misma_clave_publica_y_es_compatible_con_openssl_ed25519() {
    let release_pub = include_str!("../../../../packaging/release.pub").trim();
    let install_sh = include_str!("../../../../install.sh");
    assert!(
        install_sh.contains(release_pub),
        "install.sh debe embeber la misma clave pública Ed25519 que packaging/release.pub"
    );
    assert!(
        !install_sh.contains("ed25519_verified\": true,\n  \"notes\": \"Compilación oficial"),
        "--build-from-source no debe marcar ed25519_verified=true"
    );
    assert!(
        install_sh.contains("${M_VERSION#v}\" != \"$TARGET_VER\"")
            && install_sh.contains("\"$M_ARTIFACT\" != \"$PKG_FILE\""),
        "install.sh debe validar coincidencia de versión y artefacto del manifiesto"
    );

    let signing = SigningKey::generate(&mut OsRng);
    let mut signed = manifest();
    let signature = signing.sign(signed.canonical_payload().as_bytes());
    signed.signature = Some(hex::encode(signature.to_bytes()));
    let pub_bytes = signing.verifying_key().to_bytes();

    // Verificar que `openssl pkeyutl -verify -rawin` valida la firma generada por ed25519-dalek.
    if std::path::Path::new("/usr/bin/openssl").is_file() {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let dir =
            std::env::temp_dir().join(format!("nas-ossl-ed25519-{}-{nanos}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut der = vec![
            0x30, 0x2a, 0x30, 0x05, 0x06, 0x03, 0x2b, 0x65, 0x70, 0x03, 0x21, 0x00,
        ];
        der.extend_from_slice(&pub_bytes);
        let pub_der = dir.join("pub.der");
        let sig_bin = dir.join("sig.bin");
        let payload_txt = dir.join("payload.txt");
        std::fs::write(&pub_der, &der).unwrap();
        std::fs::write(&sig_bin, signature.to_bytes()).unwrap();
        std::fs::write(&payload_txt, signed.canonical_payload().as_bytes()).unwrap();

        let out = std::process::Command::new("openssl")
            .args([
                "pkeyutl",
                "-verify",
                "-pubin",
                "-keyform",
                "DER",
                "-inkey",
                &pub_der.to_string_lossy(),
                "-rawin",
                "-in",
                &payload_txt.to_string_lossy(),
                "-sigfile",
                &sig_bin.to_string_lossy(),
            ])
            .output()
            .expect("openssl");
        let _ = std::fs::remove_dir_all(&dir);
        assert!(
            out.status.success(),
            "openssl debe verificar firma Ed25519 de ed25519-dalek: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
}

#[test]
fn lista_releases_instaladas_y_construye_plan_de_regresion_ab() {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let root =
        std::env::temp_dir().join(format!("nas-rollback-plan-{}-{nanos}", std::process::id()));
    create_dummy_release(&root, "0.1.10", true, false);
    create_dummy_release(&root, "0.1.11", true, true);
    create_dummy_release(&root, "0.1.12", true, true);
    create_dummy_release(&root, "0.1.9", false, false);
    #[cfg(unix)]
    std::os::unix::fs::symlink(root.join("releases/0.1.12"), root.join("current")).unwrap();

    let root_str = root.to_string_lossy().to_string();
    let list = list_installed_releases(&root_str);
    let versions: Vec<&str> = list.iter().map(|r| r.version.as_str()).collect();
    assert_eq!(versions, vec!["0.1.12", "0.1.11", "0.1.10", "0.1.9"]);
    assert!(list[0].current);
    assert!(!list[1].current);
    assert!(list[1].has_binaries);
    assert!(!list[3].has_binaries);

    // Rechaza traversal, versión activa y release sin binarios.
    assert!(build_rollback_plan(&root_str, "../etc", Some("0.1.12")).is_err());
    assert!(build_rollback_plan(&root_str, "0.1.12", Some("0.1.12")).is_err());
    assert!(build_rollback_plan(&root_str, "0.1.9", Some("0.1.12")).is_err());
    assert!(build_rollback_plan(&root_str, "9.9.9", Some("0.1.12")).is_err());

    // Regresión hacia 0.1.10 (sin helpers en packaging/) omite helpers pero conserva binarios y recuperación a 0.1.12.
    let plan = build_rollback_plan(&root_str, "0.1.10", Some("0.1.12")).expect("plan ok");
    assert_eq!(plan.version, "0.1.10");
    assert!(plan
        .steps
        .iter()
        .any(|s| s[0] == "ln" && s[2].ends_with("/releases/0.1.10")));
    assert!(plan
        .steps
        .iter()
        .any(|s| s.last().is_some_and(|a| a.contains("/login"))));
    assert!(!plan
        .steps
        .iter()
        .any(|s| s.last().is_some_and(|a| a.starts_with("/usr/local/sbin/"))));
    assert!(plan
        .rollback
        .iter()
        .any(|s| s[0] == "ln" && s[2].ends_with("/releases/0.1.12")));

    let _ = std::fs::remove_dir_all(&root);
}
