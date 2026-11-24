#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

use crate::deploy::RECYCLE_PURGE_SCRIPT;

use super::acls::{dir_has_nas_acls, sync_data_root_permissions_with_principals};
use super::services::sync_artifact_needed;
use super::*;

#[test]
fn calcula_cidr_de_red_ipv4_y_descarta_loopback() {
    assert_eq!(
        ipv4_cidr_network("192.168.1.50/24").as_deref(),
        Some("192.168.1.0/24")
    );
    assert_eq!(
        ipv4_cidr_network("10.10.1.222/16").as_deref(),
        Some("10.10.0.0/16")
    );
    assert_eq!(ipv4_cidr_network("127.0.0.1/8"), None);
    assert_eq!(ipv4_cidr_network("169.254.1.2/16"), None);
    assert_eq!(ipv4_cidr_network("invalido"), None);
}

#[test]
fn extrae_cidrs_desde_ip_addr_y_ssh_connection() {
    let sample = "\
2: eth0    inet 10.10.1.20/24 brd 10.10.1.255 scope global dynamic eth0
3: eth1    inet 192.168.50.10/24 brd 192.168.50.255 scope global eth1
1: lo      inet 127.0.0.1/8 scope host lo";
    assert_eq!(
        parse_ip_addr_cidrs(sample),
        vec!["10.10.1.0/24".to_string(), "192.168.50.0/24".to_string()]
    );

    assert_eq!(
        parse_ssh_client_ip("203.0.113.10 54321 10.10.1.20 22").as_deref(),
        Some("203.0.113.10")
    );
    assert_eq!(
        parse_ssh_client_ip("2001:db8::1 54321 2001:db8::2 22"),
        None
    );
    assert_eq!(parse_ssh_client_ip("127.0.0.1 1234 127.0.0.1 22"), None);
    assert_eq!(
        parse_ssh_server_port("203.0.113.10 54321 10.10.1.20 2222"),
        2222
    );
    assert_eq!(parse_ssh_server_port("203.0.113.10 54321 10.10.1.20"), 22);
}

#[test]
fn desactiva_ipv6_en_default_ufw_idempotente() {
    let orig = "# /etc/default/ufw\nIPV6=yes\nDEFAULT_INPUT_POLICY=\"DROP\"\n";
    let updated = render_ufw_default_ipv6_disabled(orig);
    assert!(updated.contains("IPV6=no"));
    assert!(!updated.contains("IPV6=yes"));
    assert_eq!(render_ufw_default_ipv6_disabled(&updated), updated);
}

#[test]
fn dry_run_no_muta_sistema() {
    let report = harden(
        &Runner::new(),
        &HardenOptions {
            apply: false,
            safe_only: true,
            ..Default::default()
        },
    )
    .expect("dry run");
    assert!(!report.applied);
    assert!(!report.actions.is_empty());
    assert!(report
        .actions
        .iter()
        .any(|a| a.contains("nas-recycle-purge")));
    assert!(report.actions.iter().any(|a| a.contains("NAS_DATA_ROOT")));
}

#[test]
fn acl_has_effective_respeta_mascara_y_default() {
    let sample = "\
# file: /srv/nas/PUBLICO
# owner: root
# group: grp_samba
user::rwx
user:www-data:rwx
user:nas-web:rwx
group::rwx
group:grp_samba:rwx\t\t#effective:r-x
mask::r-x
other::---
default:user::rwx
default:user:nas-web:rwx
default:group::rwx
default:group:grp_samba:rwx
default:mask::rwx
default:other::---
";
    assert!(acl_has_effective(sample, "nas-web", false, "rwx", false));
    assert!(acl_has_effective(sample, "nas-web", false, "rwx", true));
    assert!(acl_has_effective(sample, "www-data", false, "rwx", false));
    assert!(!acl_has_effective(sample, "www-data", false, "rwx", true));
    assert!(
        !acl_has_effective(sample, "grp_samba", true, "rwx", false),
        "debe respetar #effective:r-x cuando mask restringe escritura"
    );
    assert!(acl_has_effective(sample, "grp_samba", true, "rx", false));
    assert!(acl_has_effective(sample, "grp_samba", true, "rwx", true));
}

