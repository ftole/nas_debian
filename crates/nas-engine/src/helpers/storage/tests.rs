use nas_exec::Runner;

use super::disks::partition_for;
use super::pools::{check_device_not_in_btrfs_pools, resolve_pool_target};
use super::*;
use crate::storage_ops::confirm_ok;

#[test]
fn confirmacion_valida() {
    assert!(confirm_ok("SI-FORMATEAR"));
    assert!(confirm_ok("SI-DESTRUIR-DATOS"));
    assert!(!confirm_ok("si"));
    assert!(!confirm_ok(""));
}

#[test]
fn calculo_de_particion() {
    assert_eq!(partition_for("/dev/sdb"), "/dev/sdb1");
    assert_eq!(partition_for("/dev/nvme0n1"), "/dev/nvme0n1p1");
    assert_eq!(partition_for("/dev/mmcblk0"), "/dev/mmcblk0p1");
    assert_eq!(partition_for("/dev/loop0"), "/dev/loop0p1");
    assert_eq!(partition_for("/dev/nbd0"), "/dev/nbd0p1");
}

#[test]
fn remove_pool_device_valida_confirmacion_y_argumentos() {
    let runner = Runner::new();
    assert!(remove_pool_device(&runner, "", "principal", "/dev/sdb1").is_err());
    assert!(remove_pool_device(&runner, "SI-FORMATEAR", "principal", "sdb1").is_err());
    assert!(remove_pool_device(&runner, "SI-FORMATEAR", "../bad", "/dev/sdb1").is_err());
}

#[test]
fn opciones_de_montaje_segun_tipo_de_disco() {
    let runner = Runner::new();
    assert_eq!(
        mount_options_for(&runner, "btrfs", "/dev/sdb1"),
        "rw,noatime,compress=zstd:3,space_cache=v2"
    );
    let ext4_opts = mount_options_for(&runner, "ext4", "/dev/sdb1");
    assert!(ext4_opts.starts_with("rw,noatime,commit=5"));
    assert!(ext4_opts.ends_with(",errors=remount-ro"));
}

#[test]
fn create_pool_valida_modos_conteo_y_duplicados() {
    let runner = Runner::new();
    assert!(create_pool(
        &runner,
        "",
        "principal",
        "btrfs_single",
        &["/dev/sdz".into()],
        false
    )
    .is_err());
    assert!(create_pool(
        &runner,
        "SI-FORMATEAR",
        "principal",
        "xfs",
        &["/dev/sdz".into()],
        false
    )
    .is_err());
    assert!(create_pool(
        &runner,
        "SI-FORMATEAR",
        "principal",
        "btrfs_single",
        &[],
        false
    )
    .is_err());
    assert!(create_pool(
        &runner,
        "SI-FORMATEAR",
        "pool2",
        "ext4",
        &["/dev/sdx".into(), "/dev/sdy".into()],
        false
    )
    .is_err());
    assert!(create_pool(
        &runner,
        "SI-FORMATEAR",
        "pool2",
        "btrfs_raid1",
        &["/dev/sdx".into()],
        false
    )
    .is_err());
    assert!(create_pool(
        &runner,
        "SI-FORMATEAR",
        "pool2",
        "btrfs_single",
        &["/dev/sdx".into(), "/dev/sdx".into()],
        false
    )
    .is_err());
    assert!(create_pool(
        &runner,
        "SI-FORMATEAR",
        "../bad",
        "btrfs_single",
        &["/dev/sdx".into()],
        false
    )
    .is_err());
}

#[test]
fn resolve_pool_target_valida_principal_y_unidades() {
    assert_eq!(
        resolve_pool_target("principal").unwrap(),
        ("/srv/nas".to_string(), None)
    );
    assert_eq!(
        resolve_pool_target("").unwrap(),
        ("/srv/nas".to_string(), None)
    );
    assert_eq!(
        resolve_pool_target("pool_bkp").unwrap(),
        (
            "/srv/nas/unidades/pool_bkp".to_string(),
            Some("pool_bkp".to_string())
        )
    );
    assert!(resolve_pool_target("../escape").is_err());
}

