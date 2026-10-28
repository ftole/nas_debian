//! Helpers para operaciones privilegiadas sobre dispositivos de almacenamiento.

use std::path::Path;
use std::time::Duration;

use nas_core::{NasError, NasResult};
use nas_exec::{CommandSpec, Runner};

use crate::storage_ops::{confirm_ok, valid_device, valid_name, valid_size};

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
    if let Ok(out) = runner.run(
        &CommandSpec::new("lsblk")
            .args(["-no", "MOUNTPOINT", device])
            .timeout(Duration::from_secs(10)),
    ) {
        for line in out.stdout.lines() {
            let mp = line.trim();
            if !mp.is_empty() {
                let _ = runner.run(&CommandSpec::new("umount").args([mp]));
            }
        }
    }
}

fn partition_for(device: &str) -> String {
    let dev = device.trim_end_matches('/');
    if dev.starts_with("/dev/nvme") || dev.starts_with("/dev/mmcblk") {
        format!("{dev}p1")
    } else {
        format!("{dev}1")
    }
}

fn mount_options_for(_runner: &Runner, fstype: &str, device: &str) -> String {
    if fstype == "btrfs" {
        return "rw,noatime,compress=zstd:3,space_cache=v2,autodefrag".to_string();
    }
    let disk_name = device.trim_start_matches("/dev/");
    let rota_path = format!("/sys/block/{disk_name}/queue/rotational");
    let is_hdd = std::fs::read_to_string(&rota_path)
        .map(|s| s.trim() == "1")
        .unwrap_or(false);
    if is_hdd {
        "rw,noatime,commit=2,errors=remount-ro".to_string()
    } else {
        "rw,noatime,commit=5,errors=remount-ro".to_string()
    }
}

fn update_fstab(runner: &Runner, source: &str, target: &str, options: &str) -> NasResult<()> {
    let fstab_path = Path::new("/etc/fstab");
    if !fstab_path.exists() {
        return Ok(());
    }

    let uuid_res = runner.run(
        &CommandSpec::new("blkid")
            .args(["-s", "UUID", "-o", "value", source])
            .timeout(Duration::from_secs(10)),
    );
    let fstab_src = match uuid_res {
        Ok(out) if out.is_success() && !out.stdout.trim().is_empty() => {
            format!("UUID={}", out.stdout.trim())
        }
        _ => source.to_string(),
    };

    let content = std::fs::read_to_string(fstab_path).unwrap_or_default();
    let mut new_lines: Vec<String> = content
        .lines()
        .filter(|line| {
            let trimmed = line.trim();
            if trimmed.starts_with('#') {
                return true;
            }
            let parts: Vec<&str> = trimmed.split_whitespace().collect();
            !(parts.len() >= 2 && parts[1] == target)
        })
        .map(ToString::to_string)
        .collect();

    new_lines.push(format!("{fstab_src} {target} auto {options} 0 0"));
    let final_content = format!("{}\n", new_lines.join("\n"));
    std::fs::write(fstab_path, final_content).map_err(|e| NasError::Denied(e.to_string()))?;
    Ok(())
}

fn prepare_mountpoint(runner: &Runner, path: &str) {
    let _ = runner.run(&CommandSpec::new("chown").args(["root:grp_samba", path]));
    let _ = runner.run(&CommandSpec::new("chmod").args(["2770", path]));
}

fn format_partition(runner: &Runner, device: &str, fstype: &str, label: &str) -> NasResult<String> {
    if !valid_device(device) {
        return Err(NasError::Invalid("dispositivo inválido".into()));
    }
    if fstype != "ext4" && fstype != "btrfs" {
        return Err(NasError::Invalid(
            "filesystem no soportado (ext4|btrfs)".into(),
        ));
    }
    os_disk_guard(runner, device)?;

    let mut target = device.to_string();
    let is_disk = runner
        .run(&CommandSpec::new("lsblk").args(["-nd", "-o", "TYPE", device]))
        .map(|out| out.stdout.trim() == "disk")
        .unwrap_or(false);

    if is_disk {
        runner
            .run(&CommandSpec::new("parted").args([
                "-s", device, "mklabel", "gpt", "mkpart", "primary", fstype, "0%", "100%",
            ]))?
            .ensure_success()?;
        let _ = runner.run(&CommandSpec::new("partprobe").args([device]));
        let _ = runner.run(&CommandSpec::new("udevadm").args(["settle"]));
        target = partition_for(device);
    }

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
    if !confirm_ok(confirm) {
        return Err(NasError::Denied(
            "confirmación textual requerida (SI-FORMATEAR o SI-DESTRUIR-DATOS)".into(),
        ));
    }
    if !valid_device(device) {
        return Err(NasError::Invalid("dispositivo inválido".into()));
    }
    os_disk_guard(runner, device)?;
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

    if unmount {
        unmount_device(runner, disk);
    }

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
    runner
        .run(&CommandSpec::new("lvcreate").args(["-y", "-n", lv, "-l", size, vg]))?
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

    Ok(format!("volumen {path_lv} montado en /srv/nas"))
}

