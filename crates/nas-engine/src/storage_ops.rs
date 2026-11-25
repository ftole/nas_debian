//! Operaciones privilegiadas de almacenamiento expuestas por el panel.
//!
//! La validación (confirmación textual, dispositivo, filesystem, disco del SO,
//! volúmenes en uso) se hace aquí; la ejecución se delega en el helper root
//! `nas-storage-apply`, autorizado de forma acotada en `/etc/sudoers.d/nas-web-rust`.

use nas_exec::{CommandSpec, Runner, SudoMode};

/// Filesystems admitidos por el panel.
pub const ALLOWED_FILESYSTEMS: [&str; 2] = ["ext4", "btrfs"];

/// Helper root con lista blanca de operaciones.
pub const HELPER: &str = "/usr/local/bin/nas";

/// Valida la confirmación textual del usuario.
pub fn confirm_ok(confirm: &str) -> bool {
    matches!(confirm, "SI-FORMATEAR" | "SI-DESTRUIR-DATOS")
}

/// Valida un dispositivo de bloque (`/dev/sdb`, `/dev/nvme0n1p1`, `/dev/mapper/vg-lv`).
pub fn valid_device(device: &str) -> bool {
    crate::validation::is_valid_device_path(device)
}

/// Valida un identificador corto (unidad, VG, LV, subvolumen).
pub fn valid_name(value: &str) -> bool {
    let value = value.trim();
    !value.is_empty()
        && value.len() <= 32
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
}

/// Valida el tamaño de un LV (`100%FREE`, `20G`, `512M`, `100%free`, `50g`).
pub fn valid_size(size: &str) -> bool {
    let size = size.trim().to_ascii_uppercase();
    !size.is_empty()
        && size.len() <= 12
        && size.chars().all(|c| {
            c.is_ascii_digit()
                || matches!(
                    c,
                    '.' | 'G' | 'M' | 'T' | 'K' | '%' | 'F' | 'R' | 'E' | 'V' | 'P' | 'S'
                )
        })
}

fn fail(message: impl Into<String>) -> String {
    message.into()
}

/// Ejecuta una acción del helper root.
fn helper(runner: &Runner, args: &[&str]) -> Result<String, String> {
    let mut argv: Vec<String> = vec!["helper".to_string(), "storage-apply".to_string()];
    argv.extend(args.iter().map(|arg| (*arg).to_string()));
    let output = runner
        .run(
            &CommandSpec::new(HELPER)
                .args(argv)
                .sudo(SudoMode::Passwordless)
                .timeout(std::time::Duration::from_secs(1800)),
        )
        .map_err(|error| error.to_string())?;
    if output.is_success() {
        Ok(output.stdout)
    } else if output.stderr.is_empty() {
        Err(output.stdout)
    } else {
        Err(output.stderr)
    }
}

fn run_capture(runner: &Runner, program: &str, args: &[&str]) -> Result<String, String> {
    let mut argv: Vec<String> = args.iter().map(|arg| (*arg).to_string()).collect();
    let output = runner
        .run(
            &CommandSpec::new(program)
                .args(argv.drain(..))
                .timeout(std::time::Duration::from_secs(120)),
        )
        .map_err(|error| error.to_string())?;
    if output.is_success() {
        Ok(output.stdout)
    } else if output.stderr.is_empty() {
        Err(output.stdout)
    } else {
        Err(output.stderr)
    }
}

/// Discos del sistema operativo (`/`, `/boot`, `/boot/efi`), con soporte para
/// subvolúmenes Btrfs (`[/@]`) y múltiples padres LVM/mdadm.
pub fn os_disks(runner: &Runner) -> Vec<String> {
    crate::helpers::storage::os_disks(runner)
}

