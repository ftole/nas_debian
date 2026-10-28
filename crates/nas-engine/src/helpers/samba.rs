//! Helper para aplicación atómica y validada de configuraciones de Samba.

use std::path::Path;
use std::time::Duration;

use nas_core::{NasError, NasResult};
use nas_exec::{CommandSpec, Runner};

/// Aplica de forma atómica una configuración preparada en staging para Samba.
///
/// Valida la ruta (prefijo `/var/lib/nas/staging/smbconf_`), ausencia de `..`,
/// existencia del archivo, sintaxis con `testparm -s`, respaldo preventivo de
/// `/etc/samba/smb.conf`, copia atómica y recarga de `smbd`.
pub fn apply_staged_samba(runner: &Runner, staged: &Path) -> NasResult<String> {
    let path_str = staged.to_string_lossy();
    if !path_str.starts_with("/var/lib/nas/staging/smbconf_") || path_str.contains("..") {
        return Err(NasError::Denied(format!(
            "ruta no permitida para configuración staged: {path_str}"
        )));
    }
    if !staged.exists() {
        return Err(NasError::NotFound(format!(
            "archivo staged no existe: {path_str}"
        )));
    }

    // 1. Validar sintaxis con testparm
    let test_out = runner.run(
        &CommandSpec::new("testparm")
            .args(["-s", &path_str])
            .timeout(Duration::from_secs(30)),
    )?;
    if !test_out.is_success() {
        let err_msg = if test_out.stderr.is_empty() {
            test_out.stdout
        } else {
            test_out.stderr
        };
        return Err(NasError::Invalid(format!(
            "testparm rechazó la configuración: {err_msg}"
        )));
    }

    // 2. Respaldo preventivo del smb.conf actual
    let stamp = chrono::Local::now().format("%Y%m%d%H%M%S").to_string();
    let backup_path = format!("/etc/samba/smb.conf.bak.{stamp}");
    let current_conf = Path::new("/etc/samba/smb.conf");
    if current_conf.exists() {
        let _ = std::fs::copy(current_conf, &backup_path);
    }

    // 3. Instalación de la configuración validada
    std::fs::copy(staged, current_conf)
        .map_err(|e| NasError::Denied(format!("error al instalar /etc/samba/smb.conf: {e}")))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(current_conf, std::fs::Permissions::from_mode(0o644));
    }

    let _ = std::fs::remove_file(staged);

    // 4. Recarga de Samba
    let _ = runner.run(
        &CommandSpec::new("systemctl")
            .args(["reload", "smbd"])
            .timeout(Duration::from_secs(30)),
    );

    Ok(format!("smb.conf aplicado (respaldo: {backup_path})"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rechaza_rutas_fuera_de_staging() {
        let runner = Runner::new();
        let err = apply_staged_samba(&runner, Path::new("/tmp/smbconf_1234")).unwrap_err();
        assert!(err.to_string().contains("ruta no permitida"));
    }

    #[test]
    fn rechaza_rutas_con_salto_de_directorio() {
        let runner = Runner::new();
        let err = apply_staged_samba(
            &runner,
            Path::new("/var/lib/nas/staging/smbconf_../etc/passwd"),
        )
        .unwrap_err();
        assert!(err.to_string().contains("ruta no permitida"));
    }

    #[test]
    fn rechaza_archivo_inexistente() {
        let runner = Runner::new();
        let err = apply_staged_samba(
            &runner,
            Path::new("/var/lib/nas/staging/smbconf_inexistente_99999"),
        )
        .unwrap_err();
        assert!(err.to_string().contains("no existe"));
    }
}
