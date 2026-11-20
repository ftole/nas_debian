//! Helpers para creación de directorios compartidos y aplicación de ACLs POSIX.

use std::path::Path;
use std::time::Duration;

use nas_core::{NasError, NasResult};
use nas_exec::{CommandSpec, Runner};
use regex::Regex;

/// Valida el nombre de un recurso compartido (`^[A-Za-z0-9_-]{1,32}\$?$`).
pub fn is_valid_share_name(name: &str) -> bool {
    let re = Regex::new(r"^[A-Za-z0-9_-]{1,32}\$?$").expect("regex válido");
    re.is_match(name)
}

/// Valida el nombre de una unidad (`^[A-Za-z0-9_-]{1,32}$`).
pub fn is_valid_unit_name(unit: &str) -> bool {
    let re = Regex::new(r"^[A-Za-z0-9_-]{1,32}$").expect("regex válido");
    re.is_match(unit)
}

/// Crea el directorio de un recurso compartido con permisos `2770` y pertenencia `root:grp_samba`.
pub fn create_share_dir(runner: &Runner, name: &str, unit: Option<&str>) -> NasResult<String> {
    let name = name.trim();
    if !is_valid_share_name(name) {
        return Err(NasError::Invalid(format!(
            "nombre de recurso inválido: '{name}'"
        )));
    }

    let unit = unit.map(str::trim).unwrap_or("principal");
    let unit = if unit.is_empty() { "principal" } else { unit };
    if !is_valid_unit_name(unit) {
        return Err(NasError::Invalid(format!(
            "unidad de almacenamiento inválida: '{unit}'"
        )));
    }

    let base = if unit == "principal" {
        "/srv/nas".to_string()
    } else {
        format!("/srv/nas/unidades/{unit}")
    };

    let dir = format!("{base}/{name}");
    if !dir.starts_with("/srv/nas") || dir.contains("..") {
        return Err(NasError::Denied("ruta fuera de /srv/nas".into()));
    }

    let dir_path = Path::new(&dir);
    std::fs::create_dir_all(dir_path)
        .map_err(|e| NasError::Denied(format!("no se pudo crear el directorio {dir}: {e}")))?;

    runner
        .run(
            &CommandSpec::new("chown")
                .args(["-R", "root:grp_samba", &dir])
                .timeout(Duration::from_secs(30)),
        )?
        .ensure_success()?;

    runner
        .run(
            &CommandSpec::new("chmod")
                .args(["2770", &dir])
                .timeout(Duration::from_secs(30)),
        )?
        .ensure_success()?;

    Ok(dir)
}

/// Aplica o revoca ACLs POSIX sobre un recurso compartido en `/srv/nas`.
pub fn apply_share_acl(
    runner: &Runner,
    path: &str,
    principal: &str,
    level: &str,
) -> NasResult<String> {
    let path = path.trim();
    if !path.starts_with("/srv/nas/") || path.contains("..") {
        return Err(NasError::Denied(
            "ruta no permitida (debe estar dentro de /srv/nas/)".into(),
        ));
    }

    let dir_path = Path::new(path);
    if !dir_path.is_dir() {
        return Err(NasError::NotFound(format!(
            "no existe el directorio del recurso: {path}"
        )));
    }

    let principal = principal.trim().trim_start_matches('@');
    let group_re = Regex::new(r"^grp_[a-z0-9_-]{1,28}$").expect("regex válido");
    let user_re = Regex::new(r"^[a-z_][a-z0-9_-]{0,31}$").expect("regex válido");

    let entry = if group_re.is_match(principal) {
        format!("g:{principal}")
    } else if user_re.is_match(principal) {
        format!("u:{principal}")
    } else {
        return Err(NasError::Invalid(format!(
            "principal inválido: '{principal}'"
        )));
    };

    let level = level.trim().to_ascii_lowercase();
    match level.as_str() {
        "none" => {
            // La revocación es best-effort: si el principal no constaba en la ACL el
            // comando falla con "no such entry" y no debe convertirlo en fallo globa.
            let _ = runner.run(
                &CommandSpec::new("setfacl")
                    .args(["-x", &entry, path])
                    .timeout(Duration::from_secs(60)),
            );
            let _ = runner.run(
                &CommandSpec::new("setfacl")
                    .args(["-d", "-x", &entry, path])
                    .timeout(Duration::from_secs(60)),
            );
            Ok(format!("{path} {entry} none"))
        }
        "read" => {
            let perms = format!("{entry}:r-x");
            runner
                .run(
                    &CommandSpec::new("setfacl")
                        .args(["-m", &perms, path])
                        .timeout(Duration::from_secs(60)),
                )?
                .ensure_success()?;
            runner
                .run(
                    &CommandSpec::new("setfacl")
                        .args(["-d", "-m", &perms, path])
                        .timeout(Duration::from_secs(60)),
                )?
                .ensure_success()?;
            Ok(format!("{path} {entry} read"))
        }
        "write" => {
            let perms = format!("{entry}:rwx");
            runner
                .run(
                    &CommandSpec::new("setfacl")
                        .args(["-m", &perms, path])
                        .timeout(Duration::from_secs(60)),
                )?
                .ensure_success()?;
            runner
                .run(
                    &CommandSpec::new("setfacl")
                        .args(["-d", "-m", &perms, path])
                        .timeout(Duration::from_secs(60)),
                )?
                .ensure_success()?;
            Ok(format!("{path} {entry} write"))
        }
        _ => Err(NasError::Invalid(format!(
            "nivel inválido: '{level}' (esperado read|write|none)"
        ))),
    }
}

