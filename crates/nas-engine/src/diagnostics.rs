//! Diagnóstico integral del servidor para el panel y la CLI.

use nas_core::NasPaths;
use nas_exec::{CommandSpec, Runner};
use serde::Serialize;

/// Resultado de una comprobación.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Check {
    /// Identificador de la comprobación.
    pub name: String,
    /// Resultado.
    pub ok: bool,
    /// Detalle legible.
    pub detail: String,
}

impl Check {
    fn new(name: &str, ok: bool, detail: impl Into<String>) -> Self {
        Self {
            name: name.to_string(),
            ok,
            detail: detail.into(),
        }
    }
}

/// Ejecuta el diagnóstico sobre las rutas y servicios del servidor.
pub fn collect(paths: &NasPaths, runner: &Runner) -> Vec<Check> {
    let mut checks = vec![
        Check::new(
            "rol",
            paths.role_file.exists(),
            paths.role_file.display().to_string(),
        ),
        Check::new(
            "samba",
            paths.samba_conf.exists(),
            paths.samba_conf.display().to_string(),
        ),
        Check::new(
            "almacenamiento",
            paths.data_root.exists(),
            paths.data_root.display().to_string(),
        ),
        Check::new(
            "estado",
            paths.state_dir.exists(),
            paths.state_dir.display().to_string(),
        ),
        Check::new(
            "sqlite",
            paths.database().exists(),
            paths.database().display().to_string(),
        ),
    ];

    for service in ["smbd", "nginx"] {
        let active = runner
            .run(&CommandSpec::new("systemctl").args(["is-active", service]))
            .map(|output| output.stdout == "active")
            .unwrap_or(false);
        checks.push(Check::new(
            &format!("servicio:{service}"),
            active,
            if active { "activo" } else { "inactivo" },
        ));
    }

    let ip = runner
        .run(&CommandSpec::new("ip").args(["-4", "-o", "addr", "show", "scope", "global"]))
        .ok()
        .and_then(|output| {
            output
                .stdout
                .split_whitespace()
                .skip_while(|token| *token != "inet")
                .nth(1)
                .and_then(|cidr| cidr.split('/').next())
                .map(str::to_string)
        });
    checks.push(Check::new(
        "red",
        ip.is_some(),
        ip.unwrap_or_else(|| "sin IPv4 global".to_string()),
    ));

    checks
}

/// Indica si todas las comprobaciones pasaron.
pub fn all_ok(checks: &[Check]) -> bool {
    checks.iter().all(|check| check.ok)
}

#[cfg(test)]
mod tests {
    use super::{all_ok, collect};
    use nas_core::NasPaths;
    use nas_exec::Runner;

    fn paths(tag: &str) -> NasPaths {
        let dir = std::env::temp_dir().join(format!(
            "nas-diag-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&dir).expect("dir");
        std::fs::write(dir.join("smb.conf"), "[global]\n").expect("smb");
        std::fs::write(dir.join("role"), "ARCHIVOS\n").expect("role");
        NasPaths {
            repo: dir.clone(),
            web_root: dir.clone(),
            state_dir: dir.clone(),
            data_root: dir.clone(),
            samba_conf: dir.join("smb.conf"),
            role_file: dir.join("role"),
        }
    }

    #[test]
    fn verifica_rutas_y_servicios() {
        let paths = paths("ok");
        let checks = collect(&paths, &Runner::new());

        let names: Vec<&str> = checks.iter().map(|check| check.name.as_str()).collect();
        assert!(names.contains(&"rol"));
        assert!(names.contains(&"samba"));
        assert!(names.contains(&"almacenamiento"));
        assert!(names.contains(&"servicio:smbd"));

        let role = checks
            .iter()
            .find(|check| check.name == "rol")
            .expect("rol");
        assert!(role.ok);

        let _ = all_ok(&checks);
        let _ = std::fs::remove_dir_all(&paths.state_dir);
    }

    #[test]
    fn detecta_rutas_ausentes() {
        let mut paths = paths("faltan");
        paths.data_root = paths.data_root.join("no-existe");
        let checks = collect(&paths, &Runner::new());
        let storage = checks
            .iter()
            .find(|check| check.name == "almacenamiento")
            .expect("almacenamiento");
        assert!(!storage.ok);
        let _ = std::fs::remove_dir_all(&paths.state_dir);
    }
}
