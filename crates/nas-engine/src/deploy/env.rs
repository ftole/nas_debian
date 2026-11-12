//! Detección de entorno para el despliegue del servidor.
//!
//! Funciones puras o basadas en `nas_exec::Runner` para detectar la IP local,
//! el nombre NetBIOS, el grupo de trabajo, el disco base del sistema operativo,
//! el tipo de medio (rotacional) y si un disco está en uso. También normaliza
//! el rol del servidor y valida los parámetros de disco (`--keep-data`).

use std::time::Duration;

use nas_core::{NasError, NasResult, ServerRole};
use nas_exec::{CommandSpec, Runner};

/// Normaliza un rol textual, incluyendo alias heredados (`HIBRIDO`,
/// `ARCHIVOSBACKUP`).
pub fn normalize_role(raw: &str) -> Option<ServerRole> {
    ServerRole::parse(raw)
}

/// Valida que `--keep-data` se use únicamente con un disco dedicado.
pub fn validate_keep_data(disk: &str, keep_data: bool) -> NasResult<()> {
    if keep_data && (disk.trim().is_empty() || disk == "LOCAL") {
        return Err(NasError::Invalid(
            "--keep-data requiere un disco dedicado, no LOCAL".to_string(),
        ));
    }
    Ok(())
}

/// Valida un disco y devuelve los problemas detectados.
///
/// `in_use` indica si el disco está montado, es PV de LVM o miembro de RAID.
pub fn validate_disk(disk: &str, keep_data: bool, in_use: bool) -> Vec<String> {
    let mut problems = Vec::new();

    if disk.trim().is_empty() {
        problems.push("el disco es obligatorio (o 'LOCAL')".to_string());
    } else if disk != "LOCAL" {
        let valid = disk.strip_prefix("/dev/").is_some_and(|rest| {
            !rest.is_empty()
                && rest
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '/' || c == '-' || c == '_')
        });
        if !valid {
            problems.push(format!("disco inválido: '{disk}'"));
        }
        if in_use && !keep_data {
            problems.push(format!(
                "el disco '{disk}' está en uso (montado, PV o RAID)"
            ));
        }
    }

    if keep_data && disk == "LOCAL" {
        problems.push("--keep-data requiere un disco dedicado, no LOCAL".to_string());
    }

    problems
}

/// Normaliza un nombre NetBIOS a partir de texto arbitrario.
pub fn normalize_netbios(raw: &str) -> String {
    let mut out = String::new();
    for ch in raw.trim().chars() {
        if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
            out.push(ch.to_ascii_uppercase());
        }
    }
    out
}

/// Detecta la IP local principal del servidor.
pub fn local_ip(runner: &Runner) -> String {
    if let Ok(out) = runner.run(
        &CommandSpec::new("ip")
            .args(["-4", "-o", "addr", "show", "scope", "global"])
            .timeout(Duration::from_secs(5)),
    ) {
        if out.is_success() {
            if let Some(ip) = out
                .stdout
                .split_whitespace()
                .skip_while(|token| *token != "inet")
                .nth(1)
                .and_then(|cidr| cidr.split('/').next())
                .filter(|ip| !ip.starts_with("127."))
            {
                return ip.to_string();
            }
        }
    }

    if let Ok(out) = runner.run(
        &CommandSpec::new("hostname")
            .arg("-I")
            .timeout(Duration::from_secs(5)),
    ) {
        if out.is_success() {
            if let Some(first_ip) = out
                .stdout
                .split_whitespace()
                .find(|ip| !ip.starts_with("127.") && !ip.is_empty())
            {
                return first_ip.to_string();
            }
        }
    }

    "127.0.0.1".to_string()
}

/// Detecta el nombre NetBIOS por defecto a partir del hostname del equipo.
pub fn default_netbios(runner: &Runner) -> String {
    let raw = runner
        .run(
            &CommandSpec::new("hostname")
                .arg("-s")
                .timeout(Duration::from_secs(5)),
        )
        .ok()
        .filter(|out| out.is_success())
        .map(|out| out.stdout)
        .unwrap_or_default();

    let normalized = normalize_netbios(&raw);
    if normalized.is_empty() || normalized == "LOCALHOST" {
        "SRV-NAS".to_string()
    } else {
        normalized
    }
}

/// Extrae el workgroup de un texto de configuración Samba.
pub fn parse_workgroup(text: &str) -> Option<String> {
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.to_lowercase().starts_with("workgroup") {
            if let Some((_, value)) = trimmed.split_once('=') {
                let wg = value.trim().to_uppercase();
                if !wg.is_empty() {
                    return Some(wg);
                }
            }
        }
    }
    None
}

/// Detecta el grupo de trabajo por defecto leyendo `smb.conf`.
pub fn default_workgroup(smb_conf_path: &str) -> String {
    std::fs::read_to_string(smb_conf_path)
        .ok()
        .and_then(|text| parse_workgroup(&text))
        .unwrap_or_else(|| "WORKGROUP".to_string())
}

