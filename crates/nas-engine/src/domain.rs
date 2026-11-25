//! Integración con Active Directory (realmd/sssd).

use nas_exec::{CommandSpec, Runner, SecretInput};
use serde::Serialize;

/// Estado de la unión al dominio.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DomainStatus {
    /// Indica si las herramientas de dominio (`realmd`) están disponibles.
    pub available: bool,
    /// Indica si el equipo está unido.
    pub joined: bool,
    /// Realm detectado.
    pub realm: Option<String>,
    /// Dominio detectado.
    pub domain: Option<String>,
    /// Líneas de detalle de `realm list`.
    pub details: Vec<String>,
    /// Indica si SSSD está activo.
    pub sssd_active: bool,
}

/// Valida un nombre de dominio (FQDN simple, sin esquema ni rutas).
pub fn is_valid_domain(domain: &str) -> bool {
    let domain = domain.trim();
    if domain.is_empty() || domain.len() > 253 || domain.contains("..") {
        return false;
    }

    domain.split('.').all(|label| {
        !label.is_empty()
            && label.len() <= 63
            && label.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
            && !label.starts_with('-')
            && !label.ends_with('-')
    })
}

fn is_valid_user_segment(segment: &str) -> bool {
    !segment.is_empty()
        && segment.len() <= 64
        && !segment.starts_with('-')
        && segment
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "._-".contains(c))
}

/// Valida un usuario de dominio (`usuario`, `DOMINIO\\usuario` o `usuario@dominio.local`).
pub fn is_valid_user(user: &str) -> bool {
    let user = user.trim();
    if user.is_empty() || user.len() > 128 || user.contains('/') {
        return false;
    }
    if let Some((prefix, name)) = user.split_once('\\') {
        if name.contains('\\') || user.contains('@') {
            return false;
        }
        return is_valid_user_segment(prefix) && is_valid_user_segment(name);
    }
    if let Some((name, domain)) = user.split_once('@') {
        if domain.contains('@') {
            return false;
        }
        return is_valid_user_segment(name) && is_valid_domain(domain);
    }
    is_valid_user_segment(user)
}

/// Interpreta la salida de `realm list`.
pub fn parse_realm_list(output: &str) -> DomainStatus {
    let mut realm = None;
    let mut domain = None;
    let mut details = Vec::new();
    let mut joined = false;

    for line in output.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        details.push(line.to_string());

        if let Some(value) = line.strip_prefix("realm-name:") {
            realm = Some(value.trim().to_string());
        } else if let Some(value) = line.strip_prefix("domain-name:") {
            domain = Some(value.trim().to_string());
        } else if let Some(value) = line.strip_prefix("configured:") {
            if value.contains("kerberos-member") || value.contains("sssd") {
                joined = true;
            }
        }
    }

    DomainStatus {
        available: true,
        joined,
        realm,
        domain,
        details,
        sssd_active: false,
    }
}

/// Consulta el estado del dominio en el servidor.
pub fn status(runner: &Runner) -> DomainStatus {
    let realm_res = runner.run(&CommandSpec::new("realm").arg("list"));
    let mut parsed = match realm_res {
        Ok(output) if output.is_success() => parse_realm_list(&output.stdout),
        Ok(output) => {
            let mut s = parse_realm_list(&output.stdout);
            s.available = true;
            s
        }
        Err(_) => DomainStatus {
            available: false,
            joined: false,
            realm: None,
            domain: None,
            details: vec![
                "Herramientas de Active Directory (realmd/adcli/sssd) no disponibles.".to_string(),
            ],
            sssd_active: false,
        },
    };

    parsed.sssd_active = runner
        .run(&CommandSpec::new("systemctl").args(["is-active", "sssd"]))
        .map(|output| output.stdout == "active")
        .unwrap_or(false);

    parsed
}

