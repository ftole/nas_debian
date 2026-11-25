//! Instalación y validación de los helpers privilegiados de root del despliegue.
//!
//! Centraliza la copia de los ejecutables auxiliares (`nas-samba-apply`,
//! `nas-share-acl`, `nas-share-dir`, `nas-accounts`, `nas-backup-manage`,
//! `nas-storage-apply` y `nas-terminal`) a `/usr/local/sbin`, la asignación de
//! permisos seguros y la validación de las reglas sudo mediante `visudo -cf`.

use std::time::Duration;

use nas_core::{NasError, NasResult};
use nas_exec::{CommandSpec, Runner};

/// Directorio de destino de los helpers privilegiados.
pub const HELPER_DIR: &str = "/usr/local/sbin";

/// Ruta por defecto del archivo sudoers del panel.
pub const SUDOERS_FILE: &str = "/etc/sudoers.d/nas-web-rust";

/// Helpers root que instala el despliegue.
pub const ROOT_HELPERS: &[&str] = &[
    "nas-samba-apply",
    "nas-share-acl",
    "nas-share-dir",
    "nas-accounts",
    "nas-backup-manage",
    "nas-storage-apply",
    "nas-terminal",
];

/// Comprueba si un nombre corresponde a uno de los helpers gestionados.
pub fn is_root_helper(name: &str) -> bool {
    ROOT_HELPERS.contains(&name)
}

/// Ruta absoluta de destino de un helper.
pub fn helper_path(name: &str) -> String {
    format!("{HELPER_DIR}/{name}")
}

/// Instala un helper individual copiándolo desde `source_dir`.
pub fn install_helper(runner: &Runner, source_dir: &str, name: &str) -> NasResult<()> {
    if !is_root_helper(name) {
        return Err(NasError::Invalid(format!("helper desconocido: {name}")));
    }
    let source = format!("{}/{}", source_dir.trim_end_matches('/'), name);
    let destination = helper_path(name);
    let output = runner.run(
        &CommandSpec::new("install")
            .args([
                "-m",
                "0755",
                "-o",
                "root",
                "-g",
                "root",
                &source,
                &destination,
            ])
            .timeout(Duration::from_secs(30)),
    )?;
    if !output.is_success() {
        return Err(NasError::Denied(format!(
            "no se pudo instalar el helper {name}: {}",
            output.stderr.trim()
        )));
    }
    Ok(())
}

/// Instala todos los helpers root desde `source_dir`.
pub fn install_helpers(runner: &Runner, source_dir: &str) -> NasResult<()> {
    for name in ROOT_HELPERS {
        install_helper(runner, source_dir, name)?;
    }
    Ok(())
}

/// Asigna permisos seguros (`0755`, `root:root`) a un helper.
pub fn set_helper_permissions(runner: &Runner, path: &str) -> NasResult<()> {
    let chmod = runner.run(
        &CommandSpec::new("chmod")
            .args(["0755", path])
            .timeout(Duration::from_secs(15)),
    )?;
    if !chmod.is_success() {
        return Err(NasError::Denied(format!(
            "no se pudo ajustar permisos de {path}: {}",
            chmod.stderr.trim()
        )));
    }
    let chown = runner.run(
        &CommandSpec::new("chown")
            .args(["root:root", path])
            .timeout(Duration::from_secs(15)),
    )?;
    if !chown.is_success() {
        return Err(NasError::Denied(format!(
            "no se pudo ajustar propietario de {path}: {}",
            chown.stderr.trim()
        )));
    }
    Ok(())
}

/// Asigna permisos seguros a todos los helpers root.
pub fn set_all_helper_permissions(runner: &Runner) -> NasResult<()> {
    for name in ROOT_HELPERS {
        set_helper_permissions(runner, &helper_path(name))?;
    }
    Ok(())
}

/// Valida un archivo sudoers con `visudo -cf`.
pub fn validate_visudo(runner: &Runner, sudoers_path: &str) -> NasResult<()> {
    let output = runner.run(
        &CommandSpec::new("visudo")
            .args(["-cf", sudoers_path])
            .timeout(Duration::from_secs(30)),
    )?;
    if !output.is_success() {
        return Err(NasError::Denied(format!(
            "sudoers inválido ({}): {}",
            sudoers_path,
            output.stderr.trim()
        )));
    }
    Ok(())
}

/// Valida un archivo sudoers y devuelve si es válido.
pub fn visudo_ok(runner: &Runner, sudoers_path: &str) -> NasResult<bool> {
    let output = runner.run(
        &CommandSpec::new("visudo")
            .args(["-cf", sudoers_path])
            .timeout(Duration::from_secs(30)),
    )?;
    Ok(output.is_success())
}

/// Escribe (o reemplaza) un archivo sudoers y lo valida con `visudo`.
pub fn install_sudoers(runner: &Runner, content: &str, path: &str) -> NasResult<()> {
    std::fs::write(path, content)
        .map_err(|e| NasError::Denied(format!("no se pudo escribir {path}: {e}")))?;
    let chmod = runner.run(
        &CommandSpec::new("chmod")
            .args(["0440", path])
            .timeout(Duration::from_secs(15)),
    )?;
    if !chmod.is_success() {
        return Err(NasError::Denied(format!(
            "no se pudo ajustar permisos de {path}: {}",
            chmod.stderr.trim()
        )));
    }
    validate_visudo(runner, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lista_helpers_contiene_los_siete_esperados() {
        assert_eq!(ROOT_HELPERS.len(), 7);
        assert!(ROOT_HELPERS.contains(&"nas-samba-apply"));
        assert!(ROOT_HELPERS.contains(&"nas-share-acl"));
        assert!(ROOT_HELPERS.contains(&"nas-share-dir"));
        assert!(ROOT_HELPERS.contains(&"nas-accounts"));
        assert!(ROOT_HELPERS.contains(&"nas-backup-manage"));
        assert!(ROOT_HELPERS.contains(&"nas-storage-apply"));
        assert!(ROOT_HELPERS.contains(&"nas-terminal"));
    }

    #[test]
    fn paridad_helpers_con_packaging_install() {
        let install_sh = include_str!("../../../../packaging/install.sh");
        for helper in ROOT_HELPERS {
            assert!(
                install_sh.contains(helper),
                "packaging/install.sh debe instalar/autorizar {helper}"
            );
        }
    }

    #[test]
    fn valida_nombres_de_helpers() {
        assert!(is_root_helper("nas-terminal"));
        assert!(!is_root_helper("bash"));
        assert!(is_root_helper("nas-accounts"));
    }

    #[test]
    fn construye_rutas_de_destino() {
        assert_eq!(helper_path("nas-terminal"), "/usr/local/sbin/nas-terminal");
        assert_eq!(helper_path("nas-accounts"), "/usr/local/sbin/nas-accounts");
        assert_eq!(
            helper_path("nas-samba-apply"),
            "/usr/local/sbin/nas-samba-apply"
        );
    }

    #[test]
    fn expone_constantes_y_helpers() {
        assert_eq!(HELPER_DIR, "/usr/local/sbin");
        assert_eq!(SUDOERS_FILE, "/etc/sudoers.d/nas-web-rust");
        assert_eq!(ROOT_HELPERS.len(), 7);
    }
}
