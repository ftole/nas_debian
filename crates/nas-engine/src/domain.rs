//! Integración con Active Directory (realmd/sssd).

use nas_exec::{CommandSpec, Runner, SecretInput};
use serde::Serialize;

/// Estado de la unión al dominio.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DomainStatus {
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

/// Valida un usuario de dominio (`usuario` o `DOMINIO\\usuario`).
pub fn is_valid_user(user: &str) -> bool {
    let name = user.rsplit(['\\', '/']).next().unwrap_or(user);
    !name.is_empty()
        && name.len() <= 64
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "._-".contains(c))
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
        joined,
        realm,
        domain,
        details,
        sssd_active: false,
    }
}

/// Consulta el estado del dominio en el servidor.
pub fn status(runner: &Runner) -> DomainStatus {
    let mut parsed = runner
        .run(&CommandSpec::new("realm").arg("list"))
        .ok()
        .filter(|output| output.is_success())
        .map(|output| parse_realm_list(&output.stdout))
        .unwrap_or(DomainStatus {
            joined: false,
            realm: None,
            domain: None,
            details: Vec::new(),
            sssd_active: false,
        });

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

/// Ejecuta la unión con la contraseña por `stdin` (nunca en `argv`).
pub fn join(runner: &Runner, domain: &str, user: &str, password: &str) -> Result<(), String> {
    if !is_valid_domain(domain) {
        return Err(format!("dominio inválido: {domain}"));
    }
    if !is_valid_user(user) {
        return Err(format!("usuario inválido: {user}"));
    }

    let output = runner
        .run(
            &CommandSpec::new("realm")
                .args(["join", "--user", user, domain])
                .sudo(nas_exec::SudoMode::Passwordless)
                .stdin_secret(SecretInput::new(password))
                .timeout(std::time::Duration::from_secs(180)),
        )
        .map_err(|error| error.to_string())?;

    if output.is_success() {
        Ok(())
    } else {
        Err(output.stderr)
    }
}

/// Ejecuta la salida del dominio.
pub fn leave(runner: &Runner) -> Result<(), String> {
    let output = runner
        .run(
            &CommandSpec::new("realm")
                .arg("leave")
                .sudo(nas_exec::SudoMode::Passwordless)
                .timeout(std::time::Duration::from_secs(120)),
        )
        .map_err(|error| error.to_string())?;

    if output.is_success() {
        Ok(())
    } else {
        Err(output.stderr)
    }
}

#[cfg(test)]
mod tests {
    use super::{is_valid_domain, is_valid_user, join_plan, leave_plan, parse_realm_list};

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
        assert!(is_valid_user("juan.perez"));
        assert!(!is_valid_user(""));
        assert!(!is_valid_user("a;rm -rf /"));
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
}