/// Plan de unión al dominio (la contraseña viaja por `stdin`).
pub fn join_plan(domain: &str, user: &str) -> Vec<Vec<String>> {
    vec![
        vec![
            "realm".into(),
            "discover".into(),
            "--verbose".into(),
            domain.into(),
        ],
        vec![
            "realm".into(),
            "join".into(),
            "--user".into(),
            user.into(),
            domain.into(),
        ],
        vec![
            "systemctl".into(),
            "enable".into(),
            "--now".into(),
            "sssd".into(),
        ],
    ]
}

/// Plan de salida del dominio.
pub fn leave_plan() -> Vec<Vec<String>> {
    vec![
        vec!["realm".into(), "leave".into()],
        vec![
            "systemctl".into(),
            "disable".into(),
            "--now".into(),
            "sssd".into(),
        ],
    ]
}

/// Valida una unidad organizativa LDAP opcional (ej. `OU=Servidores,DC=empresa,DC=local`).
pub fn is_valid_ou(ou: &str) -> bool {
    let ou = ou.trim();
    !ou.is_empty()
        && ou.len() <= 256
        && !ou.starts_with('-')
        && !ou.chars().any(char::is_control)
        && ou
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '=' | ',' | '.' | '_' | '-' | ' '))
}

/// Ejecuta la unión con la contraseña por `stdin` (nunca en `argv`).
pub fn join(runner: &Runner, domain: &str, user: &str, password: &str) -> Result<(), String> {
    join_with_ou(runner, domain, user, password, None)
}

/// Ejecuta la unión al dominio admitiendo una unidad organizativa (`--computer-ou`) opcional.
///
/// Tras `realm join` habilita y arranca `sssd`; el fallo del demonio se
/// propaga porque el dominio no es usable sin él.
pub fn join_with_ou(
    runner: &Runner,
    domain: &str,
    user: &str,
    password: &str,
    ou: Option<&str>,
) -> Result<(), String> {
    if !is_valid_domain(domain) {
        return Err(format!("dominio inválido: {domain}"));
    }
    if !is_valid_user(user) {
        return Err(format!("usuario inválido: {user}"));
    }
    if password.chars().any(char::is_control) {
        return Err("la contraseña contiene caracteres de control no permitidos".into());
    }
    let clean_ou = ou.map(str::trim).filter(|s| !s.is_empty());
    if let Some(ou_val) = clean_ou {
        if !is_valid_ou(ou_val) {
            return Err("unidad organizativa (OU) inválida".into());
        }
    }

    let mut args: Vec<String> = vec!["join".into(), "--user".into(), user.into()];
    if let Some(ou_val) = clean_ou {
        args.push("--computer-ou".into());
        args.push(ou_val.into());
    }
    args.push(domain.into());

    let safe_runner = runner.clone().with_secret(password);
    let output = safe_runner
        .run(
            &CommandSpec::new("realm")
                .args(args)
                .sudo(nas_exec::SudoMode::Passwordless)
                .stdin_secret(SecretInput::new(password))
                .timeout(std::time::Duration::from_secs(180)),
        )
        .map_err(|error| error.to_string())?;

    if !output.is_success() {
        return Err(output.stderr);
    }

    for args in [["enable", "sssd"], ["restart", "sssd"]] {
        let svc = runner
            .run(
                &CommandSpec::new("systemctl")
                    .args(args)
                    .sudo(nas_exec::SudoMode::Passwordless)
                    .timeout(std::time::Duration::from_secs(60)),
            )
            .map_err(|error| error.to_string())?;
        if !svc.is_success() {
            return Err(format!(
                "realm join ok, pero {} sssd falló: {}",
                args[0],
                if svc.stderr.is_empty() {
                    svc.stdout
                } else {
                    svc.stderr
                }
            ));
        }
    }
    Ok(())
}

/// Ejecuta la salida del dominio y detiene el demonio SSSD.
pub fn leave(runner: &Runner) -> Result<(), String> {
    leave_with_credentials(runner, None, None)
}

