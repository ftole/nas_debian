//! Diagnóstico remoto (`doctor`), procedimiento de alta (`onboard`) y utilidades de inventario.

use std::path::PathBuf;
use std::process::ExitCode;

use nas_core::{ApiResponse, NasError, NasResult};
use nas_inventory::{Inventory, ResolvedHost};
use nas_ssh::{with_sudo, OpenSsh, RemoteExecutor, SshTarget};
use serde::Serialize;

/// Verificación remota de un requisito del host de desarrollo.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DoctorCheck {
    pub(crate) name: String,
    pub(crate) argv: Vec<String>,
    pub(crate) sudo: bool,
    pub(crate) hint: String,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct DoctorResult {
    pub(crate) name: String,
    pub(crate) ok: bool,
    pub(crate) detail: String,
    pub(crate) hint: String,
}

pub(crate) fn ensure_development(host: &ResolvedHost) -> NasResult<()> {
    if host.is_development() {
        Ok(())
    } else {
        Err(NasError::Denied(format!(
            "el host '{}' pertenece al entorno '{}': nas-dev sólo opera en desarrollo",
            host.alias, host.env
        )))
    }
}

pub(crate) fn target_for(
    host: &ResolvedHost,
    user: Option<String>,
    identity: Option<PathBuf>,
    known_hosts: Option<PathBuf>,
) -> SshTarget {
    let mut target = SshTarget::new(
        host.address.clone(),
        host.port,
        user.unwrap_or_else(|| host.user.clone()),
    )
    .with_connect_timeout(host.timeout_secs);

    if let Some(identity) = identity {
        target = target.with_identity(identity);
    }

    target.with_known_hosts(known_hosts.unwrap_or_else(|| default_known_hosts(&host.alias)))
}

pub(crate) fn build_doctor_checks(host: &ResolvedHost) -> Vec<DoctorCheck> {
    let plain = |name: &str, argv: &[&str], sudo: bool, hint: &str| DoctorCheck {
        name: name.to_string(),
        argv: argv.iter().map(|arg| (*arg).to_string()).collect(),
        sudo,
        hint: hint.to_string(),
    };

    vec![
        plain(
            "usuario-ssh",
            &["id", "-un"],
            false,
            "el usuario SSH debe existir en el host",
        ),
        plain(
            "sudo-sin-password",
            &["true"],
            true,
            "configure /etc/sudoers.d/nas-dev con NOPASSWD para el usuario",
        ),
        plain(
            "rsync",
            &["rsync", "--version"],
            false,
            "instale rsync (apt-get install rsync)",
        ),
        plain("tar", &["tar", "--version"], false, "instale tar"),
        plain(
            "curl",
            &["curl", "--version"],
            false,
            "instale curl para el healthcheck posterior al cambio",
        ),
        DoctorCheck {
            name: "docroot".to_string(),
            argv: vec!["test".into(), "-d".into(), host.web.clone()],
            sudo: true,
            hint: format!("cree el docroot de desarrollo {}", host.web),
        },
        DoctorCheck {
            name: "docroot-escribible".to_string(),
            argv: vec!["test".into(), "-w".into(), host.web.clone()],
            sudo: true,
            hint: "ajuste propietario y permisos del docroot".to_string(),
        },
        DoctorCheck {
            name: "estado".to_string(),
            argv: vec!["test".into(), "-d".into(), host.state.clone()],
            sudo: true,
            hint: format!("cree el directorio de estado {}", host.state),
        },
        plain(
            "nginx-config",
            &["nginx", "-t"],
            true,
            "corrija la configuración de nginx",
        ),
        plain(
            "nginx-activo",
            &["systemctl", "is-active", "--quiet", "nginx"],
            true,
            "inicie nginx en el host",
        ),
    ]
}

pub(crate) fn doctor(
    alias: &str,
    inventory_path: Option<PathBuf>,
    user: Option<String>,
    identity: Option<PathBuf>,
    known_hosts: Option<PathBuf>,
    json: bool,
) -> NasResult<ExitCode> {
    let inventory = load_inventory(inventory_path)?;
    let host = inventory.resolve(alias)?;
    ensure_development(&host)?;

    let target = target_for(&host, user, identity, known_hosts);
    ensure_known_hosts_dir(&target)?;

    let executor = OpenSsh::new();
    let mut results = Vec::new();

    for check in build_doctor_checks(&host) {
        let argv = if check.sudo {
            with_sudo(&check.argv)
        } else {
            check.argv.clone()
        };

        let (ok, detail) = match executor.run(&target, &argv) {
            Ok(output) if output.is_success() => (true, "OK".to_string()),
            Ok(output) => (
                false,
                if output.stderr.is_empty() {
                    output.stdout
                } else {
                    output.stderr
                },
            ),
            Err(error) => (false, error.to_string()),
        };

        results.push(DoctorResult {
            name: check.name,
            ok,
            detail,
            hint: check.hint,
        });
    }

    let failures = results.iter().filter(|result| !result.ok).count();

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&ApiResponse::ok(
                serde_json::json!({ "host": host.alias, "failures": failures, "checks": results }),
                "Diagnóstico del host de desarrollo"
            ))?
        );
    } else {
        println!(
            "DIAGNÓSTICO DEL HOST DE DESARROLLO: {} ({})",
            host.alias, host.env
        );
        println!("Destino: {}", target.destination());
        println!();
        for result in &results {
            let mark = if result.ok { "PASS" } else { "FAIL" };
            println!("[{mark}] {:<20} {}", result.name, result.detail);
            if !result.ok {
                println!("       ↳ {}", result.hint);
            }
        }
        println!();
        println!("Fallos: {failures}");
    }

    if failures == 0 {
        Ok(ExitCode::SUCCESS)
    } else {
        Ok(ExitCode::from(1))
    }
}