#[test]
fn discover_active_share_dirs_excluye_respaldos_logs_trash_staging_y_symlinks() {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let base =
        std::env::temp_dir().join(format!("nas_sec_discover_{}_{}", std::process::id(), nanos));
    let data_root = base.join("srv_nas");
    let external = base.join("external");
    std::fs::create_dir_all(&external).unwrap();

    for rel in [
        "PUBLICO",
        "Test",
        "EXTRA_FROM_CONF",
        "BACKUPS_HISTORICOS/t1/snap1",
        "BACKUPS_PERMANENTES/t2",
        "LOGS_BACKUP",
        ".trash",
        ".nas_staging",
        ".snapshots",
        "lost+found",
        "unidades/u1/CONTABILIDAD",
        "unidades/u1/BACKUPS_HISTORICOS/s1",
        "unidades/u1/LOGS_BACKUP",
        "unidades/u1/.trash",
        "unidades/u1/.nas_staging",
    ] {
        std::fs::create_dir_all(data_root.join(rel)).unwrap();
    }
    std::os::unix::fs::symlink(&external, data_root.join("symlink_share")).unwrap();

    let smb_conf = base.join("smb.conf");
    std::fs::write(
        &smb_conf,
        format!(
            "[global]\n   workgroup = WG\n\
                 [PUBLICO]\n   path = {}\n\
                 [EXTRA]\n   path = {}\n\
                 [BKP_NESTED]\n   path = {}\n\
                 [OUTSIDE]\n   path = {}\n",
            data_root.join("PUBLICO").display(),
            data_root.join("EXTRA_FROM_CONF").display(),
            data_root.join("BACKUPS_HISTORICOS/t1").display(),
            external.display(),
        ),
    )
    .unwrap();

    let unit_roots = discover_unit_roots(&data_root);
    assert_eq!(unit_roots, vec![data_root.join("unidades/u1")]);

    let shares = discover_active_share_dirs(&data_root, &smb_conf, &unit_roots);
    let share_set: std::collections::BTreeSet<PathBuf> = shares.into_iter().collect();

    assert!(share_set.contains(&data_root.join("PUBLICO")));
    assert!(share_set.contains(&data_root.join("Test")));
    assert!(share_set.contains(&data_root.join("EXTRA_FROM_CONF")));
    assert!(share_set.contains(&data_root.join("unidades/u1/CONTABILIDAD")));

    assert!(!share_set.contains(&data_root.join("BACKUPS_HISTORICOS")));
    assert!(!share_set.contains(&data_root.join("BACKUPS_HISTORICOS/t1")));
    assert!(!share_set.contains(&data_root.join("BACKUPS_PERMANENTES")));
    assert!(!share_set.contains(&data_root.join("LOGS_BACKUP")));
    assert!(!share_set.contains(&data_root.join(".trash")));
    assert!(!share_set.contains(&data_root.join(".nas_staging")));
    assert!(!share_set.contains(&data_root.join(".snapshots")));
    assert!(!share_set.contains(&data_root.join("lost+found")));
    assert!(!share_set.contains(&data_root.join("unidades")));
    assert!(!share_set.contains(&data_root.join("unidades/u1")));
    assert!(!share_set.contains(&data_root.join("unidades/u1/BACKUPS_HISTORICOS")));
    assert!(!share_set.contains(&data_root.join("symlink_share")));
    assert!(!share_set.contains(&external));

    let _ = std::fs::remove_dir_all(&base);
}

