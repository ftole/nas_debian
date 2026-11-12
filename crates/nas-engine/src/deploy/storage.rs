//! Operaciones de almacenamiento para el despliegue: formateo, LVM, montaje,
//! `fstab`, `fstrim.timer` y scrub periódico de Btrfs.
//!
//! Todas las operaciones privilegiadas se ejecutan a través de [`nas_exec::Runner`]
//! con timeouts acotados y validación previa del disco (protección del disco del
//! sistema operativo). Soporta la reutilización de datos existentes (`keep_data`).

use std::time::Duration;

use nas_core::{NasError, NasResult};
use nas_exec::{CommandSpec, Runner};

/// Sistema de archivos soportado por el despliegue.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileSystem {
    /// ext4.
    Ext4,
    /// Btrfs.
    Btrfs,
}

impl FileSystem {
    /// Nombre usado por `mkfs`.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ext4 => "ext4",
            Self::Btrfs => "btrfs",
        }
    }

    /// Parsea desde una cadena.
    pub fn parse(value: &str) -> Option<Self> {
        match value.to_ascii_lowercase().as_str() {
            "ext4" => Some(Self::Ext4),
            "btrfs" => Some(Self::Btrfs),
            _ => None,
        }
    }
}

/// Detecta si el disco es mecánico consultando `/sys/block`.
pub fn detect_rotational(runner: &Runner, device: &str) -> NasResult<bool> {
    let name = device
        .trim_start_matches("/dev/")
        .trim_end_matches(|c: char| !c.is_alphanumeric());
    let path = format!("/sys/block/{name}/queue/rotational");
    match runner.run(
        &CommandSpec::new("cat")
            .arg(&path)
            .timeout(Duration::from_secs(5)),
    ) {
        Ok(out) if out.is_success() => Ok(out.stdout.trim() == "1"),
        _ => Ok(true),
    }
}

/// Ruta de la primera partición de un disco.
pub fn partition_path(device: &str) -> Option<String> {
    if device == "LOCAL" {
        return None;
    }
    let suffix = if device.starts_with("/dev/nvme") || device.starts_with("/dev/mmcblk") {
        "p1"
    } else {
        "1"
    };
    Some(format!("{}{}", device.trim_end_matches('/'), suffix))
}

/// Opciones de montaje derivadas del sistema de archivos y del tipo de disco.
pub fn mount_options(fs: FileSystem, rotational: bool) -> String {
    match (fs, rotational) {
        (FileSystem::Ext4, true) => "rw,noatime,commit=2".to_string(),
        (FileSystem::Ext4, false) => "rw,noatime,commit=5".to_string(),
        (FileSystem::Btrfs, true) => {
            "rw,noatime,compress=zstd:3,space_cache=v2,autodefrag".to_string()
        }
        (FileSystem::Btrfs, false) => {
            "rw,noatime,compress=zstd:3,space_cache=v2,ssd,discard=async".to_string()
        }
    }
}

/// Opciones de montaje a partir de cadenas.
pub fn mount_options_str(fs: &str, rotational: bool) -> String {
    FileSystem::parse(fs)
        .map(|f| mount_options(f, rotational))
        .unwrap_or_else(|| "rw,noatime".to_string())
}

/// Valida que el disco no sea el del sistema operativo (salvo reutilización).
pub fn ensure_not_os_disk(runner: &Runner, device: &str, keep_data: bool) -> NasResult<()> {
    if device == "LOCAL" || keep_data {
        return Ok(());
    }
    if crate::helpers::storage::is_os_disk(runner, device)? {
        return Err(NasError::Denied(format!(
            "el disco {} contiene el sistema operativo y está protegido contra formateo",
            device
        )));
    }
    Ok(())
}

/// Valida un dispositivo de bloque.
pub fn valid_device(device: &str) -> bool {
    device.starts_with("/dev/")
        && device.len() <= 64
        && !device.contains("..")
        && device
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '/' | '-' | '_' | '.'))
}

