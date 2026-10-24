//! Edición de `smb.conf` y cálculo de ACLs de recursos compartidos.

use nas_core::{NasError, NasResult};
use serde::{Deserialize, Serialize};

/// Definición de un recurso compartido para alta/edición.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShareSpec {
    /// Nombre de la sección.
    pub name: String,
    /// Ruta en disco.
    pub path: String,
    /// Solo lectura.
    pub read_only: bool,
    /// Visible en la red.
    pub browseable: bool,
    /// Acceso de invitados.
    pub guest_ok: bool,
    /// Grupos autorizados (`@grp_x`).
    pub valid_users: Vec<String>,
    /// Principales con escritura.
    pub write_list: Vec<String>,
    /// Comentario.
    pub comment: String,
}

/// Valida el nombre de un recurso (letras, números, `_` y `-`, máx. 32).
pub fn validate_name(name: &str) -> NasResult<()> {
    let trimmed = name.trim().trim_end_matches('$');
    if trimmed.is_empty() || trimmed.len() > 32 {
        return Err(NasError::Invalid(
            "el nombre del recurso debe tener 1-32 caracteres".into(),
        ));
    }
    if !trimmed
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        return Err(NasError::Invalid(
            "el nombre solo admite letras, números, '_' y '-'".into(),
        ));
    }
    if trimmed.eq_ignore_ascii_case("global") {
        return Err(NasError::Invalid(
            "'global' es una sección reservada".into(),
        ));
    }
    Ok(())
}

/// Valida que la ruta quede dentro de `/srv/nas`.
pub fn validate_path(path: &str) -> NasResult<()> {
    let path = path.trim();
    if !path.starts_with("/srv/nas/") {
        return Err(NasError::Denied(
            "la ruta del recurso debe estar dentro de /srv/nas".into(),
        ));
    }
    if path.contains("..") || path.contains('\0') {
        return Err(NasError::Denied("ruta inválida para el recurso".into()));
    }
    Ok(())
}

/// Valida un grupo autorizado (`@grp_...`).
pub fn validate_group(group: &str) -> NasResult<()> {
    let name = group.trim().trim_start_matches('@');
    if !name.starts_with("grp_")
        || !name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
    {
        return Err(NasError::Invalid(format!("grupo inválido: {group}")));
    }
    Ok(())
}

/// Añade o actualiza una sección en `smb.conf`, preservando el resto.
pub fn upsert_share(conf: &str, spec: &ShareSpec) -> NasResult<String> {
    validate_name(&spec.name)?;
    validate_path(&spec.path)?;
    for group in &spec.valid_users {
        validate_group(group)?;
    }
    for group in &spec.write_list {
        validate_group(group)?;
    }

    let block = render_block(spec);
    let (before, after) = split_around_section(conf, &spec.name);
    let mut output = String::new();
    output.push_str(before.trim_end());
    if !output.is_empty() {
        output.push_str("\n\n");
    }
    output.push_str(&block);
    let after = after.trim_start();
    if !after.is_empty() {
        output.push_str("\n\n");
        output.push_str(after.trim_end());
    }
    output.push('\n');
    Ok(output)
}

/// Elimina una sección de `smb.conf`.
pub fn remove_share(conf: &str, name: &str) -> NasResult<String> {
    validate_name(name)?;
    let (before, after) = split_around_section(conf, name);
    let mut output = before.trim_end().to_string();
    let after = after.trim_start();
    if !after.is_empty() {
        if !output.is_empty() {
            output.push_str("\n\n");
        }
        output.push_str(after.trim_end());
    }
    output.push('\n');
    Ok(output)
}

fn render_block(spec: &ShareSpec) -> String {
    let mut block = format!("[{}]\n", spec.name);
    block.push_str(&format!("   path = {}\n", spec.path.trim()));
    block.push_str(&format!(
        "   read only = {}\n",
        if spec.read_only { "yes" } else { "no" }
    ));
    block.push_str(&format!(
        "   browseable = {}\n",
        if spec.browseable { "yes" } else { "no" }
    ));
    block.push_str(&format!(
        "   guest ok = {}\n",
        if spec.guest_ok { "yes" } else { "no" }
    ));
    if !spec.valid_users.is_empty() {
        block.push_str(&format!(
            "   valid users = {}\n",
            spec.valid_users.join(" ")
        ));
    }
    if !spec.write_list.is_empty() {
        block.push_str(&format!(
            "   write list = {}
",
            spec.write_list.join(" ")
        ));
    }
    if !spec.comment.trim().is_empty() {
        block.push_str(&format!("   comment = {}\n", spec.comment.trim()));
    }
    block.push_str("   create mask = 0660\n");
    block.push_str("   directory mask = 2770\n");
    block.trim_end().to_string()
}

