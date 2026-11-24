use super::parsers::mount_entry_in_proc_mounts;
use super::pools::{compute_disks_from_lsblk, resolve_target_device};
use super::*;

#[test]
fn interpreta_df_completo() {
    let output = "F 1024-blocks Used Available Capacity Mounted\n/dev/sdb1 1921800000 900000000 1021800000 47% /srv/nas\n";
    let usage = parse_df_full(output).expect("df");
    assert_eq!(usage.total_kb, 1_921_800_000);
    assert_eq!(usage.used_kb, 900_000_000);
    assert_eq!(usage.available_kb, 1_021_800_000);
    assert!((usage.used_percent - 47.0).abs() < f64::EPSILON);
}

#[test]
fn df_invalido_devuelve_none() {
    assert!(parse_df_full("").is_none());
    assert!(parse_df_full("solo cabecera\n").is_none());
}

#[test]
fn valida_dispositivos() {
    assert!(is_valid_device("/dev/sdb"));
    assert!(is_valid_device("/dev/nvme0n1"));
    assert!(!is_valid_device("/dev/../etc"));
    assert!(!is_valid_device("sdb"));
    assert!(!is_valid_device("/dev/"));
    assert!(!is_valid_device("/dev/sdb;rm -rf /"));
}

#[test]
fn deduce_el_disco_padre() {
    assert_eq!(parent_disk("/dev/sdb1").as_deref(), Some("sdb"));
    assert_eq!(parent_disk("/dev/sdb").as_deref(), Some("sdb"));
    assert_eq!(parent_disk("/dev/nvme0n1p2").as_deref(), Some("nvme0n1"));
    assert_eq!(parent_disk("/dev/nvme0n1").as_deref(), Some("nvme0n1"));
    assert_eq!(parent_disk("/dev/mmcblk0p1").as_deref(), Some("mmcblk0"));
    assert_eq!(parent_disk("/dev/mmcblk0").as_deref(), Some("mmcblk0"));
    assert_eq!(parent_disk(""), None);
}

#[test]
fn resuelve_dispositivo_destino() {
    assert_eq!(
        resolve_target_device("/dev/mapper/nas_vg-nas_lv").as_deref(),
        Some("/dev/mapper/nas_vg-nas_lv")
    );
    assert_eq!(
        resolve_target_device("/dev/dm-0").as_deref(),
        Some("/dev/dm-0")
    );
    assert_eq!(
        resolve_target_device("/dev/nas_vg/nas_lv").as_deref(),
        Some("/dev/nas_vg/nas_lv")
    );
    assert_eq!(
        resolve_target_device("/dev/sdb1").as_deref(),
        Some("/dev/sdb")
    );
    assert_eq!(
        resolve_target_device("/dev/nvme0n1p2").as_deref(),
        Some("/dev/nvme0n1")
    );
}

#[test]
fn marca_protegido_el_disco_del_sistema_y_cuenta_hijos() {
    let sample = r#"{
            "blockdevices": [
                {
                    "name": "nvme0n1",
                    "size": "476.9G",
                    "type": "disk",
                    "mountpoint": null,
                    "fstype": null,
                    "model": "NVMe OS",
                    "rota": false,
                    "children": [
                        {"name": "nvme0n1p1", "size": "512M", "type": "part", "mountpoint": "/boot/efi", "fstype": "vfat", "model": null, "rota": false},
                        {"name": "nvme0n1p2", "size": "476.4G", "type": "part", "mountpoint": "/", "fstype": "ext4", "model": null, "rota": false}
                    ]
                },
                {
                    "name": "sdb",
                    "size": "1.8T",
                    "type": "disk",
                    "mountpoint": null,
                    "fstype": null,
                    "model": "NAS HDD",
                    "rota": true,
                    "children": []
                }
            ]
        }"#;
    let parsed = crate::parsers::lsblk::parse(sample).expect("lsblk");
    let list = compute_disks_from_lsblk(&parsed, Some("nvme0n1"));
    assert_eq!(list.len(), 2);
    assert!(list[0].protected);
    assert_eq!(list[0].children_count, 2);
    assert!(!list[1].protected);
    assert_eq!(list[1].children_count, 0);
}

