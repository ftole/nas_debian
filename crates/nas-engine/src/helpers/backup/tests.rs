use nas_exec::Runner;

use crate::backup_control::valid_id;
use crate::backup_spec::BackupSpec;

use super::install::install_backup_spec_with_dirs;

#[test]
fn rechaza_id_invalido() {
    assert!(!valid_id("id con espacios"));
    assert!(!valid_id(""));
    assert!(!valid_id("../escapes"));
}

#[test]
fn valida_id_correcto() {
    assert!(valid_id("bkp_servidor1"));
    assert!(valid_id("tarea-diaria-1"));
}

#[test]
fn instala_spec_y_permite_parsear_runner_generado() {
    let dir = std::env::temp_dir().join(format!("nas-bkp-spec-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let runner_dir = dir.join("bin");
    let cron_dir = dir.join("cron.d");
    let cred_dir = dir.join("creds");
    let dest_root = dir.join("dest");

    let spec = BackupSpec {
        id: "demo_cifs".into(),
        proto: "cifs".into(),
        ip: "10.0.0.10".into(),
        share: "Datos".into(),
        path: String::new(),
        user: "bkpuser".into(),
        password: "Secreta#1".into(),
        cron: "30 1 * * *".into(),
        retention: 14,
        dest_unit: "principal".into(),
        port: None,
        ssh_key: None,
        auto_permanent: true,
        preserve_existing_auth: false,
    };

    let msg = install_backup_spec_with_dirs(
        &Runner::new(),
        &spec,
        &runner_dir,
        &cron_dir,
        &cred_dir,
        Some(&dest_root),
    )
    .expect("instalación");
    assert_eq!(msg, "tarea demo_cifs instalada");

    let runner_path = runner_dir.join("backup_demo_cifs.sh");
    let cron_path = cron_dir.join("backup_demo_cifs");
    let cred_path = cred_dir.join("demo_cifs.cred");
    assert!(runner_path.exists());
    assert!(cron_path.exists());
    assert!(cred_path.exists());
    assert!(dest_root.join("demo_cifs").is_dir());

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let runner_mode = std::fs::metadata(&runner_path)
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        let cron_mode = std::fs::metadata(&cron_path).unwrap().permissions().mode() & 0o777;
        let cred_mode = std::fs::metadata(&cred_path).unwrap().permissions().mode() & 0o777;
        assert_eq!(runner_mode, 0o755);
        assert_eq!(cron_mode, 0o644);
        assert_eq!(cred_mode, 0o600);
    }

    let runner_text = std::fs::read_to_string(&runner_path).unwrap();
    let parsed = crate::tasks::BackupTask::parse("demo_cifs", &runner_text).expect("parseable");
    assert_eq!(parsed.retention, 14);
    assert_eq!(parsed.kind, crate::tasks::SourceKind::Cifs);

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn actualiza_spec_conservando_credenciales_y_clave_si_password_vacio() {
    let dir = std::env::temp_dir().join(format!("nas-bkp-preserve-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let runner_dir = dir.join("bin");
    let cron_dir = dir.join("cron.d");
    let cred_dir = dir.join("creds");
    let dest_root = dir.join("dest");

    let cifs_initial = BackupSpec {
        id: "cifs_keep".into(),
        proto: "cifs".into(),
        ip: "10.0.0.10".into(),
        share: "Datos".into(),
        path: String::new(),
        user: "bkpuser".into(),
        password: "Secreta#1".into(),
        cron: "30 1 * * *".into(),
        retention: 14,
        dest_unit: "principal".into(),
        port: None,
        ssh_key: None,
        auto_permanent: true,
        preserve_existing_auth: false,
    };
    install_backup_spec_with_dirs(
        &Runner::new(),
        &cifs_initial,
        &runner_dir,
        &cron_dir,
        &cred_dir,
        Some(&dest_root),
    )
    .expect("instalación inicial cifs");

    let mut cifs_update = cifs_initial;
    cifs_update.user = "DOM\\otro_user".into();
    cifs_update.password = String::new();
    cifs_update.retention = 30;
    cifs_update.preserve_existing_auth = true;
    install_backup_spec_with_dirs(
        &Runner::new(),
        &cifs_update,
        &runner_dir,
        &cron_dir,
        &cred_dir,
        Some(&dest_root),
    )
    .expect("actualización cifs sin password");

    let cifs_cred = std::fs::read_to_string(cred_dir.join("cifs_keep.cred")).unwrap();
    assert!(cifs_cred.contains("domain=DOM\n"));
    assert!(cifs_cred.contains("username=otro_user\n"));
    assert!(cifs_cred.contains("password=Secreta#1\n"));

    let ssh_key = "-----BEGIN OPENSSH PRIVATE KEY-----\nb3BlbnNzaC1rZXktdjEAAAAABG5vbmUAAAAEbm9uZQAAAAAAAAABAAAAMwAAAAtzc2gtZW\nQyNTUxOQAAACD012345678901234567890123456789012345678901234567890123456\n-----END OPENSSH PRIVATE KEY-----\n";
    let ssh_initial = BackupSpec {
        id: "ssh_keep".into(),
        proto: "ssh".into(),
        ip: "10.0.0.20".into(),
        share: String::new(),
        path: "/var/data".into(),
        user: "backup".into(),
        password: String::new(),
        cron: "0 3 * * *".into(),
        retention: 7,
        dest_unit: "principal".into(),
        port: Some(22),
        ssh_key: Some(ssh_key.into()),
        auto_permanent: true,
        preserve_existing_auth: false,
    };
    install_backup_spec_with_dirs(
        &Runner::new(),
        &ssh_initial,
        &runner_dir,
        &cron_dir,
        &cred_dir,
        Some(&dest_root),
    )
    .expect("instalación inicial ssh");

    let mut ssh_update = ssh_initial;
    ssh_update.ssh_key = None;
    ssh_update.retention = 10;
    ssh_update.preserve_existing_auth = true;
    install_backup_spec_with_dirs(
        &Runner::new(),
        &ssh_update,
        &runner_dir,
        &cron_dir,
        &cred_dir,
        Some(&dest_root),
    )
    .expect("actualización ssh conservando clave");
    assert!(cred_dir.join("ssh_keep.id_ed25519").exists());

    let _ = std::fs::remove_dir_all(&dir);
}