/// Disco padre de un dispositivo (`/dev/sda1` → `/dev/sda`, `/dev/nvme0n1p2[/@]` → `/dev/nvme0n1`).
pub fn parent_disk(runner: &Runner, device: &str) -> Option<String> {
    let device = device.trim();
    let device = device
        .split('[')
        .next()
        .unwrap_or(device)
        .trim()
        .trim_end_matches('/');
    if device.is_empty() {
        return None;
    }
    if let Ok(output) = run_capture(runner, "lsblk", &["-ndo", "PKNAME", device]) {
        let name = output
            .lines()
            .map(str::trim)
            .find(|l| !l.is_empty())
            .unwrap_or_default();
        if !name.is_empty() {
            let disk_name = crate::validation::disk_name_from_partition(name);
            return Some(format!("/dev/{disk_name}"));
        }
    }
    let base = device.rsplit('/').next().unwrap_or_default();
    if base.is_empty() || device.starts_with("/dev/mapper/") {
        return None;
    }
    let disk_name = crate::validation::disk_name_from_partition(base);
    Some(format!("/dev/{disk_name}"))
}

/// Comprueba si `device` coincide con un miembro de pool, su disco padre o viceversa,
/// evitando falsos positivos por prefijo (p. ej. `/dev/sda` vs `/dev/sdaa1`).
pub(crate) fn device_matches_pool_member(runner: &Runner, member: &str, device: &str) -> bool {
    let member = member.trim().trim_end_matches('/');
    let device = device.trim().trim_end_matches('/');
    if member.is_empty() || device.is_empty() {
        return false;
    }
    member == device
        || parent_disk(runner, member).as_deref() == Some(device)
        || parent_disk(runner, device).as_deref() == Some(member)
}

/// Puntos de montaje del dispositivo o de sus particiones.
pub fn mount_points(runner: &Runner, device: &str) -> Vec<String> {
    let mut mounts = Vec::new();
    if let Ok(output) = run_capture(runner, "lsblk", &["-no", "MOUNTPOINT", device]) {
        for line in output.lines() {
            let line = line.trim();
            if !line.is_empty() {
                mounts.push(line.to_string());
            }
        }
    }
    if let Ok(text) = std::fs::read_to_string("/proc/mounts") {
        for line in text.lines() {
            let mut parts = line.split_whitespace();
            let source = parts.next().unwrap_or_default();
            let target = parts.next().unwrap_or_default();
            if source == device && !target.is_empty() {
                mounts.push(target.to_string());
            }
        }
    }
    mounts.sort();
    mounts.dedup();
    mounts
}

/// Indica si el dispositivo contiene firmas LVM/RAID (`lvm`, `raid0`, `raid1`, `raid5`, `raid6`, `raid10`, etc.).
pub fn has_critical_metadata(runner: &Runner, device: &str) -> bool {
    if let Ok(output) = run_capture(runner, "lsblk", &["-no", "TYPE", device]) {
        if output
            .lines()
            .map(str::trim)
            .any(|kind| kind == "lvm" || kind.starts_with("raid"))
        {
            return true;
        }
    }
    false
}

/// Protecciones comunes antes de cualquier operación destructiva.
fn ensure_safe(runner: &Runner, device: &str, confirm: &str) -> Result<(), String> {
    if crate::helpers::storage::is_os_disk(runner, device).unwrap_or(false) {
        return Err(fail(
            "operación prohibida: es el disco del sistema operativo",
        ));
    }
    if has_critical_metadata(runner, device) && confirm != "SI-DESTRUIR-DATOS" {
        return Err(fail(
            "operación prohibida: el disco contiene LVM o RAID activo",
        ));
    }
    if let Ok(devs) = crate::storage::lsblk_devices_with_uuid(runner) {
        for pool in crate::storage::btrfs_pools_from_lsblk(&devs) {
            let Some(mp) = pool.mountpoint else {
                continue;
            };
            let matches = pool
                .devices
                .iter()
                .any(|member| device_matches_pool_member(runner, member, device));
            if matches {
                return Err(fail(format!(
                    "operación prohibida: {device} pertenece al pool Btrfs montado en {mp}"
                )));
            }
        }
    }
    Ok(())
}

