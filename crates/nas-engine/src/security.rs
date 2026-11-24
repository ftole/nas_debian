//! Endurecimiento de seguridad del sistema operativo y sincronización post-instalación/actualización.
//!
//! Implementa de forma idempotente:
//! - Cortafuegos de red (`ufw` con backend `iptables-nft` en Debian 13) con detección automática
//!   de subred LAN y protección anti-lockout sobre `$SSH_CONNECTION`.
//! - Prevención de intrusiones (`fail2ban` con `backend = systemd` y `banaction = nftables`).
//! - Actualizaciones de seguridad desatendidas (`unattended-upgrades`, sin reinicios automáticos).
//! - Reglas `auditd`, exclusiones `aide`, unidad `nas-web.service` (sin `AmbientCapabilities`),
//!   `sudoers` validado con `visudo -cf` y máscara de suspensión ACPI.

use std::path::Path;
use std::time::Duration;

use nas_core::NasResult;
use nas_exec::{CommandSpec, Runner, SudoMode};
use serde::Serialize;

use crate::deploy::web::{resolve_data_root, write_artifact, SUDOERS, SYSTEMD_UNIT};
use crate::deploy::{apply_journald_and_sleep_mask, deps, Artifact, RECYCLE_PURGE_SCRIPT};

mod acls;
mod firewall;
mod services;

#[cfg(test)]
mod tests;

pub(crate) use acls::sync_data_root_permissions_with_principals;
pub use acls::{acl_has_effective, discover_active_share_dirs, discover_unit_roots};
pub use firewall::{
    ipv4_cidr_network, parse_cidr_list, parse_ip_addr_cidrs, parse_ssh_client_ip,
    parse_ssh_server_port, render_ufw_default_ipv6_disabled,
};

use acls::{has_acl_tools, resolve_state_dir};
use firewall::{configure_ufw_firewall, is_ufw_enabled_file};
use services::{
    has_any_ssh_authorized_key, sync_artifact_if_changed, sync_nginx_conf_validated,
    sync_sudoers_validated, AIDE_NAS_RULES, APT_AUTO_UPGRADES, APT_UNATTENDED_NAS, AUDIT_RULES,
    FAIL2BAN_NAS_LOCAL, SSH_HARDENING_CONF, UDEV_READAHEAD,
};

/// Estado consolidado de endurecimiento y controles de seguridad del sistema.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SecurityStatus {
    /// Cortafuegos activo (`ufw` o `nftables`).
    pub firewall_active: bool,
    /// Backend detectado del cortafuegos (`ufw`, `nftables` o `ninguno`).
    pub firewall_backend: String,
    /// `fail2ban` activo.
    pub fail2ban_active: bool,
    /// Reglas locales de `fail2ban` desplegadas.
    pub fail2ban_configured: bool,
    /// Actualizaciones de seguridad desatendidas configuradas.
    pub unattended_upgrades_configured: bool,
    /// Servicio `auditd` activo.
    pub auditd_active: bool,
    /// Reglas `/etc/audit/rules.d/nas.rules` presentes.
    pub audit_rules_present: bool,
    /// Exclusiones `/etc/aide/aide.conf.d/75_aide_nas` presentes.
    pub aide_rules_present: bool,
    /// Base criptográfica inicial de AIDE generada.
    pub aide_db_initialized: bool,
    /// Unidad `nas-web.service` sincronizada sin `AmbientCapabilities` elevadas.
    pub systemd_hardened: bool,
    /// Reglas `/etc/sudoers.d/nas-web-rust` sincronizadas.
    pub sudoers_synced: bool,
    /// Suspensión/hibernación ACPI enmascaradas.
    pub sleep_masked: bool,
    /// Drop-in SSH restrictivo presente.
    pub ssh_hardened: bool,
}

