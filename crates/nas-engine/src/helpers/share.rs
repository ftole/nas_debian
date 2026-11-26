//! Helpers para creación de directorios compartidos y aplicación de ACLs POSIX.

use std::path::Path;
use std::time::Duration;

use nas_core::{NasError, NasResult};
use nas_exec::{CommandSpec, Runner};

mod privileged_delete;
pub use privileged_delete::{
    is_valid_os_username, privileged_delete_file, PrivilegedDeleteRequest, PrivilegedFileAction,
};

/// Valida el nombre de un recurso compartido (`^[A-Za-z0-9_-]{1,32}\$?$`).
pub fn is_valid_share_name(name: &str) -> bool {
    crate::validation::is_valid_share_name(name)
}

/// Valida el nombre de una unidad (`^[A-Za-z0-9_-]{1,32}$`).
pub fn is_valid_unit_name(unit: &str) -> bool {
    crate::validation::is_valid_unit_name(unit)
}

const PROTECTED_SHARE_SEGMENTS: [&str; 6] = [
    "BACKUPS_HISTORICOS",
    "BACKUPS_PERMANENTES",
    "LOGS_BACKUP",
    ".trash",
    ".nas_staging",
    "unidades",
];

fn is_protected_share_path(path: &str) -> bool {
    let Some(rel) = path.strip_prefix("/srv/nas/") else {
        return true;
    };
    let mut comps: Vec<String> = Vec::new();
    for comp in Path::new(rel).components() {
        match comp {
            std::path::Component::Normal(s) => comps.push(s.to_string_lossy().to_string()),
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir
            | std::path::Component::RootDir
            | std::path::Component::Prefix(_) => return true,
        }
    }
    if comps.is_empty() {
        return true;
    }
    if comps[0] == "unidades" {
        if comps.len() < 3 {
            return true;
        }
        return PROTECTED_SHARE_SEGMENTS.contains(&comps[2].as_str());
    }
    PROTECTED_SHARE_SEGMENTS.contains(&comps[0].as_str())
}

fn ensure_no_symlink_under_srv_nas(path: &Path) -> NasResult<()> {
    let base = Path::new("/srv/nas");
    let Ok(rel) = path.strip_prefix(base) else {
        return Err(NasError::Denied("ruta fuera de /srv/nas".into()));
    };
    let mut cur = base.to_path_buf();
    for comp in rel.components() {
        match comp {
            std::path::Component::Normal(part) => {
                cur.push(part);
                if let Ok(meta) = std::fs::symlink_metadata(&cur) {
                    if meta.file_type().is_symlink() {
                        return Err(NasError::Denied(format!(
                            "ruta con enlace simbólico no permitido: {}",
                            cur.display()
                        )));
                    }
                }
            }
            std::path::Component::ParentDir => {
                return Err(NasError::Denied(
                    "recorrido de directorios no permitido".into(),
                ));
            }
            _ => {}
        }
    }
    Ok(())
}

