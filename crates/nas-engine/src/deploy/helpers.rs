//! Instalación y validación de los helpers privilegiados de root del despliegue.
//!
//! Centraliza la copia de los ejecutables auxiliares (`nas-samba-apply`,
//! `nas-share-acl`, `nas-accounts`, `nas-backup-manage`, `nas-storage-apply` y
//! `nas-terminal`) a `/usr/local/sbin`, la asignación de permisos seguros y la
//! validación de las reglas sudo mediante `visudo -cf`.

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
    "nas-accounts",
    "nas-backup-manage",
    "nas-storage-apply",
    "nas-terminal",
];

/// Devuelve la lista de helpers root gestionados.
pub fn root_helpers() -> &'static [&'static str] {
    ROOT_HELPERS
}

/// Alias de [`root_helpers`].
pub fn helper_names() -> &'static [&'static str] {
    ROOT_HELPERS
}

/// Alias de [`root_helpers`].
pub fn helpers() -> &'static [&'static str] {
    ROOT_HELPERS
}

/// Comprueba si un nombre corresponde a uno de los helpers gestionados.
pub fn is_root_helper(name: &str) -> bool {
    ROOT_HELPERS.contains(&name)
}

/// Alias de [`is_root_helper`].
pub fn is_helper(name: &str) -> bool {
    is_root_helper(name)
}

/// Alias de [`is_root_helper`].
pub fn valid_helper(name: &str) -> bool {
    is_root_helper(name)
}

/// Ruta absoluta de destino de un helper.
pub fn helper_path(name: &str) -> String {
    format!("{HELPER_DIR}/{name}")
}

/// Alias de [`helper_path`].
pub fn destination(name: &str) -> String {
    helper_path(name)
}

/// Alias de [`helper_path`].
pub fn helper_destination(name: &str) -> String {
    helper_path(name)
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

/// Alias de [`install_helper`].
pub fn install_root_helper(runner: &Runner, source_dir: &str, name: &str) -> NasResult<()> {
    install_helper(runner, source_dir, name)
}

/// Alias de [`install_helper`].
pub fn install_one(runner: &Runner, source_dir: &str, name: &str) -> NasResult<()> {
    install_helper(runner, source_dir, name)
}

/// Instala todos los helpers root desde `source_dir`.
pub fn install_helpers(runner: &Runner, source_dir: &str) -> NasResult<()> {
    for name in ROOT_HELPERS {
        install_helper(runner, source_dir, name)?;
    }
    Ok(())
}

/// Alias de [`install_helpers`].
pub fn install_root_helpers(runner: &Runner, source_dir: &str) -> NasResult<()> {
    install_helpers(runner, source_dir)
}

/// Alias de [`install_helpers`].
pub fn install_all_helpers(runner: &Runner, source_dir: &str) -> NasResult<()> {
    install_helpers(runner, source_dir)
}

/// Alias de [`install_helpers`].
pub fn install_all(runner: &Runner, source_dir: &str) -> NasResult<()> {
    install_helpers(runner, source_dir)
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

/// Alias de [`set_helper_permissions`].
pub fn set_permissions(runner: &Runner, path: &str) -> NasResult<()> {
    set_helper_permissions(runner, path)
}

/// Alias de [`set_helper_permissions`].
pub fn chmod_helper(runner: &Runner, path: &str) -> NasResult<()> {
    set_helper_permissions(runner, path)
}

/// Alias de [`set_helper_permissions`].
pub fn fix_permissions(runner: &Runner, path: &str) -> NasResult<()> {
    set_helper_permissions(runner, path)
}

/// Asigna permisos seguros a todos los helpers root.
pub fn set_all_helper_permissions(runner: &Runner) -> NasResult<()> {
    for name in ROOT_HELPERS {
        set_helper_permissions(runner, &helper_path(name))?;
    }
    Ok(())
}

/// Alias de [`set_all_helper_permissions`].
pub fn set_all_permissions(runner: &Runner) -> NasResult<()> {
    set_all_helper_permissions(runner)
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

/// Alias de [`validate_visudo`].
pub fn validate_sudoers(runner: &Runner, sudoers_path: &str) -> NasResult<()> {
    validate_visudo(runner, sudoers_path)
}

/// Alias de [`validate_visudo`].
pub fn visudo_check(runner: &Runner, sudoers_path: &str) -> NasResult<()> {
    validate_visudo(runner, sudoers_path)
}

/// Alias de [`validate_visudo`].
pub fn check_visudo(runner: &Runner, sudoers_path: &str) -> NasResult<()> {
    validate_visudo(runner, sudoers_path)
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

/// Alias de [`visudo_ok`].
pub fn sudoers_valid(runner: &Runner, sudoers_path: &str) -> NasResult<bool> {
    visudo_ok(runner, sudoers_path)
}

/// Alias de [`visudo_ok`].
pub fn is_sudoers_valid(runner: &Runner, sudoers_path: &str) -> NasResult<bool> {
    visudo_ok(runner, sudoers_path)
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

/// Alias de [`install_sudoers`].
pub fn write_sudoers(runner: &Runner, content: &str, path: &str) -> NasResult<()> {
    install_sudoers(runner, content, path)
}

/// Alias de [`install_sudoers`].
pub fn apply_sudoers(runner: &Runner, content: &str, path: &str) -> NasResult<()> {
    install_sudoers(runner, content, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lista_helpers_contiene_los_seis_esperados() {
        assert_eq!(ROOT_HELPERS.len(), 6);
        assert!(ROOT_HELPERS.contains(&"nas-samba-apply"));
        assert!(ROOT_HELPERS.contains(&"nas-share-acl"));
        assert!(ROOT_HELPERS.contains(&"nas-accounts"));
        assert!(ROOT_HELPERS.contains(&"nas-backup-manage"));
        assert!(ROOT_HELPERS.contains(&"nas-storage-apply"));
        assert!(ROOT_HELPERS.contains(&"nas-terminal"));
    }

    #[test]
    fn valida_nombres_de_helpers() {
        assert!(is_root_helper("nas-terminal"));
        assert!(!is_root_helper("bash"));
        assert!(valid_helper("nas-accounts"));
    }

    #[test]
    fn construye_rutas_de_destino() {
        assert_eq!(helper_path("nas-terminal"), "/usr/local/sbin/nas-terminal");
        assert_eq!(destination("nas-accounts"), "/usr/local/sbin/nas-accounts");
        assert_eq!(
            helper_destination("nas-samba-apply"),
            "/usr/local/sbin/nas-samba-apply"
        );
    }

    #[test]
    fn expone_constantes_y_alias() {
        assert_eq!(HELPER_DIR, "/usr/local/sbin");
        assert_eq!(SUDOERS_FILE, "/etc/sudoers.d/nas-web-rust");
        assert_eq!(helpers(), root_helpers());
        assert_eq!(helper_names().len(), 6);
    }
}
