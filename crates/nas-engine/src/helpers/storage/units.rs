//! Subvolúmenes Btrfs y montaje/desmontaje de unidades de almacenamiento adicionales.

use std::path::Path;
use std::time::Duration;

use nas_core::{NasError, NasResult};
use nas_exec::{CommandSpec, Runner};

use crate::storage::{is_mountpoint, proc_mounts_children_under};
use crate::storage_ops::{confirm_ok, valid_name};

use super::{
    ensure_device_not_in_active_pool, format_partition, mount_options_for, prepare_mountpoint,
    register_btrfs_scrub_for_target, remove_btrfs_scrub_for_unit, update_fstab,
};

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
    let mount_dir = format!("/srv/nas/unidades/{unit}");
    if is_mountpoint(Path::new(&mount_dir)) {
        return Err(NasError::Denied(format!(
            "ya existe un volumen montado en {mount_dir} (bloqueo anti-overmount)"
        )));
    }
    ensure_device_not_in_active_pool(runner, device)?;
    let mps = crate::storage_ops::mount_points(runner, device);
    if !mps.is_empty() {
        return Err(NasError::Denied(format!(
            "el dispositivo {device} está montado en {}; desmóntalo antes de crear la unidad",
            mps.join(", ")
        )));
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

    let unidades_root = "/srv/nas/unidades";
    let _ = std::fs::create_dir_all(unidades_root);
    prepare_mountpoint(runner, unidades_root);
    let _ = std::fs::create_dir_all(&mount_dir);
    runner
        .run(&CommandSpec::new("mount").args(["-o", &options, &target, &mount_dir]))?
        .ensure_success()?;
    prepare_mountpoint(runner, &mount_dir);
    update_fstab(runner, &target, &mount_dir, &options)?;

    if fstype.eq_ignore_ascii_case("btrfs") {
        register_btrfs_scrub_for_target(&mount_dir, Some(unit));
    }
    if crate::storage::detect_device_rotational(runner, device) == Some(false) {
        let _ = crate::deploy::storage::enable_fstrim(runner);
    }

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
    if !is_mountpoint(Path::new(&mount_dir)) {
        return Err(NasError::NotFound(format!(
            "{mount_dir} no está montado actualmente"
        )));
    }
    if let Ok(raw) = std::fs::read_to_string("/proc/mounts") {
        let children = proc_mounts_children_under(&raw, Path::new(&mount_dir));
        if !children.is_empty() {
            return Err(NasError::Denied(format!(
                "existen montajes hijos bajo {mount_dir} ({}); desmóntalos primero en orden LIFO",
                children.join(", ")
            )));
        }
    }

    let out = runner.run(&CommandSpec::new("umount").args([&mount_dir]))?;
    if !out.is_success() {
        let detail = if out.stderr.is_empty() {
            out.stdout
        } else {
            out.stderr
        };
        return Err(NasError::Invalid(format!(
            "no se pudo desmontar {mount_dir}: {detail}"
        )));
    }

    let fstab_path = Path::new("/etc/fstab");
    if fstab_path.exists() {
        let content =
            std::fs::read_to_string(fstab_path).map_err(|e| NasError::io(fstab_path, e))?;
        let new_lines: Vec<&str> = content
            .lines()
            .filter(|line| {
                let trimmed = line.trim();
                if trimmed.is_empty() || trimmed.starts_with('#') {
                    return true;
                }
                let parts: Vec<&str> = trimmed.split_whitespace().collect();
                !(parts.len() >= 2 && parts[1] == mount_dir)
            })
            .collect();
        let final_content = format!("{}\n", new_lines.join("\n"));
        crate::deploy::install_fstab_content(runner, &final_content)?;
    }
    remove_btrfs_scrub_for_unit(unit);
    let _ = std::fs::remove_dir(&mount_dir);
    Ok(format!("unidad {unit} desmontada y retirada de fstab"))
}

fn resolve_unit_mount_dir(unit: &str) -> NasResult<String> {
    if unit.trim().is_empty() || unit == "principal" {
        return Ok("/srv/nas".to_string());
    }
    if !valid_name(unit) {
        return Err(NasError::Invalid("identificador de unidad inválido".into()));
    }
    let mount_dir = format!("/srv/nas/unidades/{unit}");
    if !is_mountpoint(Path::new(&mount_dir)) {
        return Err(NasError::NotFound(format!(
            "la unidad {unit} no está montada"
        )));
    }
    Ok(mount_dir)
}

/// Consulta `btrfs scrub status` de `/srv/nas` o de una unidad `/srv/nas/unidades/<id>`.
pub fn scrub_unit_status(runner: &Runner, unit: &str) -> NasResult<String> {
    let mount_dir = resolve_unit_mount_dir(unit)?;
    let out = runner.run(
        &CommandSpec::new("btrfs")
            .args(["scrub", "status", &mount_dir])
            .timeout(Duration::from_secs(15)),
    )?;
    if out.is_success() {
        Ok(out.stdout)
    } else {
        Err(NasError::Invalid(if out.stderr.is_empty() {
            out.stdout
        } else {
            out.stderr
        }))
    }
}

/// Inicia `btrfs scrub start` de `/srv/nas` o de una unidad `/srv/nas/unidades/<id>`.
pub fn scrub_unit_start(runner: &Runner, unit: &str) -> NasResult<String> {
    let mount_dir = resolve_unit_mount_dir(unit)?;
    let out = runner.run(
        &CommandSpec::new("btrfs")
            .args(["scrub", "start", "-c", "3", &mount_dir])
            .timeout(Duration::from_secs(30)),
    )?;
    if out.is_success() {
        Ok(out.stdout)
    } else {
        Err(NasError::Invalid(if out.stderr.is_empty() {
            out.stdout
        } else {
            out.stderr
        }))
    }
}

/// Ejecuta `fstrim -v` sobre `/srv/nas` o sobre una unidad `/srv/nas/unidades/<id>`.
pub fn trim_unit(runner: &Runner, unit: &str) -> NasResult<String> {
    let mount_dir = resolve_unit_mount_dir(unit)?;
    let out = runner.run(
        &CommandSpec::new("fstrim")
            .args(["-v", &mount_dir])
            .timeout(Duration::from_secs(120)),
    )?;
    if out.is_success() {
        Ok(out.stdout)
    } else {
        Err(NasError::Invalid(if out.stderr.is_empty() {
            out.stdout
        } else {
            out.stderr
        }))
    }
}
