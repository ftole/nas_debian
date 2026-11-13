//! Estado y gestión de los demonios del servidor.

use nas_exec::{CommandSpec, Runner, SudoMode};
use serde::Serialize;

/// Demonios gestionables por el panel.
pub const DEFAULT_SERVICES: &[&str] = &["smbd", "nmbd", "wsdd2", "nginx"];

/// Servicios adicionales gestionables desde el panel.
pub const EXTRA_SERVICES: &[&str] = &["cron"];

/// Estado de un servicio.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ServiceStatus {
    /// Nombre de la unidad.
    pub service: String,
    /// Indica si está activo.
    pub active: bool,
}

/// Acciones permitidas sobre un servicio.
pub fn is_allowed_action(action: &str) -> bool {
    matches!(action, "start" | "stop" | "restart" | "reload")
}

/// Indica si el servicio está en la lista blanca.
pub fn is_allowed_service(service: &str) -> bool {
    DEFAULT_SERVICES.contains(&service) || EXTRA_SERVICES.contains(&service) || service == "nas-web"
}

/// Consulta el estado de los servicios indicados.
pub fn collect(runner: &Runner, services: &[&str]) -> Vec<ServiceStatus> {
    services
        .iter()
        .map(|service| ServiceStatus {
            service: (*service).to_string(),
            active: runner
                .run(&CommandSpec::new("systemctl").args(["is-active", service]))
                .map(|output| output.stdout == "active")
                .unwrap_or(false),
        })
        .collect()
}

/// Ejecuta una acción sobre un servicio, validando nombre y acción.
pub fn manage(runner: &Runner, service: &str, action: &str, sudo: bool) -> Result<(), String> {
    if !is_allowed_service(service) {
        return Err(format!("servicio no permitido: {service}"));
    }
    if !is_allowed_action(action) {
        return Err(format!("acción no permitida: {action}"));
    }

    let mut spec = CommandSpec::new("systemctl")
        .args([action, service])
        .timeout(std::time::Duration::from_secs(60));
    if sudo {
        spec = spec.sudo(SudoMode::Passwordless);
    }

    let output = runner.run(&spec).map_err(|error| error.to_string())?;

    if output.is_success() {
        Ok(())
    } else {
        Err(if output.stderr.is_empty() {
            output.stdout
        } else {
            output.stderr
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{collect, is_allowed_action, is_allowed_service, ServiceStatus};
    use nas_exec::Runner;

    #[test]
    fn valida_servicios_y_acciones() {
        assert!(is_allowed_service("smbd"));
        assert!(!is_allowed_service("php8.4-fpm"));
        assert!(!is_allowed_service("sshd"));
        assert!(!is_allowed_service("smbd;rm -rf /"));

        assert!(is_allowed_action("restart"));
        assert!(is_allowed_action("reload"));
        assert!(!is_allowed_action("mask"));
    }

    #[test]
    fn recolecta_un_estado_por_servicio() {
        let statuses: Vec<ServiceStatus> = collect(&Runner::new(), &["smbd", "nginx"]);
        assert_eq!(statuses.len(), 2);
        assert_eq!(statuses[0].service, "smbd");
        assert_eq!(statuses[1].service, "nginx");
    }

    #[test]
    fn autoriza_servicio_nas_web() {
        assert!(is_allowed_service("nas-web"));
    }
}