fn run_checked(runner: &Runner, spec: CommandSpec) -> NasResult<()> {
    let out = runner.run(&spec)?;
    if out.is_success() {
        Ok(())
    } else {
        Err(NasError::Denied(if out.stderr.is_empty() {
            out.stdout
        } else {
            out.stderr
        }))
    }
}

/// Formatea un disco como ext4 (omite si `keep_data`).
pub fn format_ext4(runner: &Runner, device: &str, keep_data: bool) -> NasResult<()> {
    if keep_data {
        return Ok(());
    }
    ensure_not_os_disk(runner, device, keep_data)?;
    let Some(part) = partition_path(device) else {
        return Ok(());
    };
    // Tabla de particiones GPT.
    run_checked(
        runner,
        CommandSpec::new("parted")
            .args([
                "-s", device, "mklabel", "gpt", "mkpart", "primary", "ext4", "0%", "100%",
            ])
            .sudo(nas_exec::SudoMode::Passwordless)
            .timeout(Duration::from_secs(120)),
    )?;
    let _ = runner.run(
        &CommandSpec::new("partprobe")
            .arg(device)
            .timeout(Duration::from_secs(30)),
    );
    let _ = runner.run(
        &CommandSpec::new("udevadm")
            .args(["settle"])
            .timeout(Duration::from_secs(30)),
    );
    run_checked(
        runner,
        CommandSpec::new("mkfs.ext4")
            .args(["-F", "-m", "1", "-L", "NAS_DATA", &part])
            .sudo(nas_exec::SudoMode::Passwordless)
            .timeout(Duration::from_secs(300)),
    )?;
    Ok(())
}

/// Formatea un disco como Btrfs (omite si `keep_data`).
pub fn format_btrfs(runner: &Runner, device: &str, keep_data: bool) -> NasResult<()> {
    if keep_data {
        return Ok(());
    }
    ensure_not_os_disk(runner, device, keep_data)?;
    let Some(part) = partition_path(device) else {
        return Ok(());
    };
    run_checked(
        runner,
        CommandSpec::new("parted")
            .args([
                "-s", device, "mklabel", "gpt", "mkpart", "primary", "btrfs", "0%", "100%",
            ])
            .sudo(nas_exec::SudoMode::Passwordless)
            .timeout(Duration::from_secs(120)),
    )?;
    let _ = runner.run(
        &CommandSpec::new("partprobe")
            .arg(device)
            .timeout(Duration::from_secs(30)),
    );
    let _ = runner.run(
        &CommandSpec::new("udevadm")
            .args(["settle"])
            .timeout(Duration::from_secs(30)),
    );
    run_checked(
        runner,
        CommandSpec::new("mkfs.btrfs")
            .args(["-f", "-L", "NAS_DATA", &part])
            .sudo(nas_exec::SudoMode::Passwordless)
            .timeout(Duration::from_secs(300)),
    )?;
    Ok(())
}

/// Formatea según el sistema de archivos indicado.
pub fn format_disk(
    runner: &Runner,
    device: &str,
    fs: FileSystem,
    keep_data: bool,
) -> NasResult<()> {
    match fs {
        FileSystem::Ext4 => format_ext4(runner, device, keep_data),
        FileSystem::Btrfs => format_btrfs(runner, device, keep_data),
    }
}

/// Crea un volumen físico LVM.
pub fn lvm_pv_create(runner: &Runner, device: &str) -> NasResult<()> {
    if !valid_device(device) {
        return Err(NasError::Invalid(format!("dispositivo inválido: {device}")));
    }
    ensure_not_os_disk(runner, device, false)?;
    run_checked(
        runner,
        CommandSpec::new("pvcreate")
            .arg(device)
            .sudo(nas_exec::SudoMode::Passwordless)
            .timeout(Duration::from_secs(120)),
    )
}

