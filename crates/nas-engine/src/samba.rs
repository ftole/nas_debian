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

/// Valida que la ruta quede dentro de `/srv/nas` y no contenga caracteres de control.
pub fn validate_path(path: &str) -> NasResult<()> {
    let path = path.trim();
    if !path.starts_with("/srv/nas/") {
        return Err(NasError::Denied(
            "la ruta del recurso debe estar dentro de /srv/nas".into(),
        ));
    }
    if path.contains("..") || path.chars().any(char::is_control) {
        return Err(NasError::Denied("ruta inválida para el recurso".into()));
    }
    Ok(())
}

/// Valida el comentario del recurso evitando inyección de directivas (`\r`, `\n`, control).
pub fn validate_comment(comment: &str) -> NasResult<()> {
    if comment.len() > 256 {
        return Err(NasError::Invalid(
            "el comentario no puede superar 256 caracteres".into(),
        ));
    }
    if comment.chars().any(char::is_control) {
        return Err(NasError::Invalid(
            "el comentario contiene caracteres de control no permitidos".into(),
        ));
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

/// Valida un principal autorizado en Samba (`@grp_...` o cuenta de usuario no reservada).
pub fn validate_principal(principal: &str) -> NasResult<()> {
    let trimmed = principal.trim();
    if trimmed.is_empty() {
        return Err(NasError::Invalid("principal vacío".into()));
    }
    if trimmed.starts_with('@') {
        return validate_group(trimmed);
    }
    const RESERVED: &[&str] = &[
        "root",
        "daemon",
        "bin",
        "sys",
        "sync",
        "nobody",
        "nogroup",
        "www-data",
        "nas-web",
        "sshd",
        "messagebus",
    ];
    let lower = trimmed.to_ascii_lowercase();
    if RESERVED.contains(&lower.as_str()) || lower.starts_with("systemd-") {
        return Err(NasError::Invalid(format!(
            "cuenta reservada no permitida en Samba: {trimmed}"
        )));
    }
    let mut chars = trimmed.chars();
    let Some(first) = chars.next() else {
        return Err(NasError::Invalid("principal vacío".into()));
    };
    if !(first.is_ascii_lowercase() || first == '_')
        || trimmed.len() > 32
        || !chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-')
    {
        return Err(NasError::Invalid(format!(
            "principal inválido: {principal}"
        )));
    }
    Ok(())
}

/// Añade o actualiza una sección en `smb.conf`, preservando el resto.
pub fn upsert_share(conf: &str, spec: &ShareSpec) -> NasResult<String> {
    validate_name(&spec.name)?;
    validate_path(&spec.path)?;
    validate_comment(&spec.comment)?;
    for principal in &spec.valid_users {
        validate_principal(principal)?;
    }
    for principal in &spec.write_list {
        validate_principal(principal)?;
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
    let mut block = format!("[{}]\n", spec.name.trim());
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
        let joined: Vec<&str> = spec.valid_users.iter().map(|s| s.trim()).collect();
        block.push_str(&format!("   valid users = {}\n", joined.join(" ")));
    }
    if !spec.write_list.is_empty() {
        let joined: Vec<&str> = spec.write_list.iter().map(|s| s.trim()).collect();
        block.push_str(&format!("   write list = {}\n", joined.join(" ")));
    }
    if !spec.comment.trim().is_empty() {
        block.push_str(&format!("   comment = {}\n", spec.comment.trim()));
    }
    block.push_str("   create mask = 0660\n");
    block.push_str("   directory mask = 2770\n");
    block.trim_end().to_string()
}

/// Devuelve (texto antes de la sección objetivo, texto después de la sección objetivo),
/// descartando por completo el cuerpo anterior de la sección objetivo.
fn split_around_section(conf: &str, name: &str) -> (String, String) {
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum SectionPos {
        Before,
        InTarget,
        After,
    }

    let mut before = Vec::new();
    let mut after = Vec::new();
    let mut pos = SectionPos::Before;

    for line in conf.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            let section = trimmed[1..trimmed.len() - 1].trim();
            if section.eq_ignore_ascii_case(name) {
                pos = SectionPos::InTarget;
                continue;
            }
            if pos == SectionPos::InTarget {
                pos = SectionPos::After;
            }
        }
        match pos {
            SectionPos::Before => before.push(line),
            SectionPos::InTarget => {}
            SectionPos::After => after.push(line),
        }
    }

    (before.join("\n"), after.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::{
        remove_share, upsert_share, validate_comment, validate_name, validate_path,
        validate_principal, ShareSpec,
    };

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

    #[test]
    fn actualiza_y_elimina_entre_multiples_secciones_sin_dejar_huerfanos() {
        let multi = "[global]\n   workgroup = TEAM-JOFRATO\n\n[PRIMERO]\n   path = /srv/nas/PRIMERO\n   read only = no\n\n[SISTEMAS]\n   path = /srv/nas/SISTEMAS\n   read only = no\n   comment = Viejo\n\n[ULTIMO]\n   path = /srv/nas/ULTIMO\n   read only = yes\n";
        let mut edited = spec("SISTEMAS");
        edited.read_only = true;
        edited.comment = "Actualizado".to_string();

        let updated = upsert_share(multi, &edited).expect("upsert");
        assert_eq!(updated.matches("[SISTEMAS]").count(), 1);
        assert_eq!(updated.matches("path = /srv/nas/SISTEMAS").count(), 1);
        assert!(!updated.contains("comment = Viejo"));
        assert!(updated.contains("read only = yes"));
        assert!(updated.contains("comment = Actualizado"));
        let pos_primero = updated.find("[PRIMERO]").expect("primero");
        let pos_sistemas = updated.find("[SISTEMAS]").expect("sistemas");
        let pos_ultimo = updated.find("[ULTIMO]").expect("ultimo");
        assert!(pos_primero < pos_sistemas && pos_sistemas < pos_ultimo);

        let removed = remove_share(&updated, "SISTEMAS").expect("remove");
        assert!(!removed.contains("[SISTEMAS]"));
        assert!(!removed.contains("/srv/nas/SISTEMAS"));
        assert!(!removed.contains("comment = Actualizado"));
        assert!(removed.contains("[PRIMERO]"));
        assert!(removed.contains("[ULTIMO]"));
    }

    #[test]
    fn rechaza_inyeccion_crlf_y_admite_usuarios_no_reservados() {
        assert!(validate_comment("Recursos del área contable").is_ok());
        assert!(validate_comment("Inyección\n   guest ok = yes").is_err());
        assert!(validate_comment("Inyección\r   root preexec = id").is_err());
        assert!(validate_path("/srv/nas/A\n   guest ok = yes").is_err());

        assert!(validate_principal("@grp_samba").is_ok());
        assert!(validate_principal("asesor").is_ok());
        assert!(validate_principal("empleado_1").is_ok());
        assert!(validate_principal("root").is_err());
        assert!(validate_principal("nas-web").is_err());
        assert!(validate_principal("@otro_grupo").is_err());

        let mut bad = spec("SEGURO");
        bad.comment = "ok\n   guest ok = yes\n   force user = root".to_string();
        assert!(upsert_share(CONF, &bad).is_err());

        let mut with_user = spec("MIXTO");
        with_user.valid_users = vec!["@grp_samba".to_string(), "asesor".to_string()];
        with_user.write_list = vec!["asesor".to_string()];
        let rendered = upsert_share(CONF, &with_user).expect("upsert user");
        assert!(rendered.contains("valid users = @grp_samba asesor"));
        assert!(rendered.contains("write list = asesor"));
    }
}
