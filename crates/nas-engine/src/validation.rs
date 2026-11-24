//! Validadores compartidos y utilidades puras de bloques e identidad.

use std::sync::OnceLock;

use regex::Regex;

/// Cuentas reservadas del sistema operativo que nunca pueden gestionarse desde el panel o Samba.
pub const RESERVED_SYSTEM_USERS: &[&str] = &[
    "root",
    "daemon",
    "bin",
    "sys",
    "sync",
    "nobody",
    "nogroup",
    "www-data",
    "nas",
    "nas-web",
    "sshd",
    "messagebus",
];

/// Limpia espacios y retira el sufijo Btrfs `[subvol]` de una ruta de bloque.
pub fn clean_block_source(device: &str) -> &str {
    device.trim().split('[').next().unwrap_or("").trim()
}

/// Valida que una ruta de dispositivo de bloque (`/dev/...`) sea sintácticamente segura.
pub fn is_valid_device_path(device: &str) -> bool {
    let trimmed = device.trim();
    if !trimmed.starts_with("/dev/")
        || trimmed.len() <= 5
        || trimmed.len() > 64
        || trimmed.ends_with('/')
        || trimmed.contains("..")
    {
        return false;
    }
    trimmed
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '/' | '-' | '_' | '.'))
}

/// Extrae el nombre del disco base a partir del nombre de una partición o dispositivo sin `/dev/`.
pub fn disk_name_from_partition(name: &str) -> &str {
    if let Some((disk, part)) = name.rsplit_once('p') {
        if !part.is_empty()
            && part.chars().all(|c| c.is_ascii_digit())
            && disk.ends_with(|c: char| c.is_ascii_digit())
        {
            return disk;
        }
    }
    if name.starts_with("nvme")
        || name.starts_with("mmcblk")
        || name.starts_with("loop")
        || name.starts_with("nbd")
        || name.starts_with("md")
        || name.starts_with("ram")
        || name.starts_with("pmem")
    {
        return name;
    }
    let trimmed = name.trim_end_matches(|c: char| c.is_ascii_digit());
    if trimmed.is_empty() {
        name
    } else {
        trimmed
    }
}

/// Deduce el nombre del disco padre (sin `/dev/`) a partir de una ruta o fuente con `[subvol]`.
pub fn parent_disk_name(device: &str) -> Option<String> {
    let name = clean_block_source(device).rsplit('/').next()?.trim();
    if name.is_empty() {
        return None;
    }
    let disk = disk_name_from_partition(name);
    if disk.is_empty() {
        None
    } else {
        Some(disk.to_string())
    }
}

/// Construye la ruta de la primera partición de un disco (`/dev/sda1`, `/dev/nvme0n1p1`, `/dev/loop0p1`).
pub fn first_partition_path(device: &str) -> String {
    let dev = device.trim().trim_end_matches('/');
    if dev.ends_with(|c: char| c.is_ascii_digit()) {
        format!("{dev}p1")
    } else {
        format!("{dev}1")
    }
}

/// Indica si una cuenta corresponde a un usuario reservado del sistema operativo.
pub fn is_reserved_system_user(name: &str) -> bool {
    let lower = name.trim().to_ascii_lowercase();
    RESERVED_SYSTEM_USERS.contains(&lower.as_str()) || lower.starts_with("systemd-")
}

/// Valida la sintaxis POSIX de un nombre de usuario (`^[a-z_][a-z0-9_-]{0,31}$`).
pub fn is_valid_username_syntax(username: &str) -> bool {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| Regex::new(r"^[a-z_][a-z0-9_-]{0,31}$").expect("regex válido"));
    re.is_match(username)
}

/// Valida si un nombre de grupo cumple la convención corporativa (`^grp_[a-z0-9_-]{1,28}$`).
pub fn is_valid_group_name(group: &str) -> bool {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| Regex::new(r"^grp_[a-z0-9_-]{1,28}$").expect("regex válido"));
    re.is_match(group)
}

/// Valida el nombre de un recurso compartido Samba (`^[A-Za-z0-9_-]{1,32}\$?$`).
pub fn is_valid_share_name(name: &str) -> bool {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| Regex::new(r"^[A-Za-z0-9_-]{1,32}\$?$").expect("regex válido"));
    re.is_match(name)
}

/// Valida el nombre de una unidad de almacenamiento adicional (`^[A-Za-z0-9_-]{1,32}$`).
pub fn is_valid_unit_name(unit: &str) -> bool {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| Regex::new(r"^[A-Za-z0-9_-]{1,32}$").expect("regex válido"));
    re.is_match(unit)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valida_rutas_de_dispositivos_y_limpieza_subvol() {
        assert_eq!(clean_block_source(" /dev/sda1[/@] "), "/dev/sda1");
        assert!(is_valid_device_path("/dev/sdb"));
        assert!(is_valid_device_path("/dev/nvme0n1"));
        assert!(is_valid_device_path("/dev/mapper/nas_vg-nas_lv"));
        assert!(!is_valid_device_path("/dev/"));
        assert!(!is_valid_device_path("sdb"));
        assert!(!is_valid_device_path("/dev/../etc"));
        assert!(!is_valid_device_path("/dev/sdb;rm -rf /"));
    }

    #[test]
    fn deduce_disco_padre_y_primera_particion() {
        assert_eq!(parent_disk_name("/dev/sdb1[/@]").as_deref(), Some("sdb"));
        assert_eq!(
            parent_disk_name("/dev/nvme0n1p2").as_deref(),
            Some("nvme0n1")
        );
        assert_eq!(parent_disk_name("/dev/nvme0n1").as_deref(), Some("nvme0n1"));
        assert_eq!(
            parent_disk_name("/dev/mmcblk0p1").as_deref(),
            Some("mmcblk0")
        );
        assert_eq!(parent_disk_name("/dev/loop0p1").as_deref(), Some("loop0"));
        assert_eq!(parent_disk_name(""), None);

        assert_eq!(first_partition_path("/dev/sdb"), "/dev/sdb1");
        assert_eq!(first_partition_path("/dev/nvme0n1"), "/dev/nvme0n1p1");
        assert_eq!(first_partition_path("/dev/mmcblk0"), "/dev/mmcblk0p1");
        assert_eq!(first_partition_path("/dev/loop0/"), "/dev/loop0p1");
    }

    #[test]
    fn valida_cuentas_reservadas_y_regex_compartidos() {
        assert!(is_reserved_system_user("root"));
        assert!(is_reserved_system_user("nas"));
        assert!(is_reserved_system_user("nas-web"));
        assert!(is_reserved_system_user("systemd-network"));
        assert!(!is_reserved_system_user("operador1"));

        assert!(is_valid_username_syntax("operador_1"));
        assert!(!is_valid_username_syntax("Operador"));
        assert!(is_valid_group_name("grp_contabilidad"));
        assert!(!is_valid_group_name("contabilidad"));
        assert!(is_valid_share_name("BACKUPS_WIN$"));
        assert!(!is_valid_share_name("../escape"));
        assert!(is_valid_unit_name("unidad_1"));
        assert!(!is_valid_unit_name("unidad$"));
    }
}
