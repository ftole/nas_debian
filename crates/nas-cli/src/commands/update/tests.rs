use std::os::unix::fs::symlink;

use super::{
    detect_previous_version, post_update_step_timeout_secs, post_update_sync_steps,
    read_deployed_version, update_list, update_rollback,
};

#[test]
fn read_deployed_version_respeta_precedencia_y_respaldo() {
    let dir = std::env::temp_dir().join(format!(
        "nas_cli_ver_test_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    std::fs::create_dir_all(dir.join("current")).expect("mkdir current");
    let p_current = dir.join("current/version.json");
    let p_etc = dir.join("etc_version.json");
    let p_root = dir.join("root_version.json");
    std::fs::write(&p_etc, r#"{"version":"1.0.0"}"#).expect("write etc");
    std::fs::write(&p_root, r#"{"version":"0.9.0"}"#).expect("write root");

    assert_eq!(
        read_deployed_version(&[&p_current, &p_etc, &p_root], "0.0.0"),
        "1.0.0"
    );

    std::fs::write(&p_current, r#"{"version":"1.2.3"}"#).expect("write current");
    assert_eq!(
        read_deployed_version(&[&p_current, &p_etc, &p_root], "0.0.0"),
        "1.2.3"
    );

    assert_eq!(
        read_deployed_version(&[&dir.join("missing.json")], "9.9.9"),
        "9.9.9"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn detect_previous_version_resuelve_enlace_current() {
    let dir = std::env::temp_dir().join(format!(
        "nas_cli_prev_test_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    let prev = dir.join("releases/1.2.2");
    std::fs::create_dir_all(&prev).expect("mkdir prev");
    symlink(&prev, dir.join("current")).expect("symlink current");

    let root_str = dir.to_string_lossy();
    assert_eq!(
        detect_previous_version(&root_str, "1.2.3"),
        Some("1.2.2".to_string())
    );
    assert_eq!(detect_previous_version(&root_str, "1.2.2"), None);

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn post_update_sync_steps_incluye_recursos_y_sincroniza_release_pub() {
    let dir = std::env::temp_dir().join(format!(
        "nas_cli_post_sync_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    let src_pkg = dir.join("current/packaging");
    std::fs::create_dir_all(&src_pkg).expect("mkdir pkg");
    let fake_bin = dir.join("nas");
    std::fs::write(&fake_bin, b"bin").expect("write fake nas");
    let dst_pub = dir.join("etc_release.pub");
    let valid_pub = "a".repeat(64);
    let old_pub = "b".repeat(64);
    std::fs::write(src_pkg.join("release.pub"), format!("{valid_pub}\n")).expect("write src");
    std::fs::write(&dst_pub, format!("{old_pub}\n")).expect("write dst");

    let root_str = dir.to_string_lossy();
    let steps = post_update_sync_steps(&root_str, &fake_bin, &dst_pub);
    assert!(steps
        .iter()
        .any(|s| s.ends_with(&["backup".to_string(), "repair-runners".to_string()])));
    assert!(steps
        .iter()
        .any(|s| s.ends_with(&["deploy".to_string(), "resources".to_string()])));
    assert!(steps
        .iter()
        .any(|s| s.ends_with(&["deploy".to_string(), "ensure-deps".to_string()])));
    assert!(steps.iter().any(|s| s.ends_with(&[
        "security".to_string(),
        "harden".to_string(),
        "--apply".to_string(),
        "--safe-only".to_string(),
    ])));
    assert!(steps
        .iter()
        .any(|s| s == &vec!["systemctl", "restart", "nas-web"]));
    for s in &steps {
        let t = post_update_step_timeout_secs(s);
        if s.iter().any(|a| a == "ensure-deps" || a == "security") {
            assert_eq!(t, 900);
        } else {
            assert_eq!(t, 60);
        }
    }
    assert!(steps
        .iter()
        .any(|s| s.first().is_some_and(|a| a == "install")
            && s.last().is_some_and(|p| p == &dst_pub.to_string_lossy())));

    std::fs::write(&dst_pub, format!("{valid_pub}\n")).expect("sync dst");
    let steps_same = post_update_sync_steps(&root_str, &fake_bin, &dst_pub);
    assert!(!steps_same
        .iter()
        .any(|s| s.first().is_some_and(|a| a == "install")));

    std::fs::write(src_pkg.join("release.pub"), "ssh-ed25519 INVALIDA\n").expect("bad src");
    std::fs::write(&dst_pub, format!("{old_pub}\n")).expect("restore dst");
    let steps_bad = post_update_sync_steps(&root_str, &fake_bin, &dst_pub);
    assert!(!steps_bad
        .iter()
        .any(|s| s.first().is_some_and(|a| a == "install")));

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn update_list_y_rollback_validan_confirmacion_y_version() {
    let dir = std::env::temp_dir().join(format!(
        "nas_cli_rb_test_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    let r1 = dir.join("releases/0.1.11");
    let r2 = dir.join("releases/0.1.12");
    std::fs::create_dir_all(r1.join("bin")).unwrap();
    std::fs::create_dir_all(r2.join("bin")).unwrap();
    for b in ["nas", "nas-web"] {
        std::fs::write(r1.join("bin").join(b), b"bin").unwrap();
        std::fs::write(r2.join("bin").join(b), b"bin").unwrap();
    }
    symlink(&r2, dir.join("current")).unwrap();

    let root = dir.to_string_lossy().to_string();
    assert!(update_list(&root, true).is_ok());
    assert!(update_rollback(&root, Some("0.1.11"), false, false, true).is_err());
    assert!(update_rollback(&root, Some("../etc"), true, false, true).is_err());
    assert!(update_rollback(&root, Some("0.1.12"), true, false, true).is_err());
    assert!(update_rollback(&root, Some("9.9.9"), true, false, true).is_err());

    let _ = std::fs::remove_dir_all(&dir);
}