/// Opciones de ejecución de endurecimiento (`nas security harden`).
#[derive(Debug, Clone, Default)]
pub struct HardenOptions {
    /// Aplica cambios reales; si es `false` solo planifica (`dry-run`).
    pub apply: bool,
    /// Modo seguro para instalación/actualización: evita deshabilitar contraseñas SSH ni sobreescribir reglas de firewall activas.
    pub safe_only: bool,
    /// Reconfigura el cortafuegos aunque ya esté activo.
    pub force_firewall: bool,
    /// Subred(es) LAN permitidas en formato CIDR separadas por coma (si es `None`, autodetecta).
    pub lan_cidr: Option<String>,
    /// Instala y aplica drop-in SSH restrictivo si existen `authorized_keys` válidas.
    pub harden_ssh: bool,
    /// Inicializa la base de AIDE si aún no existe (`/var/lib/aide/aide.db`).
    pub init_aide: bool,
    /// Eleva privilegios con `sudo -n`.
    pub sudo: bool,
}

/// Resultado de `nas security harden`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct HardenReport {
    /// Indica si se ejecutaron cambios reales (`true`) o fue simulación (`false`).
    pub applied: bool,
    /// Pasos aplicados o planificados.
    pub actions: Vec<String>,
    /// Advertencias no fatales (ej. entorno sin systemd o sin paquetes).
    pub warnings: Vec<String>,
}

pub(super) fn spec_with_sudo(mut spec: CommandSpec, sudo: bool) -> CommandSpec {
    if sudo {
        spec = spec.sudo(SudoMode::Passwordless);
    }
    spec
}

pub(super) fn service_is_active(runner: &Runner, unit: &str) -> bool {
    runner
        .run(
            &CommandSpec::new("systemctl")
                .args(["is-active", unit])
                .timeout(Duration::from_secs(10)),
        )
        .map(|out| out.stdout.trim() == "active")
        .unwrap_or(false)
}

/// Recopila el estado actual de los controles de seguridad del host.
pub fn status(runner: &Runner) -> SecurityStatus {
    let ufw_on = is_ufw_enabled_file(Path::new("/etc/ufw/ufw.conf"));
    let nft_on = service_is_active(runner, "nftables");
    let (firewall_active, firewall_backend) = if ufw_on {
        (true, "ufw".to_string())
    } else if nft_on {
        (true, "nftables".to_string())
    } else {
        (false, "ninguno".to_string())
    };

    let systemd_hardened = std::fs::read_to_string("/etc/systemd/system/nas-web.service")
        .ok()
        .is_some_and(|cur| cur == SYSTEMD_UNIT);

    let sudoers_synced = std::fs::read_to_string("/etc/sudoers.d/nas-web-rust")
        .ok()
        .is_some_and(|cur| cur == SUDOERS);

    let sleep_masked = runner
        .run(
            &CommandSpec::new("systemctl")
                .args(["is-enabled", "sleep.target"])
                .timeout(Duration::from_secs(5)),
        )
        .map(|o| o.stdout.trim() == "masked")
        .unwrap_or(false);

    let unattended_bin_present = Path::new("/usr/bin/unattended-upgrade").is_file()
        || Path::new("/usr/bin/unattended-upgrades").is_file();

    SecurityStatus {
        firewall_active,
        firewall_backend,
        fail2ban_active: service_is_active(runner, "fail2ban"),
        fail2ban_configured: Path::new("/etc/fail2ban/jail.d/nas.local").is_file(),
        unattended_upgrades_configured: unattended_bin_present
            && Path::new("/etc/apt/apt.conf.d/20auto-upgrades").is_file(),
        auditd_active: service_is_active(runner, "auditd"),
        audit_rules_present: Path::new("/etc/audit/rules.d/nas.rules").is_file(),
        aide_rules_present: Path::new("/etc/aide/aide.conf.d/75_aide_nas").is_file(),
        aide_db_initialized: Path::new("/var/lib/aide/aide.db").is_file(),
        systemd_hardened,
        sudoers_synced,
        sleep_masked,
        ssh_hardened: Path::new("/etc/ssh/sshd_config.d/99-nas-hardening.conf").is_file(),
    }
}

