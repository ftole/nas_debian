//! Operaciones sobre grupos corporativos (`grp_*`) y estado de cuentas.

use std::collections::HashSet;

use nas_core::{NasError, NasResult};
use nas_exec::{CommandSpec, Runner};

use super::{ensure_manageable_existing_user, is_valid_group, PROTECTED_GROUPS};

/// Crea un grupo corporativo (`grp_*`).
pub fn create_group(runner: &Runner, group: &str) -> NasResult<String> {
    let g = group.trim();
    if !is_valid_group(g) {
        return Err(NasError::Invalid(format!(
            "grupo inválido: '{g}' (debe ser grp_<nombre>)"
        )));
    }
    runner
        .run(&CommandSpec::new("groupadd").args([g]))?
        .ensure_success()?;
    Ok(format!("grupo {g} creado"))
}

/// Renombra un grupo corporativo.
pub fn rename_group(runner: &Runner, old_name: &str, new_name: &str) -> NasResult<String> {
    let old = old_name.trim();
    let new = new_name.trim();
    if !is_valid_group(old) || !is_valid_group(new) {
        return Err(NasError::Invalid("nombre de grupo inválido".into()));
    }
    if PROTECTED_GROUPS.contains(&old) {
        return Err(NasError::Denied(format!("el grupo '{old}' está protegido")));
    }
    runner
        .run(&CommandSpec::new("groupmod").args(["-n", new, old]))?
        .ensure_success()?;
    Ok(format!("grupo {old} renombrado a {new}"))
}

/// Elimina un grupo corporativo.
pub fn delete_group(runner: &Runner, group: &str) -> NasResult<String> {
    let g = group.trim();
    if !is_valid_group(g) {
        return Err(NasError::Invalid(format!("grupo inválido: '{g}'")));
    }
    if PROTECTED_GROUPS.contains(&g) {
        return Err(NasError::Denied(format!("el grupo '{g}' está protegido")));
    }
    runner
        .run(&CommandSpec::new("groupdel").args([g]))?
        .ensure_success()?;
    Ok(format!("grupo {g} eliminado"))
}

/// Añade un usuario a un grupo corporativo.
pub fn add_group_member(runner: &Runner, username: &str, group: &str) -> NasResult<String> {
    let u = username.trim();
    let g = group.trim();
    let _uid = ensure_manageable_existing_user(runner, u)?;
    if !is_valid_group(g) {
        return Err(NasError::Invalid("grupo inválido".into()));
    }
    if g == "grp_superadmin" {
        return Err(NasError::Denied(
            "el grupo grp_superadmin está protegido".into(),
        ));
    }
    runner
        .run(&CommandSpec::new("gpasswd").args(["-a", u, g]))?
        .ensure_success()?;
    Ok(format!("{u} add-member {g}"))
}

/// Quita un usuario de un grupo corporativo.
pub fn remove_group_member(runner: &Runner, username: &str, group: &str) -> NasResult<String> {
    let u = username.trim();
    let g = group.trim();
    let _uid = ensure_manageable_existing_user(runner, u)?;
    if !is_valid_group(g) {
        return Err(NasError::Invalid("grupo inválido".into()));
    }
    if g == "grp_superadmin" {
        return Err(NasError::Denied(
            "el grupo grp_superadmin está protegido".into(),
        ));
    }
    runner
        .run(&CommandSpec::new("gpasswd").args(["-d", u, g]))?
        .ensure_success()?;
    Ok(format!("{u} remove-member {g}"))
}

/// Reporta el estado TSV de las cuentas gestionables.
pub fn accounts_status(runner: &Runner) -> NasResult<String> {
    let mut smb_active = HashSet::new();
    if let Ok(out) = runner.run(&CommandSpec::new("pdbedit").args(["-L", "-w"])) {
        if out.is_success() {
            for line in out.stdout.lines() {
                let parts: Vec<&str> = line.split(':').collect();
                if parts.len() >= 4 {
                    let u = parts[0];
                    let flags = parts[3];
                    if !u.is_empty() && !flags.contains('D') {
                        smb_active.insert(u.to_string());
                    }
                }
            }
        }
    }

    let mut tsv = String::new();
    let (users, _) = crate::users::load_local();
    for u in users {
        if u.system || u.uid < 1000 || u.uid >= 60000 {
            continue;
        }
        let mut enabled = "1";
        if let Ok(out) = runner.run(&CommandSpec::new("passwd").args(["-S", &u.name])) {
            if out.stdout.split_whitespace().nth(1) == Some("L") {
                enabled = "0";
            }
        }
        let smb = if smb_active.contains(&u.name) {
            "1"
        } else {
            "0"
        };
        tsv.push_str(&format!("{}\t{}\t{}\n", u.name, enabled, smb));
    }
    Ok(tsv)
}