#[test]
fn detecta_rotacional_en_lvm_md_particion_y_pool_mixto() {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let sys = std::env::temp_dir().join(format!("nas-sysfs-{}-{nanos}", std::process::id()));

    // Discos base: nvme0n1 (SSD=0), sda (HDD=1), sdb (SSD=0), vda (VM=0)
    for (disk, val) in [("nvme0n1", "0"), ("sda", "1"), ("sdb", "0"), ("vda", "0")] {
        let q = sys.join(format!("block/{disk}/queue"));
        std::fs::create_dir_all(&q).unwrap();
        std::fs::write(q.join("rotational"), format!("{val}\n")).unwrap();
    }

    // LVM dm-0 sobre nvme0n1p1 -> SSD (0)
    std::fs::create_dir_all(sys.join("block/dm-0/slaves/nvme0n1p1")).unwrap();
    std::fs::create_dir_all(sys.join("block/dm-0/queue")).unwrap();
    std::fs::write(sys.join("block/dm-0/queue/rotational"), "1\n").unwrap();
    assert_eq!(
        super::detect_rotational_in_sysfs(&sys, "/dev/dm-0"),
        Some(false)
    );

    // MD RAID md0 mixto (sdb1=0 + sda1=1) -> conservador true
    std::fs::create_dir_all(sys.join("block/md0/slaves/sdb1")).unwrap();
    std::fs::create_dir_all(sys.join("block/md0/slaves/sda1")).unwrap();
    assert_eq!(
        super::detect_rotational_in_sysfs(&sys, "/dev/md0"),
        Some(true)
    );

    // MD RAID md1 puro SSD (sdb1=0 + nvme0n1p1=0) -> false
    std::fs::create_dir_all(sys.join("block/md1/slaves/sdb1")).unwrap();
    std::fs::create_dir_all(sys.join("block/md1/slaves/nvme0n1p1")).unwrap();
    assert_eq!(
        super::detect_rotational_in_sysfs(&sys, "/dev/md1"),
        Some(false)
    );

    // VM virtio vda1 -> false
    assert_eq!(
        super::detect_rotational_in_sysfs(&sys, "/dev/vda1"),
        Some(false)
    );

    // Fuente de montaje Btrfs con subvolumen `[/@subvol]` -> false
    assert_eq!(
        super::detect_rotational_in_sysfs(&sys, "/dev/nvme0n1p1[/@subvol]"),
        Some(false)
    );
    assert_eq!(
        super::detect_rotational_in_sysfs(&sys, "/dev/sda1[/@]"),
        Some(true)
    );

    let _ = std::fs::remove_dir_all(&sys);
}