fn check_mounted(runner: &Runner, device: &str, unmount: bool) -> Result<(), String> {
    if mount_points(runner, device).is_empty() {
        return Ok(());
    }
    if unmount {
        Ok(())
    } else {
        Err(fail(
            "el dispositivo está montado; habilita la opción de desmontar para continuar",
        ))
    }
}

/// Formatea y monta un dispositivo en `/srv/nas`.
pub fn format_device(
    runner: &Runner,
    device: &str,
    fstype: &str,
    confirm: &str,
    unmount: bool,
) -> Result<String, String> {
    let device = device.trim();
    let fstype = fstype.trim().to_ascii_lowercase();
    if !confirm_ok(confirm) {
        return Err(fail(
            "confirmación requerida (SI-FORMATEAR o SI-DESTRUIR-DATOS)",
        ));
    }
    if !valid_device(device) {
        return Err(fail("dispositivo inválido"));
    }
    if !ALLOWED_FILESYSTEMS.contains(&fstype.as_str()) {
        return Err(fail("sistema de archivos no soportado (ext4|btrfs)"));
    }
    ensure_safe(runner, device, confirm)?;
    check_mounted(runner, device, unmount)?;

    let flag = if unmount { "1" } else { "0" };
    helper(runner, &["format", confirm, device, &fstype, flag])?;
    Ok(format!(
        "{device} formateado en {fstype} y montado en /srv/nas"
    ))
}

/// Crea PV → VG → LV, formatea y monta en `/srv/nas`.
#[allow(clippy::too_many_arguments)]
pub fn lvm_create(
    runner: &Runner,
    disk: &str,
    vg: &str,
    lv: &str,
    size: &str,
    fstype: &str,
    confirm: &str,
    unmount: bool,
) -> Result<String, String> {
    if !confirm_ok(confirm) {
        return Err(fail("confirmación requerida"));
    }
    if !valid_device(disk) || !valid_name(vg) || !valid_name(lv) {
        return Err(fail("disco, VG o LV inválidos"));
    }
    let fstype = fstype.trim().to_ascii_lowercase();
    if !ALLOWED_FILESYSTEMS.contains(&fstype.as_str()) {
        return Err(fail("filesystem no soportado"));
    }
    let size = if size.trim().is_empty() {
        "100%FREE".to_string()
    } else {
        size.trim().to_ascii_uppercase()
    };
    if !valid_size(&size) {
        return Err(fail("tamaño de volumen inválido"));
    }
    ensure_safe(runner, disk, confirm)?;
    check_mounted(runner, disk, unmount)?;

    let flag = if unmount { "1" } else { "0" };
    helper(
        runner,
        &["lvm", confirm, disk, vg, lv, &size, &fstype, flag],
    )?;
    Ok(format!(
        "volumen /dev/{vg}/{lv} creado y montado en /srv/nas"
    ))
}

/// Crea un subvolumen Btrfs bajo `/srv/nas`.
pub fn create_subvolume(
    runner: &Runner,
    device: &str,
    name: &str,
    confirm: &str,
) -> Result<String, String> {
    if !confirm_ok(confirm) {
        return Err(fail("confirmación requerida"));
    }
    if !valid_name(name) {
        return Err(fail("nombre de subvolumen inválido"));
    }
    if !device.trim().is_empty() && !valid_device(device) {
        return Err(fail("dispositivo inválido"));
    }
    helper(runner, &["subvolume", confirm, name])?;
    Ok(format!("subvolumen creado: /srv/nas/{name}"))
}

