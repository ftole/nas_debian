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
    let device = device.trim();
    if !device.starts_with("/dev/") || device.len() > 64 || device.contains("..") {
        return false;
    }
    device
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '/' | '-' | '_' | '.'))
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

/// Discos del sistema operativo (`/`, `/boot`, `/boot/efi`).
pub fn os_disks(runner: &Runner) -> Vec<String> {
    let mut disks = Vec::new();
    for mount in ["/", "/boot", "/boot/efi"] {
        if let Ok(source) = run_capture(runner, "findmnt", &["-no", "SOURCE", mount]) {
            if let Some(parent) = parent_disk(runner, source.trim()) {
                disks.push(parent);
            }
        }
    }
    disks.sort();
    disks.dedup();
    disks
}

/// Disco padre de un dispositivo (`/dev/sda1` → `/dev/sda`).
pub fn parent_disk(runner: &Runner, device: &str) -> Option<String> {
    if let Ok(output) = run_capture(runner, "lsblk", &["-no", "PKNAME", device]) {
        let name = output.lines().next().unwrap_or_default().trim();
        if !name.is_empty() {
            return Some(format!("/dev/{name}"));
        }
    }
    let base = device.rsplit('/').next().unwrap_or_default();
    if base.is_empty() || device.starts_with("/dev/mapper/") {
        return None;
    }
    if base.starts_with("nvme") || base.starts_with("mmcblk") {
        if let Some((disk, part)) = base.rsplit_once('p') {
            if !part.is_empty() && part.chars().all(|c| c.is_ascii_digit()) {
                return Some(format!("/dev/{disk}"));
            }
        }
        return Some(format!("/dev/{base}"));
    }
    let trimmed = base.trim_end_matches(|c: char| c.is_ascii_digit());
    if trimmed.is_empty() {
        Some(format!("/dev/{base}"))
    } else {
        Some(format!("/dev/{trimmed}"))
    }
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

/// Indica si el dispositivo contiene firmas LVM/RAID.
pub fn has_critical_metadata(runner: &Runner, device: &str) -> bool {
    if let Ok(output) = run_capture(runner, "lsblk", &["-no", "TYPE", device]) {
        if output
            .lines()
            .map(str::trim)
            .any(|kind| kind == "lvm" || kind == "raid")
        {
            return true;
        }
    }
    false
}

/// Protecciones comunes antes de cualquier operación destructiva.
fn ensure_safe(runner: &Runner, device: &str, confirm: &str) -> Result<(), String> {
    if let Some(parent) = parent_disk(runner, device) {
        if os_disks(runner)
            .iter()
            .any(|disk| disk == &parent || disk == device)
        {
            return Err(fail(
                "operación prohibida: es el disco del sistema operativo",
            ));
        }
    }
    if has_critical_metadata(runner, device) && confirm != "SI-DESTRUIR-DATOS" {
        return Err(fail(
            "operación prohibida: el disco contiene LVM o RAID activo",
        ));
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

/// Parámetros de creación de un volumen LVM.
#[derive(Debug, Clone, Copy)]
pub struct LvmRequest<'a> {
    /// Disco físico a usar como PV.
    pub disk: &'a str,
    /// Nombre del grupo de volúmenes.
    pub vg: &'a str,
    /// Nombre del volumen lógico.
    pub lv: &'a str,
    /// Tamaño (`100%FREE`, `20G`).
    pub size: &'a str,
    /// Filesystem (`ext4` o `btrfs`).
    pub fstype: &'a str,
    /// Confirmación textual.
    pub confirm: &'a str,
    /// Desmontar si está en uso.
    pub unmount: bool,
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
    ensure_safe(runner, device, confirm)?;
    helper(runner, &["add-unit", confirm, device, unit_id, &fstype])?;
    Ok(format!(
        "unidad {unit_id} creada en /srv/nas/unidades/{unit_id}"
    ))
}

/// Extiende el VG que contiene `/srv/nas` con un disco adicional.
pub fn extend_pool(runner: &Runner, device: &str, confirm: &str) -> Result<String, String> {
    if !confirm_ok(confirm) {
        return Err(fail("confirmación requerida"));
    }
    if !valid_device(device) {
        return Err(fail("dispositivo inválido"));
    }
    ensure_safe(runner, device, confirm)?;
    helper(runner, &["extend-pool", confirm, device])?;
    Ok(format!("pool extendido con {device}"))
}

#[cfg(test)]
mod tests {
    use super::{
        confirm_ok, create_subvolume, extend_pool, format_device, lvm_create, parent_disk,
        valid_device, valid_name, valid_size,
    };

    #[test]
    fn valida_confirmaciones_y_nombres() {
        assert!(confirm_ok("SI-FORMATEAR"));
        assert!(confirm_ok("SI-DESTRUIR-DATOS"));
        assert!(!confirm_ok("si"));
        assert!(!confirm_ok(""));

        assert!(valid_device("/dev/sdb"));
        assert!(valid_device("/dev/nvme0n1p1"));
        assert!(valid_device("/dev/mapper/vg-data"));
        assert!(!valid_device("/dev/../etc/passwd"));
        assert!(!valid_device("rm -rf /"));
        assert!(!valid_device("sdb"));

        assert!(valid_name("unidad_1"));
        assert!(!valid_name("unidad 1"));
        assert!(!valid_name(""));
        assert!(!valid_name("a/b"));

        assert!(valid_size("100%FREE"));
        assert!(valid_size("100%free"));
        assert!(valid_size("20G"));
        assert!(valid_size("20g"));
        assert!(valid_size("500M"));
        assert!(valid_size("50%vg"));
        assert!(!valid_size("rm -rf"));
    }

    #[test]
    fn heuristica_de_disco_padre() {
        let runner = nas_exec::Runner::new();
        assert_eq!(
            parent_disk(&runner, "/dev/nvme0n1p2").as_deref(),
            Some("/dev/nvme0n1")
        );
        assert_eq!(
            parent_disk(&runner, "/dev/nvme0n1").as_deref(),
            Some("/dev/nvme0n1")
        );
        assert_eq!(
            parent_disk(&runner, "/dev/sdb").as_deref(),
            Some("/dev/sdb")
        );
    }

    #[test]
    fn rechaza_operaciones_sin_confirmacion() {
        let runner = nas_exec::Runner::new();
        assert!(format_device(&runner, "/dev/sdz", "ext4", "", false).is_err());
        assert!(
            lvm_create(&runner, "/dev/sdz", "vg", "lv", "100%FREE", "ext4", "", false).is_err()
        );
        assert!(create_subvolume(&runner, "", "vol", "").is_err());
        assert!(extend_pool(&runner, "/dev/sdz", "").is_err());
        assert!(format_device(&runner, "../etc", "ext4", "SI-FORMATEAR", false).is_err());
        assert!(format_device(&runner, "/dev/sdz", "xfs", "SI-FORMATEAR", false).is_err());
    }

    #[test]
    fn ensure_safe_permite_si_destruir_datos() {
        let runner = nas_exec::Runner::new();
        // Dispositivos que no sean del sistema operativo
        assert!(super::ensure_safe(&runner, "/dev/sdz", "SI-DESTRUIR-DATOS").is_ok());
    }
}