/// Aplica (o simula con `options.apply == false`) el endurecimiento integral del servidor.
pub fn harden(runner: &Runner, options: &HardenOptions) -> NasResult<HardenReport> {
    let mut actions = Vec::new();
    let mut warnings = Vec::new();

    if !options.apply {
        actions.push("[dry-run] sincronizar nas-web.service, sudoers, auditd, aide, udev, journald, nas-recycle-purge y máscara ACPI".into());
        actions.push("[dry-run] asegurar ACLs y permisos para nas-web y grp_samba en NAS_DATA_ROOT, unidades, shares activos y .nas_staging".into());
        actions.push(
            "[dry-run] configurar fail2ban (/etc/fail2ban/jail.d/nas.local) y unattended-upgrades"
                .into(),
        );
        let _ = configure_ufw_firewall(runner, options, &mut actions, &mut warnings);
        if options.harden_ssh && !options.safe_only {
            actions.push("[dry-run] endurecer sshd (/etc/ssh/sshd_config.d/99-nas-hardening.conf) si hay authorized_keys".into());
        }
        return Ok(HardenReport {
            applied: false,
            actions,
            warnings,
        });
    }

    // 1. Sincronizar unidad systemd nas-web.service (retira AmbientCapabilities heredadas) y sudoers acotado.
    let mut restart_nas_web = false;
    if Path::new("/etc/systemd/system").is_dir() {
        match sync_artifact_if_changed(
            runner,
            "/etc/systemd/system/nas-web.service",
            0o644,
            SYSTEMD_UNIT,
            options.sudo,
        ) {
            Ok(true) => {
                let _ = runner.run(&spec_with_sudo(
                    CommandSpec::new("systemctl")
                        .arg("daemon-reload")
                        .timeout(Duration::from_secs(20)),
                    options.sudo,
                ));
                restart_nas_web = true;
                actions.push("sincronizado /etc/systemd/system/nas-web.service".into());
            }
            Ok(false) => {}
            Err(e) => warnings.push(format!("no se pudo sincronizar nas-web.service: {e}")),
        }
    }

    if Path::new("/etc/sudoers.d").is_dir() {
        match sync_sudoers_validated(runner, options.sudo) {
            Ok(true) => actions.push("sincronizado /etc/sudoers.d/nas-web-rust (visudo OK)".into()),
            Ok(false) => {}
            Err(e) => warnings.push(format!("no se pudo sincronizar sudoers: {e}")),
        }
    }

    if Path::new("/etc/nginx/sites-available/nas-web").is_file() {
        match sync_nginx_conf_validated(runner, options.sudo) {
            Ok(true) => actions.push("sincronizado /etc/nginx/sites-available/nas-web".into()),
            Ok(false) => {}
            Err(e) => warnings.push(format!("no se pudo sincronizar nginx: {e}")),
        }
    }

    if Path::new("/etc/cron.daily").is_dir() {
        match sync_artifact_if_changed(
            runner,
            "/etc/cron.daily/nas-recycle-purge",
            0o755,
            RECYCLE_PURGE_SCRIPT,
            options.sudo,
        ) {
            Ok(true) => actions.push("sincronizado /etc/cron.daily/nas-recycle-purge".into()),
            Ok(false) => {}
            Err(e) => warnings.push(format!("no se pudo sincronizar nas-recycle-purge: {e}")),
        }
    }

    // 2. Kernel/udev/journald/sleep mask + auditd + AIDE exclusions.
    if let Err(e) = apply_journald_and_sleep_mask(runner, options.sudo) {
        warnings.push(format!("journald/sleep mask: {e}"));
    } else {
        actions.push("cuotas journald y máscara ACPI de suspensión verificadas".into());
    }

    if Path::new("/etc/udev/rules.d").is_dir()
        && sync_artifact_if_changed(
            runner,
            "/etc/udev/rules.d/60-nas-readahead.rules",
            0o644,
            UDEV_READAHEAD,
            options.sudo,
        )
        .unwrap_or(false)
    {
        let _ = runner.run(&spec_with_sudo(
            CommandSpec::new("udevadm")
                .args(["control", "--reload-rules"])
                .timeout(Duration::from_secs(15)),
            options.sudo,
        ));
        actions.push("sincronizado /etc/udev/rules.d/60-nas-readahead.rules".into());
    }

    // Auto-aprovisionamiento idempotente de paquetes de seguridad faltantes (premisa cero-supuestos).
    let mut missing_sec = Vec::new();
    if !has_acl_tools() {
        missing_sec.push("acl");
    }
    if !Path::new("/etc/audit").is_dir() {
        missing_sec.push("auditd");
    }
    if !Path::new("/etc/aide/aide.conf.d").is_dir() {
        missing_sec.push("aide");
    }
    if !Path::new("/etc/fail2ban/jail.d").is_dir() {
        missing_sec.push("fail2ban");
    }
    if !Path::new("/usr/bin/unattended-upgrade").is_file()
        && !Path::new("/usr/bin/unattended-upgrades").is_file()
    {
        missing_sec.push("unattended-upgrades");
    }
    if !missing_sec.is_empty() {
        if let Err(err) = deps::ensure_packages(runner, &missing_sec, options.sudo) {
            warnings.push(format!(
                "no se pudieron instalar paquetes de seguridad ({}): {err}",
                missing_sec.join(", ")
            ));
        }
    }

    // 2b. Continuidad operativa de nas-web sin CAP_DAC_OVERRIDE en NAS_DATA_ROOT, unidades, shares y staging.
    let env_file = Path::new("/etc/nas-web.env");
    let data_root_str = resolve_data_root(env_file);
    let state_dir = resolve_state_dir(env_file);
    let samba_conf_str =
        std::env::var("NAS_SAMBA_CONF").unwrap_or_else(|_| "/etc/samba/smb.conf".to_string());
    if sync_data_root_permissions_with_principals(
        runner,
        Path::new(&data_root_str),
        Some(&state_dir),
        Path::new(&samba_conf_str),
        "nas-web",
        "grp_samba",
        Some(0),
        options.sudo,
        &mut actions,
        &mut warnings,
    ) {
        restart_nas_web = true;
    }

    if restart_nas_web && service_is_active(runner, "nas-web") {
        let _ = runner.run(&spec_with_sudo(
            CommandSpec::new("systemctl")
                .args(["restart", "nas-web"])
                .timeout(Duration::from_secs(20)),
            options.sudo,
        ));
    }

    if Path::new("/etc/audit").is_dir() {
        match sync_artifact_if_changed(
            runner,
            "/etc/audit/rules.d/nas.rules",
            0o640,
            AUDIT_RULES,
            options.sudo,
        ) {
            Ok(changed) => {
                if changed {
                    let _ = runner.run(&spec_with_sudo(
                        CommandSpec::new("augenrules")
                            .arg("--load")
                            .timeout(Duration::from_secs(20)),
                        options.sudo,
                    ));
                    let _ = runner.run(&spec_with_sudo(
                        CommandSpec::new("systemctl")
                            .args(["enable", "--now", "auditd"])
                            .timeout(Duration::from_secs(20)),
                        options.sudo,
                    ));
                    let _ = runner.run(&spec_with_sudo(
                        CommandSpec::new("systemctl")
                            .args(["restart", "auditd"])
                            .timeout(Duration::from_secs(20)),
                        options.sudo,
                    ));
                    actions.push("reglas auditd sincronizadas y recargadas".into());
                }
            }
            Err(e) => warnings.push(format!("auditd: {e}")),
        }
    }

    if Path::new("/etc/aide/aide.conf.d").is_dir()
        && sync_artifact_if_changed(
            runner,
            "/etc/aide/aide.conf.d/75_aide_nas",
            0o644,
            AIDE_NAS_RULES,
            options.sudo,
        )
        .unwrap_or(false)
    {
        actions.push("sincronizado /etc/aide/aide.conf.d/75_aide_nas".into());
    }

    // 3. Fail2ban + Unattended Upgrades.
    if Path::new("/etc/fail2ban/jail.d").is_dir() {
        match sync_artifact_if_changed(
            runner,
            "/etc/fail2ban/jail.d/nas.local",
            0o644,
            FAIL2BAN_NAS_LOCAL,
            options.sudo,
        ) {
            Ok(changed) => {
                let _ = runner.run(&spec_with_sudo(
                    CommandSpec::new("systemctl")
                        .args(["enable", "--now", "fail2ban"])
                        .timeout(Duration::from_secs(20)),
                    options.sudo,
                ));
                if changed {
                    let _ = runner.run(&spec_with_sudo(
                        CommandSpec::new("systemctl")
                            .args(["reload-or-restart", "fail2ban"])
                            .timeout(Duration::from_secs(20)),
                        options.sudo,
                    ));
                }
                actions.push("fail2ban configurado y habilitado (sshd backend=systemd)".into());
            }
            Err(e) => warnings.push(format!("fail2ban: {e}")),
        }
    }

    let unattended_bin_present = Path::new("/usr/bin/unattended-upgrade").is_file()
        || Path::new("/usr/bin/unattended-upgrades").is_file();
    if unattended_bin_present && Path::new("/etc/apt/apt.conf.d").is_dir() {
        let mut apt_changed = false;
        if let Ok(c) = sync_artifact_if_changed(
            runner,
            "/etc/apt/apt.conf.d/20auto-upgrades",
            0o644,
            APT_AUTO_UPGRADES,
            options.sudo,
        ) {
            apt_changed |= c;
        }
        if let Ok(c) = sync_artifact_if_changed(
            runner,
            "/etc/apt/apt.conf.d/51nas-unattended-upgrades",
            0o644,
            APT_UNATTENDED_NAS,
            options.sudo,
        ) {
            apt_changed |= c;
        }
        if apt_changed {
            actions.push(
                "actualizaciones de seguridad automáticas (unattended-upgrades) configuradas"
                    .into(),
            );
        }
    }

    // 4. Cortafuegos de red UFW (con anti-lockout).
    configure_ufw_firewall(runner, options, &mut actions, &mut warnings)?;

    // 5. Endurecimiento SSH (solo si se solicita explícitamente y hay llaves públicas válidas).
    if options.harden_ssh && !options.safe_only && Path::new("/etc/ssh/sshd_config.d").is_dir() {
        if !has_any_ssh_authorized_key() {
            warnings.push(
                "endurecimiento SSH omitido: no se detectaron llaves públicas válidas en authorized_keys (protección anti-lockout)".into(),
            );
        } else {
            let ssh_path = "/etc/ssh/sshd_config.d/99-nas-hardening.conf";
            let prev = std::fs::read_to_string(ssh_path).ok();
            if write_artifact(
                runner,
                &Artifact {
                    path: ssh_path.into(),
                    mode: 0o644,
                    content: SSH_HARDENING_CONF.into(),
                },
                options.sudo,
            )
            .is_ok()
            {
                let test = runner.run(&spec_with_sudo(
                    CommandSpec::new("sshd")
                        .arg("-t")
                        .timeout(Duration::from_secs(15)),
                    options.sudo,
                ));
                if test.is_ok_and(|o| o.is_success()) {
                    let _ = runner.run(&spec_with_sudo(
                        CommandSpec::new("systemctl")
                            .args(["reload", "ssh"])
                            .timeout(Duration::from_secs(15)),
                        options.sudo,
                    ));
                    actions.push("endurecimiento SSH aplicado y validado con sshd -t".into());
                } else {
                    if let Some(old) = prev {
                        let _ = write_artifact(
                            runner,
                            &Artifact {
                                path: ssh_path.into(),
                                mode: 0o644,
                                content: old,
                            },
                            options.sudo,
                        );
                    } else {
                        let _ = runner.run(&spec_with_sudo(
                            CommandSpec::new("rm")
                                .args(["-f", ssh_path])
                                .timeout(Duration::from_secs(10)),
                            options.sudo,
                        ));
                    }
                    warnings.push(
                        "sshd -t rechazó 99-nas-hardening.conf; se revirtió automáticamente".into(),
                    );
                }
            }
        }
    }

    // 6. Inicialización opcional de AIDE bajo demanda.
    if options.init_aide && !Path::new("/var/lib/aide/aide.db").is_file() {
        let out = runner.run(&spec_with_sudo(
            CommandSpec::new("aideinit")
                .args(["-y", "-f"])
                .timeout(Duration::from_secs(900)),
            options.sudo,
        ));
        if out.is_ok_and(|o| o.is_success()) {
            actions.push("base inicial de AIDE generada".into());
        } else {
            warnings.push("no se pudo inicializar la base de AIDE automáticamente".into());
        }
    }

    Ok(HardenReport {
        applied: true,
        actions,
        warnings,
    })
}

/// Aplica el endurecimiento seguro estándar durante instalación, despliegue o actualización.
pub fn apply_safe_hardening(runner: &Runner, sudo: bool) -> NasResult<HardenReport> {
    harden(
        runner,
        &HardenOptions {
            apply: true,
            safe_only: true,
            force_firewall: false,
            lan_cidr: None,
            harden_ssh: false,
            init_aide: false,
            sudo,
        },
    )
}