/// Añade una unidad montada en `/srv/nas/unidades/<id>`.
pub fn add_unit(
    runner: &Runner,
    device: &str,
    unit_id: &str,
    fstype: &str,
    confirm: &str,
) -> Result<String, String> {
    if !valid_name(unit_id) {
        return Err(fail("identificador de unidad inválido"));
    }
    if !confirm_ok(confirm) {
        return Err(fail("confirmación requerida"));
    }
    if !valid_device(device) {
        return Err(fail("dispositivo inválido"));
    }
    let fstype = if fstype.trim().is_empty() {
        "ext4".to_string()
    } else {
        fstype.trim().to_ascii_lowercase()
    };
    if !ALLOWED_FILESYSTEMS.contains(&fstype.as_str()) {
        return Err(fail("filesystem no soportado"));
    }
    let mount_dir = format!("/srv/nas/unidades/{unit_id}");
    if crate::storage::is_mountpoint(std::path::Path::new(&mount_dir)) {
        return Err(fail(format!(
            "ya existe un volumen montado en {mount_dir} (bloqueo anti-overmount)"
        )));
    }
    ensure_safe(runner, device, confirm)?;
    check_mounted(runner, device, false)?;
    helper(runner, &["add-unit", confirm, device, unit_id, &fstype])?;
    Ok(format!(
        "unidad {unit_id} creada en /srv/nas/unidades/{unit_id}"
    ))
}

/// Extiende el pool (LVM o Btrfs) que contiene `/srv/nas` con un disco adicional.
pub fn extend_pool(runner: &Runner, device: &str, confirm: &str) -> Result<String, String> {
    extend_pool_target(runner, "principal", device, confirm, false)
}

/// Extiende un pool existente (`principal` o `<unit_id>`), soportando LVM y Btrfs (`btrfs device add`).
pub fn extend_pool_target(
    runner: &Runner,
    target: &str,
    device: &str,
    confirm: &str,
    metadata_raid1: bool,
) -> Result<String, String> {
    if !confirm_ok(confirm) {
        return Err(fail("confirmación requerida"));
    }
    let target = target.trim();
    let target_arg = if target.is_empty() {
        "principal"
    } else if target != "principal" && !valid_name(target) {
        return Err(fail("identificador de pool inválido"));
    } else {
        target
    };
    if !valid_device(device) {
        return Err(fail("dispositivo inválido"));
    }
    ensure_safe(runner, device, confirm)?;
    check_mounted(runner, device, false)?;
    let raid_flag = if metadata_raid1 { "1" } else { "0" };
    helper(
        runner,
        &["extend-pool-target", confirm, target_arg, device, raid_flag],
    )
}

/// Crea un nuevo pool (1 o N discos) en `principal` (`/srv/nas`) o en `/srv/nas/unidades/<id>`.
pub fn create_pool(
    runner: &Runner,
    target: &str,
    mode: &str,
    devices: &[String],
    confirm: &str,
    unmount: bool,
) -> Result<String, String> {
    if !confirm_ok(confirm) {
        return Err(fail(
            "confirmación requerida (SI-FORMATEAR o SI-DESTRUIR-DATOS)",
        ));
    }
    let target = target.trim();
    let target_arg = if target.is_empty() || target == "principal" {
        "principal"
    } else if valid_name(target) {
        target
    } else {
        return Err(fail("identificador de pool o unidad inválido"));
    };
    let mode = mode.trim().to_ascii_lowercase();
    if !crate::helpers::storage::ALLOWED_POOL_MODES.contains(&mode.as_str()) {
        return Err(fail(format!("modo de pool no soportado: {mode}")));
    }
    if devices.is_empty() {
        return Err(fail("selecciona al menos un disco"));
    }
    if mode == "ext4" && devices.len() != 1 {
        return Err(fail(
            "ext4 directo solo admite 1 disco; selecciona Btrfs o LVM para varios discos",
        ));
    }
    if mode == "btrfs_raid1" && devices.len() < 2 {
        return Err(fail("Btrfs RAID1 (espejo) requiere al menos 2 discos"));
    }
    let mut clean_devs = Vec::with_capacity(devices.len());
    for raw in devices {
        let dev = raw.trim().to_string();
        if !valid_device(&dev) {
            return Err(fail(format!("dispositivo inválido: {dev}")));
        }
        if clean_devs.contains(&dev) {
            return Err(fail(format!("dispositivo duplicado: {dev}")));
        }
        ensure_safe(runner, &dev, confirm)?;
        check_mounted(runner, &dev, unmount)?;
        clean_devs.push(dev);
    }
    if target_arg != "principal" {
        let mount_dir = format!("/srv/nas/unidades/{target_arg}");
        if crate::storage::is_mountpoint(std::path::Path::new(&mount_dir)) {
            return Err(fail(format!(
                "ya existe un volumen montado en {mount_dir} (bloqueo anti-overmount)"
            )));
        }
    }
    let joined = clean_devs.join(",");
    let flag = if unmount { "1" } else { "0" };
    helper(
        runner,
        &["create-pool", confirm, target_arg, &mode, &joined, flag],
    )
}

