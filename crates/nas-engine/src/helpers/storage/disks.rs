//! Operaciones de particionado, formateo, LVM y protección del disco del sistema.

use std::path::Path;
use std::time::Duration;

use nas_core::{NasError, NasResult};
use nas_exec::{CommandSpec, Runner};

use crate::storage::{check_block_health, is_mountpoint, proc_mounts_children_under};
use crate::storage_ops::{confirm_ok, valid_device, valid_name, valid_size};

use super::maintenance::register_btrfs_scrub_for_target;
use super::pools::ensure_device_not_in_active_pool;

/// Comprueba si el dispositivo de bloque o alguno de sus ancestros contiene el sistema operativo.
pub fn is_os_disk(runner: &Runner, device: &str) -> NasResult<bool> {
    if !valid_device(device) {
        return Ok(false);
    }

    // 1. Obtener ancestros del dispositivo objetivo con lsblk
    let mut target_ancestors = Vec::new();
    let mut cur = device.to_string();
    for _ in 0..8 {
        target_ancestors.push(cur.clone());
        if let Ok(out) = runner.run(
            &CommandSpec::new("lsblk")
                .args(["-no", "PKNAME", &cur])
                .timeout(Duration::from_secs(5)),
        ) {
            let next = out.stdout.trim();
            if next.is_empty() {
                break;
            }
            cur = format!("/dev/{next}");
        } else {
            break;
        }
    }

    // 2. Comprobar contra montajes críticos del sistema operativo
    for mount in ["/", "/boot", "/boot/efi"] {
        if let Ok(out) = runner.run(
            &CommandSpec::new("findmnt")
                .args(["-no", "SOURCE", mount])
                .timeout(Duration::from_secs(5)),
        ) {
            let src = out.stdout.trim();
            if src.is_empty() {
                continue;
            }
            let mut src_cur = src.to_string();
            for _ in 0..8 {
                if target_ancestors.iter().any(|a| a == &src_cur) {
                    return Ok(true);
                }
                if let Ok(pout) = runner.run(
                    &CommandSpec::new("lsblk")
                        .args(["-no", "PKNAME", &src_cur])
                        .timeout(Duration::from_secs(5)),
                ) {
                    let next = pout.stdout.trim();
                    if next.is_empty() {
                        break;
                    }
                    src_cur = format!("/dev/{next}");
                } else {
                    break;
                }
            }
        }
    }

    // 3. Comprobar si el dispositivo tiene algún punto de montaje de sistema
    if let Ok(out) = runner.run(
        &CommandSpec::new("lsblk")
            .args(["-no", "MOUNTPOINT", device])
            .timeout(Duration::from_secs(5)),
    ) {
        for line in out.stdout.lines() {
            let mp = line.trim();
            if matches!(mp, "/" | "/boot" | "/boot/efi" | "[SWAP]") {
                return Ok(true);
            }
        }
    }

    Ok(false)
}

/// Guarda estricta: aborta si el dispositivo pertenece al sistema operativo.
pub fn os_disk_guard(runner: &Runner, device: &str) -> NasResult<()> {
    if is_os_disk(runner, device)? {
        return Err(NasError::Denied(
            "operación prohibida: es el disco del sistema operativo".into(),
        ));
    }
    Ok(())
}

pub(super) fn unmount_device(runner: &Runner, device: &str) {
    let mut mps: Vec<String> = Vec::new();
    if let Ok(out) = runner.run(
        &CommandSpec::new("lsblk")
            .args(["-no", "MOUNTPOINT", device])
            .timeout(Duration::from_secs(10)),
    ) {
        for line in out.stdout.lines() {
            let mp = line.trim();
            if !mp.is_empty() && !mps.iter().any(|m| m == mp) {
                mps.push(mp.to_string());
            }
        }
    }
    // Orden LIFO: desmontar primero rutas más profundas
    mps.sort_by(|a, b| b.len().cmp(&a.len()).then_with(|| b.cmp(a)));
    for mp in mps {
        if let Ok(res) = runner.run(&CommandSpec::new("umount").args([&mp])) {
            if !res.is_success() {
                let _ = runner.run(&CommandSpec::new("umount").args(["-l", &mp]));
            }
        }
    }
}

pub(super) fn partition_for(device: &str) -> String {
    crate::validation::first_partition_path(device)
}

pub(super) fn mount_options_for(runner: &Runner, fstype: &str, device: &str) -> String {
    let is_hdd = crate::storage::detect_device_rotational(runner, device).unwrap_or(true);
    crate::deploy::storage::mount_options_str(fstype, is_hdd)
}

pub(super) fn mount_options_for_devices(
    runner: &Runner,
    fstype: &str,
    devices: &[String],
) -> String {
    let any_hdd = devices
        .iter()
        .any(|dev| crate::storage::detect_device_rotational(runner, dev).unwrap_or(true));
    crate::deploy::storage::mount_options_str(fstype, any_hdd || devices.is_empty())
}

pub(super) fn update_fstab(
    runner: &Runner,
    source: &str,
    target: &str,
    options: &str,
) -> NasResult<()> {
    crate::deploy::update_fstab(runner, source, target, options)
}