#[test]
fn interpreta_estado_smartctl_y_montaje_ro() {
    assert_eq!(
        super::parse_smartctl_health("SMART overall-health self-assessment test result: PASSED\n"),
        Some(true)
    );
    assert_eq!(
        super::parse_smartctl_health("SMART overall-health self-assessment test result: FAILED!\n"),
        Some(false)
    );
    assert_eq!(
        super::parse_smartctl_health("SMART Health Status: OK\n"),
        Some(true)
    );
    assert_eq!(super::parse_smartctl_health("Device lacks SMART"), None);

    let hc = super::DiskHealthCheck {
        healthy: true,
        smart_available: true,
        detail: "/dev/sda: SMART PASSED".to_string(),
    };
    let hc_json = serde_json::to_string(&hc).expect("serialize DiskHealthCheck");
    let hc_back: super::DiskHealthCheck =
        serde_json::from_str(&hc_json).expect("deserialize DiskHealthCheck");
    assert_eq!(hc, hc_back);

    let wc = super::WriteCacheProbe {
        supported: true,
        enabled: Some(false),
        detail: "desactivada".to_string(),
    };
    let wc_json = serde_json::to_string(&wc).expect("serialize WriteCacheProbe");
    let wc_back: super::WriteCacheProbe =
        serde_json::from_str(&wc_json).expect("deserialize WriteCacheProbe");
    assert_eq!(wc, wc_back);

    let mounts = "/dev/nvme0n1p2 / ext4 ro,relatime 0 0\n\
                      /dev/sda1 /srv/nas btrfs rw,noatime,compress=zstd:3 0 0\n";
    let root_entry = mount_entry_in_proc_mounts(mounts, std::path::Path::new("/etc"));
    assert!(root_entry.is_some_and(|(_, opts)| opts.split(',').any(|f| f == "ro")));
    let data_entry = mount_entry_in_proc_mounts(mounts, std::path::Path::new("/srv/nas/SISTEMAS"));
    assert!(data_entry.is_some_and(|(_, opts)| opts.split(',').any(|f| f == "rw")));
}

#[test]
fn agrupa_pool_btrfs_multidisco_y_marca_todos_los_miembros_en_uso() {
    let sample = r#"{
            "blockdevices": [
                {
                    "name": "nvme0n1",
                    "size": "238.5G",
                    "type": "disk",
                    "mountpoint": null,
                    "fstype": null,
                    "model": "OS NVMe",
                    "rota": false,
                    "uuid": null,
                    "children": [
                        {"name": "nvme0n1p2", "size": "230G", "type": "part", "mountpoint": "/", "fstype": "ext4", "model": null, "rota": false, "uuid": "root-uuid"}
                    ]
                },
                {
                    "name": "sda",
                    "size": "1.8T",
                    "type": "disk",
                    "mountpoint": null,
                    "fstype": null,
                    "model": "HDD A",
                    "rota": true,
                    "uuid": null,
                    "children": [
                        {"name": "sda1", "size": "1.8T", "type": "part", "mountpoint": "/srv/nas", "fstype": "btrfs", "model": null, "rota": true, "uuid": "pool-uuid-123"}
                    ]
                },
                {
                    "name": "sdb",
                    "size": "1.8T",
                    "type": "disk",
                    "mountpoint": null,
                    "fstype": null,
                    "model": "HDD B",
                    "rota": true,
                    "uuid": null,
                    "children": [
                        {"name": "sdb1", "size": "1.8T", "type": "part", "mountpoint": null, "fstype": "btrfs", "model": null, "rota": true, "uuid": "pool-uuid-123"}
                    ]
                },
                {
                    "name": "sdc",
                    "size": "3.6T",
                    "type": "disk",
                    "mountpoint": null,
                    "fstype": null,
                    "model": "HDD Libre",
                    "rota": true,
                    "uuid": null,
                    "children": []
                }
            ]
        }"#;
    let parsed = crate::parsers::lsblk::parse(sample).expect("lsblk");
    let list = compute_disks_from_lsblk(&parsed, Some("nvme0n1"));
    assert_eq!(list.len(), 4);
    assert!(list[0].protected);
    // sda y sdb forman el mismo pool Btrfs montado en /srv/nas
    assert!(list[1].in_use);
    assert!(list[1].is_pool_member);
    assert_eq!(list[1].mountpoint.as_deref(), Some("/srv/nas"));
    assert!(
        list[2].in_use,
        "sdb debe marcarse en uso por compartir UUID Btrfs"
    );
    assert!(list[2].is_pool_member);
    assert_eq!(list[2].mountpoint.as_deref(), Some("/srv/nas"));
    assert_eq!(list[2].pool_uuid.as_deref(), Some("pool-uuid-123"));
    assert_eq!(list[2].pool_members, vec!["/dev/sda1", "/dev/sdb1"]);
    // sdc está libre
    assert!(!list[3].in_use);
    assert!(!list[3].is_pool_member);
}