/// Devuelve (texto antes de la sección, texto después de la sección).
fn split_around_section<'a>(conf: &'a str, name: &'a str) -> (String, String) {
    let mut before = Vec::new();
    let mut after = Vec::new();
    let mut in_target = false;

    for line in conf.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            let section = trimmed[1..trimmed.len() - 1].trim();
            if section.eq_ignore_ascii_case(name) {
                in_target = true;
                continue;
            }
            if in_target {
                in_target = false;
            }
        }
        if in_target {
            after.push(line);
        } else {
            before.push(line);
        }
    }

    (before.join("\n"), after.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::{remove_share, upsert_share, validate_name, validate_path, ShareSpec};

    fn spec(name: &str) -> ShareSpec {
        ShareSpec {
            name: name.to_string(),
            path: format!("/srv/nas/{name}"),
            read_only: false,
            browseable: true,
            guest_ok: false,
            valid_users: vec!["@grp_samba".to_string()],
            write_list: Vec::new(),
            comment: "Share de prueba".to_string(),
        }
    }

    const CONF: &str = "[global]\n   workgroup = TEAM-JOFRATO\n\n[SISTEMAS]\n   path = /srv/nas/SISTEMAS\n   read only = no\n";

    #[test]
    fn agrega_una_seccion_sin_tocar_las_demas() {
        let updated = upsert_share(CONF, &spec("CONTABILIDAD")).expect("upsert");
        assert!(updated.contains("[global]"));
        assert!(updated.contains("[SISTEMAS]"));
        assert!(updated.contains("[CONTABILIDAD]"));
        assert!(updated.contains("path = /srv/nas/CONTABILIDAD"));
        assert!(updated.contains("valid users = @grp_samba"));
    }

    #[test]
    fn actualiza_una_seccion_existente_sin_duplicarla() {
        let mut edited = spec("SISTEMAS");
        edited.read_only = true;
        edited.comment = "Actualizado".to_string();

        let updated = upsert_share(CONF, &edited).expect("upsert");
        assert_eq!(updated.matches("[SISTEMAS]").count(), 1);
        assert!(updated.contains("read only = yes"));
        assert!(updated.contains("comment = Actualizado"));
    }

    #[test]
    fn elimina_la_seccion_solicitada() {
        let updated = remove_share(CONF, "SISTEMAS").expect("remove");
        assert!(!updated.contains("[SISTEMAS]"));
        assert!(updated.contains("[global]"));
    }

    #[test]
    fn valida_nombres_rutas_y_grupos() {
        assert!(validate_name("CONTABILIDAD").is_ok());
        assert!(validate_name("CONTABILIDAD$").is_ok());
        assert!(validate_name("global").is_err());
        assert!(validate_name("a b").is_err());
        assert!(validate_name("").is_err());

        assert!(validate_path("/srv/nas/SISTEMAS").is_ok());
        assert!(validate_path("/etc").is_err());
        assert!(validate_path("/srv/nas/../etc").is_err());

        assert!(super::validate_group("@grp_samba").is_ok());
        assert!(super::validate_group("root").is_err());
    }

    #[test]
    fn renderiza_write_list() {
        let mut spec = spec("ESCRITURA");
        spec.write_list = vec!["@grp_emp".to_string()];
        let updated = upsert_share(CONF, &spec).expect("upsert");
        assert!(updated.contains("write list = @grp_emp"));
    }

    #[test]
    fn rechaza_grupo_invalido_en_especificacion() {
        let mut bad = spec("X");
        bad.valid_users = vec!["root".to_string()];
        assert!(upsert_share(CONF, &bad).is_err());
    }
}