pub(crate) fn onboard(
    alias: &str,
    inventory_path: Option<PathBuf>,
    json: bool,
) -> NasResult<ExitCode> {
    let inventory = load_inventory(inventory_path)?;
    let host = inventory.resolve(alias)?;
    ensure_development(&host)?;

    let script = onboard_script(&host);

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&ApiResponse::ok(
                serde_json::json!({ "host": host.alias, "script": script }),
                "Procedimiento de alta del host"
            ))?
        );
    } else {
        println!(
            "# Procedimiento de alta para '{}' ({})",
            host.alias, host.env
        );
        println!("# Ejecutar como root en el servidor destino. Revisar antes de aplicar.\n");
        println!("{script}");
    }

    Ok(ExitCode::SUCCESS)
}

pub(crate) fn onboard_script(host: &ResolvedHost) -> String {
    let user = &host.user;
    format!(
        r#"set -euo pipefail

# 1. Usuario de desarrollo sin shell de sistema
id -u {user} >/dev/null 2>&1 || useradd -m -s /bin/bash {user}

# 2. Clave pública (pegue la clave del operador)
install -d -m 700 -o {user} -g {user} /home/{user}/.ssh
touch /home/{user}/.ssh/authorized_keys
chown {user}:{user} /home/{user}/.ssh/authorized_keys
chmod 600 /home/{user}/.ssh/authorized_keys

# 3. Sudo acotado a los comandos del plan (revisar y endurecer antes de usar)
cat > /etc/sudoers.d/nas-dev <<'SUDO'
{user} ALL=(root) NOPASSWD: /usr/bin/mkdir, /usr/bin/rm, /usr/bin/tar, /usr/bin/mv, /usr/bin/rsync, /usr/bin/curl, /usr/bin/test, /usr/bin/systemctl reload nginx, /usr/bin/systemctl restart nas-web
SUDO
chmod 0440 /etc/sudoers.d/nas-dev
visudo -cf /etc/sudoers.d/nas-dev

# 4. Docroot y estado aislados de producción
install -d -m 775 -o nas-web -g nas-web {web}
install -d -m 750 -o {user} -g {user} {state}/dev-sync/incoming {state}/dev-sync/stage {state}/dev-sync/backups

# 5. Verificación
sudo -u {user} sudo -n true && echo 'sudo sin contraseña OK'
"#,
        web = host.web,
        state = host.state,
        user = user
    )
}

pub(crate) fn load_inventory(explicit: Option<PathBuf>) -> NasResult<Inventory> {
    let path = explicit
        .or_else(|| std::env::var("NAS_INVENTORY").ok().map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from("inventory.yaml"));
    Inventory::load(path)
}

pub(crate) fn ensure_known_hosts_dir(target: &SshTarget) -> NasResult<()> {
    if let Some(path) = &target.known_hosts {
        if let Some(dir) = path.parent() {
            if !dir.as_os_str().is_empty() {
                std::fs::create_dir_all(dir).map_err(|e| NasError::io(dir, e))?;
            }
        }
    }
    Ok(())
}

fn default_known_hosts(alias: &str) -> PathBuf {
    home_dir().join(".ssh").join("known_hosts.d").join(alias)
}

fn home_dir() -> PathBuf {
    std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .map_or_else(|_| PathBuf::from("."), PathBuf::from)
}

pub(crate) fn local_audit_path() -> PathBuf {
    std::env::var("NAS_DEV_AUDIT").map_or_else(
        |_| {
            if cfg!(windows) {
                std::env::var("LOCALAPPDATA").map_or_else(
                    |_| PathBuf::from(".nas-dev/audit.jsonl"),
                    |base| PathBuf::from(base).join("nas-dev").join("audit.jsonl"),
                )
            } else {
                home_dir().join(".local/state/nas-dev/audit.jsonl")
            }
        },
        PathBuf::from,
    )
}

pub(crate) fn current_actor() -> String {
    std::env::var("USER")
        .or_else(|_| std::env::var("USERNAME"))
        .unwrap_or_else(|_| "nas-dev".to_string())
}
