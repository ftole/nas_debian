//! Endurecimiento de seguridad del sistema operativo y sincronización post-instalación/actualización.
//!
//! Implementa de forma idempotente:
//! - Cortafuegos de red (`ufw` con backend `iptables-nft` en Debian 13) con detección automática
//!   de subred LAN y protección anti-lockout sobre `$SSH_CONNECTION`.
//! - Prevención de intrusiones (`fail2ban` con `backend = systemd` y `banaction = nftables`).
//! - Actualizaciones de seguridad desatendidas (`unattended-upgrades`, sin reinicios automáticos).
//! - Reglas `auditd`, exclusiones `aide`, unidad `nas-web.service` (sin `AmbientCapabilities`),
//!   `sudoers` validado con `visudo -cf` y máscara de suspensión ACPI.

use std::collections::BTreeSet;
use std::net::Ipv4Addr;
#[cfg(unix)]
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Component, Path, PathBuf};
use std::time::Duration;

use nas_core::{NasError, NasResult};
use nas_exec::{CommandSpec, Runner, SudoMode};
use serde::Serialize;

use crate::deploy::web::{
    resolve_data_root, validate_sudoers, write_artifact, SUDOERS, SYSTEMD_UNIT,
};
use crate::deploy::{apply_journald_and_sleep_mask, deps, Artifact, RECYCLE_PURGE_SCRIPT};

const AUDIT_RULES: &str = include_str!("../../../packaging/nas.rules");
const AIDE_NAS_RULES: &str = include_str!("../../../packaging/75_aide_nas");
const UDEV_READAHEAD: &str = include_str!("../../../packaging/60-nas-readahead.rules");

const FAIL2BAN_NAS_LOCAL: &str = "\
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

const APT_AUTO_UPGRADES: &str = "\
APT::Periodic::Update-Package-Lists \"1\";
APT::Periodic::Unattended-Upgrade \"1\";
";

const APT_UNATTENDED_NAS: &str = "\
Unattended-Upgrade::Automatic-Reboot \"false\";
";

const SSH_HARDENING_CONF: &str = "\
PermitRootLogin no
PasswordAuthentication no
PubkeyAuthentication yes
X11Forwarding no
";

/// Estado resumido de los controles de seguridad del host.
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

fn spec_with_sudo(mut spec: CommandSpec, sudo: bool) -> CommandSpec {
    if sudo {
        spec = spec.sudo(SudoMode::Passwordless);
    }
    spec
}

fn service_is_active(runner: &Runner, unit: &str) -> bool {
    runner
        .run(
            &CommandSpec::new("systemctl")
                .args(["is-active", unit])
                .timeout(Duration::from_secs(10)),
        )
        .map(|out| out.stdout.trim() == "active")
        .unwrap_or(false)
}

fn is_ufw_enabled_file(path: &Path) -> bool {
    std::fs::read_to_string(path).is_ok_and(|raw| {
        raw.lines().any(|line| {
            line.split_once('=').is_some_and(|(k, v)| {
                k.trim().eq_ignore_ascii_case("ENABLED") && v.trim().eq_ignore_ascii_case("yes")
            })
        })
    })
}

fn is_ufw_active(runner: &Runner, sudo: bool) -> bool {
    if is_ufw_enabled_file(Path::new("/etc/ufw/ufw.conf")) {
        return true;
    }
    let out = runner.run(&spec_with_sudo(
        CommandSpec::new("ufw")
            .arg("status")
            .timeout(Duration::from_secs(10)),
        sudo,
    ));
    out.is_ok_and(|o| o.is_success() && o.stdout.contains("Status: active"))
}

/// Recolecta el estado actual de los controles de seguridad del sistema.
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

/// Calcula la dirección de red CIDR IPv4 (ej. `192.168.1.50/24` -> `192.168.1.0/24`).
pub fn ipv4_cidr_network(cidr: &str) -> Option<String> {
    let (ip_str, prefix_str) = cidr.trim().split_once('/')?;
    let ip: Ipv4Addr = ip_str.trim().parse().ok()?;
    let prefix: u32 = prefix_str.trim().parse().ok()?;
    if prefix > 32 {
        return None;
    }
    if ip.is_loopback() || ip.is_link_local() {
        return None;
    }
    let mask = if prefix == 0 {
        0u32
    } else {
        u32::MAX << (32 - prefix)
    };
    let net = Ipv4Addr::from(u32::from(ip) & mask);
    Some(format!("{net}/{prefix}"))
}

/// Valida una lista separada por comas de subredes CIDR IPv4.
pub fn parse_cidr_list(raw: &str) -> NasResult<Vec<String>> {
    let mut out = Vec::new();
    for item in raw.split(',') {
        let trimmed = item.trim();
        if trimmed.is_empty() {
            continue;
        }
        let net = ipv4_cidr_network(trimmed)
            .ok_or_else(|| NasError::Invalid(format!("subred CIDR IPv4 inválida: '{trimmed}'")))?;
        if !out.contains(&net) {
            out.push(net);
        }
    }
    Ok(out)
}

/// Detecta las subredes LAN IPv4 activas a partir de la salida de `ip -4 -o addr show scope global`.
pub fn parse_ip_addr_cidrs(stdout: &str) -> Vec<String> {
    let mut cidrs = Vec::new();
    for line in stdout.lines() {
        let mut tokens = line.split_whitespace();
        while let Some(tok) = tokens.next() {
            if tok == "inet" {
                if let Some(addr_cidr) = tokens.next() {
                    if let Some(net) = ipv4_cidr_network(addr_cidr) {
                        if !cidrs.contains(&net) {
                            cidrs.push(net);
                        }
                    }
                }
                break;
            }
        }
    }
    cidrs
}

/// Extrae la IPv4 de origen de `SSH_CONNECTION` para la regla anti-lockout de UFW (`IPV6=no`).
pub fn parse_ssh_client_ip(ssh_connection: &str) -> Option<String> {
    let ip_raw = ssh_connection.split_whitespace().next()?.trim();
    if let Ok(ip) = ip_raw.parse::<Ipv4Addr>() {
        if !ip.is_loopback() && !ip.is_link_local() {
            return Some(ip.to_string());
        }
    }
    None
}

/// Extrae el puerto de servidor SSH desde `SSH_CONNECTION` (4º campo; por defecto 22).
pub fn parse_ssh_server_port(ssh_connection: &str) -> u16 {
    ssh_connection
        .split_whitespace()
        .nth(3)
        .and_then(|p| p.trim().parse::<u16>().ok())
        .filter(|&p| p > 0)
        .unwrap_or(22)
}

/// Actualiza `IPV6=no` en `/etc/default/ufw` de forma idempotente.
pub fn render_ufw_default_ipv6_disabled(existing: &str) -> String {
    let mut lines = Vec::new();
    let mut found = false;
    for line in existing.lines() {
        if line.trim_start().starts_with("IPV6=") {
            lines.push("IPV6=no".to_string());
            found = true;
        } else {
            lines.push(line.to_string());
        }
    }
    if !found {
        lines.push("IPV6=no".to_string());
    }
    let mut out = lines.join("\n");
    out.push('\n');
    out
}

