//! Desinstalación de la plataforma Rust (panel, CLI y helpers).
//!
//! El alcance es deliberadamente acotado: retira los artefactos de la
//! plataforma y su estado, pero **nunca** borra los datos de `/srv/nas`.
//! El desaprovisionamiento completo del sistema operativo (sysctl, udev, auditd)
//! se gestiona de forma unificada mediante el alcance `os`.
//!
//! Los scripts legacy de `src/core/` actúan como shims hacia esta lógica.

use serde::Serialize;

/// Paso del plan de desinstalación.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Step {
    /// Descripción legible.
    pub description: String,
    /// Comando a ejecutar.
    pub argv: Vec<String>,
    /// Indica si el fallo debe ignorarse (recurso ya ausente).
    pub optional: bool,
}

/// Plan de desinstalación.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct UninstallPlan {
    /// Pasos ordenados.
    pub steps: Vec<Step>,
    /// Indica si se elimina también el estado (`/var/lib/nas`).
    pub purge_state: bool,
}

fn step(description: &str, argv: &[&str], optional: bool) -> Step {
    Step {
        description: description.to_string(),
        argv: argv.iter().map(|arg| (*arg).to_string()).collect(),
        optional,
    }
}

/// Construye el plan de desinstalación del panel.
pub fn plan(purge_state: bool) -> UninstallPlan {
    plan_with_scope(purge_state, false)
}

/// Construye el plan incluyendo, opcionalmente, los ajustes del sistema operativo.
pub fn plan_with_scope(purge_state: bool, os_scope: bool) -> UninstallPlan {
    let mut steps = vec![
        step(
            "detener y deshabilitar el panel",
            &["systemctl", "disable", "--now", "nas-web.service"],
            true,
        ),
        step(
            "retirar la unidad systemd",
            &["rm", "-f", "/etc/systemd/system/nas-web.service"],
            true,
        ),
        step("recargar systemd", &["systemctl", "daemon-reload"], true),
        step(
            "retirar la configuración del panel",
            &["rm", "-f", "/etc/nas-web.env"],
            true,
        ),
        step(
            "retirar reglas sudo",
            &["rm", "-f", "/etc/sudoers.d/nas-web-rust"],
            true,
        ),
        step(
            "retirar el sitio de Nginx",
            &[
                "rm",
                "-f",
                "/etc/nginx/sites-enabled/nas-web",
                "/etc/nginx/sites-available/nas-web",
            ],
            true,
        ),
        step("validar Nginx", &["nginx", "-t"], true),
        step("recargar Nginx", &["systemctl", "reload", "nginx"], true),
        step(
            "retirar binarios de la plataforma",
            &[
                "rm",
                "-f",
                "/usr/local/bin/nas",
                "/usr/local/bin/nas-web",
                "/usr/local/bin/nas-tui",
                "/usr/local/bin/nasctl",
                "/usr/local/bin/nas-dev",
            ],
            true,
        ),
        step(
            "retirar helpers privilegiados",
            &[
                "rm",
                "-f",
                "/usr/local/sbin/nas-samba-apply",
                "/usr/local/sbin/nas-share-dir",
                "/usr/local/sbin/nas-share-acl",
                "/usr/local/sbin/nas-accounts",
                "/usr/local/sbin/nas-backup-manage",
                "/usr/local/sbin/nas-terminal",
                "/usr/local/sbin/nas-storage-apply",
            ],
            true,
        ),
        step(
            "retirar credenciales de respaldo",
            &["rm", "-rf", "/etc/backup-credentials"],
            true,
        ),
    ];

    if os_scope {
        steps.push(step(
            "retirar ajustes de kernel y udev",
            &[
                "rm",
                "-f",
                "/etc/sysctl.d/99-nas-tuning.conf",
                "/etc/udev/rules.d/60-nas-readahead.rules",
                "/etc/udev/rules.d/80-udisks2-hide-os.rules",
            ],
            true,
        ));
        steps.push(step(
            "retirar rotación y auditoría",
            &[
                "rm",
                "-f",
                "/etc/logrotate.d/nas-backups",
                "/etc/audit/rules.d/nas.rules",
                "/etc/cron.d/nas-btrfs-scrub",
            ],
            true,
        ));
        steps.push(step(
            "recargar sysctl y udev",
            &["sysctl", "--system"],
            true,
        ));
        steps.push(step(
            "recargar reglas udev",
            &["udevadm", "control", "--reload-rules"],
            true,
        ));
        steps.push(step(
            "regenerar reglas de auditoría",
            &["augenrules", "--load"],
            true,
        ));
        steps.push(step(
            "reiniciar demonio de auditoría",
            &["systemctl", "restart", "auditd"],
            true,
        ));
    }

    if purge_state {
        steps.push(step(
            "retirar el estado del panel (conserva /srv/nas)",
            &["rm", "-rf", "/var/lib/nas"],
            true,
        ));
        steps.push(step(
            "retirar la configuración de rol",
            &["rm", "-rf", "/etc/nas"],
            true,
        ));
    }

    UninstallPlan { steps, purge_state }
}

#[cfg(test)]
mod tests {
    use super::plan;

    #[test]
    fn plan_sin_purga_conserva_estado() {
        let plan = plan(false);
        assert!(!plan.purge_state);
        let paths: Vec<String> = plan
            .steps
            .iter()
            .flat_map(|step| step.argv.iter().cloned())
            .collect();
        assert!(paths.contains(&"/etc/nas-web.env".to_string()));
        assert!(!paths.iter().any(|path| path.contains("/var/lib/nas")));
        assert!(!paths.iter().any(|path| path.contains("/srv/nas")));
    }

    #[test]
    fn plan_con_purga_elimina_estado_pero_no_datos() {
        let plan = plan(true);
        let paths: Vec<String> = plan
            .steps
            .iter()
            .flat_map(|step| step.argv.iter().cloned())
            .collect();
        assert!(paths.contains(&"/var/lib/nas".to_string()));
        assert!(paths.contains(&"/etc/nas".to_string()));
        assert!(!paths.iter().any(|path| path.starts_with("/srv/nas")));
    }

    #[test]
    fn siempre_detiene_el_servicio_primero() {
        let plan = plan(true);
        assert_eq!(
            plan.steps[0].argv,
            vec!["systemctl", "disable", "--now", "nas-web.service"]
        );
    }
}

#[cfg(test)]
mod os_tests {
    use super::plan_with_scope;

    #[test]
    fn alcance_os_incluye_ajustes_del_sistema() {
        let plan = plan_with_scope(false, true);
        let paths: Vec<String> = plan
            .steps
            .iter()
            .flat_map(|step| step.argv.iter().cloned())
            .collect();
        assert!(paths.contains(&"/etc/sysctl.d/99-nas-tuning.conf".to_string()));
        assert!(paths.contains(&"/etc/udev/rules.d/60-nas-readahead.rules".to_string()));
        assert!(paths.contains(&"/etc/audit/rules.d/nas.rules".to_string()));
        assert!(paths.contains(&"sysctl".to_string()));
        assert!(paths.contains(&"augenrules".to_string()));
        assert!(paths.contains(&"auditd".to_string()));
    }

    #[test]
    fn alcance_panel_no_toca_ajustes_del_sistema() {
        let plan = plan_with_scope(false, false);
        let paths: Vec<String> = plan
            .steps
            .iter()
            .flat_map(|step| step.argv.iter().cloned())
            .collect();
        assert!(!paths.contains(&"/etc/sysctl.d/99-nas-tuning.conf".to_string()));
    }
}