pub(super) fn prepare_mountpoint(runner: &Runner, path: &str) {
    let _ = runner.run(&CommandSpec::new("chown").args(["root:grp_samba", path]));
    let _ = runner.run(&CommandSpec::new("chmod").args(["2770", path]));
    let _ = runner.run(&CommandSpec::new("setfacl").args([
        "-m",
        "u:nas-web:rwx,g:grp_samba:rwx,m::rwx,d:u:nas-web:rwx,d:g:grp_samba:rwx,d:m::rwx",
        path,
    ]));
}

pub(super) fn ensure_no_child_mounts_under_data_root() -> NasResult<()> {
    if let Ok(raw) = std::fs::read_to_string("/proc/mounts") {
        let children = proc_mounts_children_under(&raw, Path::new("/srv/nas"));
        if !children.is_empty() {
            return Err(NasError::Denied(format!(
                "existen unidades montadas bajo /srv/nas ({}); desmóntalas en orden LIFO antes de reconfigurar /srv/nas",
                children.join(", ")
            )));
        }
    }
    Ok(())
}

pub(super) fn partition_disk_single_gpt(
    runner: &Runner,
    device: &str,
    fstype: &str,
) -> NasResult<String> {
    let mut target = device.to_string();
    let is_disk = runner
        .run(&CommandSpec::new("lsblk").args(["-nd", "-o", "TYPE", device]))
        .map(|out| out.stdout.trim() == "disk")
        .unwrap_or(false);

    if is_disk {
        let _ = runner.run(&CommandSpec::new("wipefs").args(["-a", device]));
        runner
            .run(&CommandSpec::new("parted").args([
                "-s", device, "mklabel", "gpt", "mkpart", "primary", fstype, "0%", "100%",
            ]))?
            .ensure_success()?;
        let _ = runner.run(&CommandSpec::new("partprobe").args([device]));
        let _ = runner.run(&CommandSpec::new("udevadm").args(["settle"]));
        target = partition_for(device);
    }
    Ok(target)
}

pub(super) fn format_partition(
    runner: &Runner,
    device: &str,
    fstype: &str,
    label: &str,
) -> NasResult<String> {
    if !valid_device(device) {
        return Err(NasError::Invalid("dispositivo inválido".into()));
    }
    if fstype != "ext4" && fstype != "btrfs" {
        return Err(NasError::Invalid(
            "filesystem no soportado (ext4|btrfs)".into(),
        ));
    }
    os_disk_guard(runner, device)?;
    ensure_device_not_in_active_pool(runner, device)?;
    let health = check_block_health(runner, device);
    if !health.healthy {
        return Err(NasError::Denied(health.detail));
    }

    let target = partition_disk_single_gpt(runner, device, fstype)?;

    if fstype == "btrfs" {
        runner
            .run(&CommandSpec::new("mkfs.btrfs").args(["-f", "-L", label, &target]))?
            .ensure_success()?;
    } else {
        runner
            .run(&CommandSpec::new("mkfs.ext4").args(["-F", "-m", "1", "-L", label, &target]))?
            .ensure_success()?;
    }
    let _ = runner.run(&CommandSpec::new("udevadm").args(["settle"]));
    Ok(target)
}

/// Borra las tablas de partición y firmas de un disco.
pub fn wipe_disk(runner: &Runner, confirm: &str, device: &str) -> NasResult<String> {
    if confirm != "SI-DESTRUIR-DATOS" {
        return Err(NasError::Denied(
            "confirmación textual requerida: escribe SI-DESTRUIR-DATOS".into(),
        ));
    }
    if !valid_device(device) {
        return Err(NasError::Invalid("dispositivo inválido".into()));
    }
    os_disk_guard(runner, device)?;
    ensure_device_not_in_active_pool(runner, device)?;
    unmount_device(runner, device);

    runner
        .run(&CommandSpec::new("wipefs").args(["-a", device]))?
        .ensure_success()?;
    Ok(format!("disco {device} limpiado (firmas eliminadas)"))
}

/// Formatea un dispositivo y lo monta en `/srv/nas`.
pub fn format_single(
    runner: &Runner,
    confirm: &str,
    device: &str,
    fstype: Option<&str>,
    unmount: bool,
) -> NasResult<String> {
    if !confirm_ok(confirm) {
        return Err(NasError::Denied(
            "confirmación requerida (SI-FORMATEAR o SI-DESTRUIR-DATOS)".into(),
        ));
    }
    let fstype = fstype.unwrap_or("ext4");
    ensure_no_child_mounts_under_data_root()?;
    if is_mountpoint(Path::new("/srv/nas")) {
        if !unmount {
            return Err(NasError::Denied(
                "/srv/nas ya tiene un volumen montado; habilita el desmontaje para reemplazarlo"
                    .into(),
            ));
        }
        let _ = runner.run(&CommandSpec::new("umount").args(["/srv/nas"]));
    }
    if unmount {
        unmount_device(runner, device);
    }
    let target = format_partition(runner, device, fstype, "NAS_DATA")?;
    let options = mount_options_for(runner, fstype, device);

    let _ = std::fs::create_dir_all("/srv/nas");
    runner
        .run(&CommandSpec::new("mount").args(["-o", &options, &target, "/srv/nas"]))?
        .ensure_success()?;
    prepare_mountpoint(runner, "/srv/nas");
    update_fstab(runner, &target, "/srv/nas", &options)?;
    if fstype == "btrfs" {
        register_btrfs_scrub_for_target("/srv/nas", None);
    }
    if crate::storage::detect_device_rotational(runner, device) == Some(false) {
        let _ = crate::deploy::storage::enable_fstrim(runner);
    }

    Ok(format!(
        "{target} formateado en {fstype} y montado en /srv/nas"
    ))
}

