//! Plantillas, validación y sincronización de servicios de seguridad del SO.

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::time::Duration;

use nas_core::{NasError, NasResult};
use nas_exec::{CommandSpec, Runner};

use crate::deploy::web::{validate_sudoers, write_artifact, SUDOERS};
use crate::deploy::Artifact;

use super::spec_with_sudo;

pub(super) const AUDIT_RULES: &str = include_str!("../../../../packaging/nas.rules");
pub(super) const AIDE_NAS_RULES: &str = include_str!("../../../../packaging/75_aide_nas");
pub(super) const UDEV_READAHEAD: &str =
    include_str!("../../../../packaging/60-nas-readahead.rules");

pub(super) const FAIL2BAN_NAS_LOCAL: &str = "\
[DEFAULT]
banaction = nftables
banaction_allports = nftables[type=allports]

[sshd]
enabled = true
backend = systemd
maxretry = 5
findtime = 10m
bantime = 1h
";

pub(super) const APT_AUTO_UPGRADES: &str = "\
APT::Periodic::Update-Package-Lists \"1\";
APT::Periodic::Unattended-Upgrade \"1\";
";

pub(super) const APT_UNATTENDED_NAS: &str = "\
Unattended-Upgrade::Automatic-Reboot \"false\";
";

pub(super) const SSH_HARDENING_CONF: &str = "\
PermitRootLogin no
PasswordAuthentication no
PubkeyAuthentication yes
X11Forwarding no
";

/// Comprueba si existe alguna clave SSH autorizada en `/root` o `/home/*`.
pub(super) fn has_any_ssh_authorized_key() -> bool {
    let mut candidates = vec![std::path::PathBuf::from("/root/.ssh/authorized_keys")];
    if let Ok(entries) = std::fs::read_dir("/home") {
        for entry in entries.flatten() {
            candidates.push(entry.path().join(".ssh/authorized_keys"));
        }
    }
    candidates.into_iter().any(|p| {
        std::fs::read_to_string(p).is_ok_and(|txt| {
            txt.lines().any(|line| {
                let t = line.trim();
                !t.is_empty()
                    && !t.starts_with('#')
                    && (t.contains("ssh-ed25519 ")
                        || t.contains("ssh-rsa ")
                        || t.contains("ecdsa-sha2-"))
            })
        })
    })
}

pub(super) fn sync_artifact_needed(path: &Path, mode: u32, content: &str) -> bool {
    let content_changed = std::fs::read_to_string(path)
        .map(|cur| cur != content)
        .unwrap_or(true);
    #[cfg(unix)]
    let mode_changed = std::fs::metadata(path)
        .ok()
        .map_or(true, |meta| (meta.permissions().mode() & 0o777) != mode);
    #[cfg(not(unix))]
    let mode_changed = false;
    content_changed || mode_changed
}

pub(super) fn sync_artifact_if_changed(
    runner: &Runner,
    path: &str,
    mode: u32,
    content: &str,
    sudo: bool,
) -> NasResult<bool> {
    if !sync_artifact_needed(Path::new(path), mode, content) {
        return Ok(false);
    }
    let artifact = Artifact {
        path: path.to_string(),
        mode,
        content: content.to_string(),
    };
    write_artifact(runner, &artifact, sudo)?;
    Ok(true)
}

pub(super) fn sync_sudoers_validated(runner: &Runner, sudo: bool) -> NasResult<bool> {
    let target = "/etc/sudoers.d/nas-web-rust";
    let changed = std::fs::read_to_string(target)
        .map(|cur| cur != SUDOERS)
        .unwrap_or(true);
    if !changed {
        return Ok(false);
    }
    let staged = crate::deploy::write_unique_temp_file("nas-sudoers-check", SUDOERS)?;
    let visudo_res = validate_sudoers(runner, &staged.to_string_lossy(), sudo);
    let _ = std::fs::remove_file(&staged);
    visudo_res?;

    let artifact = Artifact {
        path: target.to_string(),
        mode: 0o440,
        content: SUDOERS.to_string(),
    };
    write_artifact(runner, &artifact, sudo)?;
    validate_sudoers(runner, target, sudo)?;
    Ok(true)
}

pub(super) fn sync_nginx_conf_validated(runner: &Runner, sudo: bool) -> NasResult<bool> {
    let target = "/etc/nginx/sites-available/nas-web";
    if !Path::new("/etc/nginx/sites-available").is_dir() {
        return Ok(false);
    }
    let prev = std::fs::read_to_string(target).ok();
    if prev.as_deref() == Some(crate::deploy::web::NGINX_CONF) {
        return Ok(false);
    }
    let artifact = Artifact {
        path: target.to_string(),
        mode: 0o644,
        content: crate::deploy::web::NGINX_CONF.to_string(),
    };
    write_artifact(runner, &artifact, sudo)?;

    let check = runner.run(&spec_with_sudo(
        CommandSpec::new("nginx")
            .arg("-t")
            .timeout(Duration::from_secs(15)),
        sudo,
    ));
    let ok = check.as_ref().is_ok_and(|o| o.is_success());
    if !ok {
        if let Some(old) = prev {
            let _ = write_artifact(
                runner,
                &Artifact {
                    path: target.to_string(),
                    mode: 0o644,
                    content: old,
                },
                sudo,
            );
        }
        return Err(NasError::Denied(
            "nginx -t rechazó la nueva configuración; se revirtió el archivo previo".into(),
        ));
    }
    let _ = runner.run(&spec_with_sudo(
        CommandSpec::new("systemctl")
            .args(["reload", "nginx"])
            .timeout(Duration::from_secs(20)),
        sudo,
    ));
    Ok(true)
}
