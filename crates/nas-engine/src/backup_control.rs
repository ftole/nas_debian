//! Control de tareas de respaldo desde el panel (ejecutar, detener y bitácora).
//!
//! Las operaciones se delegan en `nas-backup-manage`, un helper root con lista
//! blanca de acciones, para no conceder al servicio web permisos generales.

use nas_exec::{CommandSpec, Runner, SudoMode};

/// Identificador válido de tarea (`[A-Za-z0-9_-]{1,32}`).
pub fn valid_id(id: &str) -> bool {
    let id = id.trim();
    !id.is_empty()
        && id.len() <= 32
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-'))
}

/// Límite de líneas de bitácora devueltas al panel.
pub const MAX_LOG_LINES: usize = 500;

fn helper(runner: &Runner, args: &[&str]) -> Result<String, String> {
    let mut argv: Vec<String> = args.iter().map(|arg| (*arg).to_string()).collect();
    let output = runner
        .run(
            &CommandSpec::new("/usr/local/sbin/nas-backup-manage")
                .args(argv.drain(..))
                .sudo(SudoMode::Passwordless)
                .timeout(std::time::Duration::from_secs(60)),
        )
        .map_err(|error| error.to_string())?;
    if output.is_success() {
        Ok(output.stdout)
    } else if output.stderr.is_empty() {
        Err(output.stdout)
    } else {
        Err(output.stderr)
    }
}

/// Ejecuta la tarea en segundo plano (no bloquea la petición HTTP).
pub fn run(runner: &Runner, id: &str) -> Result<String, String> {
    if !valid_id(id) {
        return Err("identificador de tarea inválido".to_string());
    }
    helper(runner, &["run", id])
}

/// Detiene la ejecución en curso de una tarea.
pub fn stop(runner: &Runner, id: &str) -> Result<String, String> {
    if !valid_id(id) {
        return Err("identificador de tarea inválido".to_string());
    }
    helper(runner, &["stop", id])
}

/// Devuelve las últimas líneas de la bitácora de la tarea.
pub fn logs(runner: &Runner, id: &str, lines: usize) -> Result<String, String> {
    if !valid_id(id) {
        return Err("identificador de tarea inválido".to_string());
    }
    let lines = lines.clamp(1, MAX_LOG_LINES).to_string();
    helper(runner, &["logs", id, &lines])
}

#[cfg(test)]
mod tests {
    use super::{logs, run, stop, valid_id, MAX_LOG_LINES};

    #[test]
    fn valida_identificadores() {
        assert!(valid_id("contabilidad"));
        assert!(valid_id("didi_admin-2"));
        assert!(!valid_id(""));
        assert!(!valid_id("../etc"));
        assert!(!valid_id("a b"));
        assert!(!valid_id(&"x".repeat(33)));
    }

    #[test]
    fn rechaza_ids_invalidos_antes_de_ejecutar() {
        let runner = nas_exec::Runner::new();
        assert!(run(&runner, "../x").is_err());
        assert!(stop(&runner, "a/b").is_err());
        assert!(logs(&runner, "", 10).is_err());
    }

    #[test]
    fn limite_de_lineas_es_acotado() {
        assert_eq!(MAX_LOG_LINES, 500);
    }
}
