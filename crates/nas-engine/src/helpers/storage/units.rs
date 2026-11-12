//! Subvolúmenes Btrfs y montaje/desmontaje de unidades de almacenamiento adicionales.

use std::path::Path;

use nas_core::{NasError, NasResult};
use nas_exec::{CommandSpec, Runner};

use crate::storage_ops::{confirm_ok, valid_name};

use super::{format_partition, mount_options_for, prepare_mountpoint, update_fstab};

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
                let parts: Vec<&str> = line.split_whitespace().collect();
                !(parts.len() >= 2 && parts[1] == mount_dir)
            })
            .collect();
        let staged = std::env::temp_dir().join(format!("nas-fstab-{}", std::process::id()));
        std::fs::write(&staged, format!("{}\n", new_lines.join("\n")))
            .map_err(|e| NasError::io(&staged, e))?;
        let argv = vec![
            "install".to_string(),
            "-m".to_string(),
            "0644".to_string(),
            "-o".to_string(),
            "root".to_string(),
            "-g".to_string(),
            "root".to_string(),
            staged.to_string_lossy().to_string(),
            fstab_path.to_string_lossy().to_string(),
        ];
        let res = runner.run(
            &CommandSpec::new(&argv[0])
                .args(argv[1..].to_vec())
                .sudo(nas_exec::SudoMode::Passwordless)
                .timeout(std::time::Duration::from_secs(30)),
        );
        let _ = std::fs::remove_file(&staged);
        res?.ensure_success()?;
    }
    Ok(format!("unidad {unit} desmontada"))
}
