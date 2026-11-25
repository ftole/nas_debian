use super::{
    confirm_ok, create_subvolume, extend_pool, format_device, lvm_create, parent_disk,
    valid_device, valid_name, valid_size,
};

#[test]
fn valida_confirmaciones_y_nombres() {
    assert!(confirm_ok("SI-FORMATEAR"));
    assert!(confirm_ok("SI-DESTRUIR-DATOS"));
    assert!(!confirm_ok("si"));
    assert!(!confirm_ok(""));

    assert!(valid_device("/dev/sdb"));
    assert!(valid_device("/dev/nvme0n1p1"));
    assert!(valid_device("/dev/mapper/vg-data"));
    assert!(!valid_device("/dev/../etc/passwd"));
    assert!(!valid_device("/dev/"));
    assert!(!valid_device("/dev/sda/"));
    assert!(!valid_device("rm -rf /"));
    assert!(!valid_device("sdb"));

    assert!(valid_name("unidad_1"));
    assert!(!valid_name("unidad 1"));
    assert!(!valid_name(""));
    assert!(!valid_name("a/b"));

    assert!(valid_size("100%FREE"));
    assert!(valid_size("100%free"));
    assert!(valid_size("20G"));
    assert!(valid_size("20g"));
    assert!(valid_size("500M"));
    assert!(valid_size("50%vg"));
    assert!(!valid_size("rm -rf"));
}

#[test]
fn heuristica_de_disco_padre() {
    let runner = nas_exec::Runner::new();
    assert_eq!(
        parent_disk(&runner, "/dev/nvme0n1p2").as_deref(),
        Some("/dev/nvme0n1")
    );
    assert_eq!(
        parent_disk(&runner, "/dev/nvme0n1").as_deref(),
        Some("/dev/nvme0n1")
    );
    assert_eq!(
        parent_disk(&runner, "/dev/sdb").as_deref(),
        Some("/dev/sdb")
    );
}

#[test]
fn rechaza_operaciones_sin_confirmacion() {
    let runner = nas_exec::Runner::new();
    assert!(format_device(&runner, "/dev/sdz", "ext4", "", false).is_err());
    assert!(lvm_create(&runner, "/dev/sdz", "vg", "lv", "100%FREE", "ext4", "", false).is_err());
    assert!(create_subvolume(&runner, "", "vol", "").is_err());
    assert!(extend_pool(&runner, "/dev/sdz", "").is_err());
    assert!(format_device(&runner, "../etc", "ext4", "SI-FORMATEAR", false).is_err());
    assert!(format_device(&runner, "/dev/sdz", "xfs", "SI-FORMATEAR", false).is_err());
}

#[test]
fn ensure_safe_permite_si_destruir_datos() {
    let runner = nas_exec::Runner::new();
    // Dispositivos que no sean del sistema operativo
    assert!(super::ensure_safe(&runner, "/dev/sdz", "SI-DESTRUIR-DATOS").is_ok());
}

#[test]
fn create_pool_y_wipe_validan_reglas_antes_de_helper() {
    let runner = nas_exec::Runner::new();
    assert!(super::create_pool(
        &runner,
        "principal",
        "btrfs_single",
        &[],
        "SI-FORMATEAR",
        false
    )
    .is_err());
    assert!(super::create_pool(
        &runner,
        "pool2",
        "ext4",
        &["/dev/sdx".into(), "/dev/sdy".into()],
        "SI-FORMATEAR",
        false
    )
    .is_err());
    assert!(super::create_pool(
        &runner,
        "pool2",
        "btrfs_raid1",
        &["/dev/sdx".into()],
        "SI-FORMATEAR",
        false
    )
    .is_err());
    assert!(super::wipe_disk(&runner, "/dev/sdx", "SI-FORMATEAR", false).is_err());
    assert!(super::umount_unit(&runner, "../bad", "SI-FORMATEAR").is_err());
    assert!(
        super::extend_pool_target(&runner, "../bad", "/dev/sdx", "SI-FORMATEAR", false).is_err()
    );
}

#[test]
fn device_matches_pool_member_sin_colisiones_de_prefijo() {
    let runner = nas_exec::Runner::new();
    assert!(!super::device_matches_pool_member(
        &runner,
        "/dev/sdaa1",
        "/dev/sda"
    ));
    assert!(!super::device_matches_pool_member(
        &runner,
        "/dev/nvme0n10p1",
        "/dev/nvme0n1"
    ));
    assert!(!super::device_matches_pool_member(
        &runner,
        "/dev/loop10p1",
        "/dev/loop1"
    ));
    assert!(!super::device_matches_pool_member(
        &runner,
        "/dev/loop10",
        "/dev/loop1"
    ));
    assert!(super::device_matches_pool_member(
        &runner,
        "/dev/sda1",
        "/dev/sda"
    ));
    assert!(super::device_matches_pool_member(
        &runner,
        "/dev/sda",
        "/dev/sda1"
    ));
    assert!(super::device_matches_pool_member(
        &runner,
        "/dev/nvme0n1p1",
        "/dev/nvme0n1"
    ));
    assert!(super::device_matches_pool_member(
        &runner,
        "/dev/loop1p1",
        "/dev/loop1"
    ));
    assert!(!super::device_matches_pool_member(
        &runner,
        "/dev/mmcblk10p1",
        "/dev/mmcblk1"
    ));
    assert!(super::device_matches_pool_member(
        &runner,
        "/dev/mmcblk0p1",
        "/dev/mmcblk0"
    ));
    assert!(super::device_matches_pool_member(
        &runner,
        "/dev/sda1",
        "/dev/sda/"
    ));
}