#[test]
fn parsea_btrfs_show_y_btrfs_df() {
    let show = "\
Label: 'NAS_DATA'  uuid: 07168427-c131-4b01-822d-8ebdcf220d4e
\tTotal devices 2 FS bytes used 123.17GiB
\tdevid    1 size 232.88GiB used 124.00GiB path /dev/sda1
\tdevid    2 size 465.76GiB used 124.00GiB path /dev/sdb1
";
    let pools = super::parse_btrfs_filesystem_show(show);
    assert_eq!(pools.len(), 1);
    assert_eq!(pools[0].label.as_deref(), Some("NAS_DATA"));
    assert_eq!(pools[0].uuid, "07168427-c131-4b01-822d-8ebdcf220d4e");
    assert_eq!(pools[0].total_devices, 2);
    assert_eq!(pools[0].devices, vec!["/dev/sda1", "/dev/sdb1"]);

    let df = "\
Data, single: total=215.73GiB, used=123.17GiB
System, RAID1: total=32.00MiB, used=48.00KiB
Metadata, RAID1: total=2.00GiB, used=1.04GiB
GlobalReserve, single: total=292.42MiB, used=0.00B
";
    let prof = super::parse_btrfs_df(df).expect("btrfs df");
    assert_eq!(prof.data_profile.as_deref(), Some("single"));
    assert_eq!(prof.metadata_profile.as_deref(), Some("RAID1"));
    assert_eq!(prof.system_profile.as_deref(), Some("RAID1"));
}

#[test]
fn audita_fstab_detectando_huerfanos_falta_nofail_y_opciones_peligrosas() {
    let fstab = "\
# /etc/fstab
UUID=root-123 / ext4 errors=remount-ro 0 1
/dev/mapper/old_vg-old_lv /srv/nas ext4 defaults 0 2
UUID=btrfs-456 /srv/nas/unidades/pool2 btrfs rw,noatime,autodefrag,discard=async 0 0
UUID=btrfs-789 /srv/nas/unidades/sano btrfs rw,noatime,compress=zstd:3,nofail,x-systemd.device-timeout=15s 0 0
";
    let audit = super::audit_fstab_content(fstab, &|src| src != "/dev/mapper/old_vg-old_lv");
    assert!(!audit.healthy);
    assert!(audit
        .issues
        .iter()
        .any(|i| i.code == "missing_device" && i.source == "/dev/mapper/old_vg-old_lv"));
    assert!(audit
        .issues
        .iter()
        .any(|i| i.code == "missing_nofail" && i.target == "/srv/nas"));
    assert!(audit
        .issues
        .iter()
        .any(|i| i.code == "missing_nofail" && i.target == "/srv/nas/unidades/pool2"));
    assert!(audit
        .issues
        .iter()
        .any(|i| i.code == "unsafe_mount_option" && i.detail.contains("autodefrag")));
    assert!(audit
        .issues
        .iter()
        .any(|i| i.code == "unsafe_mount_option" && i.detail.contains("discard=async")));
    assert!(!audit
        .issues
        .iter()
        .any(|i| i.target == "/srv/nas/unidades/sano"));
}

#[test]
fn detecta_montajes_exactos_e_hijos_lifo_en_proc_mounts() {
    let mounts = "\
/dev/sda1 /srv/nas btrfs rw,noatime 0 0
/dev/sdb1 /srv/nas/unidades/extra btrfs rw,noatime 0 0
/dev/sdc1 /srv/nas/unidades/extra/sub ext4 rw 0 0
";
    assert!(super::is_mountpoint_in_proc_mounts(
        mounts,
        std::path::Path::new("/srv/nas")
    ));
    assert!(super::is_mountpoint_in_proc_mounts(
        mounts,
        std::path::Path::new("/srv/nas/unidades/extra/")
    ));
    assert!(!super::is_mountpoint_in_proc_mounts(
        mounts,
        std::path::Path::new("/srv/nas/unidades/otro")
    ));
    let children = super::proc_mounts_children_under(mounts, std::path::Path::new("/srv/nas"));
    assert_eq!(
        children,
        vec!["/srv/nas/unidades/extra/sub", "/srv/nas/unidades/extra"]
    );
}

