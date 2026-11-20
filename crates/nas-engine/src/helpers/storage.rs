//! Helpers para operaciones privilegiadas sobre dispositivos de almacenamiento.

use std::path::Path;
use std::time::Duration;

use nas_core::{NasError, NasResult};
use nas_exec::{CommandSpec, Runner};

use crate::storage::{
    btrfs_pools_from_lsblk, check_block_health, is_mountpoint, lsblk_devices_with_uuid,
    path_is_mounted_readonly, proc_mounts_children_under,
};
use crate::storage_ops::{confirm_ok, valid_device, valid_name, valid_size};

mod units;

pub use units::{
    create_subvolume, delete_subvolume, mount_unit, scrub_unit_start, scrub_unit_status, trim_unit,
    umount_unit,
};

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

fn unmount_device(runner: &Runner, device: &str) {
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

fn partition_for(device: &str) -> String {
    let dev = device.trim_end_matches('/');
    if dev.ends_with(|c: char| c.is_ascii_digit()) {
        format!("{dev}p1")
    } else {
        format!("{dev}1")
    }
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
        "u:nas-web:rwx,g:grp_samba:rwx,d:u:nas-web:rwx,d:g:grp_samba:rwx",
        path,
    ]));
}

pub(super) fn check_device_not_in_btrfs_pools(
    runner: &Runner,
    pools: &[crate::storage::BtrfsPoolInfo],
    device: &str,
) -> NasResult<()> {
    for pool in pools {
        if pool.mountpoint.is_none() {
            continue;
        }
        let matches = pool
            .devices
            .iter()
            .any(|member| crate::storage_ops::device_matches_pool_member(runner, member, device));
        if matches {
            return Err(NasError::Denied(format!(
                "operación prohibida: {device} forma parte del pool Btrfs activo en {} (UUID {})",
                pool.mountpoint.as_deref().unwrap_or_default(),
                pool.uuid
            )));
        }
    }
    Ok(())
}