fn has_any_ssh_authorized_key() -> bool {
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

fn sync_artifact_needed(path: &Path, mode: u32, content: &str) -> bool {
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

fn sync_artifact_if_changed(
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

const EXCLUDED_DATA_DIRS: &[&str] = &[
    "BACKUPS_HISTORICOS",
    "BACKUPS_PERMANENTES",
    "LOGS_BACKUP",
    ".trash",
    ".nas_staging",
    ".recycle",
    ".snapshots",
    "lost+found",
];

fn has_acl_tools() -> bool {
    let has_setfacl =
        Path::new("/usr/bin/setfacl").is_file() || Path::new("/bin/setfacl").is_file();
    let has_getfacl =
        Path::new("/usr/bin/getfacl").is_file() || Path::new("/bin/getfacl").is_file();
    has_setfacl && has_getfacl
}

fn is_excluded_component(name: &str) -> bool {
    EXCLUDED_DATA_DIRS.contains(&name)
        || name.starts_with(".inprogress_")
        || name.starts_with(".upload-")
        || (name.starts_with("zip_") && name.ends_with(".tmp"))
}

fn is_excluded_relative_path(rel: &Path) -> bool {
    rel.components().any(|comp| match comp {
        Component::Normal(part) => is_excluded_component(&part.to_string_lossy()),
        _ => true,
    })
}

fn is_real_dir(path: &Path) -> bool {
    std::fs::symlink_metadata(path)
        .is_ok_and(|meta| !meta.file_type().is_symlink() && meta.is_dir())
}

/// Comprueba si la salida de `getfacl -p` otorga efectivamente los permisos `required`
/// (`r`, `w`, `x`) al usuario o grupo indicado, respetando la máscara (`#effective:`).
pub fn acl_has_effective(
    getfacl_output: &str,
    principal: &str,
    is_group: bool,
    required: &str,
    default_acl: bool,
) -> bool {
    let prefix = match (default_acl, is_group) {
        (true, true) => "default:group:",
        (false, true) => "group:",
        (true, false) => "default:user:",
        (false, false) => "user:",
    };
    getfacl_output.lines().any(|raw_line| {
        let line = raw_line.trim();
        let Some(rest) = line.strip_prefix(prefix) else {
            return false;
        };
        let Some((name, perms_and_comment)) = rest.split_once(':') else {
            return false;
        };
        if name != principal {
            return false;
        }
        let effective = if let Some((_, eff)) = perms_and_comment.split_once("#effective:") {
            eff.trim()
        } else {
            perms_and_comment.split('#').next().unwrap_or("").trim()
        };
        required.chars().all(|ch| effective.contains(ch))
    })
}

fn dir_has_nas_acls(runner: &Runner, dir: &Path, user: &str, group: &str, sudo: bool) -> bool {
    let dir_str = dir.to_string_lossy();
    let out = runner.run(&spec_with_sudo(
        CommandSpec::new("getfacl")
            .args(["-p", "--", dir_str.as_ref()])
            .timeout(Duration::from_secs(15)),
        sudo,
    ));
    let Ok(out) = out else {
        return false;
    };
    if !out.is_success() {
        return false;
    }
    acl_has_effective(&out.stdout, user, false, "rwx", false)
        && acl_has_effective(&out.stdout, group, true, "rwx", false)
        && acl_has_effective(&out.stdout, user, false, "rwx", true)
        && acl_has_effective(&out.stdout, group, true, "rwx", true)
}

fn split_getfacl_blocks(stdout: &str) -> Vec<String> {
    let mut blocks = Vec::new();
    let mut current = String::new();
    for line in stdout.lines() {
        if line.starts_with("# file:") && !current.is_empty() {
            blocks.push(std::mem::take(&mut current));
        }
        if !line.trim().is_empty() {
            current.push_str(line);
            current.push('\n');
        }
    }
    if !current.is_empty() {
        blocks.push(current);
    }
    blocks
}

fn batch_has_nas_acls(
    runner: &Runner,
    paths: &[PathBuf],
    user: &str,
    group: &str,
    is_dirs: bool,
    sudo: bool,
) -> bool {
    if paths.is_empty() {
        return true;
    }
    for chunk in paths.chunks(256) {
        let mut args = vec!["-p".to_string(), "--".to_string()];
        args.extend(chunk.iter().map(|p| p.to_string_lossy().to_string()));
        let Ok(out) = runner.run(&spec_with_sudo(
            CommandSpec::new("getfacl")
                .args(args)
                .timeout(Duration::from_secs(30)),
            sudo,
        )) else {
            return false;
        };
        if !out.is_success() {
            return false;
        }
        let blocks = split_getfacl_blocks(&out.stdout);
        if blocks.len() != chunk.len() {
            return false;
        }
        let all_ok = blocks.iter().all(|block| {
            if is_dirs {
                acl_has_effective(block, user, false, "rwx", false)
                    && acl_has_effective(block, group, true, "rwx", false)
                    && acl_has_effective(block, user, false, "rwx", true)
                    && acl_has_effective(block, group, true, "rwx", true)
            } else {
                acl_has_effective(block, user, false, "rw", false)
                    && acl_has_effective(block, group, true, "rw", false)
            }
        });
        if !all_ok {
            return false;
        }
    }
    true
}

fn share_tree_needs_nas_acls(
    runner: &Runner,
    share_dir: &Path,
    user: &str,
    group: &str,
    sudo: bool,
) -> bool {
    if !dir_has_nas_acls(runner, share_dir, user, group, sudo) {
        return true;
    }

    let mut stack = vec![share_dir.to_path_buf()];
    let mut dir_batch: Vec<PathBuf> = Vec::new();
    let mut file_batch: Vec<PathBuf> = Vec::new();

    while let Some(current) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&current) else {
            return true;
        };
        for entry in entries {
            let Ok(entry) = entry else {
                return true;
            };
            let path = entry.path();
            let Ok(meta) = std::fs::symlink_metadata(&path) else {
                return true;
            };
            if meta.file_type().is_symlink() {
                continue;
            }
            if meta.is_dir() {
                #[cfg(unix)]
                if (meta.mode() & 0o070) != 0o070 {
                    return true;
                }
                stack.push(path.clone());
                dir_batch.push(path);
                if dir_batch.len() >= 256 {
                    if !batch_has_nas_acls(runner, &dir_batch, user, group, true, sudo) {
                        return true;
                    }
                    dir_batch.clear();
                }
            } else if meta.is_file() {
                #[cfg(unix)]
                if (meta.mode() & 0o060) != 0o060 {
                    return true;
                }
                file_batch.push(path);
                if file_batch.len() >= 256 {
                    if !batch_has_nas_acls(runner, &file_batch, user, group, false, sudo) {
                        return true;
                    }
                    file_batch.clear();
                }
            }
        }
    }

    if !batch_has_nas_acls(runner, &dir_batch, user, group, true, sudo) {
        return true;
    }
    if !batch_has_nas_acls(runner, &file_batch, user, group, false, sudo) {
        return true;
    }

    false
}

fn resolve_unix_user_ids(runner: &Runner, user: &str) -> Option<(u32, u32, String)> {
    let uid_out = runner
        .run(
            &CommandSpec::new("id")
                .args(["-u", user])
                .timeout(Duration::from_secs(5)),
        )
        .ok()
        .filter(|o| o.is_success())?;
    let gid_out = runner
        .run(
            &CommandSpec::new("id")
                .args(["-g", user])
                .timeout(Duration::from_secs(5)),
        )
        .ok()
        .filter(|o| o.is_success())?;
    let gname_out = runner
        .run(
            &CommandSpec::new("id")
                .args(["-gn", user])
                .timeout(Duration::from_secs(5)),
        )
        .ok()
        .filter(|o| o.is_success())?;
    let uid = uid_out.stdout.trim().parse::<u32>().ok()?;
    let gid = gid_out.stdout.trim().parse::<u32>().ok()?;
    let gname = gname_out.stdout.trim().to_string();
    if gname.is_empty() {
        return None;
    }
    Some((uid, gid, gname))
}

fn resolve_unix_group_gid(runner: &Runner, group: &str) -> Option<u32> {
    let out = runner
        .run(
            &CommandSpec::new("getent")
                .args(["group", group])
                .timeout(Duration::from_secs(5)),
        )
        .ok()
        .filter(|o| o.is_success())?;
    out.stdout.trim().split(':').nth(2)?.parse::<u32>().ok()
}

fn user_in_group(runner: &Runner, user: &str, group: &str) -> bool {
    runner
        .run(
            &CommandSpec::new("id")
                .args(["-nG", user])
                .timeout(Duration::from_secs(5)),
        )
        .is_ok_and(|o| o.is_success() && o.stdout.split_whitespace().any(|g| g == group))
}

/// Descubre los directorios raíz de unidades adicionales montadas bajo `<data_root>/unidades`.
pub fn discover_unit_roots(data_root: &Path) -> Vec<PathBuf> {
    let unidades_dir = data_root.join("unidades");
    if !is_real_dir(&unidades_dir) {
        return Vec::new();
    }
    let mut roots = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&unidades_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();
            if name == "unidades" || is_excluded_component(&name) {
                continue;
            }
            if is_real_dir(&path) {
                roots.push(path);
            }
        }
    }
    roots.sort();
    roots
}