/// Ejecuta la salida del dominio pasando credenciales opcionales por `stdin` (`--remove` / `--user`).
pub fn leave_with_credentials(
    runner: &Runner,
    user: Option<&str>,
    password: Option<&str>,
) -> Result<(), String> {
    let clean_user = user.map(str::trim).filter(|s| !s.is_empty());
    let clean_pass = password.filter(|s| !s.is_empty());
    if let Some(u) = clean_user {
        if !is_valid_user(u) {
            return Err(format!("usuario inválido: {u}"));
        }
    }
    if let Some(p) = clean_pass {
        if p.chars().any(char::is_control) {
            return Err("la contraseña contiene caracteres de control no permitidos".into());
        }
    }

    let mut args: Vec<String> = vec!["leave".into()];
    if let Some(u) = clean_user {
        args.push("--remove".into());
        args.push("--user".into());
        args.push(u.into());
    }
    let mut spec = CommandSpec::new("realm")
        .args(args)
        .sudo(nas_exec::SudoMode::Passwordless)
        .timeout(std::time::Duration::from_secs(120));
    let exec_runner = if let Some(p) = clean_pass {
        spec = spec.stdin_secret(SecretInput::new(p));
        runner.clone().with_secret(p)
    } else {
        runner.clone()
    };

    let output = exec_runner.run(&spec).map_err(|error| error.to_string())?;

    if !output.is_success() {
        return Err(output.stderr);
    }

    // Best-effort en stop/disable: si el demo ya estaba parado no es fallo.
    for args in [["stop", "sssd"], ["disable", "sssd"]] {
        let _ = runner.run(
            &CommandSpec::new("systemctl")
                .args(args)
                .sudo(nas_exec::SudoMode::Passwordless)
                .timeout(std::time::Duration::from_secs(60)),
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        is_valid_domain, is_valid_user, join, join_plan, leave_plan, parse_realm_list, Runner,
    };

    const REALM_LIST: &str = "\
team.local
  type: kerberos
  realm-name: TEAM.LOCAL
  domain-name: team.local
  configured: kerberos-member
  server-software: active-directory
  client-software: sssd
";

    #[test]
    fn interpreta_union_al_dominio() {
        let status = parse_realm_list(REALM_LIST);
        assert!(status.joined);
        assert_eq!(status.realm.as_deref(), Some("TEAM.LOCAL"));
        assert_eq!(status.domain.as_deref(), Some("team.local"));
        assert!(!status.details.is_empty());
    }

    #[test]
    fn salida_vacia_no_esta_unida() {
        let status = parse_realm_list("");
        assert!(!status.joined);
        assert!(status.realm.is_none());
    }

    #[test]
    fn valida_dominios_y_usuarios() {
        assert!(is_valid_domain("team.local"));
        assert!(is_valid_domain("corp.ead-col.com"));
        assert!(!is_valid_domain("team..local"));
        assert!(!is_valid_domain("-team.local"));
        assert!(!is_valid_domain("team.local/../../etc"));
        assert!(!is_valid_domain(""));

        assert!(is_valid_user("administrador"));
        assert!(is_valid_user("TEAM\\administrador"));
        assert!(is_valid_user("admin@team.local"));
        assert!(is_valid_user("juan.perez"));
        assert!(!is_valid_user(""));
        assert!(!is_valid_user("a;rm -rf /"));
        assert!(!is_valid_user(";id\\administrador"));
        assert!(!is_valid_user("-oOption\\administrador"));
        assert!(!is_valid_user("TEAM/administrador"));
        assert!(!is_valid_user("A\\B\\C"));
        assert!(!is_valid_user("-administrador"));
    }

    #[test]
    fn planes_incluyen_comandos_esperados() {
        let join = join_plan("team.local", "administrador");
        assert_eq!(join.len(), 3);
        assert_eq!(join[0][0], "realm");
        assert_eq!(join[1][1], "join");
        assert!(join[1].contains(&"team.local".to_string()));

        let leave = leave_plan();
        assert_eq!(leave[0], vec!["realm", "leave"]);
    }

    #[test]
    fn join_rechaza_caracteres_de_control_en_contrasena() {
        let runner = Runner::new();
        let err = join(&runner, "team.local", "administrador", "Clave\n123").unwrap_err();
        assert!(err.contains("caracteres de control"));
    }
}