/// Crea el directorio de un recurso compartido con permisos `2770` y pertenencia `root:grp_samba`.
pub fn create_share_dir(runner: &Runner, name: &str, unit: Option<&str>) -> NasResult<String> {
    let name = name.trim();
    if !is_valid_share_name(name) {
        return Err(NasError::Invalid(format!(
            "nombre de recurso inválido: '{name}'"
        )));
    }
    if PROTECTED_SHARE_SEGMENTS.contains(&name.trim_end_matches('$')) {
        return Err(NasError::Denied(format!(
            "nombre de recurso reservado o protegido: '{name}'"
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
    if !dir.starts_with("/srv/nas/") || dir.contains("..") || is_protected_share_path(&dir) {
        return Err(NasError::Denied(
            "ruta fuera de /srv/nas o directorio protegido del sistema".into(),
        ));
    }

    let dir_path = Path::new(&dir);
    ensure_no_symlink_under_srv_nas(dir_path)?;
    std::fs::create_dir_all(dir_path)
        .map_err(|e| NasError::Denied(format!("no se pudo crear el directorio {dir}: {e}")))?;
    ensure_no_symlink_under_srv_nas(dir_path)?;
    if let (Ok(canon_root), Ok(canon_dir)) = (
        std::fs::canonicalize("/srv/nas"),
        std::fs::canonicalize(dir_path),
    ) {
        if !canon_dir.starts_with(&canon_root) || canon_dir == canon_root {
            return Err(NasError::Denied(
                "el directorio del recurso escapa de /srv/nas".into(),
            ));
        }
    }

    let had_entries = std::fs::read_dir(dir_path).is_ok_and(|mut it| it.next().is_some());

    // Evitar tormenta recursiva de metadatos (DT-M15): en carpetas ya pobladas
    // ajustamos solo el contenedor raíz y propagamos ACLs mínimas sin seguir symlinks.
    let chown_args: Vec<&str> = if had_entries {
        vec!["root:grp_samba", "--", &dir]
    } else {
        vec!["-R", "root:grp_samba", "--", &dir]
    };
    runner
        .run(
            &CommandSpec::new("chown")
                .args(chown_args)
                .timeout(Duration::from_secs(30)),
        )?
        .ensure_success()?;

    runner
        .run(
            &CommandSpec::new("chmod")
                .args(["2770", "--", &dir])
                .timeout(Duration::from_secs(30)),
        )?
        .ensure_success()?;

    let _ = runner.run(
        &CommandSpec::new("setfacl")
            .args([
                "-m",
                "u:nas-web:rwx,g:grp_samba:rwx,m::rwx,d:u:nas-web:rwx,d:g:grp_samba:rwx,d:m::rwx",
                "--",
                &dir,
            ])
            .timeout(Duration::from_secs(30)),
    );
    if had_entries {
        let _ = runner.run(
            &CommandSpec::new("setfacl")
                .args([
                    "-RP",
                    "-m",
                    "u:nas-web:rwX,g:grp_samba:rwX,d:u:nas-web:rwx,d:g:grp_samba:rwx",
                    "--",
                    &dir,
                ])
                .timeout(Duration::from_secs(60)),
        );
    }

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
    if !path.starts_with("/srv/nas/") || path.contains("..") || path.contains('\0') {
        return Err(NasError::Denied(
            "ruta no permitida (debe estar dentro de /srv/nas/)".into(),
        ));
    }
    if is_protected_share_path(path) {
        return Err(NasError::Denied(
            "ruta protegida del sistema: no se permite modificar ACLs sobre respaldos, papelera o staging".into(),
        ));
    }

    let dir_path = Path::new(path);
    ensure_no_symlink_under_srv_nas(dir_path)?;
    if !dir_path.is_dir() {
        return Err(NasError::NotFound(format!(
            "no existe el directorio del recurso: {path}"
        )));
    }
    if let (Ok(canon_root), Ok(canon_dir)) = (
        std::fs::canonicalize("/srv/nas"),
        std::fs::canonicalize(dir_path),
    ) {
        if !canon_dir.starts_with(&canon_root) || canon_dir == canon_root {
            return Err(NasError::Denied(
                "la ruta del recurso escapa de /srv/nas".into(),
            ));
        }
        if let Ok(canon_rel) = canon_dir.strip_prefix(&canon_root) {
            let normalized = format!("/srv/nas/{}", canon_rel.to_string_lossy());
            if is_protected_share_path(&normalized) {
                return Err(NasError::Denied(
                    "ruta protegida del sistema: no se permite modificar ACLs sobre respaldos, papelera o staging".into(),
                ));
            }
        }
    }

    let principal = principal.trim().trim_start_matches('@');
    let entry = if crate::validation::is_valid_group_name(principal) {
        format!("g:{principal}")
    } else if crate::validation::is_valid_username_syntax(principal) {
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
        assert!(
            apply_share_acl(&runner, "/srv/nas/BACKUPS_HISTORICOS", "grp_samba", "write").is_err()
        );
        assert!(apply_share_acl(
            &runner,
            "/srv/nas/./BACKUPS_HISTORICOS",
            "grp_samba",
            "write"
        )
        .is_err());
        assert!(apply_share_acl(
            &runner,
            "/srv/nas/unidades/u1/./.nas_staging",
            "grp_samba",
            "write"
        )
        .is_err());
        assert!(apply_share_acl(
            &runner,
            "/srv/nas/unidades/u1/BACKUPS_PERMANENTES",
            "grp_samba",
            "write"
        )
        .is_err());
        assert!(apply_share_acl(&runner, "/srv/nas/.nas_staging", "grp_samba", "write").is_err());
        assert!(create_share_dir(&runner, "BACKUPS_HISTORICOS", None).is_err());
        assert!(create_share_dir(&runner, "unidades", None).is_err());
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
