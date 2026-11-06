use nas_core::ServerRole;
use nas_engine::deploy::{DeployParams, FileSystem};

fn params(keep_data: bool) -> DeployParams {
    DeployParams {
        disk: "/dev/sdb".into(),
        workgroup: "TEAM-JOFRATO".into(),
        netbios: "SRV-TEST".into(),
        admin_user: "admin".into(),
        admin_password: Some("VerySecreta9".into()),
        role: ServerRole::ArquivosBackup,
        keep_data,
        filesystem: FileSystem::Btrfs,
        rotational: true,
    }
}

#[test]
fn artefactos_del_despliegue_incluyen_samba_y_superadmin() {
    // Con keep_data=true: superadmin sí, smb.conf no (se conservan los datos).
    let keep = params(true).all_artifacts();
    let keep_paths: Vec<&str> = keep.iter().map(|a| a.path.as_str()).collect();
    assert!(
        keep_paths.contains(&"/etc/nas/superadmin"),
        "con keep-data debe seguir escribiendo superadmin"
    );
    assert!(
        !keep_paths.iter().any(|p| p.contains("smb.conf")),
        "con keep-data NO debe reescribir smb.conf"
    );

    // Sin keep-data: smb.conf sí.
    let mut fresh_params = params(true);
    fresh_params.keep_data = false;
    let fresh = fresh_params.all_artifacts();
    let fresh_paths: Vec<&str> = fresh.iter().map(|a| a.path.as_str()).collect();
    assert!(
        fresh_paths.iter().any(|p| p.contains("smb.conf")),
        "sin keep-data debe provisionar smb.conf"
    );
    assert!(
        fresh_paths.contains(&"/etc/nas/role"),
        "debe escribir el rol"
    );
}

#[test]
fn fstab_plan_usa_opciones_de_montaje_correctas() {
    let params = params(false);
    let plan = params.plan();
    let mount: Vec<&Vec<String>> = plan
        .iter()
        .filter(|s| s.first() == Some(&"mount".to_string()))
        .collect();
    assert!(!mount.is_empty(), "debe montar /srv/nas");
    assert!(mount[0].iter().any(|arg| arg.contains("noatime")));
}

#[test]
fn password_del_admin_es_aceptada_en_el_plan() {
    let params = params(false);
    let letra_de_la_pass = params.admin_password.clone().unwrap();
    assert!(letra_de_la_pass.len() >= 6);
    assert!(params.validate().is_empty());
    // disable_dead_code para que no queje por el campo sin uso en tests.
    let _ = &params.admin_user;
}