/// Extiende el grupo de volúmenes de `/srv/nas` con un nuevo disco.
pub fn extend_pool(runner: &Runner, confirm: &str, device: &str) -> NasResult<String> {
    if !confirm_ok(confirm) {
        return Err(NasError::Denied("confirmación requerida".into()));
    }
    if !valid_device(device) {
        return Err(NasError::Invalid("dispositivo inválido".into()));
    }
    os_disk_guard(runner, device)?;

    let source = runner
        .run(&CommandSpec::new("findmnt").args(["-no", "SOURCE", "/srv/nas"]))
        .map(|o| o.stdout.trim().to_string())
        .unwrap_or_default();
    let fstype = runner
        .run(&CommandSpec::new("findmnt").args(["-no", "FSTYPE", "/srv/nas"]))
        .map(|o| o.stdout.trim().to_string())
        .unwrap_or_default();

    let vg = runner
        .run(&CommandSpec::new("lvs").args(["--noheadings", "-o", "vg_name", &source]))
        .map(|o| o.stdout.trim().to_string())
        .unwrap_or_default();
    if vg.is_empty() {
        return Err(NasError::Invalid("/srv/nas no está sobre LVM".into()));
    }

    runner
        .run(&CommandSpec::new("pvcreate").args(["-y", "-ff", device]))?
        .ensure_success()?;
    runner
        .run(&CommandSpec::new("vgextend").args([&vg, device]))?
        .ensure_success()?;
    runner
        .run(&CommandSpec::new("lvextend").args(["-l", "+100%FREE", &source]))?
        .ensure_success()?;

    if fstype == "btrfs" {
        runner
            .run(&CommandSpec::new("btrfs").args(["filesystem", "resize", "max", "/srv/nas"]))?
            .ensure_success()?;
    } else {
        runner
            .run(&CommandSpec::new("resize2fs").args([&source]))?
            .ensure_success()?;
    }

    Ok(format!("pool {vg} extendido con {device}"))
}

/// Crea un subvolumen Btrfs en `/srv/nas`.
pub fn create_subvolume(runner: &Runner, confirm: &str, name: &str) -> NasResult<String> {
    if !confirm_ok(confirm) {
        return Err(NasError::Denied("confirmación requerida".into()));
    }
    if !valid_name(name) {
        return Err(NasError::Invalid("nombre de subvolumen inválido".into()));
    }
    let subvol_path = format!("/srv/nas/{name}");
    runner
        .run(&CommandSpec::new("btrfs").args(["subvolume", "create", &subvol_path]))?
        .ensure_success()?;
    prepare_mountpoint(runner, &subvol_path);
    Ok(format!("subvolumen {subvol_path} creado"))
}

/// Elimina un subvolumen Btrfs de `/srv/nas`.
pub fn delete_subvolume(runner: &Runner, confirm: &str, name: &str) -> NasResult<String> {
    if !confirm_ok(confirm) {
        return Err(NasError::Denied("confirmación requerida".into()));
    }
    if !valid_name(name) {
        return Err(NasError::Invalid("nombre de subvolumen inválido".into()));
    }
    let subvol_path = format!("/srv/nas/{name}");
    runner
        .run(&CommandSpec::new("btrfs").args(["subvolume", "delete", &subvol_path]))?
        .ensure_success()?;
    Ok(format!("subvolumen {subvol_path} eliminado"))
}

/// Monta una unidad de almacenamiento adicional en `/srv/nas/unidades/<unidad>`.
pub fn mount_unit(
    runner: &Runner,
    confirm: &str,
    device: &str,
    unit: &str,
    fstype: Option<&str>,
) -> NasResult<String> {
    if !confirm_ok(confirm) {
        return Err(NasError::Denied("confirmación requerida".into()));
    }
    if !valid_name(unit) {
        return Err(NasError::Invalid("nombre de unidad inválido".into()));
    }
    let fstype = fstype.unwrap_or("ext4");
    let label = format!(
        "NAS_U_{}",
        unit.to_ascii_uppercase()
            .chars()
            .take(10)
            .collect::<String>()
    );
    let target = format_partition(runner, device, fstype, &label)?;
    let options = mount_options_for(runner, fstype, device);
    let mount_dir = format!("/srv/nas/unidades/{unit}");

    let _ = std::fs::create_dir_all(&mount_dir);
    runner
        .run(&CommandSpec::new("mount").args(["-o", &options, &target, &mount_dir]))?
        .ensure_success()?;
    prepare_mountpoint(runner, &mount_dir);
    update_fstab(runner, &target, &mount_dir, &options)?;

    Ok(format!("unidad {unit} montada en {mount_dir}"))
}

/// Desmonta una unidad de almacenamiento y la retira de `/etc/fstab`.
pub fn umount_unit(runner: &Runner, confirm: &str, unit: &str) -> NasResult<String> {
    if !confirm_ok(confirm) {
        return Err(NasError::Denied("confirmación requerida".into()));
    }
    if !valid_name(unit) {
        return Err(NasError::Invalid("nombre de unidad inválido".into()));
    }
    let mount_dir = format!("/srv/nas/unidades/{unit}");
    let _ = runner.run(&CommandSpec::new("umount").args([&mount_dir]));

    // Quitar de /etc/fstab
    let fstab_path = Path::new("/etc/fstab");
    if fstab_path.exists() {
        if let Ok(content) = std::fs::read_to_string(fstab_path) {
            let new_lines: Vec<&str> = content
                .lines()
                .filter(|line| {
                    let parts: Vec<&str> = line.split_whitespace().collect();
                    !(parts.len() >= 2 && parts[1] == mount_dir)
                })
                .collect();
            let _ = std::fs::write(fstab_path, format!("{}\n", new_lines.join("\n")));
        }
    }
    Ok(format!("unidad {unit} desmontada"))
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
    }
}