#[test]
fn deduce_disco_padre_en_loop_nbd_y_subvolumen_btrfs() {
    assert_eq!(parent_disk("/dev/loop0p1").as_deref(), Some("loop0"));
    assert_eq!(parent_disk("/dev/loop0").as_deref(), Some("loop0"));
    assert_eq!(parent_disk("/dev/nbd0p2").as_deref(), Some("nbd0"));
    assert_eq!(parent_disk("/dev/sda1[/@subvol]").as_deref(), Some("sda"));
    assert_eq!(
        resolve_target_device(super::clean_block_source("/dev/sda1[/@]")).as_deref(),
        Some("/dev/sda")
    );
}

#[test]
fn fstab_audit_cuenta_problemas_auto_corregibles() {
    let fstab = "UUID=missing-123 /srv/nas/unidades/ext btrfs rw,noatime,nofail,x-systemd.device-timeout=15s 0 0\n";
    let audit = super::audit_fstab_content(fstab, &|_| false);
    assert!(!audit.healthy);
    assert_eq!(audit.issues.len(), 1);
    assert_eq!(audit.issues[0].code, "missing_uuid");
    assert_eq!(audit.fixable_count, 0);
}

#[test]
fn parsea_btrfs_device_stats_y_hdparm_write_cache() {
    let stats = "\
[/dev/sdc1].write_io_errs    0
[/dev/sdc1].read_io_errs     2
[/dev/sdc1].flush_io_errs    0
[/dev/sdc1].corruption_errs  1
[/dev/sdc1].generation_errs  0
";
    let (total, detail) = super::parse_btrfs_device_stats(stats);
    assert_eq!(total, 3);
    assert_eq!(detail.len(), 2);
    assert!(detail[0].contains("read_io_errs: 2"));

    assert_eq!(
        super::parse_hdparm_write_cache("/dev/sdb:\n write-caching =  1 (on)\n"),
        Some(true)
    );
    assert_eq!(
        super::parse_hdparm_write_cache("/dev/sdb:\n write-caching =  0 (off)\n"),
        Some(false)
    );
    assert_eq!(
        super::parse_hdparm_write_cache("/dev/sdb:\n write-caching = not supported\n"),
        None
    );

    let rule = super::render_write_cache_udev_rule(Some("ata-ST2000_ABC"), "sdb", false);
    assert!(rule.contains("ata-ST2000_ABC"));
    assert!(rule.contains("/usr/sbin/hdparm -W0 /dev/%k"));
}

#[test]
fn resuelve_montaje_superior_en_montajes_apilados() {
    assert_eq!(
        super::pools::parse_findmnt_last_line("/dev/nvme0n1p5[/nas]\n/dev/sda1\n/dev/sda1\n")
            .as_deref(),
        Some("/dev/sda1")
    );
    assert_eq!(
        super::pools::parse_findmnt_last_line("ext4\nbtrfs\nbtrfs\n").as_deref(),
        Some("btrfs")
    );

    let mounts = "\
/dev/nvme0n1p2 / ext4 rw,relatime 0 0
/dev/nvme0n1p5 /srv/nas ext4 rw,relatime 0 0
/dev/sda1 /srv/nas btrfs rw,noatime 0 0
";
    assert_eq!(
        super::parsers::mount_entry_in_proc_mounts(mounts, std::path::Path::new("/srv/nas")),
        Some(("/dev/sda1".to_string(), "rw,noatime".to_string()))
    );
}