/// Descubre los shares activos bajo `data_root` y `smb_conf`, excluyendo respaldos, logs, `.trash`,
/// `.nas_staging`, `unidades` y enlaces simbólicos.
pub fn discover_active_share_dirs(
    data_root: &Path,
    smb_conf: &Path,
    unit_roots: &[PathBuf],
) -> Vec<PathBuf> {
    let Ok(canon_data_root) = std::fs::canonicalize(data_root) else {
        return Vec::new();
    };
    let unidades_dir = data_root.join("unidades");
    let canon_unidades = std::fs::canonicalize(&unidades_dir).ok();
    let canon_units: BTreeSet<PathBuf> = unit_roots
        .iter()
        .filter_map(|p| std::fs::canonicalize(p).ok())
        .collect();

    let mut candidates: Vec<PathBuf> = Vec::new();

    for parent in std::iter::once(data_root).chain(unit_roots.iter().map(PathBuf::as_path)) {
        if let Ok(entries) = std::fs::read_dir(parent) {
            for entry in entries.flatten() {
                let path = entry.path();
                let name = entry.file_name().to_string_lossy().to_string();
                if name == "unidades" || is_excluded_component(&name) {
                    continue;
                }
                if is_real_dir(&path) {
                    candidates.push(path);
                }
            }
        }
    }

    if let Ok(conf) = std::fs::read_to_string(smb_conf) {
        for share in crate::parsers::smb::parse_shares(&conf) {
            let Some(raw_path) = share.path else {
                continue;
            };
            let trimmed = raw_path.trim();
            if trimmed.is_empty() {
                continue;
            }
            let path = PathBuf::from(trimmed);
            if !is_real_dir(&path) {
                continue;
            }
            let Ok(canon_path) = std::fs::canonicalize(&path) else {
                continue;
            };
            let Ok(rel) = canon_path.strip_prefix(&canon_data_root) else {
                continue;
            };
            if rel.as_os_str().is_empty() {
                continue;
            }
            if canon_unidades.as_ref().is_some_and(|u| u == &canon_path)
                || canon_units.contains(&canon_path)
            {
                continue;
            }
            if is_excluded_relative_path(rel) {
                continue;
            }
            candidates.push(path);
        }
    }

    let mut seen_canon: Vec<(PathBuf, PathBuf)> = Vec::new();
    for path in candidates {
        if let Ok(canon) = std::fs::canonicalize(&path) {
            if !seen_canon.iter().any(|(c, _)| c == &canon) {
                seen_canon.push((canon, path));
            }
        }
    }
    seen_canon.sort_by(|a, b| {
        a.0.components()
            .count()
            .cmp(&b.0.components().count())
            .then(a.0.cmp(&b.0))
    });

    let mut filtered: Vec<(PathBuf, PathBuf)> = Vec::new();
    for (canon, orig) in seen_canon {
        if filtered
            .iter()
            .any(|(parent_canon, _)| canon.starts_with(parent_canon))
        {
            continue;
        }
        filtered.push((canon, orig));
    }

    filtered.into_iter().map(|(_, orig)| orig).collect()
}

#[allow(clippy::too_many_arguments)]
fn ensure_root_dir_access(
    runner: &Runner,
    dir: &Path,
    service_user: &str,
    service_group: &str,
    expected_owner_uid: Option<u32>,
    grp_gid: u32,
    sudo: bool,
    warnings: &mut Vec<String>,
) -> bool {
    let Ok(meta) = std::fs::symlink_metadata(dir) else {
        return false;
    };
    if meta.file_type().is_symlink() || !meta.is_dir() {
        return false;
    }

    let dir_str = dir.to_string_lossy().to_string();
    let mut changed = false;

    #[cfg(unix)]
    let needs_chown = expected_owner_uid.is_some_and(|u| meta.uid() != u) || meta.gid() != grp_gid;
    #[cfg(not(unix))]
    let needs_chown = false;

    if needs_chown {
        let owner_spec = if expected_owner_uid.is_some() {
            format!("root:{service_group}")
        } else {
            format!(":{service_group}")
        };
        match runner.run(&spec_with_sudo(
            CommandSpec::new("chown")
                .args([&owner_spec, "--", &dir_str])
                .timeout(Duration::from_secs(20)),
            sudo,
        )) {
            Ok(out) if out.is_success() => changed = true,
            Ok(out) => warnings.push(format!(
                "no se pudo ajustar propietario/grupo en {dir_str}: {}",
                out.stderr.trim()
            )),
            Err(e) => warnings.push(format!(
                "no se pudo ajustar propietario/grupo en {dir_str}: {e}"
            )),
        }
    }

    #[cfg(unix)]
    let needs_chmod = (meta.mode() & 0o7777) != 0o2770;
    #[cfg(not(unix))]
    let needs_chmod = false;

    if needs_chmod {
        match runner.run(&spec_with_sudo(
            CommandSpec::new("chmod")
                .args(["2770", "--", &dir_str])
                .timeout(Duration::from_secs(20)),
            sudo,
        )) {
            Ok(out) if out.is_success() => changed = true,
            Ok(out) => warnings.push(format!(
                "no se pudo ajustar modo 2770 en {dir_str}: {}",
                out.stderr.trim()
            )),
            Err(e) => warnings.push(format!("no se pudo ajustar modo 2770 en {dir_str}: {e}")),
        }
    }

    if has_acl_tools() && !dir_has_nas_acls(runner, dir, service_user, service_group, sudo) {
        let acl_spec = format!(
            "u:{service_user}:rwx,g:{service_group}:rwx,m::rwx,d:u:{service_user}:rwx,d:g:{service_group}:rwx,d:m::rwx"
        );
        match runner.run(&spec_with_sudo(
            CommandSpec::new("setfacl")
                .args(["-m", &acl_spec, "--", &dir_str])
                .timeout(Duration::from_secs(30)),
            sudo,
        )) {
            Ok(out) if out.is_success() => changed = true,
            Ok(out) => warnings.push(format!(
                "no se pudieron aplicar ACLs en {dir_str}: {}",
                out.stderr.trim()
            )),
            Err(e) => warnings.push(format!("no se pudieron aplicar ACLs en {dir_str}: {e}")),
        }
    }

    changed
}

