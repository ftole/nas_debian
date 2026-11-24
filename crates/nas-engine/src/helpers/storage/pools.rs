//! Gestión de pools Btrfs (creación, extensión y retiro de miembros).

use std::path::Path;
use std::time::Duration;

use nas_core::{NasError, NasResult};
use nas_exec::{CommandSpec, Runner};

use crate::storage::{
    btrfs_pools_from_lsblk, check_block_health, is_mountpoint, lsblk_devices_with_uuid,
    path_is_mounted_readonly,
};
use crate::storage_ops::{confirm_ok, valid_device, valid_name};

use super::disks::{
    ensure_no_child_mounts_under_data_root, mount_options_for_devices, os_disk_guard,
    partition_disk_single_gpt, prepare_mountpoint, unmount_device, update_fstab,
};
use super::maintenance::register_btrfs_scrub_for_target;

fn last_non_empty_line(raw: &str) -> String {
    raw.lines()
        .rev()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("")
        .to_string()
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

pub(super) fn resolve_pool_target(target: &str) -> NasResult<(String, Option<String>)> {
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
        .map(|o| last_non_empty_line(&o.stdout))
        .unwrap_or_default();
    let fstype = runner
        .run(&CommandSpec::new("findmnt").args(["-no", "FSTYPE", "--target", &mount_dir]))
        .map(|o| last_non_empty_line(&o.stdout))
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
        .map(|o| last_non_empty_line(&o.stdout))
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
        .map(|o| last_non_empty_line(&o.stdout))
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
