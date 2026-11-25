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

/// Detecta si el disco (físico, LVM, MD o VM) es mecánico consultando sysfs `slaves/*` y `lsblk`.
pub fn detect_rotational(runner: &Runner, device: &str) -> NasResult<bool> {
    Ok(crate::storage::detect_device_rotational(runner, device).unwrap_or(true))
}

/// Ruta de la primera partición de un disco.
pub fn partition_path(device: &str) -> Option<String> {
    if device == "LOCAL" {
        return None;
    }
    Some(crate::validation::first_partition_path(device))
}

/// Opciones de montaje derivadas del sistema de archivos y del tipo de disco.
pub fn mount_options(fs: FileSystem, rotational: bool) -> String {
    match (fs, rotational) {
        (FileSystem::Ext4, true | false) => "rw,noatime,commit=5,errors=remount-ro".to_string(),
        (FileSystem::Btrfs, true) => "rw,noatime,compress=zstd:3,space_cache=v2".to_string(),
        (FileSystem::Btrfs, false) => "rw,noatime,compress=zstd:3,space_cache=v2,ssd".to_string(),
    }
}

/// Valida que el dispositivo no sea el del SO y no presente fallo S.M.A.R.T. explícito antes de formatear.
pub fn ensure_disk_ready_for_format(
    runner: &Runner,
    device: &str,
    keep_data: bool,
) -> NasResult<()> {
    ensure_not_os_disk(runner, device, keep_data)?;
    if device != "LOCAL" && !keep_data {
        let health = crate::storage::check_block_health(runner, device);
        if !health.healthy {
            return Err(NasError::Denied(health.detail));
        }
    }
    Ok(())
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
    crate::validation::is_valid_device_path(device)
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
    ensure_disk_ready_for_format(runner, device, keep_data)?;
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
    ensure_disk_ready_for_format(runner, device, keep_data)?;
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

/// Persiste una entrada de montaje en `/etc/fstab` usando el UUID si está disponible
/// y aplicando siempre `nofail,x-systemd.device-timeout=15s` en montajes secundarios.
pub fn update_fstab(runner: &Runner, source: &str, target: &str, options: &str) -> NasResult<()> {
    super::artifacts::update_fstab(runner, source, target, options)
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
    let content = "SHELL=/bin/bash\nPATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin\n0 2 1 * * root nice -n 19 ionice -c 3 btrfs scrub start -B /srv/nas >/dev/null 2>&1\n";
    let path = "/etc/cron.d/nas-btrfs-scrub";
    std::fs::write(path, content)
        .map_err(|e| NasError::Denied(format!("no se pudo escribir {path}: {e}")))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mount_options_ext4_hdd() {
        assert_eq!(
            mount_options(FileSystem::Ext4, true),
            "rw,noatime,commit=5,errors=remount-ro"
        );
    }

    #[test]
    fn mount_options_ext4_ssd() {
        assert_eq!(
            mount_options(FileSystem::Ext4, false),
            "rw,noatime,commit=5,errors=remount-ro"
        );
    }

    #[test]
    fn mount_options_btrfs_hdd() {
        let o = mount_options(FileSystem::Btrfs, true);
        assert!(o.contains("compress=zstd:3"));
        assert!(!o.contains("autodefrag"));
    }

    #[test]
    fn mount_options_btrfs_ssd() {
        let o = mount_options(FileSystem::Btrfs, false);
        assert!(o.contains("ssd"));
        assert!(!o.contains("discard=async"));
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