fn staging_has_foreign_owner(staging: &Path, uid: u32, gid: u32) -> bool {
    let Ok(entries) = std::fs::read_dir(staging) else {
        return false;
    };
    for entry in entries.flatten() {
        if let Ok(meta) = std::fs::symlink_metadata(entry.path()) {
            #[cfg(unix)]
            if meta.uid() != uid || meta.gid() != gid {
                return true;
            }
            if !meta.file_type().is_symlink()
                && meta.is_dir()
                && staging_has_foreign_owner(&entry.path(), uid, gid)
            {
                return true;
            }
        }
    }
    false
}

#[allow(clippy::too_many_arguments)]
fn ensure_staging_path(
    runner: &Runner,
    staging: &Path,
    create_if_missing: bool,
    service_user: &str,
    service_user_group: &str,
    uid: u32,
    user_gid: u32,
    sudo: bool,
    warnings: &mut Vec<String>,
) -> bool {
    let staging_str = staging.to_string_lossy().to_string();

    match std::fs::symlink_metadata(staging) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            if !create_if_missing {
                return false;
            }
            match runner.run(&spec_with_sudo(
                CommandSpec::new("install")
                    .args([
                        "-d",
                        "-m",
                        "00700",
                        "-o",
                        service_user,
                        "-g",
                        service_user_group,
                        "--",
                        &staging_str,
                    ])
                    .timeout(Duration::from_secs(20)),
                sudo,
            )) {
                Ok(out) if out.is_success() => {
                    if has_acl_tools() {
                        let _ = runner.run(&spec_with_sudo(
                            CommandSpec::new("setfacl")
                                .args(["-b", "--", &staging_str])
                                .timeout(Duration::from_secs(20)),
                            sudo,
                        ));
                    }
                    true
                }
                Ok(out) => {
                    warnings.push(format!(
                        "no se pudo crear {staging_str}: {}",
                        out.stderr.trim()
                    ));
                    false
                }
                Err(err) => {
                    warnings.push(format!("no se pudo crear {staging_str}: {err}"));
                    false
                }
            }
        }
        Err(_) => false,
        Ok(meta) => {
            if meta.file_type().is_symlink() || !meta.is_dir() {
                warnings.push(format!(
                    "{staging_str} no es un directorio regular; omitido"
                ));
                return false;
            }

            #[cfg(unix)]
            let needs_fix = meta.uid() != uid
                || meta.gid() != user_gid
                || (meta.mode() & 0o7777) != 0o700
                || staging_has_foreign_owner(staging, uid, user_gid);
            #[cfg(not(unix))]
            let needs_fix = false;

            if !needs_fix {
                return false;
            }

            let owner_spec = format!("{service_user}:{service_user_group}");
            let mut changed = false;
            match runner.run(&spec_with_sudo(
                CommandSpec::new("chown")
                    .args(["-PR", &owner_spec, "--", &staging_str])
                    .timeout(Duration::from_secs(60)),
                sudo,
            )) {
                Ok(out) if out.is_success() => changed = true,
                Ok(out) => warnings.push(format!(
                    "no se pudo corregir propietario de {staging_str}: {}",
                    out.stderr.trim()
                )),
                Err(e) => warnings.push(format!(
                    "no se pudo corregir propietario de {staging_str}: {e}"
                )),
            }

            if has_acl_tools() {
                let _ = runner.run(&spec_with_sudo(
                    CommandSpec::new("setfacl")
                        .args(["-b", "--", &staging_str])
                        .timeout(Duration::from_secs(20)),
                    sudo,
                ));
            }

            match runner.run(&spec_with_sudo(
                CommandSpec::new("chmod")
                    .args(["00700", "--", &staging_str])
                    .timeout(Duration::from_secs(20)),
                sudo,
            )) {
                Ok(out) if out.is_success() => changed = true,
                Ok(out) => warnings.push(format!(
                    "no se pudo ajustar modo 0700 en {staging_str}: {}",
                    out.stderr.trim()
                )),
                Err(e) => warnings.push(format!(
                    "no se pudo ajustar modo 0700 en {staging_str}: {e}"
                )),
            }

            changed
        }
    }
}

fn ensure_active_share_acls(
    runner: &Runner,
    share_dir: &Path,
    service_user: &str,
    service_group: &str,
    sudo: bool,
    warnings: &mut Vec<String>,
) -> bool {
    if !is_real_dir(share_dir) || !has_acl_tools() {
        return false;
    }
    if !share_tree_needs_nas_acls(runner, share_dir, service_user, service_group, sudo) {
        return false;
    }

    let share_str = share_dir.to_string_lossy().to_string();
    let acl_spec = format!(
        "u:{service_user}:rwX,g:{service_group}:rwX,d:u:{service_user}:rwx,d:g:{service_group}:rwx"
    );
    match runner.run(&spec_with_sudo(
        CommandSpec::new("setfacl")
            .args(["-RP", "-m", &acl_spec, "--", &share_str])
            .timeout(Duration::from_secs(300)),
        sudo,
    )) {
        Ok(out) if out.is_success() => true,
        Ok(out) => {
            warnings.push(format!(
                "algunos elementos de {share_str} no admitieron ACLs para {service_user}: {}",
                out.stderr.trim()
            ));
            false
        }
        Err(e) => {
            warnings.push(format!(
                "no se pudieron sincronizar ACLs en {share_str}: {e}"
            ));
            false
        }
    }
}

fn resolve_state_dir(env_file: &Path) -> PathBuf {
    if let Ok(val) = std::env::var("NAS_STATE_DIR") {
        let trimmed = val.trim();
        if trimmed.starts_with('/') && !trimmed.contains("..") {
            return PathBuf::from(trimmed);
        }
    }
    if let Ok(raw) = std::fs::read_to_string(env_file) {
        for line in raw.lines() {
            let trimmed = line.trim();
            if let Some(rest) = trimmed.strip_prefix("NAS_STATE_DIR=") {
                let unquoted = rest.trim_matches(|c| c == '"' || c == '\'' || c == ' ');
                if unquoted.starts_with('/') && !unquoted.contains("..") {
                    return PathBuf::from(unquoted);
                }
            }
        }
    }
    PathBuf::from("/var/lib/nas")
}

