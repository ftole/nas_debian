//! Inventario de almacenamiento: uso, discos e inspección.

mod health;
mod parsers;
mod pools;
mod write_cache;

#[cfg(test)]
mod tests;

pub use health::{
    check_block_health, check_block_health_direct, check_path_storage_health,
    detect_device_rotational, detect_path_rotational, detect_rotational_in_sysfs,
    parse_rotational_flag, parse_smartctl_health, DiskHealthCheck,
};
pub use parsers::{
    audit_fstab_content, audit_system_fstab, btrfs_pools_from_lsblk, is_mountpoint,
    is_mountpoint_in_proc_mounts, parse_btrfs_device_stats, parse_btrfs_df,
    parse_btrfs_filesystem_show, parse_df_full, path_is_mounted_readonly,
    proc_mounts_children_under, BtrfsPoolInfo, BtrfsProfileInfo, DfFull, FstabAudit, FstabIssue,
};
pub use pools::{
    collect_btrfs_device_errors, disks, inspect, is_path_on_os_disk, lsblk_devices_with_uuid,
    overview, overview_for, Disk, StorageOverview,
};
pub use write_cache::{
    inspect_write_cache, inspect_write_cache_direct, parse_hdparm_write_cache,
    render_write_cache_udev_rule, WriteCacheProbe,
};

/// Valida que el dispositivo sea una ruta `/dev/...` simple.
pub fn is_valid_device(device: &str) -> bool {
    crate::validation::is_valid_device_path(device)
}

pub(super) fn clean_block_source(device: &str) -> &str {
    crate::validation::clean_block_source(device)
}

pub(super) fn parent_disk(device: &str) -> Option<String> {
    crate::validation::parent_disk_name(device)
}