/// Desmonta una unidad adicional `/srv/nas/unidades/<id>` y la retira de `/etc/fstab`.
pub fn umount_unit(runner: &Runner, unit_id: &str, confirm: &str) -> Result<String, String> {
    if !confirm_ok(confirm) {
        return Err(fail("confirmación requerida"));
    }
    if !valid_name(unit_id) {
        return Err(fail("identificador de unidad inválido"));
    }
    helper(runner, &["umount-unit", confirm, unit_id])
}

/// Retira de forma segura un disco miembro de un pool Btrfs (`btrfs device remove`).
pub fn remove_pool_device(
    runner: &Runner,
    target: &str,
    device: &str,
    confirm: &str,
) -> Result<String, String> {
    if !confirm_ok(confirm) {
        return Err(fail(
            "confirmación requerida (SI-FORMATEAR o SI-DESTRUIR-DATOS)",
        ));
    }
    let target = target.trim();
    let target_arg = if target.is_empty() || target == "principal" {
        "principal"
    } else if valid_name(target) {
        target
    } else {
        return Err(fail("identificador de pool inválido"));
    };
    let device = device.trim();
    if !valid_device(device) {
        return Err(fail("dispositivo inválido"));
    }
    helper(runner, &["remove-pool-device", confirm, target_arg, device])
}

/// Borra firmas y tabla de particiones de un disco no protegido.
pub fn wipe_disk(
    runner: &Runner,
    device: &str,
    confirm: &str,
    unmount: bool,
) -> Result<String, String> {
    if confirm != "SI-DESTRUIR-DATOS" {
        return Err(fail(
            "se requiere escribir exactamente SI-DESTRUIR-DATOS para limpiar firmas de disco",
        ));
    }
    if !valid_device(device) {
        return Err(fail("dispositivo inválido"));
    }
    ensure_safe(runner, device, confirm)?;
    check_mounted(runner, device, unmount)?;
    helper(runner, &["wipe-disk", confirm, device])
}

/// Sanea `/etc/fstab` retirando entradas huérfanas y normalizando `nofail`.
pub fn sanitize_fstab(runner: &Runner, confirm: &str) -> Result<String, String> {
    if !confirm_ok(confirm) && confirm != "SI-SANEAR" {
        return Err(fail("confirmación requerida"));
    }
    helper(runner, &["sanitize-fstab", confirm])
}

/// Consulta `btrfs scrub status` de una unidad `/srv/nas/unidades/<id>`.
pub fn unit_scrub_status(runner: &Runner, unit_id: &str) -> Result<String, String> {
    if !valid_name(unit_id) {
        return Err(fail("identificador de unidad inválido"));
    }
    helper(runner, &["scrub-unit-status", unit_id])
}

mod maintenance;
#[cfg(test)]
mod tests;

pub use maintenance::{set_write_cache, unit_scrub_start, unit_trim, ups_configure, ups_disable};