fn recycle_find_args<'a>(root: &'a str, target_type: &'a str, extra: &[&'a str]) -> Vec<&'a str> {
    let mut args = vec![
        root,
        "(",
        "-name",
        "BACKUPS_HISTORICOS",
        "-o",
        "-name",
        "BACKUPS_PERMANENTES",
        "-o",
        "-name",
        "LOGS_BACKUP",
        "-o",
        "-name",
        ".nas_staging",
        ")",
        "-prune",
        "-o",
        "-type",
        target_type,
        "-path",
        "*/.recycle/*",
    ];
    args.extend_from_slice(extra);
    args
}

/// Purga archivos en las papeleras de reciclaje Samba (`.recycle`) dentro de `data_root`
/// con más de `days` días, podando árboles históricos/logs de backups para evitar
/// escaneos masivos innecesarios ni alterar snapshots.
pub fn purge_recycle_in(runner: &Runner, data_root: &Path, days: u32) -> NasResult<String> {
    let days = days.max(1);
    let mtime_arg = format!("+{days}");
    let root_str = data_root.to_string_lossy();

    // 1. Eliminar archivos mayores a `days` días en .recycle podando BACKUPS_* / LOGS_BACKUP / .nas_staging.
    //    No usar `-delete` junto a `-prune` porque GNU find activa `-depth` e invalida `-prune`.
    let file_args = recycle_find_args(
        root_str.as_ref(),
        "f",
        &["-mtime", &mtime_arg, "-exec", "rm", "-f", "{}", "+"],
    );
    let purge_files = runner.run(
        &CommandSpec::new("find")
            .args(file_args)
            .timeout(Duration::from_secs(120)),
    );

    // 2. Eliminar directorios vacíos remanentes en .recycle (best-effort)
    let dir_args = recycle_find_args(
        root_str.as_ref(),
        "d",
        &["-empty", "-exec", "rmdir", "{}", "+"],
    );
    let _ = runner.run(
        &CommandSpec::new("find")
            .args(dir_args)
            .timeout(Duration::from_secs(60)),
    );

    match purge_files {
        Ok(res) if res.is_success() => Ok(format!(
            "Purga de papeleras Samba (.recycle) completada con éxito (antigüedad > {days} días)"
        )),
        Ok(res) => Err(NasError::Command {
            command: res.command,
            code: res.code,
            stderr: res.stderr,
        }),
        Err(e) => Err(e),
    }
}

/// Purga archivos en las papeleras de reciclaje Samba (.recycle) con más de `days` días.
pub fn purge_recycle(runner: &Runner, days: u32) -> NasResult<String> {
    let data_root = nas_core::NasPaths::from_env().data_root;
    purge_recycle_in(runner, &data_root, days)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validacion_de_nombres_de_recursos() {
        assert!(is_valid_share_name("SISTEMAS"));
        assert!(is_valid_share_name("BACKUPS_WINDOWS$"));
        assert!(is_valid_share_name("campana_uno"));
        assert!(!is_valid_share_name("../escape"));
        assert!(!is_valid_share_name(""));
        assert!(!is_valid_share_name(
            "nombre_demasiado_largo_que_excede_los_treinta_y_dos_caracteres_totales"
        ));
    }

    #[test]
    fn validacion_de_nombres_de_unidades() {
        assert!(is_valid_unit_name("principal"));
        assert!(is_valid_unit_name("u1"));
        assert!(is_valid_unit_name("unidad_extra"));
        assert!(!is_valid_unit_name("u1$"));
        assert!(!is_valid_unit_name("../u1"));
    }

    #[test]
    fn rechaza_rutas_fuera_de_srv_nas() {
        let runner = Runner::new();
        let err = apply_share_acl(&runner, "/etc/samba", "grp_samba", "read").unwrap_err();
        assert!(err.to_string().contains("ruta no permitida"));
    }

    #[test]
    fn rechaza_principales_invalidos() {
        let runner = Runner::new();
        let err =
            apply_share_acl(&runner, "/srv/nas/SISTEMAS", "usuario;malicioso", "read").unwrap_err();
        assert!(
            err.to_string().contains("principal inválido") || err.to_string().contains("no existe")
        );
    }

    #[test]
    fn purge_recycle_poda_respaldos_y_conserva_snapshots_historicos() {
        let base = std::env::temp_dir().join(format!(
            "nas_purge_recycle_test_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        let share_recycle = base.join("SISTEMAS/.recycle");
        let unit_share_recycle = base.join("unidades/u1/CONTABILIDAD/.recycle");
        let backup_recycle = base.join("BACKUPS_HISTORICOS/t1/snapshot_1/.recycle");
        let unit_backup_recycle = base.join("unidades/u1/BACKUPS_PERMANENTES/t1/FULL_1/.recycle");

        for dir in [
            &share_recycle,
            &unit_share_recycle,
            &backup_recycle,
            &unit_backup_recycle,
        ] {
            std::fs::create_dir_all(dir).expect("mkdir recycle");
            std::fs::write(dir.join("archivo.tmp"), b"x").expect("write");
        }

        let runner = Runner::new();
        for dir in [
            &share_recycle,
            &unit_share_recycle,
            &backup_recycle,
            &unit_backup_recycle,
        ] {
            let file = dir.join("archivo.tmp");
            let _ = runner.run(&CommandSpec::new("touch").args([
                "-d",
                "30 days ago",
                &file.to_string_lossy(),
            ]));
        }

        purge_recycle_in(&runner, &base, 15).expect("purge_recycle_in");
        assert!(!share_recycle.join("archivo.tmp").exists());
        assert!(!unit_share_recycle.join("archivo.tmp").exists());
        assert!(backup_recycle.join("archivo.tmp").exists());
        assert!(unit_backup_recycle.join("archivo.tmp").exists());

        let _ = std::fs::remove_dir_all(&base);
    }
}
