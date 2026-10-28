//! Terminal web real sobre sesiones `tmux` aisladas por usuario.
//!
//! Cada sesión corre como el usuario autenticado mediante `sudo -n -u`, en un
//! socket de tmux propio (`-L nas`) y con un nombre validado. Los argumentos
//! viajan siempre separados y los textos se envían con terminador `--`.

use nas_exec::{CommandSpec, Runner, SudoMode};
use serde::Serialize;

/// Prefijo de las sesiones gestionadas por el panel.
pub const SESSION_PREFIX: &str = "nas-web-";

/// Socket de tmux dedicado al panel.
pub const TMUX_SOCKET: &str = "nas";

/// Sesión de terminal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TerminalSession {
    /// Usuario propietario.
    pub user: String,
    /// Nombre de la sesión tmux.
    pub name: String,
}

/// Valida el nombre de una sesión.
pub fn is_valid_session_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 40
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
}

/// Deriva un nombre de sesión seguro a partir del usuario.
pub fn session_name(user: &str) -> String {
    let sanitized: String = user
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    let sanitized = sanitized.trim_matches('-');
    let base = if sanitized.is_empty() {
        "invitado"
    } else {
        sanitized
    };
    let trimmed: String = base.chars().take(24).collect();
    format!("{SESSION_PREFIX}{trimmed}")
}

/// Valida el texto enviado al terminal.
pub fn is_valid_input(data: &str) -> bool {
    !data.contains('\0') && data.len() <= 4096
}

/// Valida dimensiones del terminal.
pub fn is_valid_geometry(cols: u16, rows: u16) -> bool {
    (20..=500).contains(&cols) && (5..=300).contains(&rows)
}

/// Ruta del helper root con lista blanca de acciones.
pub const HELPER: &str = "/usr/local/bin/nas";

/// Construye `sudo -n /usr/local/bin/nas helper terminal -u <usuario> <args>`.
fn helper(user: &str, args: &[&str]) -> CommandSpec {
    let mut argv: Vec<String> = vec![
        "helper".to_string(),
        "terminal".to_string(),
        "-u".to_string(),
        user.to_string(),
    ];
    argv.extend(args.iter().map(|arg| (*arg).to_string()));
    CommandSpec::new(HELPER)
        .args(argv)
        .sudo(SudoMode::Passwordless)
        .timeout(std::time::Duration::from_secs(30))
}

/// Construye `sudo -n -u <usuario> tmux -L nas <args>` (legado, sin uso).
#[allow(dead_code)]
fn tmux(user: &str, args: &[&str]) -> CommandSpec {
    let mut argv: Vec<String> = vec![
        "-n".to_string(),
        "-u".to_string(),
        user.to_string(),
        "tmux".to_string(),
        "-L".to_string(),
        TMUX_SOCKET.to_string(),
    ];
    argv.extend(args.iter().map(|arg| (*arg).to_string()));

    CommandSpec::new("sudo")
        .args(argv)
        .timeout(std::time::Duration::from_secs(20))
}

fn failure(output: nas_exec::CommandOutput) -> String {
    if output.stderr.is_empty() {
        output.stdout
    } else {
        output.stderr
    }
}

/// Indica si la sesión existe para el usuario indicado.
pub fn exists(runner: &Runner, user: &str, name: &str) -> Result<bool, String> {
    if !is_valid_session_name(name) {
        return Err(format!("sesión inválida: {name}"));
    }

    let output = runner
        .run(&helper(user, &["exists"]))
        .map_err(|error| error.to_string())?;
    Ok(output.is_success())
}

/// Crea la sesión si no existe.
pub fn start(runner: &Runner, user: &str, cols: u16, rows: u16) -> Result<TerminalSession, String> {
    if !is_valid_geometry(cols, rows) {
        return Err("geometría del terminal fuera de rango".to_string());
    }

    let name = session_name(user);
    if !is_valid_session_name(&name) {
        return Err(format!("sesión inválida: {name}"));
    }

    if exists(runner, user, &name)? {
        return Ok(TerminalSession {
            user: user.to_string(),
            name,
        });
    }

    let cols = cols.to_string();
    let rows = rows.to_string();
    let output = runner
        .run(&helper(user, &["start", &cols, &rows]))
        .map_err(|error| error.to_string())?;

    if output.is_success() {
        Ok(TerminalSession {
            user: user.to_string(),
            name,
        })
    } else {
        Err(failure(output))
    }
}

/// Envía texto a la sesión, opcionalmente seguido de Enter.
pub fn send(
    runner: &Runner,
    user: &str,
    name: &str,
    data: &str,
    enter: bool,
) -> Result<(), String> {
    if !is_valid_session_name(name) {
        return Err(format!("sesión inválida: {name}"));
    }
    if !is_valid_input(data) {
        return Err("entrada inválida para el terminal".to_string());
    }

    let action = if enter { "raw_enter" } else { "raw" };
    let output = runner
        .run(&helper(user, &[action, data]))
        .map_err(|error| error.to_string())?;
    if output.is_success() {
        Ok(())
    } else {
        Err(failure(output))
    }
}

/// Teclas especiales admitidas por el helper `nas-terminal key`.
pub fn is_allowed_key(key: &str) -> bool {
    matches!(
        key,
        "ENTER"
            | "TAB"
            | "CTRL_C"
            | "CTRL_D"
            | "CTRL_L"
            | "ESC"
            | "BACKSPACE"
            | "Up"
            | "Down"
            | "Left"
            | "Right"
    )
}