/// Crea un grupo de volúmenes LVM.
pub fn lvm_vg_create(runner: &Runner, vg: &str, device: &str) -> NasResult<()> {
    if !crate::storage_ops::valid_name(vg) {
        return Err(NasError::Invalid(format!("nombre de VG inválido: {vg}")));
    }
    run_checked(
        runner,
        CommandSpec::new("vgcreate")
            .args([vg, device])
            .sudo(nas_exec::SudoMode::Passwordless)
            .timeout(Duration::from_secs(120)),
    )
}

/// Crea un volumen lógico LVM.
pub fn lvm_lv_create(runner: &Runner, vg: &str, lv: &str, size: &str) -> NasResult<()> {
    if !crate::storage_ops::valid_name(vg) || !crate::storage_ops::valid_name(lv) {
        return Err(NasError::Invalid("nombre LVM inválido".into()));
    }
    if !crate::storage_ops::valid_size(size) {
        return Err(NasError::Invalid(format!("tamaño inválido: {size}")));
    }
    run_checked(
        runner,
        CommandSpec::new("lvcreate")
            .args(["-L", size, "-n", lv, vg])
            .sudo(nas_exec::SudoMode::Passwordless)
            .timeout(Duration::from_secs(120)),
    )
}

/// Extiende un volumen lógico LVM.
pub fn lvm_lv_extend(runner: &Runner, vg: &str, lv: &str, size: &str) -> NasResult<()> {
    if !crate::storage_ops::valid_name(vg) || !crate::storage_ops::valid_name(lv) {
        return Err(NasError::Invalid("nombre LVM inválido".into()));
    }
    if !crate::storage_ops::valid_size(size) {
        return Err(NasError::Invalid(format!("tamaño inválido: {size}")));
    }
    run_checked(
        runner,
        CommandSpec::new("lvextend")
            .args(["-L", size, &format!("/dev/{vg}/{lv}")])
            .sudo(nas_exec::SudoMode::Passwordless)
            .timeout(Duration::from_secs(120)),
    )
}

/// Monta un dispositivo en un punto de montaje con las opciones indicadas.
pub fn mount(runner: &Runner, source: &str, target: &str, options: &str) -> NasResult<()> {
    let _ = runner.run(
        &CommandSpec::new("mkdir")
            .args(["-p", target])
            .timeout(Duration::from_secs(10)),
    );
    run_checked(
        runner,
        CommandSpec::new("mount")
            .args(["-o", options, source, target])
            .sudo(nas_exec::SudoMode::Passwordless)
            .timeout(Duration::from_secs(60)),
    )
}

/// Desmonta un punto de montaje.
pub fn unmount(runner: &Runner, target: &str) -> NasResult<()> {
    run_checked(
        runner,
        CommandSpec::new("umount")
            .arg(target)
            .sudo(nas_exec::SudoMode::Passwordless)
            .timeout(Duration::from_secs(60)),
    )
}

/// Persiste una entrada de montaje en `/etc/fstab` usando el UUID si está disponible.
pub fn update_fstab(runner: &Runner, source: &str, target: &str, options: &str) -> NasResult<()> {
    let fstab_path = std::path::Path::new("/etc/fstab");
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
    if std::fs::write(fstab_path, &final_content).is_err() {
        let staged = std::env::temp_dir().join(format!("nas-fstab-{}", std::process::id()));
        std::fs::write(&staged, &final_content).map_err(|e| {
            NasError::Denied(format!(
                "no se pudo preparar archivo temporal de fstab: {e}"
            ))
        })?;
        let res = runner.run(
            &CommandSpec::new("install")
                .args([
                    "-m",
                    "0644",
                    "-o",
                    "root",
                    "-g",
                    "root",
                    &staged.to_string_lossy(),
                    "/etc/fstab",
                ])
                .sudo(nas_exec::SudoMode::Passwordless)
                .timeout(Duration::from_secs(30)),
        );
        let _ = std::fs::remove_file(&staged);
        match res {
            Ok(out) if !out.is_success() => {
                return Err(NasError::Denied(format!(
                    "no se pudo actualizar /etc/fstab: {}",
                    out.stderr
                )));
            }
            Err(e) => {
                return Err(NasError::Denied(format!(
                    "no se pudo actualizar /etc/fstab: {e}"
                )));
            }
            _ => {}
        }
    }
    Ok(())
}

