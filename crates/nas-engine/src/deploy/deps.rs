//! Auto-aprovisionamiento idempotente de dependencias del sistema operativo (Debian 13).
//!
//! La plataforma asume un sistema Debian limpio actualizado únicamente con
//! `curl` y `wget`, instalando bajo demanda o en actualización las herramientas
//! necesarias sin suponer paquetes preinstalados.

use std::time::Duration;

use nas_core::{NasError, NasResult};
use nas_exec::{CommandSpec, Runner, SudoMode};

/// Paquetes base mínimos necesarios para almacenamiento, S.M.A.R.T., caché de disco,
/// compartición SMB, respaldos, auditoría y panel web en Debian 13.
pub const BASE_RUNTIME_PACKAGES: &[&str] = &[
    "sudo",
    "curl",
    "wget",
    "ca-certificates",
    "tar",
    "gzip",
    "acl",
    "openssl",
    "btrfs-progs",
    "lvm2",
    "parted",
    "e2fsprogs",
    "util-linux",
    "psmisc",
    "procps",
    "iputils-ping",
    "smartmontools",
    "hdparm",
    "logrotate",
    "auditd",
    "aide",
    "ufw",
    "fail2ban",
    "unattended-upgrades",
    "cron",
    "samba",
    "samba-vfs-modules",
    "smbclient",
    "wsdd2",
    "cifs-utils",
    "rsync",
    "openssh-client",
    "sshpass",
    "sqlite3",
    "python3",
    "nginx-light",
    "tmux",
    "realmd",
    "adcli",
    "krb5-user",
    "packagekit",
    "sssd",
    "sssd-tools",
    "sssd-ad",
    "libnss-sss",
    "libpam-sss",
];

/// Paquetes opcionales para monitoreo y apagado automático ante cortes con UPS USB (`nut`).
pub const UPS_PACKAGES: &[&str] = &["nut", "nut-client", "nut-server"];

fn is_package_installed(runner: &Runner, pkg: &str) -> bool {
    let Ok(out) = runner.run(
        &CommandSpec::new("dpkg-query")
            .args(["-W", "-f=${Status}", pkg])
            .timeout(Duration::from_secs(10)),
    ) else {
        return false;
    };
    out.is_success() && out.stdout.contains("install ok installed")
}

/// Devuelve el subconjunto de paquetes de `packages` que aún no está instalado en el host.
pub fn missing_packages<'a>(runner: &Runner, packages: &[&'a str]) -> Vec<&'a str> {
    packages
        .iter()
        .copied()
        .filter(|pkg| !is_package_installed(runner, pkg))
        .collect()
}

/// Instala de forma idempotente y con baja prioridad de E/S/CPU los paquetes faltantes.
pub fn ensure_packages(runner: &Runner, packages: &[&str], sudo: bool) -> NasResult<Vec<String>> {
    let missing = missing_packages(runner, packages);
    if missing.is_empty() {
        return Ok(Vec::new());
    }

    let mut update_spec = CommandSpec::new("env")
        .args(["DEBIAN_FRONTEND=noninteractive", "apt-get", "update", "-qq"])
        .timeout(Duration::from_secs(180));
    if sudo {
        update_spec = update_spec.sudo(SudoMode::Passwordless);
    }
    let _ = runner.run(&update_spec);

    let mut args: Vec<String> = vec![
        "DEBIAN_FRONTEND=noninteractive".to_string(),
        "nice".to_string(),
        "-n".to_string(),
        "10".to_string(),
        "apt-get".to_string(),
        "install".to_string(),
        "-y".to_string(),
        "-qq".to_string(),
        "--no-install-recommends".to_string(),
    ];
    args.extend(missing.iter().map(|s| (*s).to_string()));

    let mut install_spec = CommandSpec::new("env")
        .args(args)
        .timeout(Duration::from_secs(600));
    if sudo {
        install_spec = install_spec.sudo(SudoMode::Passwordless);
    }

    let out = runner.run(&install_spec)?;
    if !out.is_success() {
        let detail = if out.stderr.trim().is_empty() {
            out.stdout.trim()
        } else {
            out.stderr.trim()
        };
        return Err(NasError::Denied(format!(
            "no se pudieron instalar los paquetes faltantes ({}): {detail}",
            missing.join(", ")
        )));
    }

    Ok(missing.into_iter().map(ToString::to_string).collect())
}

/// Garantiza la presencia de todos los paquetes base definidos en `BASE_RUNTIME_PACKAGES`.
pub fn ensure_base_runtime_deps(runner: &Runner, sudo: bool) -> NasResult<Vec<String>> {
    ensure_packages(runner, BASE_RUNTIME_PACKAGES, sudo)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn install_sh_cubre_todos_los_paquetes_base() {
        let install_sh = include_str!("../../../../install.sh");
        for pkg in BASE_RUNTIME_PACKAGES {
            assert!(
                install_sh.contains(pkg),
                "install.sh debe incluir el paquete base obligatorio '{pkg}'"
            );
        }
    }

    #[test]
    fn lista_base_incluye_herramientas_de_disco_y_resiliencia() {
        for required in [
            "smartmontools",
            "hdparm",
            "btrfs-progs",
            "lvm2",
            "parted",
            "e2fsprogs",
            "psmisc",
            "logrotate",
            "aide",
        ] {
            assert!(BASE_RUNTIME_PACKAGES.contains(&required));
        }
        assert!(UPS_PACKAGES.contains(&"nut"));
    }
}
