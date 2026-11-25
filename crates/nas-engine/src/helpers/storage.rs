//! Helpers privilegiados para gestión de almacenamiento (discos, LVM, Btrfs y montaje).

mod disks;
mod maintenance;
mod pools;
mod units;

#[cfg(test)]
mod tests;

pub use disks::{format_single, is_os_disk, os_disk_guard, os_disks, setup_lvm, wipe_disk};
pub use maintenance::{sanitize_fstab, set_write_cache};
pub use pools::{
    create_pool, extend_pool, extend_pool_target, remove_pool_device, ALLOWED_POOL_MODES,
};
pub use units::{
    create_subvolume, delete_subvolume, mount_unit, scrub_unit_start, scrub_unit_status, trim_unit,
    umount_unit,
};

use disks::{format_partition, mount_options_for, prepare_mountpoint, update_fstab};
use maintenance::{register_btrfs_scrub_for_target, remove_btrfs_scrub_for_unit};
use pools::ensure_device_not_in_active_pool;