#[test]
fn check_device_not_in_btrfs_pools_evita_colision_de_prefijo() {
    let runner = Runner::new();
    let pools = vec![crate::storage::BtrfsPoolInfo {
        label: Some("NAS_DATA".to_string()),
        uuid: "11111111-2222-3333-4444-555555555555".to_string(),
        total_devices: 3,
        devices: vec![
            "/dev/sdaa1".to_string(),
            "/dev/nvme0n10p1".to_string(),
            "/dev/loop10p1".to_string(),
        ],
        mountpoint: Some("/srv/nas".to_string()),
        data_profile: Some("single".to_string()),
        metadata_profile: Some("RAID1".to_string()),
    }];

    // No debe haber colisión por prefijo con /dev/sda, /dev/nvme0n1 ni /dev/loop1
    assert!(check_device_not_in_btrfs_pools(&runner, &pools, "/dev/sda").is_ok());
    assert!(check_device_not_in_btrfs_pools(&runner, &pools, "/dev/nvme0n1").is_ok());
    assert!(check_device_not_in_btrfs_pools(&runner, &pools, "/dev/loop1").is_ok());
    assert!(check_device_not_in_btrfs_pools(&runner, &pools, "/dev/loop10p2").is_ok());

    // Sí debe bloquear coincidencias exactas o por disco padre real
    assert!(check_device_not_in_btrfs_pools(&runner, &pools, "/dev/sdaa1").is_err());
    assert!(check_device_not_in_btrfs_pools(&runner, &pools, "/dev/sdaa").is_err());
    assert!(check_device_not_in_btrfs_pools(&runner, &pools, "/dev/sdaa/").is_err());
    assert!(check_device_not_in_btrfs_pools(&runner, &pools, "/dev/nvme0n10").is_err());
    assert!(check_device_not_in_btrfs_pools(&runner, &pools, "/dev/loop10").is_err());

    // Pools sin montar no bloquean la operación
    let unmounted = vec![crate::storage::BtrfsPoolInfo {
        label: None,
        uuid: "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee".to_string(),
        total_devices: 1,
        devices: vec!["/dev/sdb1".to_string()],
        mountpoint: None,
        data_profile: None,
        metadata_profile: None,
    }];
    assert!(check_device_not_in_btrfs_pools(&runner, &unmounted, "/dev/sdb").is_ok());
}

#[test]
fn wipe_disk_exige_si_destruir_datos_y_dispositivo_valido() {
    let runner = Runner::new();
    assert!(wipe_disk(&runner, "SI-FORMATEAR", "/dev/sdz").is_err());
    assert!(wipe_disk(&runner, "SI-DESTRUIR-DATOS", "/dev/sdz/").is_err());
    assert!(wipe_disk(&runner, "SI-DESTRUIR-DATOS", "/dev/").is_err());
}

#[test]
fn strip_subvol_suffix_y_parse_pknames_soportan_btrfs_y_multi_padre() {
    use super::disks::{parse_pknames, strip_subvol_suffix};

    assert_eq!(strip_subvol_suffix("/dev/nvme0n1p2[/@]"), "/dev/nvme0n1p2");
    assert_eq!(strip_subvol_suffix("/dev/sda2[/@rootfs]"), "/dev/sda2");
    assert_eq!(strip_subvol_suffix("/dev/sda1/"), "/dev/sda1");

    let parents = parse_pknames("sda1\nsdb1\nsda1\n\n");
    assert_eq!(parents, vec!["/dev/sda1", "/dev/sdb1"]);
}

#[test]
fn pv_belongs_to_disk_evita_colision_de_prefijo() {
    use super::disks::pv_belongs_to_disk;

    assert!(pv_belongs_to_disk("/dev/sda1", "/dev/sda"));
    assert!(pv_belongs_to_disk("/dev/sda", "/dev/sda"));
    assert!(!pv_belongs_to_disk("/dev/sdaa1", "/dev/sda"));
    assert!(pv_belongs_to_disk("/dev/nvme0n1p1", "/dev/nvme0n1"));
    assert!(!pv_belongs_to_disk("/dev/nvme0n10p1", "/dev/nvme0n1"));
}