/// Sincroniza de forma idempotente (y sin recursión destructiva sobre `BACKUPS_*` ni `LOGS_BACKUP`)
/// los permisos y ACLs de `service_user` y `service_group` en `data_root`, `unidades`, shares activos, `.nas_staging` y `<state_dir>/staging`.
#[allow(clippy::too_many_arguments)]
pub(crate) fn sync_data_root_permissions_with_principals(
    runner: &Runner,
    data_root: &Path,
    state_dir: Option<&Path>,
    smb_conf: &Path,
    service_user: &str,
    service_group: &str,
    expected_root_owner_uid: Option<u32>,
    sudo: bool,
    actions: &mut Vec<String>,
    warnings: &mut Vec<String>,
) -> bool {
    if !is_real_dir(data_root) {
        return false;
    }

    let Some((uid, user_gid, user_primary_group)) = resolve_unix_user_ids(runner, service_user)
    else {
        warnings.push(format!(
            "usuario '{service_user}' inexistente; omitida sincronización de ACLs en {}",
            data_root.display()
        ));
        return false;
    };
    let Some(grp_gid) = resolve_unix_group_gid(runner, service_group) else {
        warnings.push(format!(
            "grupo '{service_group}' inexistente; omitida sincronización de ACLs en {}",
            data_root.display()
        ));
        return false;
    };

    let mut restart_needed = false;
    if !user_in_group(runner, service_user, service_group) {
        match runner.run(&spec_with_sudo(
            CommandSpec::new("usermod")
                .args(["-aG", service_group, service_user])
                .timeout(Duration::from_secs(20)),
            sudo,
        )) {
            Ok(out) if out.is_success() => {
                actions.push(format!(
                    "agregado {service_user} al grupo suplementario {service_group}"
                ));
                restart_needed = true;
            }
            Ok(out) => warnings.push(format!(
                "no se pudo agregar {service_user} a {service_group}: {}",
                out.stderr.trim()
            )),
            Err(e) => warnings.push(format!(
                "no se pudo agregar {service_user} a {service_group}: {e}"
            )),
        }
    }

    let mut adjusted_roots = Vec::new();
    if ensure_root_dir_access(
        runner,
        data_root,
        service_user,
        service_group,
        expected_root_owner_uid,
        grp_gid,
        sudo,
        warnings,
    ) {
        adjusted_roots.push(data_root.display().to_string());
    }

    let unidades_dir = data_root.join("unidades");
    if is_real_dir(&unidades_dir)
        && ensure_root_dir_access(
            runner,
            &unidades_dir,
            service_user,
            service_group,
            expected_root_owner_uid,
            grp_gid,
            sudo,
            warnings,
        )
    {
        adjusted_roots.push(unidades_dir.display().to_string());
    }

    let unit_roots = discover_unit_roots(data_root);
    for unit_root in &unit_roots {
        if ensure_root_dir_access(
            runner,
            unit_root,
            service_user,
            service_group,
            expected_root_owner_uid,
            grp_gid,
            sudo,
            warnings,
        ) {
            adjusted_roots.push(unit_root.display().to_string());
        }
    }
    if !adjusted_roots.is_empty() {
        actions.push(format!(
            "permisos/ACLs de {service_user} y {service_group} sincronizados en raíces: {}",
            adjusted_roots.join(", ")
        ));
    }

    let mut adjusted_staging = Vec::new();
    let data_staging = data_root.join(".nas_staging");
    if ensure_staging_path(
        runner,
        &data_staging,
        true,
        service_user,
        &user_primary_group,
        uid,
        user_gid,
        sudo,
        warnings,
    ) {
        adjusted_staging.push(data_staging.display().to_string());
    }
    for unit_root in &unit_roots {
        let unit_staging = unit_root.join(".nas_staging");
        if ensure_staging_path(
            runner,
            &unit_staging,
            false,
            service_user,
            &user_primary_group,
            uid,
            user_gid,
            sudo,
            warnings,
        ) {
            adjusted_staging.push(unit_staging.display().to_string());
        }
    }
    if let Some(st_dir) = state_dir {
        let state_staging = st_dir.join("staging");
        if ensure_staging_path(
            runner,
            &state_staging,
            false,
            service_user,
            &user_primary_group,
            uid,
            user_gid,
            sudo,
            warnings,
        ) {
            adjusted_staging.push(state_staging.display().to_string());
        }
    }
    if !adjusted_staging.is_empty() {
        actions.push(format!(
            "propiedad/permisos de staging sincronizados para {service_user}: {}",
            adjusted_staging.join(", ")
        ));
    }

    let share_dirs = discover_active_share_dirs(data_root, smb_conf, &unit_roots);
    let mut adjusted_shares = Vec::new();
    for share_dir in share_dirs {
        if ensure_active_share_acls(
            runner,
            &share_dir,
            service_user,
            service_group,
            sudo,
            warnings,
        ) {
            adjusted_shares.push(share_dir.display().to_string());
        }
    }
    if !adjusted_shares.is_empty() {
        actions.push(format!(
            "ACLs de acceso para {service_user} y {service_group} sincronizadas en shares: {}",
            adjusted_shares.join(", ")
        ));
    }

    restart_needed
}