pub(super) fn ensure_device_not_in_active_pool(runner: &Runner, device: &str) -> NasResult<()> {
    if let Ok(lsblk_devs) = lsblk_devices_with_uuid(runner) {
        let pools = btrfs_pools_from_lsblk(&lsblk_devs);
        check_device_not_in_btrfs_pools(runner, &pools, device)?;
    }
    Ok(())
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

pub(super) fn register_btrfs_scrub_for_target(target: &str, unit_id: Option<&str>) {
    let (cron_path, minute_hour) = match unit_id {
        Some(id) if valid_name(id) => (
            format!("/etc/cron.d/nas-btrfs-scrub-unit-{id}"),
            "0 3 1 * *",
        ),
        _ => ("/etc/cron.d/nas-btrfs-scrub".to_string(), "0 2 1 * *"),
    };
    let content = format!(
        "SHELL=/bin/bash\nPATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin\n{minute_hour} root nice -n 19 ionice -c 3 btrfs scrub start -B {target} >/dev/null 2>&1\n"
    );
    let _ = std::fs::write(&cron_path, content);
}

pub(super) fn remove_btrfs_scrub_for_unit(unit_id: &str) {
    if valid_name(unit_id) {
        let cron_path = format!("/etc/cron.d/nas-btrfs-scrub-unit-{unit_id}");
        let _ = std::fs::remove_file(cron_path);
    }
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

fn resolve_pool_target(target: &str) -> NasResult<(String, Option<String>)> {
    let clean = target.trim();
    if clean.is_empty() || clean == "principal" || clean == "/srv/nas" {
        Ok(("/srv/nas".to_string(), None))
    } else if valid_name(clean) {
        Ok((
            format!("/srv/nas/unidades/{clean}"),
            Some(clean.to_string()),
        ))
    } else {
        Err(NasError::Invalid(
            "destino de pool inválido (use 'principal' o identificador de unidad)".into(),
        ))
    }
}

/// Modos de creación de pool soportados.
pub const ALLOWED_POOL_MODES: [&str; 5] = [
    "btrfs_single",
    "btrfs_raid1",
    "ext4",
    "lvm_ext4",
    "lvm_btrfs",
];

/// Crea un nuevo pool (1 o N discos) en `/srv/nas` (`principal`) o en `/srv/nas/unidades/<id>`.
pub fn create_pool(
    runner: &Runner,
    confirm: &str,
    target: &str,
    mode: &str,
    devices: &[String],
    unmount: bool,
) -> NasResult<String> {
    if !confirm_ok(confirm) {
        return Err(NasError::Denied(
            "confirmación requerida (SI-FORMATEAR o SI-DESTRUIR-DATOS)".into(),
        ));
    }
    let (mount_dir, unit_opt) = resolve_pool_target(target)?;
    let mode = mode.trim().to_ascii_lowercase();
    if !ALLOWED_POOL_MODES.contains(&mode.as_str()) {
        return Err(NasError::Invalid(format!(
            "modo de pool no soportado ({mode})"
        )));
    }
    if devices.is_empty() {
        return Err(NasError::Invalid("selecciona al menos un disco".into()));
    }
    if mode == "ext4" && devices.len() != 1 {
        return Err(NasError::Invalid(
            "ext4 directo solo admite 1 disco; elige Btrfs o LVM para combinar varios discos"
                .into(),
        ));
    }
    if mode == "btrfs_raid1" && devices.len() < 2 {
        return Err(NasError::Invalid(
            "Btrfs RAID1 (espejo) requiere al menos 2 discos".into(),
        ));
    }

    let mut clean_devs: Vec<String> = Vec::with_capacity(devices.len());
    for raw in devices {
        let dev = raw.trim().to_string();
        if !valid_device(&dev) {
            return Err(NasError::Invalid(format!("dispositivo inválido: {dev}")));
        }
        if clean_devs.contains(&dev) {
            return Err(NasError::Invalid(format!(
                "dispositivo duplicado en la selección: {dev}"
            )));
        }
        os_disk_guard(runner, &dev)?;
        ensure_device_not_in_active_pool(runner, &dev)?;
        let health = check_block_health(runner, &dev);
        if !health.healthy {
            return Err(NasError::Denied(health.detail));
        }
        if !unmount {
            let mps = crate::storage_ops::mount_points(runner, &dev);
            if !mps.is_empty() {
                return Err(NasError::Denied(format!(
                    "el dispositivo {dev} está montado en {}; desmóntalo antes de usarlo",
                    mps.join(", ")
                )));
            }
        }
        clean_devs.push(dev);
    }

    if unit_opt.is_some() {
        if is_mountpoint(Path::new(&mount_dir)) {
            return Err(NasError::Denied(format!(
                "ya existe un volumen montado en {mount_dir} (bloqueo anti-overmount)"
            )));
        }
    } else {
        ensure_no_child_mounts_under_data_root()?;
        if is_mountpoint(Path::new(&mount_dir)) {
            if !unmount {
                return Err(NasError::Denied(
                    "/srv/nas ya tiene un volumen montado; habilita el desmontaje o crea una unidad adicional".into(),
                ));
            }
            let umount_res = runner.run(&CommandSpec::new("umount").args([&mount_dir]))?;
            if !umount_res.is_success() || is_mountpoint(Path::new(&mount_dir)) {
                let detail = if umount_res.stderr.trim().is_empty() {
                    umount_res.stdout.trim()
                } else {
                    umount_res.stderr.trim()
                };
                return Err(NasError::Denied(format!(
                    "no se pudo desmontar {mount_dir} antes de crear el pool: {detail}"
                )));
            }
        }
    }

    if unmount {
        for dev in &clean_devs {
            unmount_device(runner, dev);
        }
    }

    let label = match &unit_opt {
        Some(id) => format!(
            "NAS_U_{}",
            id.to_ascii_uppercase().chars().take(10).collect::<String>()
        ),
        None => "NAS_DATA".to_string(),
    };

    let (mount_src, fstype) = match mode.as_str() {
        "ext4" => {
            let part = partition_disk_single_gpt(runner, &clean_devs[0], "ext4")?;
            runner
                .run(&CommandSpec::new("mkfs.ext4").args(["-F", "-m", "1", "-L", &label, &part]))?
                .ensure_success()?;
            let _ = runner.run(&CommandSpec::new("udevadm").args(["settle"]));
            (part, "ext4")
        }
        "btrfs_single" | "btrfs_raid1" => {
            let mut parts = Vec::with_capacity(clean_devs.len());
            for dev in &clean_devs {
                parts.push(partition_disk_single_gpt(runner, dev, "btrfs")?);
            }
            let mut args = vec!["-f".to_string(), "-L".to_string(), label.clone()];
            if mode == "btrfs_raid1" {
                args.extend(["-d".into(), "raid1".into(), "-m".into(), "raid1".into()]);
            } else if parts.len() >= 2 {
                args.extend(["-d".into(), "single".into(), "-m".into(), "raid1".into()]);
            }
            args.extend(parts.iter().cloned());
            runner
                .run(
                    &CommandSpec::new("mkfs.btrfs")
                        .args(args)
                        .timeout(Duration::from_secs(300)),
                )?
                .ensure_success()?;
            let _ = runner.run(&CommandSpec::new("udevadm").args(["settle"]));
            let _ = runner.run(&CommandSpec::new("btrfs").args(["device", "scan"]));
            (parts[0].clone(), "btrfs")
        }
        "lvm_ext4" | "lvm_btrfs" => {
            let fs = if mode == "lvm_btrfs" { "btrfs" } else { "ext4" };
            let vg = match &unit_opt {
                Some(id) => format!("nas_vg_{id}"),
                None => "nas_vg".to_string(),
            };
            let lv = "nas_lv".to_string();
            for dev in &clean_devs {
                if let Ok(out) = runner.run(
                    &CommandSpec::new("pvs")
                        .args(["--noheadings", "-o", "vg_name", dev])
                        .timeout(Duration::from_secs(10)),
                ) {
                    for line in out.stdout.lines() {
                        let existing_vg = line.trim();
                        if !existing_vg.is_empty() {
                            let _ = runner
                                .run(&CommandSpec::new("vgchange").args(["-an", existing_vg]));
                            let _ =
                                runner.run(&CommandSpec::new("vgremove").args(["-y", existing_vg]));
                        }
                    }
                }
            }
            if runner
                .run(&CommandSpec::new("vgs").args([&vg]))
                .is_ok_and(|o| o.is_success())
            {
                return Err(NasError::Denied(format!(
                    "el Volume Group {vg} ya existe en otros discos; usa 'Ampliar' o elige otro identificador"
                )));
            }
            for dev in &clean_devs {
                let _ = runner.run(&CommandSpec::new("wipefs").args(["-a", dev]));
                let _ = runner.run(&CommandSpec::new("partprobe").args([dev]));
            }
            let _ = runner.run(&CommandSpec::new("udevadm").args(["settle"]));
            for dev in &clean_devs {
                runner
                    .run(&CommandSpec::new("pvcreate").args(["-y", "-ff", dev]))?
                    .ensure_success()?;
            }
            let mut vg_args = vec![vg.clone()];
            vg_args.extend(clean_devs.iter().cloned());
            runner
                .run(&CommandSpec::new("vgcreate").args(vg_args))?
                .ensure_success()?;
            runner
                .run(&CommandSpec::new("lvcreate").args(["-y", "-n", &lv, "-l", "100%FREE", &vg]))?
                .ensure_success()?;
            let path_lv = format!("/dev/{vg}/{lv}");
            if fs == "btrfs" {
                runner
                    .run(&CommandSpec::new("mkfs.btrfs").args(["-f", "-L", &label, &path_lv]))?
                    .ensure_success()?;
            } else {
                runner
                    .run(
                        &CommandSpec::new("mkfs.ext4")
                            .args(["-F", "-m", "1", "-L", &label, &path_lv]),
                    )?
                    .ensure_success()?;
            }
            (path_lv, fs)
        }
        _ => unreachable!(),
    };

    if unit_opt.is_some() {
        let unidades_root = "/srv/nas/unidades";
        let _ = std::fs::create_dir_all(unidades_root);
        prepare_mountpoint(runner, unidades_root);
    }
    let _ = std::fs::create_dir_all(&mount_dir);
    let options = mount_options_for_devices(runner, fstype, &clean_devs);
    runner
        .run(&CommandSpec::new("mount").args(["-o", &options, &mount_src, &mount_dir]))?
        .ensure_success()?;
    prepare_mountpoint(runner, &mount_dir);
    update_fstab(runner, &mount_src, &mount_dir, &options)?;

    if fstype == "btrfs" {
        register_btrfs_scrub_for_target(&mount_dir, unit_opt.as_deref());
    }
    let all_ssd = clean_devs
        .iter()
        .all(|dev| crate::storage::detect_device_rotational(runner, dev) == Some(false));
    if all_ssd {
        let _ = crate::deploy::storage::enable_fstrim(runner);
    }

    Ok(format!(
        "pool {mode} creado con {} disco(s) ({}) y montado en {mount_dir}",
        clean_devs.len(),
        clean_devs.join(", ")
    ))
}

/// Extiende el pool de `/srv/nas` (LVM o Btrfs) con un nuevo disco.
pub fn extend_pool(runner: &Runner, confirm: &str, device: &str) -> NasResult<String> {
    extend_pool_target(runner, confirm, "principal", device, false)
}

/// Extiende un pool existente (`/srv/nas` o `/srv/nas/unidades/<id>`), soportando tanto LVM como Btrfs nativo.
pub fn extend_pool_target(
    runner: &Runner,
    confirm: &str,
    target: &str,
    device: &str,
    convert_metadata_raid1: bool,
) -> NasResult<String> {
    if !confirm_ok(confirm) {
        return Err(NasError::Denied("confirmación requerida".into()));
    }
    if !valid_device(device) {
        return Err(NasError::Invalid("dispositivo inválido".into()));
    }
    let (mount_dir, _) = resolve_pool_target(target)?;
    if !is_mountpoint(Path::new(&mount_dir)) {
        return Err(NasError::Denied(format!(
            "{mount_dir} no es un punto de montaje activo; no se puede ampliar sobre la partición raíz (/)"
        )));
    }
    if path_is_mounted_readonly(Path::new(&mount_dir)) {
        return Err(NasError::Denied(format!(
            "{mount_dir} está montado en solo lectura (ro); repara el volumen antes de ampliarlo"
        )));
    }

    os_disk_guard(runner, device)?;
    ensure_device_not_in_active_pool(runner, device)?;
    let health = check_block_health(runner, device);
    if !health.healthy {
        return Err(NasError::Denied(health.detail));
    }
    let mps = crate::storage_ops::mount_points(runner, device);
    if !mps.is_empty() {
        return Err(NasError::Denied(format!(
            "el dispositivo {device} ya está montado en {}",
            mps.join(", ")
        )));
    }

    let source = runner
        .run(&CommandSpec::new("findmnt").args(["-no", "SOURCE", "--target", &mount_dir]))
        .map(|o| o.stdout.trim().to_string())
        .unwrap_or_default();
    let fstype = runner
        .run(&CommandSpec::new("findmnt").args(["-no", "FSTYPE", "--target", &mount_dir]))
        .map(|o| o.stdout.trim().to_string())
        .unwrap_or_default();

    let clean_source = source.split('[').next().unwrap_or(&source).trim();
    let vg = runner
        .run(&CommandSpec::new("lvs").args(["--noheadings", "-o", "vg_name", clean_source]))
        .map(|o| o.stdout.trim().to_string())
        .unwrap_or_default();

    if !vg.is_empty() {
        runner
            .run(&CommandSpec::new("pvcreate").args(["-y", "-ff", device]))?
            .ensure_success()?;
        runner
            .run(&CommandSpec::new("vgextend").args([&vg, device]))?
            .ensure_success()?;
        runner
            .run(&CommandSpec::new("lvextend").args(["-l", "+100%FREE", clean_source]))?
            .ensure_success()?;

        if fstype.eq_ignore_ascii_case("btrfs") {
            runner
                .run(&CommandSpec::new("btrfs").args(["filesystem", "resize", "max", &mount_dir]))?
                .ensure_success()?;
        } else {
            runner
                .run(&CommandSpec::new("resize2fs").args([clean_source]))?
                .ensure_success()?;
        }

        return Ok(format!(
            "pool LVM {vg} ({mount_dir}) extendido con {device}"
        ));
    }

    if fstype.eq_ignore_ascii_case("btrfs") {
        let part = partition_disk_single_gpt(runner, device, "btrfs")?;
        runner
            .run(
                &CommandSpec::new("btrfs")
                    .args(["device", "add", "-f", &part, &mount_dir])
                    .timeout(Duration::from_secs(300)),
            )?
            .ensure_success()?;
        let mut extra_note = String::new();
        if convert_metadata_raid1 {
            match runner.run(
                &CommandSpec::new("btrfs")
                    .args([
                        "balance",
                        "start",
                        "-mconvert=raid1",
                        "-sconvert=raid1",
                        "-f",
                        &mount_dir,
                    ])
                    .timeout(Duration::from_secs(600)),
            ) {
                Ok(out) if out.is_success() => {
                    extra_note = " (metadatos/sistema convertidos a RAID1)".to_string();
                }
                Ok(out) => {
                    let detail = if out.stderr.trim().is_empty() {
                        out.stdout.trim()
                    } else {
                        out.stderr.trim()
                    };
                    extra_note = format!(
                        " (aviso: no se pudo completar conversión RAID1 de metadatos: {detail})"
                    );
                }
                Err(e) => {
                    extra_note =
                        format!(" (aviso: no se pudo ejecutar balance RAID1 de metadatos: {e})");
                }
            }
        }
        let _ = runner.run(&CommandSpec::new("btrfs").args(["device", "scan"]));
        return Ok(format!(
            "pool Btrfs en {mount_dir} ampliado en línea con {part}{extra_note}"
        ));
    }

    Err(NasError::Invalid(format!(
        "{mount_dir} ({fstype}) no es LVM ni Btrfs; solo los pools LVM o Btrfs admiten ampliación"
    )))
}

/// Retira de forma segura un disco/partición miembro de un pool Btrfs multi-dispositivo (`btrfs device remove`).
pub fn remove_pool_device(
    runner: &Runner,
    confirm: &str,
    target: &str,
    device: &str,
) -> NasResult<String> {
    if !confirm_ok(confirm) {
        return Err(NasError::Denied(
            "confirmación requerida (SI-FORMATEAR o SI-DESTRUIR-DATOS)".into(),
        ));
    }
    if !valid_device(device) {
        return Err(NasError::Invalid("dispositivo inválido".into()));
    }
    let (mount_dir, _) = resolve_pool_target(target)?;
    if !is_mountpoint(Path::new(&mount_dir)) {
        return Err(NasError::NotFound(format!(
            "{mount_dir} no está montado actualmente"
        )));
    }
    if path_is_mounted_readonly(Path::new(&mount_dir)) {
        return Err(NasError::Denied(format!(
            "{mount_dir} está montado en solo lectura (ro); no se puede retirar un disco"
        )));
    }
    let fstype = runner
        .run(&CommandSpec::new("findmnt").args(["-no", "FSTYPE", "--target", &mount_dir]))
        .map(|o| o.stdout.trim().to_string())
        .unwrap_or_default();
    if !fstype.eq_ignore_ascii_case("btrfs") {
        return Err(NasError::Invalid(format!(
            "{mount_dir} ({fstype}) no es un pool Btrfs; solo Btrfs admite retirar miembros en línea"
        )));
    }

    let lsblk_devs = lsblk_devices_with_uuid(runner)?;
    let pools = btrfs_pools_from_lsblk(&lsblk_devs);
    let source = runner
        .run(&CommandSpec::new("findmnt").args(["-no", "SOURCE", "--target", &mount_dir]))
        .map(|o| o.stdout.trim().to_string())
        .unwrap_or_default();
    let clean_source = source.split('[').next().unwrap_or(&source).trim();

    let pool = pools
        .into_iter()
        .find(|p| {
            p.mountpoint.as_deref() == Some(mount_dir.as_str())
                || (!clean_source.is_empty() && p.devices.iter().any(|d| d == clean_source))
        })
        .ok_or_else(|| {
            NasError::NotFound(format!(
                "no se pudo resolver la topología Btrfs para {mount_dir}"
            ))
        })?;

    if pool.devices.len() <= 1 {
        return Err(NasError::Denied(
            "el pool solo tiene 1 disco; usa Desmontar en lugar de retirar el último miembro"
                .into(),
        ));
    }

    let member = pool
        .devices
        .iter()
        .find(|m| {
            *m == device || crate::storage_ops::parent_disk(runner, m).as_deref() == Some(device)
        })
        .cloned()
        .ok_or_else(|| {
            NasError::Invalid(format!(
                "{device} no forma parte del pool Btrfs montado en {mount_dir}"
            ))
        })?;

    runner
        .run(
            &CommandSpec::new("btrfs")
                .args(["device", "remove", &member, &mount_dir])
                .timeout(Duration::from_secs(1800)),
        )?
        .ensure_success()?;
    let _ = runner.run(&CommandSpec::new("btrfs").args(["device", "scan"]));

    // Si /etc/fstab apuntaba literalmente al dispositivo retirado, reescribir con el UUID del pool usando un miembro restante.
    if let Some(remaining) = pool.devices.iter().find(|m| *m != &member) {
        if let Ok(fstab_raw) = std::fs::read_to_string("/etc/fstab") {
            let needs_fstab_update = fstab_raw.lines().any(|line| {
                let parts: Vec<&str> = line.split_whitespace().collect();
                parts.len() >= 2 && parts[1] == mount_dir && parts[0] == member
            });
            if needs_fstab_update {
                let is_hdd =
                    crate::storage::detect_device_rotational(runner, remaining).unwrap_or(true);
                let opts = crate::deploy::storage::mount_options_str("btrfs", is_hdd);
                let _ = update_fstab(runner, remaining, &mount_dir, &opts);
            }
        }
    }

    Ok(format!(
        "dispositivo {member} retirado de forma segura del pool {mount_dir}"
    ))
}

/// Sanea `/etc/fstab` retirando entradas `/dev/...` huérfanas y normalizando `nofail` en `/srv/nas*`.
pub fn sanitize_fstab(runner: &Runner, confirm: &str) -> NasResult<String> {
    if !confirm_ok(confirm) && confirm != "SI-SANEAR" {
        return Err(NasError::Denied(
            "confirmación requerida (SI-FORMATEAR o SI-SANEAR)".into(),
        ));
    }
    let fstab_path = Path::new("/etc/fstab");
    if !fstab_path.exists() {
        return Ok("no existe /etc/fstab".into());
    }
    let content = std::fs::read_to_string(fstab_path).map_err(|e| NasError::io(fstab_path, e))?;
    let (cleaned, changes) =
        crate::deploy::sanitize_fstab_content(&content, &|src| Path::new(src).exists());
    if changes == 0 {
        return Ok("/etc/fstab ya está saneado (sin cambios)".into());
    }
    if let Ok(staged) = crate::deploy::write_unique_temp_file("nas-fstab-verify", &cleaned) {
        let pre = runner.run(
            &CommandSpec::new("findmnt")
                .args(["--verify", "--tab-file", &staged.to_string_lossy()])
                .timeout(Duration::from_secs(10)),
        );
        let _ = std::fs::remove_file(&staged);
        if let Ok(out) = pre {
            let combined = format!("{}\n{}", out.stdout, out.stderr);
            if !out.is_success() && combined.to_ascii_lowercase().contains("parse error") {
                return Err(NasError::Invalid(format!(
                    "findmnt --verify rechazó la sintaxis propuesta de fstab: {}",
                    combined.trim()
                )));
            }
        }
    }
    crate::deploy::install_fstab_content(runner, &cleaned)?;
    let verify_note = match runner.run(
        &CommandSpec::new("findmnt")
            .args(["--verify", "--fstab"])
            .timeout(Duration::from_secs(10)),
    ) {
        Ok(out) if out.is_success() => String::new(),
        Ok(out) => {
            let detail = if out.stderr.trim().is_empty() {
                out.stdout.trim()
            } else {
                out.stderr.trim()
            };
            format!(" (aviso findmnt --verify: {detail})")
        }
        Err(_) => String::new(),
    };
    Ok(format!(
        "/etc/fstab saneado ({changes} corrección(es), respaldo previo guardado){verify_note}"
    ))
}

fn find_by_id_basename_for_disk(disk_path: &str) -> Option<String> {
    let target_canon = std::fs::canonicalize(disk_path).ok()?;
    let entries = std::fs::read_dir("/dev/disk/by-id").ok()?;
    let mut best: Option<String> = None;
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if name.contains("-part") {
            continue;
        }
        if let Ok(resolved) = std::fs::canonicalize(entry.path()) {
            if resolved == target_canon {
                if name.starts_with("wwn-") || name.starts_with("nvme-eui.") {
                    return Some(name);
                }
                if best.is_none() {
                    best = Some(name);
                }
            }
        }
    }
    best
}

/// Activa o desactiva la caché volátil de escritura de un disco (`hdparm -W0`/`-W1`)
/// y persiste una regla udev para mantener la configuración tras cada reinicio.
pub fn set_write_cache(
    runner: &Runner,
    confirm: &str,
    device: &str,
    enabled: bool,
) -> NasResult<String> {
    if !confirm_ok(confirm) && confirm != "SI-APLICAR" {
        return Err(NasError::Denied(
            "confirmación requerida (SI-APLICAR o SI-FORMATEAR)".into(),
        ));
    }
    if !valid_device(device) {
        return Err(NasError::Invalid("dispositivo inválido".into()));
    }
    let disk_path = crate::storage_ops::parent_disk(runner, device)
        .unwrap_or_else(|| device.trim().to_string());
    if !valid_device(&disk_path) {
        return Err(NasError::Invalid("disco físico inválido".into()));
    }
    let disk_name = disk_path.trim_start_matches("/dev/");
    if disk_name.is_empty() || disk_name.contains('/') {
        return Err(NasError::Invalid(
            "solo los discos físicos directos admiten control hdparm write-cache".into(),
        ));
    }

    crate::deploy::deps::ensure_packages(runner, &["hdparm"], false)?;

    let flag = if enabled { "-W1" } else { "-W0" };
    runner
        .run(
            &CommandSpec::new("hdparm")
                .args([flag, &disk_path])
                .timeout(Duration::from_secs(10)),
        )?
        .ensure_success()?;

    let by_id = find_by_id_basename_for_disk(&disk_path);
    let rule_content =
        crate::storage::render_write_cache_udev_rule(by_id.as_deref(), disk_name, enabled);
    let rule_path = format!("/etc/udev/rules.d/65-nas-writecache-{disk_name}.rules");
    std::fs::create_dir_all("/etc/udev/rules.d")
        .map_err(|e| NasError::io(Path::new("/etc/udev/rules.d"), e))?;
    std::fs::write(&rule_path, rule_content).map_err(|e| NasError::io(Path::new(&rule_path), e))?;
    let _ = runner.run(&CommandSpec::new("udevadm").args(["control", "--reload-rules"]));

    let mode_text = if enabled {
        "ACTIVADA (rendimiento estándar)"
    } else {
        "DESACTIVADA (protección máxima ante cortes)"
    };
    Ok(format!(
        "caché volátil de {disk_path} {mode_text}; regla persistente en {rule_path}"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