#[cfg(unix)]
#[test]
fn sync_data_root_permissions_migra_acls_staging_y_unidades_sin_tocar_backups_e_idempotente() {
    if !has_acl_tools() {
        return;
    }
    let runner = Runner::new();
    let user_out = runner
        .run(&CommandSpec::new("id").args(["-un"]))
        .expect("id -un");
    let group_out = runner
        .run(&CommandSpec::new("id").args(["-gn"]))
        .expect("id -gn");
    if !user_out.is_success() || !group_out.is_success() {
        return;
    }
    let current_user = user_out.stdout.trim().to_string();
    let current_group = group_out.stdout.trim().to_string();

    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let base = std::env::temp_dir().join(format!("nas_sec_acls_{}_{}", std::process::id(), nanos));
    let data_root = base.join("srv_nas");
    let state_dir = base.join("state_nas");
    let external = base.join("external_target");

    std::fs::create_dir_all(data_root.join("PUBLICO/sub")).unwrap();
    std::fs::create_dir_all(data_root.join("unidades/u1/CONTABILIDAD")).unwrap();
    std::fs::create_dir_all(data_root.join("BACKUPS_HISTORICOS/snap1")).unwrap();
    std::fs::create_dir_all(data_root.join("LOGS_BACKUP")).unwrap();
    std::fs::create_dir_all(data_root.join(".nas_staging")).unwrap();
    std::fs::create_dir_all(data_root.join("unidades/u1/.nas_staging")).unwrap();
    std::fs::create_dir_all(state_dir.join("staging")).unwrap();
    std::fs::create_dir_all(&external).unwrap();

    std::fs::write(data_root.join("PUBLICO/sub/archivo.txt"), b"ok").unwrap();
    std::fs::write(data_root.join("unidades/u1/CONTABILIDAD/doc.txt"), b"ok").unwrap();
    std::fs::write(data_root.join("BACKUPS_HISTORICOS/snap1/f.txt"), b"bkp").unwrap();
    std::fs::write(data_root.join("LOGS_BACKUP/job.log"), b"log").unwrap();
    std::os::unix::fs::symlink(&external, data_root.join("PUBLICO/enlace_ext")).unwrap();

    std::fs::set_permissions(&data_root, std::fs::Permissions::from_mode(0o755)).unwrap();
    std::fs::set_permissions(
        data_root.join("unidades"),
        std::fs::Permissions::from_mode(0o2755),
    )
    .unwrap();
    std::fs::set_permissions(
        data_root.join("unidades/u1"),
        std::fs::Permissions::from_mode(0o2755),
    )
    .unwrap();
    std::fs::set_permissions(
        data_root.join(".nas_staging"),
        std::fs::Permissions::from_mode(0o755),
    )
    .unwrap();
    std::fs::set_permissions(
        data_root.join("unidades/u1/.nas_staging"),
        std::fs::Permissions::from_mode(0o755),
    )
    .unwrap();
    std::fs::set_permissions(
        state_dir.join("staging"),
        std::fs::Permissions::from_mode(0o2770),
    )
    .unwrap();
    std::fs::set_permissions(
        data_root.join("BACKUPS_HISTORICOS/snap1/f.txt"),
        std::fs::Permissions::from_mode(0o640),
    )
    .unwrap();
    std::fs::set_permissions(
        data_root.join("LOGS_BACKUP/job.log"),
        std::fs::Permissions::from_mode(0o640),
    )
    .unwrap();

    let smb_conf = base.join("smb.conf");
    std::fs::write(
        &smb_conf,
        format!(
            "[PUBLICO]\n   path = {}\n",
            data_root.join("PUBLICO").display()
        ),
    )
    .unwrap();

    let mut actions = Vec::new();
    let mut warnings = Vec::new();
    let restart = sync_data_root_permissions_with_principals(
        &runner,
        &data_root,
        Some(&state_dir),
        &smb_conf,
        &current_user,
        &current_group,
        None,
        false,
        &mut actions,
        &mut warnings,
    );
    assert!(!restart);
    assert!(warnings.is_empty(), "warnings inesperados: {warnings:?}");
    assert!(!actions.is_empty());

    for root_dir in [
        data_root.clone(),
        data_root.join("unidades"),
        data_root.join("unidades/u1"),
    ] {
        let mode = std::fs::metadata(&root_dir).unwrap().permissions().mode() & 0o7777;
        assert_eq!(mode, 0o2770, "modo 2770 esperado en {}", root_dir.display());
        assert!(dir_has_nas_acls(
            &runner,
            &root_dir,
            &current_user,
            &current_group,
            false
        ));
    }

    for staging in [
        data_root.join(".nas_staging"),
        data_root.join("unidades/u1/.nas_staging"),
        state_dir.join("staging"),
    ] {
        let mode = std::fs::metadata(&staging).unwrap().permissions().mode() & 0o7777;
        assert_eq!(mode, 0o700, "modo 0700 esperado en {}", staging.display());
    }

    for share in [
        data_root.join("PUBLICO"),
        data_root.join("PUBLICO/sub"),
        data_root.join("unidades/u1/CONTABILIDAD"),
    ] {
        assert!(
            dir_has_nas_acls(&runner, &share, &current_user, &current_group, false),
            "faltan ACLs en {}",
            share.display()
        );
    }

    let file_acl = runner
        .run(&CommandSpec::new("getfacl").args([
            "-p",
            &data_root.join("PUBLICO/sub/archivo.txt").to_string_lossy(),
        ]))
        .unwrap();
    assert!(acl_has_effective(
        &file_acl.stdout,
        &current_user,
        false,
        "rw",
        false
    ));

    let ext_acl = runner
        .run(&CommandSpec::new("getfacl").args(["-p", &external.to_string_lossy()]))
        .unwrap();
    assert!(!acl_has_effective(
        &ext_acl.stdout,
        &current_user,
        false,
        "rwx",
        false
    ));

    assert!(!dir_has_nas_acls(
        &runner,
        &data_root.join("BACKUPS_HISTORICOS"),
        &current_user,
        &current_group,
        false
    ));
    assert!(!dir_has_nas_acls(
        &runner,
        &data_root.join("LOGS_BACKUP"),
        &current_user,
        &current_group,
        false
    ));
    assert_eq!(
        std::fs::metadata(data_root.join("BACKUPS_HISTORICOS/snap1/f.txt"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o640
    );

    let mut actions2 = Vec::new();
    let mut warnings2 = Vec::new();
    sync_data_root_permissions_with_principals(
        &runner,
        &data_root,
        Some(&state_dir),
        &smb_conf,
        &current_user,
        &current_group,
        None,
        false,
        &mut actions2,
        &mut warnings2,
    );
    assert!(warnings2.is_empty(), "warnings2: {warnings2:?}");
    assert!(
        actions2.is_empty(),
        "segunda ejecución debe ser idempotente, obtuvo: {actions2:?}"
    );

    let _ = std::fs::remove_dir_all(&base);
}

#[cfg(unix)]
#[test]
fn ensure_active_share_acls_corrige_subcarpetas_y_archivos_aunque_raiz_tenga_acl() {
    if !has_acl_tools() {
        return;
    }
    let runner = Runner::new();
    let user_out = runner
        .run(&CommandSpec::new("id").args(["-un"]))
        .expect("id -un");
    let group_out = runner
        .run(&CommandSpec::new("id").args(["-gn"]))
        .expect("id -gn");
    if !user_out.is_success() || !group_out.is_success() {
        return;
    }
    let current_user = user_out.stdout.trim().to_string();
    let current_group = group_out.stdout.trim().to_string();

    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let base =
        std::env::temp_dir().join(format!("nas_sec_nested_{}_{}", std::process::id(), nanos));
    let data_root = base.join("srv_nas");
    let share = data_root.join("PUBLICO");
    let sub = share.join("sub");
    std::fs::create_dir_all(&sub).unwrap();

    let root_file = share.join("legacy.txt");
    let nested_file = sub.join("privado.txt");
    std::fs::write(&root_file, b"legacy").unwrap();
    std::fs::write(&nested_file, b"secret").unwrap();

    // Simula share cuya raíz ya tenía ACLs no recursivas, pero subcarpeta y archivos siguen con 0700/0600 sin ACL.
    std::fs::set_permissions(&data_root, std::fs::Permissions::from_mode(0o2770)).unwrap();
    let root_acl = format!(
            "u:{current_user}:rwx,g:{current_group}:rwx,m::rwx,d:u:{current_user}:rwx,d:g:{current_group}:rwx,d:m::rwx"
        );
    runner
        .run(&CommandSpec::new("setfacl").args([
            "-m",
            &root_acl,
            "--",
            &data_root.to_string_lossy(),
            &share.to_string_lossy(),
        ]))
        .unwrap()
        .ensure_success()
        .unwrap();
    std::fs::set_permissions(&sub, std::fs::Permissions::from_mode(0o700)).unwrap();
    std::fs::set_permissions(&root_file, std::fs::Permissions::from_mode(0o600)).unwrap();
    std::fs::set_permissions(&nested_file, std::fs::Permissions::from_mode(0o600)).unwrap();

    let smb_conf = base.join("smb.conf");
    std::fs::write(
        &smb_conf,
        format!("[PUBLICO]\n   path = {}\n", share.display()),
    )
    .unwrap();

    let mut actions = Vec::new();
    let mut warnings = Vec::new();
    sync_data_root_permissions_with_principals(
        &runner,
        &data_root,
        None,
        &smb_conf,
        &current_user,
        &current_group,
        None,
        false,
        &mut actions,
        &mut warnings,
    );
    assert!(warnings.is_empty(), "warnings: {warnings:?}");
    assert_eq!(
        std::fs::metadata(data_root.join(".nas_staging"))
            .unwrap()
            .permissions()
            .mode()
            & 0o7777,
        0o700,
        ".nas_staging creado bajo 2770 no debe heredar setgid"
    );
    assert!(
            actions.iter().any(|a| a.contains("PUBLICO")),
            "debe detectar y corregir subcarpetas/archivos sin ACL aunque la raíz ya tenga ACL: {actions:?}"
        );
    assert!(dir_has_nas_acls(
        &runner,
        &sub,
        &current_user,
        &current_group,
        false
    ));
    for file in [&root_file, &nested_file] {
        let out = runner
            .run(&CommandSpec::new("getfacl").args(["-p", "--", &file.to_string_lossy()]))
            .unwrap();
        assert!(acl_has_effective(
            &out.stdout,
            &current_user,
            false,
            "rw",
            false
        ));
        assert!(acl_has_effective(
            &out.stdout,
            &current_group,
            true,
            "rw",
            false
        ));
    }

    let mut actions2 = Vec::new();
    let mut warnings2 = Vec::new();
    sync_data_root_permissions_with_principals(
        &runner,
        &data_root,
        None,
        &smb_conf,
        &current_user,
        &current_group,
        None,
        false,
        &mut actions2,
        &mut warnings2,
    );
    assert!(warnings2.is_empty(), "warnings2: {warnings2:?}");
    assert!(
        actions2.is_empty(),
        "segunda pasada idempotente: {actions2:?}"
    );

    let _ = std::fs::remove_dir_all(&base);
}

#[cfg(unix)]
#[test]
fn sync_artifact_needed_detecta_recycle_purge_obsoleto_o_sin_permiso_ejecucion() {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let path =
        std::env::temp_dir().join(format!("nas_recycle_sync_{}_{}", std::process::id(), nanos));
    assert!(sync_artifact_needed(&path, 0o755, RECYCLE_PURGE_SCRIPT));

    std::fs::write(
        &path,
        "#!/bin/bash\nfind /srv/nas -type f -path '*/.recycle/*' -mtime +15 -delete\n",
    )
    .unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert!(sync_artifact_needed(&path, 0o755, RECYCLE_PURGE_SCRIPT));

    std::fs::write(&path, RECYCLE_PURGE_SCRIPT).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
    assert!(sync_artifact_needed(&path, 0o755, RECYCLE_PURGE_SCRIPT));

    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert!(!sync_artifact_needed(&path, 0o755, RECYCLE_PURGE_SCRIPT));

    let _ = std::fs::remove_file(&path);
}