/// Configura LVM sobre un disco y lo monta en `/srv/nas`.
#[allow(clippy::too_many_arguments)]
pub fn setup_lvm(
    runner: &Runner,
    confirm: &str,
    disk: &str,
    vg: &str,
    lv: &str,
    size: Option<&str>,
    fstype: Option<&str>,
    unmount: bool,
) -> NasResult<String> {
    if !confirm_ok(confirm) {
        return Err(NasError::Denied("confirmación requerida".into()));
    }
    if !valid_device(disk) || !valid_name(vg) || !valid_name(lv) {
        return Err(NasError::Invalid(
            "argumentos de dispositivo o volumen inválidos".into(),
        ));
    }
    let size = size.unwrap_or("100%FREE");
    if !valid_size(size) {
        return Err(NasError::Invalid("tamaño de LV inválido".into()));
    }
    let fstype = fstype.unwrap_or("ext4");
    os_disk_guard(runner, disk)?;
    ensure_device_not_in_active_pool(runner, disk)?;
    ensure_no_child_mounts_under_data_root()?;
    let health = check_block_health(runner, disk);
    if !health.healthy {
        return Err(NasError::Denied(health.detail));
    }
    if is_mountpoint(Path::new("/srv/nas")) {
        if !unmount {
            return Err(NasError::Denied(
                "/srv/nas ya tiene un volumen montado; habilita el desmontaje para continuar"
                    .into(),
            ));
        }
        let _ = runner.run(&CommandSpec::new("umount").args(["/srv/nas"]));
    }

    if unmount {
        unmount_device(runner, disk);
    }

    if let Ok(out) = runner.run(
        &CommandSpec::new("pvs")
            .args(["--noheadings", "-o", "vg_name", disk])
            .timeout(Duration::from_secs(10)),
    ) {
        for line in out.stdout.lines() {
            let existing_vg = line.trim();
            if !existing_vg.is_empty() {
                let _ = runner.run(&CommandSpec::new("vgchange").args(["-an", existing_vg]));
                let _ = runner.run(&CommandSpec::new("vgremove").args(["-y", existing_vg]));
            }
        }
    }
    let _ = runner.run(&CommandSpec::new("vgchange").args(["-an", vg]));
    let _ = runner.run(&CommandSpec::new("vgremove").args(["-y", vg]));

    let _ = runner.run(&CommandSpec::new("wipefs").args(["-a", disk]));
    let _ = runner.run(&CommandSpec::new("partprobe").args([disk]));
    let _ = runner.run(&CommandSpec::new("udevadm").args(["settle"]));

    runner
        .run(&CommandSpec::new("pvcreate").args(["-y", "-ff", disk]))?
        .ensure_success()?;
    let vg_exists = runner
        .run(&CommandSpec::new("vgs").args([vg]))
        .map(|o| o.is_success())
        .unwrap_or(false);
    if vg_exists {
        runner
            .run(&CommandSpec::new("vgextend").args([vg, disk]))?
            .ensure_success()?;
    } else {
        runner
            .run(&CommandSpec::new("vgcreate").args([vg, disk]))?
            .ensure_success()?;
    }
    let size_flag = if size.contains('%') { "-l" } else { "-L" };
    runner
        .run(&CommandSpec::new("lvcreate").args(["-y", "-n", lv, size_flag, size, vg]))?
        .ensure_success()?;

    let path_lv = format!("/dev/{vg}/{lv}");
    let options = mount_options_for(runner, fstype, &path_lv);
    if fstype == "btrfs" {
        runner
            .run(&CommandSpec::new("mkfs.btrfs").args(["-f", "-L", "NAS_DATA", &path_lv]))?
            .ensure_success()?;
    } else {
        runner
            .run(
                &CommandSpec::new("mkfs.ext4").args(["-F", "-m", "1", "-L", "NAS_DATA", &path_lv]),
            )?
            .ensure_success()?;
    }

    let _ = std::fs::create_dir_all("/srv/nas");
    runner
        .run(&CommandSpec::new("mount").args(["-o", &options, &path_lv, "/srv/nas"]))?
        .ensure_success()?;
    prepare_mountpoint(runner, "/srv/nas");
    update_fstab(runner, &path_lv, "/srv/nas", &options)?;
    if fstype == "btrfs" {
        register_btrfs_scrub_for_target("/srv/nas", None);
    }

    Ok(format!("volumen {path_lv} montado en /srv/nas"))
}