fn sync_sudoers_validated(runner: &Runner, sudo: bool) -> NasResult<bool> {
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

fn sync_nginx_conf_validated(runner: &Runner, sudo: bool) -> NasResult<bool> {
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

fn configure_ufw_firewall(
    runner: &Runner,
    options: &HardenOptions,
    actions: &mut Vec<String>,
    warnings: &mut Vec<String>,
) -> NasResult<()> {
    let ufw_present = Path::new("/usr/sbin/ufw").is_file() || Path::new("/sbin/ufw").is_file();
    if !ufw_present {
        if !options.apply {
            actions.push("[dry-run] instalar paquete ufw y activar reglas LAN".into());
            return Ok(());
        }
        if let Err(err) = deps::ensure_packages(runner, &["ufw"], options.sudo) {
            warnings.push(format!("no se pudo instalar ufw automáticamente: {err}"));
            return Ok(());
        }
    }

    let ufw_active = is_ufw_active(runner, options.sudo);
    let nft_active = service_is_active(runner, "nftables");
    if (ufw_active || nft_active) && !options.force_firewall {
        actions.push(format!(
            "cortafuegos ya activo ({}): reglas existentes preservadas",
            if ufw_active { "ufw" } else { "nftables" }
        ));
        return Ok(());
    }

    let explicit_cidrs = options
        .lan_cidr
        .clone()
        .or_else(|| std::env::var("NAS_LAN_CIDR").ok());
    let cidrs = if let Some(raw) = explicit_cidrs.filter(|s| !s.trim().is_empty()) {
        parse_cidr_list(&raw)?
    } else {
        let out = runner.run(
            &CommandSpec::new("ip")
                .args(["-4", "-o", "addr", "show", "scope", "global"])
                .timeout(Duration::from_secs(10)),
        );
        out.ok()
            .filter(|o| o.is_success())
            .map(|o| parse_ip_addr_cidrs(&o.stdout))
            .unwrap_or_default()
    };

    let ssh_conn_env = std::env::var("SSH_CONNECTION").ok();
    let ssh_client_ip = ssh_conn_env.as_deref().and_then(parse_ssh_client_ip);
    let ssh_port = ssh_conn_env
        .as_deref()
        .map(parse_ssh_server_port)
        .unwrap_or(22);
    let ssh_port_str = ssh_port.to_string();

    if !options.apply {
        actions.push(format!(
            "[dry-run] configurar ufw (default deny incoming, LAN={:?}, ssh_anti_lockout={:?}, ssh_port={})",
            cidrs, ssh_client_ip, ssh_port
        ));
        return Ok(());
    }

    if let Ok(default_ufw) = std::fs::read_to_string("/etc/default/ufw") {
        let updated = render_ufw_default_ipv6_disabled(&default_ufw);
        if updated != default_ufw {
            let _ = write_artifact(
                runner,
                &Artifact {
                    path: "/etc/default/ufw".into(),
                    mode: 0o644,
                    content: updated,
                },
                options.sudo,
            );
        }
    }

    let run_ufw = |args: &[&str]| -> NasResult<()> {
        let out = runner.run(&spec_with_sudo(
            CommandSpec::new("ufw")
                .args(args.iter().copied())
                .timeout(Duration::from_secs(20)),
            options.sudo,
        ))?;
        if !out.is_success() {
            return Err(NasError::Denied(format!(
                "ufw {} falló: {}",
                args.join(" "),
                if out.stderr.trim().is_empty() {
                    out.stdout.trim()
                } else {
                    out.stderr.trim()
                }
            )));
        }
        Ok(())
    };

    run_ufw(&["default", "deny", "incoming"])?;
    run_ufw(&["default", "allow", "outgoing"])?;

    if let Some(ref ssh_ip) = ssh_client_ip {
        run_ufw(&[
            "allow",
            "from",
            ssh_ip,
            "to",
            "any",
            "port",
            &ssh_port_str,
            "proto",
            "tcp",
            "comment",
            "SSH admin anti-lockout",
        ])?;
    }

    let mut ports = vec![
        ("22", "tcp", "SSH LAN"),
        ("80", "tcp", "Panel NAS HTTP redirect"),
        ("443", "tcp", "Panel NAS HTTPS"),
        ("139", "tcp", "NetBIOS SMB"),
        ("445", "tcp", "Samba SMB"),
        ("137", "udp", "NetBIOS NS"),
        ("138", "udp", "NetBIOS DGM"),
        ("3702", "udp", "WSDD2 Discovery"),
        ("5355", "tcp", "LLMNR TCP"),
        ("5355", "udp", "LLMNR UDP"),
        ("5357", "tcp", "WSDD2 HTTP"),
    ];
    if ssh_port != 22 {
        ports.push((&ssh_port_str, "tcp", "SSH LAN custom"));
    }

    if cidrs.is_empty() {
        warnings.push(
            "no se detectó subred LAN IPv4; aplicando reglas UFW por puerto sin restricción de origen para evitar lockout".into(),
        );
        for (port, proto, comment) in &ports {
            let rule = format!("{port}/{proto}");
            run_ufw(&["allow", &rule, "comment", comment])?;
        }
    } else {
        for cidr in &cidrs {
            for (port, proto, comment) in &ports {
                run_ufw(&[
                    "allow", "from", cidr, "to", "any", "port", port, "proto", proto, "comment",
                    comment,
                ])?;
            }
        }
    }

    run_ufw(&["--force", "enable"])?;
    actions.push(format!(
        "cortafuegos ufw activado (deny incoming, LAN={})",
        if cidrs.is_empty() {
            "any".to_string()
        } else {
            cidrs.join(",")
        }
    ));
    Ok(())
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn calcula_cidr_de_red_ipv4_y_descarta_loopback() {
        assert_eq!(
            ipv4_cidr_network("192.168.1.50/24").as_deref(),
            Some("192.168.1.0/24")
        );
        assert_eq!(
            ipv4_cidr_network("10.10.1.222/16").as_deref(),
            Some("10.10.0.0/16")
        );
        assert_eq!(ipv4_cidr_network("127.0.0.1/8"), None);
        assert_eq!(ipv4_cidr_network("169.254.1.2/16"), None);
        assert_eq!(ipv4_cidr_network("invalido"), None);
    }

    #[test]
    fn extrae_cidrs_desde_ip_addr_y_ssh_connection() {
        let sample = "\
2: eth0    inet 10.10.1.20/24 brd 10.10.1.255 scope global dynamic eth0
3: eth1    inet 192.168.50.10/24 brd 192.168.50.255 scope global eth1
1: lo      inet 127.0.0.1/8 scope host lo";
        assert_eq!(
            parse_ip_addr_cidrs(sample),
            vec!["10.10.1.0/24".to_string(), "192.168.50.0/24".to_string()]
        );

        assert_eq!(
            parse_ssh_client_ip("203.0.113.10 54321 10.10.1.20 22").as_deref(),
            Some("203.0.113.10")
        );
        assert_eq!(
            parse_ssh_client_ip("2001:db8::1 54321 2001:db8::2 22"),
            None
        );
        assert_eq!(parse_ssh_client_ip("127.0.0.1 1234 127.0.0.1 22"), None);
        assert_eq!(
            parse_ssh_server_port("203.0.113.10 54321 10.10.1.20 2222"),
            2222
        );
        assert_eq!(parse_ssh_server_port("203.0.113.10 54321 10.10.1.20"), 22);
    }

    #[test]
    fn desactiva_ipv6_en_default_ufw_idempotente() {
        let orig = "# /etc/default/ufw\nIPV6=yes\nDEFAULT_INPUT_POLICY=\"DROP\"\n";
        let updated = render_ufw_default_ipv6_disabled(orig);
        assert!(updated.contains("IPV6=no"));
        assert!(!updated.contains("IPV6=yes"));
        assert_eq!(render_ufw_default_ipv6_disabled(&updated), updated);
    }

    #[test]
    fn dry_run_no_muta_sistema() {
        let report = harden(
            &Runner::new(),
            &HardenOptions {
                apply: false,
                safe_only: true,
                ..Default::default()
            },
        )
        .expect("dry run");
        assert!(!report.applied);
        assert!(!report.actions.is_empty());
        assert!(report
            .actions
            .iter()
            .any(|a| a.contains("nas-recycle-purge")));
        assert!(report.actions.iter().any(|a| a.contains("NAS_DATA_ROOT")));
    }

    #[test]
    fn acl_has_effective_respeta_mascara_y_default() {
        let sample = "\
# file: /srv/nas/PUBLICO
# owner: root
# group: grp_samba
user::rwx
user:www-data:rwx
user:nas-web:rwx
group::rwx
group:grp_samba:rwx\t\t#effective:r-x
mask::r-x
other::---
default:user::rwx
default:user:nas-web:rwx
default:group::rwx
default:group:grp_samba:rwx
default:mask::rwx
default:other::---
";
        assert!(acl_has_effective(sample, "nas-web", false, "rwx", false));
        assert!(acl_has_effective(sample, "nas-web", false, "rwx", true));
        assert!(acl_has_effective(sample, "www-data", false, "rwx", false));
        assert!(!acl_has_effective(sample, "www-data", false, "rwx", true));
        assert!(
            !acl_has_effective(sample, "grp_samba", true, "rwx", false),
            "debe respetar #effective:r-x cuando mask restringe escritura"
        );
        assert!(acl_has_effective(sample, "grp_samba", true, "rx", false));
        assert!(acl_has_effective(sample, "grp_samba", true, "rwx", true));
    }

    #[test]
    fn discover_active_share_dirs_excluye_respaldos_logs_trash_staging_y_symlinks() {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let base =
            std::env::temp_dir().join(format!("nas_sec_discover_{}_{}", std::process::id(), nanos));
        let data_root = base.join("srv_nas");
        let external = base.join("external");
        std::fs::create_dir_all(&external).unwrap();

        for rel in [
            "PUBLICO",
            "Test",
            "EXTRA_FROM_CONF",
            "BACKUPS_HISTORICOS/t1/snap1",
            "BACKUPS_PERMANENTES/t2",
            "LOGS_BACKUP",
            ".trash",
            ".nas_staging",
            ".snapshots",
            "lost+found",
            "unidades/u1/CONTABILIDAD",
            "unidades/u1/BACKUPS_HISTORICOS/s1",
            "unidades/u1/LOGS_BACKUP",
            "unidades/u1/.trash",
            "unidades/u1/.nas_staging",
        ] {
            std::fs::create_dir_all(data_root.join(rel)).unwrap();
        }
        std::os::unix::fs::symlink(&external, data_root.join("symlink_share")).unwrap();

        let smb_conf = base.join("smb.conf");
        std::fs::write(
            &smb_conf,
            format!(
                "[global]\n   workgroup = WG\n\
                 [PUBLICO]\n   path = {}\n\
                 [EXTRA]\n   path = {}\n\
                 [BKP_NESTED]\n   path = {}\n\
                 [OUTSIDE]\n   path = {}\n",
                data_root.join("PUBLICO").display(),
                data_root.join("EXTRA_FROM_CONF").display(),
                data_root.join("BACKUPS_HISTORICOS/t1").display(),
                external.display(),
            ),
        )
        .unwrap();

        let unit_roots = discover_unit_roots(&data_root);
        assert_eq!(unit_roots, vec![data_root.join("unidades/u1")]);

        let shares = discover_active_share_dirs(&data_root, &smb_conf, &unit_roots);
        let share_set: std::collections::BTreeSet<PathBuf> = shares.into_iter().collect();

        assert!(share_set.contains(&data_root.join("PUBLICO")));
        assert!(share_set.contains(&data_root.join("Test")));
        assert!(share_set.contains(&data_root.join("EXTRA_FROM_CONF")));
        assert!(share_set.contains(&data_root.join("unidades/u1/CONTABILIDAD")));

        assert!(!share_set.contains(&data_root.join("BACKUPS_HISTORICOS")));
        assert!(!share_set.contains(&data_root.join("BACKUPS_HISTORICOS/t1")));
        assert!(!share_set.contains(&data_root.join("BACKUPS_PERMANENTES")));
        assert!(!share_set.contains(&data_root.join("LOGS_BACKUP")));
        assert!(!share_set.contains(&data_root.join(".trash")));
        assert!(!share_set.contains(&data_root.join(".nas_staging")));
        assert!(!share_set.contains(&data_root.join(".snapshots")));
        assert!(!share_set.contains(&data_root.join("lost+found")));
        assert!(!share_set.contains(&data_root.join("unidades")));
        assert!(!share_set.contains(&data_root.join("unidades/u1")));
        assert!(!share_set.contains(&data_root.join("unidades/u1/BACKUPS_HISTORICOS")));
        assert!(!share_set.contains(&data_root.join("symlink_share")));
        assert!(!share_set.contains(&external));

        let _ = std::fs::remove_dir_all(&base);
    }

    #[cfg(unix)]
    #[test]
    fn sync_data_root_permissions_migra_acls_staging_y_unidades_sin_tocar_backups_e_idempotente() {
        if !has_acl_tools() {
            return;
        }
        let runner = Runner::new();
        let user_out = runner
            .run(&CommandSpec::new("id").args(["-un"]))
            .expect("id -un");
        let group_out = runner
            .run(&CommandSpec::new("id").args(["-gn"]))
            .expect("id -gn");
        if !user_out.is_success() || !group_out.is_success() {
            return;
        }
        let current_user = user_out.stdout.trim().to_string();
        let current_group = group_out.stdout.trim().to_string();

        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let base =
            std::env::temp_dir().join(format!("nas_sec_acls_{}_{}", std::process::id(), nanos));
        let data_root = base.join("srv_nas");
        let state_dir = base.join("state_nas");
        let external = base.join("external_target");

        std::fs::create_dir_all(data_root.join("PUBLICO/sub")).unwrap();
        std::fs::create_dir_all(data_root.join("unidades/u1/CONTABILIDAD")).unwrap();
        std::fs::create_dir_all(data_root.join("BACKUPS_HISTORICOS/snap1")).unwrap();
        std::fs::create_dir_all(data_root.join("LOGS_BACKUP")).unwrap();
        std::fs::create_dir_all(data_root.join(".nas_staging")).unwrap();
        std::fs::create_dir_all(data_root.join("unidades/u1/.nas_staging")).unwrap();
        std::fs::create_dir_all(state_dir.join("staging")).unwrap();
        std::fs::create_dir_all(&external).unwrap();

        std::fs::write(data_root.join("PUBLICO/sub/archivo.txt"), b"ok").unwrap();
        std::fs::write(data_root.join("unidades/u1/CONTABILIDAD/doc.txt"), b"ok").unwrap();
        std::fs::write(data_root.join("BACKUPS_HISTORICOS/snap1/f.txt"), b"bkp").unwrap();
        std::fs::write(data_root.join("LOGS_BACKUP/job.log"), b"log").unwrap();
        std::os::unix::fs::symlink(&external, data_root.join("PUBLICO/enlace_ext")).unwrap();

        std::fs::set_permissions(&data_root, std::fs::Permissions::from_mode(0o755)).unwrap();
        std::fs::set_permissions(
            data_root.join("unidades"),
            std::fs::Permissions::from_mode(0o2755),
        )
        .unwrap();
        std::fs::set_permissions(
            data_root.join("unidades/u1"),
            std::fs::Permissions::from_mode(0o2755),
        )
        .unwrap();
        std::fs::set_permissions(
            data_root.join(".nas_staging"),
            std::fs::Permissions::from_mode(0o755),
        )
        .unwrap();
        std::fs::set_permissions(
            data_root.join("unidades/u1/.nas_staging"),
            std::fs::Permissions::from_mode(0o755),
        )
        .unwrap();
        std::fs::set_permissions(
            state_dir.join("staging"),
            std::fs::Permissions::from_mode(0o2770),
        )
        .unwrap();
        std::fs::set_permissions(
            data_root.join("BACKUPS_HISTORICOS/snap1/f.txt"),
            std::fs::Permissions::from_mode(0o640),
        )
        .unwrap();
        std::fs::set_permissions(
            data_root.join("LOGS_BACKUP/job.log"),
            std::fs::Permissions::from_mode(0o640),
        )
        .unwrap();

        let smb_conf = base.join("smb.conf");
        std::fs::write(
            &smb_conf,
            format!(
                "[PUBLICO]\n   path = {}\n",
                data_root.join("PUBLICO").display()
            ),
        )
        .unwrap();

        let mut actions = Vec::new();
        let mut warnings = Vec::new();
        let restart = sync_data_root_permissions_with_principals(
            &runner,
            &data_root,
            Some(&state_dir),
            &smb_conf,
            &current_user,
            &current_group,
            None,
            false,
            &mut actions,
            &mut warnings,
        );
        assert!(!restart);
        assert!(warnings.is_empty(), "warnings inesperados: {warnings:?}");
        assert!(!actions.is_empty());

        for root_dir in [
            data_root.clone(),
            data_root.join("unidades"),
            data_root.join("unidades/u1"),
        ] {
            let mode = std::fs::metadata(&root_dir).unwrap().permissions().mode() & 0o7777;
            assert_eq!(mode, 0o2770, "modo 2770 esperado en {}", root_dir.display());
            assert!(dir_has_nas_acls(
                &runner,
                &root_dir,
                &current_user,
                &current_group,
                false
            ));
        }

        for staging in [
            data_root.join(".nas_staging"),
            data_root.join("unidades/u1/.nas_staging"),
            state_dir.join("staging"),
        ] {
            let mode = std::fs::metadata(&staging).unwrap().permissions().mode() & 0o7777;
            assert_eq!(mode, 0o700, "modo 0700 esperado en {}", staging.display());
        }

        for share in [
            data_root.join("PUBLICO"),
            data_root.join("PUBLICO/sub"),
            data_root.join("unidades/u1/CONTABILIDAD"),
        ] {
            assert!(
                dir_has_nas_acls(&runner, &share, &current_user, &current_group, false),
                "faltan ACLs en {}",
                share.display()
            );
        }

        let file_acl = runner
            .run(&CommandSpec::new("getfacl").args([
                "-p",
                &data_root.join("PUBLICO/sub/archivo.txt").to_string_lossy(),
            ]))
            .unwrap();
        assert!(acl_has_effective(
            &file_acl.stdout,
            &current_user,
            false,
            "rw",
            false
        ));

        let ext_acl = runner
            .run(&CommandSpec::new("getfacl").args(["-p", &external.to_string_lossy()]))
            .unwrap();
        assert!(!acl_has_effective(
            &ext_acl.stdout,
            &current_user,
            false,
            "rwx",
            false
        ));

        assert!(!dir_has_nas_acls(
            &runner,
            &data_root.join("BACKUPS_HISTORICOS"),
            &current_user,
            &current_group,
            false
        ));
        assert!(!dir_has_nas_acls(
            &runner,
            &data_root.join("LOGS_BACKUP"),
            &current_user,
            &current_group,
            false
        ));
        assert_eq!(
            std::fs::metadata(data_root.join("BACKUPS_HISTORICOS/snap1/f.txt"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o640
        );

        let mut actions2 = Vec::new();
        let mut warnings2 = Vec::new();
        sync_data_root_permissions_with_principals(
            &runner,
            &data_root,
            Some(&state_dir),
            &smb_conf,
            &current_user,
            &current_group,
            None,
            false,
            &mut actions2,
            &mut warnings2,
        );
        assert!(warnings2.is_empty(), "warnings2: {warnings2:?}");
        assert!(
            actions2.is_empty(),
            "segunda ejecución debe ser idempotente, obtuvo: {actions2:?}"
        );

        let _ = std::fs::remove_dir_all(&base);
    }

    #[cfg(unix)]
    #[test]
    fn ensure_active_share_acls_corrige_subcarpetas_y_archivos_aunque_raiz_tenga_acl() {
        if !has_acl_tools() {
            return;
        }
        let runner = Runner::new();
        let user_out = runner
            .run(&CommandSpec::new("id").args(["-un"]))
            .expect("id -un");
        let group_out = runner
            .run(&CommandSpec::new("id").args(["-gn"]))
            .expect("id -gn");
        if !user_out.is_success() || !group_out.is_success() {
            return;
        }
        let current_user = user_out.stdout.trim().to_string();
        let current_group = group_out.stdout.trim().to_string();

        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let base =
            std::env::temp_dir().join(format!("nas_sec_nested_{}_{}", std::process::id(), nanos));
        let data_root = base.join("srv_nas");
        let share = data_root.join("PUBLICO");
        let sub = share.join("sub");
        std::fs::create_dir_all(&sub).unwrap();

        let root_file = share.join("legacy.txt");
        let nested_file = sub.join("privado.txt");
        std::fs::write(&root_file, b"legacy").unwrap();
        std::fs::write(&nested_file, b"secret").unwrap();

        // Simula share cuya raíz ya tenía ACLs no recursivas, pero subcarpeta y archivos siguen con 0700/0600 sin ACL.
        std::fs::set_permissions(&data_root, std::fs::Permissions::from_mode(0o2770)).unwrap();
        let root_acl = format!(
            "u:{current_user}:rwx,g:{current_group}:rwx,m::rwx,d:u:{current_user}:rwx,d:g:{current_group}:rwx,d:m::rwx"
        );
        runner
            .run(&CommandSpec::new("setfacl").args([
                "-m",
                &root_acl,
                "--",
                &data_root.to_string_lossy(),
                &share.to_string_lossy(),
            ]))
            .unwrap()
            .ensure_success()
            .unwrap();
        std::fs::set_permissions(&sub, std::fs::Permissions::from_mode(0o700)).unwrap();
        std::fs::set_permissions(&root_file, std::fs::Permissions::from_mode(0o600)).unwrap();
        std::fs::set_permissions(&nested_file, std::fs::Permissions::from_mode(0o600)).unwrap();

        let smb_conf = base.join("smb.conf");
        std::fs::write(
            &smb_conf,
            format!("[PUBLICO]\n   path = {}\n", share.display()),
        )
        .unwrap();

        let mut actions = Vec::new();
        let mut warnings = Vec::new();
        sync_data_root_permissions_with_principals(
            &runner,
            &data_root,
            None,
            &smb_conf,
            &current_user,
            &current_group,
            None,
            false,
            &mut actions,
            &mut warnings,
        );
        assert!(warnings.is_empty(), "warnings: {warnings:?}");
        assert_eq!(
            std::fs::metadata(data_root.join(".nas_staging"))
                .unwrap()
                .permissions()
                .mode()
                & 0o7777,
            0o700,
            ".nas_staging creado bajo 2770 no debe heredar setgid"
        );
        assert!(
            actions.iter().any(|a| a.contains("PUBLICO")),
            "debe detectar y corregir subcarpetas/archivos sin ACL aunque la raíz ya tenga ACL: {actions:?}"
        );
        assert!(dir_has_nas_acls(
            &runner,
            &sub,
            &current_user,
            &current_group,
            false
        ));
        for file in [&root_file, &nested_file] {
            let out = runner
                .run(&CommandSpec::new("getfacl").args(["-p", "--", &file.to_string_lossy()]))
                .unwrap();
            assert!(acl_has_effective(
                &out.stdout,
                &current_user,
                false,
                "rw",
                false
            ));
            assert!(acl_has_effective(
                &out.stdout,
                &current_group,
                true,
                "rw",
                false
            ));
        }

        let mut actions2 = Vec::new();
        let mut warnings2 = Vec::new();
        sync_data_root_permissions_with_principals(
            &runner,
            &data_root,
            None,
            &smb_conf,
            &current_user,
            &current_group,
            None,
            false,
            &mut actions2,
            &mut warnings2,
        );
        assert!(warnings2.is_empty(), "warnings2: {warnings2:?}");
        assert!(
            actions2.is_empty(),
            "segunda pasada idempotente: {actions2:?}"
        );

        let _ = std::fs::remove_dir_all(&base);
    }

    #[cfg(unix)]
    #[test]
    fn sync_artifact_needed_detecta_recycle_purge_obsoleto_o_sin_permiso_ejecucion() {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let path =
            std::env::temp_dir().join(format!("nas_recycle_sync_{}_{}", std::process::id(), nanos));
        assert!(sync_artifact_needed(&path, 0o755, RECYCLE_PURGE_SCRIPT));

        std::fs::write(
            &path,
            "#!/bin/bash\nfind /srv/nas -type f -path '*/.recycle/*' -mtime +15 -delete\n",
        )
        .unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(sync_artifact_needed(&path, 0o755, RECYCLE_PURGE_SCRIPT));

        std::fs::write(&path, RECYCLE_PURGE_SCRIPT).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(sync_artifact_needed(&path, 0o755, RECYCLE_PURGE_SCRIPT));

        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(!sync_artifact_needed(&path, 0o755, RECYCLE_PURGE_SCRIPT));

        let _ = std::fs::remove_file(&path);
    }
}