/// Activa `fstrim.timer` (SSD).
pub fn enable_fstrim(runner: &Runner) -> NasResult<()> {
    run_checked(
        runner,
        CommandSpec::new("systemctl")
            .args(["enable", "--now", "fstrim.timer"])
            .sudo(nas_exec::SudoMode::Passwordless)
            .timeout(Duration::from_secs(60)),
    )
}

/// Instala el cron de scrub mensual de Btrfs.
pub fn install_btrfs_scrub_cron(_runner: &Runner) -> NasResult<()> {
    let content = "SHELL=/bin/bash\nPATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin\n0 2 1 * * root btrfs scrub start -B /srv/nas >/dev/null 2>&1\n";
    let path = "/etc/cron.d/nas-btrfs-scrub";
    std::fs::write(path, content)
        .map_err(|e| NasError::Denied(format!("no se pudo escribir {path}: {e}")))?;
    Ok(())
}

/// Monta y persiste `/srv/nas` aplicando el flujo completo (formateo opcional).
pub fn setup_storage(
    runner: &Runner,
    device: &str,
    fs: FileSystem,
    rotational: bool,
    keep_data: bool,
) -> NasResult<()> {
    ensure_not_os_disk(runner, device, keep_data)?;
    if device != "LOCAL" {
        format_disk(runner, device, fs, keep_data)?;
        let part = partition_path(device).unwrap_or_else(|| device.to_string());
        let opts = mount_options(fs, rotational);
        mount(runner, &part, "/srv/nas", &opts)?;
        update_fstab(runner, &part, "/srv/nas", &opts)?;
    }
    if !rotational {
        let _ = enable_fstrim(runner);
    }
    if fs == FileSystem::Btrfs {
        let _ = install_btrfs_scrub_cron(runner);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mount_options_ext4_hdd() {
        assert_eq!(mount_options(FileSystem::Ext4, true), "rw,noatime,commit=2");
    }

    #[test]
    fn mount_options_ext4_ssd() {
        assert_eq!(
            mount_options(FileSystem::Ext4, false),
            "rw,noatime,commit=5"
        );
    }

    #[test]
    fn mount_options_btrfs_hdd() {
        let o = mount_options(FileSystem::Btrfs, true);
        assert!(o.contains("compress=zstd:3"));
        assert!(o.contains("autodefrag"));
    }

    #[test]
    fn mount_options_btrfs_ssd() {
        let o = mount_options(FileSystem::Btrfs, false);
        assert!(o.contains("discard=async"));
    }

    #[test]
    fn partition_path_nvme() {
        assert_eq!(
            partition_path("/dev/nvme0n1"),
            Some("/dev/nvme0n1p1".into())
        );
    }

    #[test]
    fn partition_path_sata() {
        assert_eq!(partition_path("/dev/sdb"), Some("/dev/sdb1".into()));
    }

    #[test]
    fn partition_path_local() {
        assert_eq!(partition_path("LOCAL"), None);
    }

    #[test]
    fn valid_device_ok() {
        assert!(valid_device("/dev/sdb"));
        assert!(!valid_device("/etc/passwd"));
    }

    #[test]
    fn filesystem_parse() {
        assert_eq!(FileSystem::parse("ext4"), Some(FileSystem::Ext4));
        assert_eq!(FileSystem::parse("BTRFS"), Some(FileSystem::Btrfs));
        assert_eq!(FileSystem::parse("xfs"), None);
    }
}
