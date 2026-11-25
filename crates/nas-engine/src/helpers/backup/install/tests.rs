use super::*;

#[test]
fn validate_cron_content_acepta_plantilla_oficial_y_rechaza_comandos_arbitrarios() {
    let valid = "SHELL=/bin/bash\nPATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin\nMAILTO=\"\"\n0 2 * * * root /usr/local/bin/backup_demo.sh\n";
    assert!(validate_cron_content("demo", valid).is_ok());

    let valid_devnull = "30 3 * * 1-5 root /usr/local/bin/backup_demo.sh >/dev/null 2>&1\n";
    assert!(validate_cron_content("demo", valid_devnull).is_ok());

    for malicious in [
        "* * * * * root /bin/sh -c 'id > /tmp/pwn'\n",
        "0 2 * * * root /usr/local/bin/backup_demo.sh; id\n",
        "0 2 * * * root /usr/local/bin/backup_demo.sh && touch /tmp/x\n",
        "0 2 * * * root /usr/local/bin/backup_otra.sh\n",
        "0 2 * * * nobody /usr/local/bin/backup_demo.sh\n",
        "PATH=/tmp:$(id)\n0 2 * * * root /usr/local/bin/backup_demo.sh\n",
        "PATH=/var/lib/nas/staging:/usr/bin\n0 2 * * * root /usr/local/bin/backup_demo.sh\n",
        "PATH=/tmp:/usr/bin\n0 2 * * * root /usr/local/bin/backup_demo.sh\n",
        "# solo comentarios sin tarea\n",
    ] {
        assert!(
            validate_cron_content("demo", malicious).is_err(),
            "debe rechazar cron malicioso: {malicious}"
        );
    }
}

#[test]
fn render_validated_runner_descarta_codigo_inyectado_y_fija_path_seguro() {
    let injected = "#!/bin/bash\nSOURCE_TYPE=\"local\"\nSRC_PATH=\"/datos/app\"\nRETENTION=5\ntouch /tmp/pwned\n";
    let task = crate::tasks::BackupTask::parse("demo", injected).expect("parse");
    let cron = "0 3 * * * root /usr/local/bin/backup_demo.sh\n";
    let (rendered, dest_root) =
        render_validated_runner("demo", &task, cron, None, None).expect("render");
    assert_eq!(dest_root, "/srv/nas/BACKUPS_HISTORICOS");
    assert!(!rendered.contains("/tmp/pwned"));
    assert!(rendered
        .contains("export PATH=\"/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin\""));
    assert!(rendered.contains("SRC_PATH='/datos/app'"));
}

#[test]
fn render_validated_runner_rechaza_dest_root_fuera_de_srv_nas_y_soporta_cifs_cred() {
    let bad_dest = "#!/bin/bash\nSOURCE_TYPE=\"local\"\nSRC_PATH=\"/datos/app\"\nDEST_ROOT=\"/etc\"\nRETENTION=5\n";
    let bad_task = crate::tasks::BackupTask::parse("demo", bad_dest).expect("parse");
    let cron = "0 3 * * * root /usr/local/bin/backup_demo.sh\n";
    assert!(render_validated_runner("demo", &bad_task, cron, None, None).is_err());

    let legacy_cifs = "#!/bin/bash\nSOURCE_TYPE=\"cifs\"\nSRC_IP=\"192.168.1.20\"\nSRC_SHARE=\"datos\"\nRETENTION=7\n";
    let cifs_task = crate::tasks::BackupTask::parse("demo", legacy_cifs).expect("parse");
    let (rendered, _) = render_validated_runner(
        "demo",
        &cifs_task,
        cron,
        Some("username=respaldo\npassword=secreto\n"),
        None,
    )
    .expect("render cifs legacy");
    assert!(rendered.contains("SRC_USER='respaldo'"));
}

#[test]
fn write_cron_y_write_cred_rechazan_archivos_fuera_de_staging() {
    assert!(write_cron("demo", Some(Path::new("/etc/shadow")), None).is_err());
    assert!(write_cron("demo", Some(Path::new("/etc/passwd")), None).is_err());
    assert!(write_cred("demo", Some(Path::new("/etc/shadow")), None).is_err());
    assert!(write_cred(
        "demo",
        Some(Path::new("/var/lib/nas/staging/../etc/passwd")),
        None
    )
    .is_err());
    assert!(write_cron("demo", None, Some("* * * * * root /bin/bash -c id\n")).is_err());
}

#[cfg(unix)]
#[test]
fn read_confined_file_rechaza_symlinks_y_rutas_fuera_del_directorio() {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let root = std::env::temp_dir().join(format!("nas-install-sec-{}-{nanos}", std::process::id()));
    let staging = root.join("staging");
    let outside = root.join("outside");
    std::fs::create_dir_all(&staging).unwrap();
    std::fs::create_dir_all(&outside).unwrap();

    let ok_file = staging.join("ok.cron");
    let secret = outside.join("secret.txt");
    let link = staging.join("link.cron");
    std::fs::write(&ok_file, "0 2 * * * root /usr/local/bin/backup_demo.sh\n").unwrap();
    std::fs::write(&secret, "secreto").unwrap();
    std::os::unix::fs::symlink(&secret, &link).unwrap();

    assert!(read_confined_file(&ok_file, &[&staging], MAX_CRON_BYTES).is_ok());
    assert!(read_confined_file(&secret, &[&staging], MAX_CRON_BYTES).is_err());
    assert!(read_confined_file(&link, &[&staging], MAX_CRON_BYTES).is_err());

    let _ = std::fs::remove_dir_all(&root);
}