/// Envía una tecla especial validada (`TAB`, `CTRL_C`, `ENTER`, etc.) a la sesión.
pub fn send_key(runner: &Runner, user: &str, name: &str, key: &str) -> Result<(), String> {
    if !is_valid_session_name(name) {
        return Err(format!("sesión inválida: {name}"));
    }
    if !is_allowed_key(key) {
        return Err(format!("tecla especial no permitida: {key}"));
    }

    let output = runner
        .run(&helper(user, &["key", key]))
        .map_err(|error| error.to_string())?;
    if output.is_success() {
        Ok(())
    } else {
        Err(failure(output))
    }
}

/// Captura el panel y el directorio actual de la sesión.
pub fn capture_details(
    runner: &Runner,
    user: &str,
    _name: &str,
) -> Result<(String, String), String> {
    if !is_valid_session_name(&session_name(user)) {
        return Err("sesión inválida".to_string());
    }
    let output = runner
        .run(&helper(user, &["capture"]).preserve_whitespace())
        .map_err(|error| error.to_string())?;
    if !output.is_success() {
        let stderr = output.stderr.trim();
        if stderr.contains("can't find session") || stderr.contains("no server") {
            return Ok((String::new(), String::new()));
        }
        return Err(failure(output));
    }
    let mut text = String::new();
    let mut cwd = String::new();
    for line in output.stdout.lines() {
        if let Some(value) = line.strip_prefix("___NAS_CWD___") {
            cwd = value.trim().to_string();
            continue;
        }
        text.push_str(line);
        text.push('\n');
    }
    Ok((text, cwd))
}

/// Captura el contenido visible del panel.
pub fn capture(runner: &Runner, user: &str, name: &str) -> Result<String, String> {
    if !is_valid_session_name(name) {
        return Err(format!("sesión inválida: {name}"));
    }

    let output = runner
        .run(&helper(user, &["capture"]).preserve_whitespace())
        .map_err(|error| error.to_string())?;

    if output.is_success() {
        let mut text = String::new();
        for line in output.stdout.lines() {
            if line.starts_with("___NAS_CWD___") {
                continue;
            }
            text.push_str(line);
            text.push('\n');
        }
        Ok(text)
    } else if output.stderr.contains("can't find session") || output.stderr.contains("no server") {
        Ok(String::new())
    } else {
        Err(failure(output))
    }
}

/// Redimensiona la ventana de la sesión.
pub fn resize(runner: &Runner, user: &str, name: &str, cols: u16, rows: u16) -> Result<(), String> {
    if !is_valid_session_name(name) {
        return Err(format!("sesión inválida: {name}"));
    }
    if !is_valid_geometry(cols, rows) {
        return Err("geometría del terminal fuera de rango".to_string());
    }

    let cols = cols.to_string();
    let rows = rows.to_string();
    let output = runner
        .run(&helper(user, &["resize", &cols, &rows]))
        .map_err(|error| error.to_string())?;

    if output.is_success() {
        Ok(())
    } else {
        Err(failure(output))
    }
}

/// Termina la sesión.
pub fn kill(runner: &Runner, user: &str, name: &str) -> Result<(), String> {
    if !is_valid_session_name(name) {
        return Err(format!("sesión inválida: {name}"));
    }

    let output = runner
        .run(&helper(user, &["kill"]))
        .map_err(|error| error.to_string())?;

    if output.is_success() {
        Ok(())
    } else {
        Err(failure(output))
    }
}

#[cfg(test)]
mod tests {
    use super::{
        is_valid_geometry, is_valid_input, is_valid_session_name, session_name, SESSION_PREFIX,
    };

    #[test]
    fn deriva_sesiones_seguras() {
        assert_eq!(session_name("sistemas"), "nas-web-sistemas");
        assert_eq!(session_name("Juan Perez"), "nas-web-juan-perez");
        assert_eq!(session_name("../../etc"), "nas-web-etc");
        assert!(session_name("sistemas").starts_with(SESSION_PREFIX));
        assert!(session_name("").starts_with(SESSION_PREFIX));
    }

    #[test]
    fn valida_nombres_de_sesion() {
        assert!(is_valid_session_name("nas-web-sistemas"));
        assert!(!is_valid_session_name("Nas Web"));
        assert!(!is_valid_session_name(""));
        assert!(!is_valid_session_name("a;rm -rf /"));
        assert!(!is_valid_session_name(&"a".repeat(41)));
    }

    #[test]
    fn valida_entrada_y_geometria() {
        assert!(is_valid_input("ls -la"));
        assert!(is_valid_input(&"x".repeat(4096)));
        assert!(!is_valid_input(&"x".repeat(4097)));
        assert!(!is_valid_input("a\0b"));

        assert!(is_valid_geometry(200, 50));
        assert!(!is_valid_geometry(10, 50));
        assert!(!is_valid_geometry(200, 1000));
    }

    #[test]
    fn valida_teclas_especiales() {
        assert!(super::is_allowed_key("TAB"));
        assert!(super::is_allowed_key("CTRL_C"));
        assert!(super::is_allowed_key("ENTER"));
        assert!(!super::is_allowed_key("rm -rf /"));
        assert!(!super::is_allowed_key(""));
    }
}