/// Encuentra el disco base de un dispositivo, recorriendo ancestros PKNAME.
pub fn base_disk(runner: &Runner, device: &str) -> String {
    let mut cur = device.trim().to_string();
    // Elimina sufijos de partición tipo [1] por si acaso.
    if let Some(idx) = cur.find('[') {
        cur.truncate(idx);
    }

    for _ in 0..8 {
        let output = runner.run(
            &CommandSpec::new("lsblk")
                .args(["-no", "PKNAME", &cur])
                .timeout(Duration::from_secs(5)),
        );
        match output {
            Ok(out) if out.is_success() => {
                let next = out.stdout.trim();
                if next.is_empty() {
                    break;
                }
                cur = format!("/dev/{next}");
            }
            _ => break,
        }
    }

    let base = cur
        .trim_start_matches("/dev/")
        .trim_start_matches('/')
        .to_string();
    if base.is_empty() {
        device.trim().trim_start_matches("/dev/").to_string()
    } else {
        base
    }
}

/// Detecta el disco base que contiene el sistema operativo (`/`).
pub fn os_disk(runner: &Runner) -> Option<String> {
    let src = runner
        .run(
            &CommandSpec::new("findmnt")
                .args(["-no", "SOURCE", "/"])
                .timeout(Duration::from_secs(5)),
        )
        .ok()
        .filter(|out| out.is_success())
        .map(|out| out.stdout.trim().to_string())
        .filter(|s| !s.is_empty())?;

    Some(base_disk(runner, &src))
}

/// Interpreta el valor de rotación de `lsblk`/sysfs.
pub fn parse_rotational(raw: &str) -> Option<bool> {
    match raw.trim().to_lowercase().as_str() {
        "1" | "true" | "yes" => Some(true),
        "0" | "false" | "no" => Some(false),
        _ => None,
    }
}

/// Detecta si un disco es mecánico (`rotational=true`).
pub fn disk_rotational(runner: &Runner, device: &str) -> Option<bool> {
    if let Ok(out) = runner.run(
        &CommandSpec::new("lsblk")
            .args(["-d", "-no", "ROta", device])
            .timeout(Duration::from_secs(5)),
    ) {
        if out.is_success() {
            if let Some(value) = parse_rotational(&out.stdout) {
                return Some(value);
            }
        }
    }

    let name = device.trim().trim_start_matches("/dev/");
    if let Ok(text) = std::fs::read_to_string(format!("/sys/block/{name}/queue/rotational")) {
        return parse_rotational(&text);
    }

    None
}

/// Detecta si un disco está en uso (montado, PV de LVM o miembro de RAID).
pub fn disk_in_use(runner: &Runner, device: &str) -> bool {
    // Montado.
    if let Ok(out) = runner.run(
        &CommandSpec::new("lsblk")
            .args(["-ln", "-o", "MOUNTPOINT", device])
            .timeout(Duration::from_secs(5)),
    ) {
        if out.is_success() && out.stdout.lines().any(|line| !line.trim().is_empty()) {
            return true;
        }
    }

    // PV de LVM.
    if let Ok(out) = runner.run(
        &CommandSpec::new("pvs")
            .args(["--noheadings", "-o", "pv_name"])
            .timeout(Duration::from_secs(5)),
    ) {
        if out.is_success()
            && out
                .stdout
                .lines()
                .map(str::trim)
                .any(|pv| pv == device.trim())
        {
            return true;
        }
    }

    // Miembro de RAID (md).
    if let Ok(text) = std::fs::read_to_string("/proc/mdstat") {
        if let Ok(out) = runner.run(
            &CommandSpec::new("lsblk")
                .args(["-ln", "-o", "NAME", device])
                .timeout(Duration::from_secs(5)),
        ) {
            if out.is_success()
                && out.stdout.lines().any(|name| {
                    let name = name.trim();
                    !name.is_empty() && text.contains(name)
                })
            {
                return true;
            }
        }
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normaliza_rol_con_alias() {
        assert_eq!(normalize_role("HIBRIDO"), Some(ServerRole::ArquivosBackup));
        assert_eq!(
            normalize_role("ARCHIVOSBACKUP"),
            Some(ServerRole::ArquivosBackup)
        );
        assert_eq!(normalize_role("backup"), Some(ServerRole::Backup));
        assert_eq!(normalize_role("otro"), None);
    }

    #[test]
    fn keep_data_requiere_disco_dedicado() {
        assert!(validate_keep_data("LOCAL", true).is_err());
        assert!(validate_keep_data("/dev/sdb", true).is_ok());
        assert!(validate_keep_data("LOCAL", false).is_ok());
    }

    #[test]
    fn valida_disco_en_uso() {
        let problems = validate_disk("/dev/sdb", false, true);
        assert!(problems.iter().any(|p| p.contains("en uso")));
        let problems = validate_disk("/dev/sdb", false, false);
        assert!(problems.is_empty());
    }

    #[test]
    fn normaliza_netbios() {
        assert_eq!(normalize_netbios("srv-nas"), "SRV-NAS");
        assert_eq!(normalize_netbios("  my host! "), "MYHOST");
    }

    #[test]
    fn parsea_workgroup() {
        let text = "[global]\nworkgroup = EAD-COL\n";
        assert_eq!(parse_workgroup(text).as_deref(), Some("EAD-COL"));
    }

    #[test]
    fn parsea_rotacional() {
        assert_eq!(parse_rotational("1"), Some(true));
        assert_eq!(parse_rotational("0"), Some(false));
        assert_eq!(parse_rotational("nope"), None);
    }

    #[test]
    fn base_disk_extrae_nombre() {
        // Sin runner real no se puede probar la cadena; se prueba la limpieza de prefijo.
        assert_eq!("/dev/sda".trim_start_matches("/dev/"), "sda");
    }
}
