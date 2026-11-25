//! Despliegue del panel web (nas-web), Nginx, helpers root y sudoers.

use std::path::Path;

use nas_core::{NasError, NasResult, ResourceBudget};
use nas_exec::{CommandSpec, Runner, SudoMode};

use crate::deploy::Artifact;

/// Unidad systemd del servicio nas-web (fuente única en `packaging/nas-web.service`).
pub const SYSTEMD_UNIT: &str = include_str!("../../../../packaging/nas-web.service");

/// Configuracion de Nginx (fuente única en `packaging/nginx-nas-web.conf`).
pub const NGINX_CONF: &str = include_str!("../../../../packaging/nginx-nas-web.conf");

/// Reglas sudo acotadas (paridad exacta con `packaging/install.sh`).
pub const SUDOERS: &str = "\
nas-web ALL=(root) NOPASSWD: /usr/bin/systemctl start smbd, /usr/bin/systemctl stop smbd, \\
    /usr/bin/systemctl restart smbd, /usr/bin/systemctl reload smbd, \\
    /usr/bin/systemctl start nmbd, /usr/bin/systemctl stop nmbd, \\
    /usr/bin/systemctl restart nmbd, /usr/bin/systemctl reload nmbd, \\
    /usr/bin/systemctl start wsdd2, /usr/bin/systemctl stop wsdd2, \\
    /usr/bin/systemctl restart wsdd2, /usr/bin/systemctl reload wsdd2, \\
    /usr/bin/systemctl start nginx, /usr/bin/systemctl stop nginx, \\
    /usr/bin/systemctl restart nginx, /usr/bin/systemctl reload nginx, \\
    /usr/bin/systemctl start nas-web, /usr/bin/systemctl stop nas-web, \\
    /usr/bin/systemctl restart nas-web, /usr/bin/systemctl reload nas-web, \\
    /usr/bin/systemctl enable sssd, /usr/bin/systemctl disable sssd, /usr/bin/systemctl start sssd, /usr/bin/systemctl stop sssd, /usr/bin/systemctl restart sssd, /usr/bin/systemctl reboot, \\
    /usr/bin/systemctl start cron, /usr/bin/systemctl stop cron, \\
    /usr/bin/systemctl restart cron, /usr/bin/systemctl reload cron, \\
    /usr/bin/systemd-run --no-block --collect systemctl reboot, \\
    /usr/bin/systemd-run --no-block --collect --unit=nas-system-update /usr/local/bin/nas --json update apply --repo ftole/nas_debian --root /opt/nas_debian --yes, \\
    /usr/bin/systemd-run --no-block --collect --unit=nas-system-rollback /usr/local/bin/nas --json update rollback --root /opt/nas_debian --yes --version *, \\
    /usr/bin/smbstatus -b, /usr/bin/testparm -s, \\
    /usr/sbin/realm list, /usr/sbin/realm discover *, /usr/sbin/realm join *, /usr/sbin/realm leave, /usr/sbin/realm leave --remove --user *, \\
    /usr/local/bin/nas --json security status, \\
    /usr/bin/btrfs scrub status /srv/nas, /usr/bin/btrfs scrub start /srv/nas, /usr/bin/btrfs subvolume list /srv/nas, \\
    /usr/bin/fstrim -v /srv/nas, \\
    /usr/local/bin/nas helper *, \\
    /usr/local/sbin/nas-samba-apply /var/lib/nas/staging/smbconf_*, \\
    /usr/local/sbin/nas-share-dir *, /usr/local/sbin/nas-share-acl *, \\
    /usr/local/sbin/nas-accounts *, /usr/bin/pdbedit -L -s, /usr/bin/passwd -S *, \\
    /usr/local/sbin/nas-backup-manage *, /usr/local/sbin/nas-terminal *, \\
    /usr/local/sbin/nas-storage-apply *, \\
    /usr/bin/true
";

/// Genera el contenido de /etc/nas-web.env.
pub fn env_file_content(
    bind: &str,
    admin_user: &str,
    secret: &str,
    password_hash: &str,
    state_dir: &str,
    data_root: &str,
    repo_dir: &str,
) -> String {
    format!(
        "NAS_WEB_BIND={bind}\nNAS_WEB_USER={admin_user}\nNAS_WEB_SECRET={secret}\nNAS_WEB_PASSWORD_SHA256={password_hash}\nNAS_WEB_TTL_SECS=28800\nNAS_REPO_DIR={repo_dir}\nNAS_STATE_DIR={state_dir}\nNAS_DATA_ROOT={data_root}\nNAS_SAMBA_CONF=/etc/samba/smb.conf\nNAS_ROLE_FILE=/etc/nas/role\nNAS_BACKUP_DIR=/usr/local/bin\nNAS_INVENTORY=/etc/nas/inventory.yaml\n"
    )
}

/// Ruta del drop-in dinámico de cuotas systemd para `nas-web`.
pub const SYSTEMD_RESOURCES_DROPIN_PATH: &str =
    "/etc/systemd/system/nas-web.service.d/10-resources.conf";

fn valid_dropin_data_root(path: &str) -> bool {
    let trimmed = path.trim();
    trimmed.starts_with('/')
        && trimmed.len() <= 256
        && !trimmed.contains("..")
        && !trimmed.chars().any(|c| c.is_control() || c.is_whitespace())
}

/// Obtiene `NAS_DATA_ROOT` desde variable de entorno, `/etc/nas-web.env` o `/srv/nas`.
pub fn resolve_data_root(env_file: &Path) -> String {
    if let Ok(val) = std::env::var("NAS_DATA_ROOT") {
        let trimmed = val.trim();
        if valid_dropin_data_root(trimmed) {
            return trimmed.to_string();
        }
    }
    if let Ok(raw) = std::fs::read_to_string(env_file) {
        for line in raw.lines() {
            let trimmed = line.trim();
            if let Some(rest) = trimmed.strip_prefix("NAS_DATA_ROOT=") {
                let unquoted = rest.trim_matches(|c| c == '"' || c == '\'' || c == ' ');
                if valid_dropin_data_root(unquoted) {
                    return unquoted.to_string();
                }
            }
        }
    }
    "/srv/nas".to_string()
}

/// Genera el drop-in dinámico `/etc/systemd/system/nas-web.service.d/10-resources.conf`
/// ajustado a la RAM, CPUs efectivas y `NAS_DATA_ROOT` del servidor.
pub fn systemd_resource_dropin(budget: &ResourceBudget, data_root: &str) -> String {
    let (mem_high_mb, mem_max_mb, cpu_quota_pct, tasks_max) = budget.systemd_resource_limits();
    let mut out = format!(
        "# Generado automáticamente según hardware y entorno (no editar manualmente)\n\
         [Service]\n\
         MemoryHigh={mem_high_mb}M\n\
         MemoryMax={mem_max_mb}M\n\
         CPUQuota={cpu_quota_pct}%\n\
         TasksMax={tasks_max}\n"
    );
    let clean_root = data_root.trim().trim_end_matches('/');
    if !clean_root.is_empty() && clean_root != "/srv/nas" && valid_dropin_data_root(clean_root) {
        out.push_str(&format!("ReadWritePaths={clean_root}\n"));
    }
    out
}

/// Artefacto del drop-in dinámico de recursos systemd para `nas-web`.
pub fn resource_dropin_artifact(budget: &ResourceBudget, data_root: &str) -> Artifact {
    Artifact {
        path: SYSTEMD_RESOURCES_DROPIN_PATH.into(),
        mode: 0o644,
        content: systemd_resource_dropin(budget, data_root),
    }
}

/// Conjunto de artefactos estaticos del despliegue web.
pub fn artifacts() -> Vec<Artifact> {
    vec![
        Artifact {
            path: "/etc/systemd/system/nas-web.service".into(),
            mode: 0o644,
            content: SYSTEMD_UNIT.to_string(),
        },
        Artifact {
            path: "/etc/nginx/sites-available/nas-web".into(),
            mode: 0o644,
            content: NGINX_CONF.to_string(),
        },
        Artifact {
            path: "/etc/sudoers.d/nas-web-rust".into(),
            mode: 0o440,
            content: SUDOERS.to_string(),
        },
    ]
}

/// Escribe un artefacto usando install(1).
pub fn write_artifact(runner: &Runner, artifact: &Artifact, sudo: bool) -> NasResult<()> {
    let staged = super::write_unique_temp_file("nas-web-artifact", &artifact.content)?;
    let mode_str = format!("{:04o}", artifact.mode);
    let mut spec = CommandSpec::new("install")
        .args([
            "-D",
            "-m",
            &mode_str,
            "-o",
            "root",
            "-g",
            "root",
            &staged.to_string_lossy(),
            &artifact.path,
        ])
        .timeout(std::time::Duration::from_secs(30));
    if sudo {
        spec = spec.sudo(SudoMode::Passwordless);
    }
    let out = runner.run(&spec);
    let _ = std::fs::remove_file(&staged);
    let out = out?;
    if !out.is_success() {
        return Err(NasError::Denied(format!(
            "no se pudo instalar {}: {}",
            artifact.path, out.stderr
        )));
    }
    Ok(())
}

/// Valida el archivo de sudoers con visudo -cf.
pub fn validate_sudoers(runner: &Runner, path: &str, sudo: bool) -> NasResult<()> {
    let mut spec = CommandSpec::new("visudo")
        .args(["-cf", path])
        .timeout(std::time::Duration::from_secs(30));
    if sudo {
        spec = spec.sudo(SudoMode::Passwordless);
    }
    let out = runner.run(&spec)?;
    if !out.is_success() {
        return Err(NasError::Denied(format!(
            "sudoers invalido ({}): {}",
            path, out.stderr
        )));
    }
    Ok(())
}

/// Recarga systemd para tomar la nueva unidad.
pub fn daemon_reload(runner: &Runner, sudo: bool) -> NasResult<()> {
    let mut spec = CommandSpec::new("systemctl")
        .args(["daemon-reload"])
        .timeout(std::time::Duration::from_secs(30));
    if sudo {
        spec = spec.sudo(SudoMode::Passwordless);
    }
    let _ = runner.run(&spec)?;
    Ok(())
}

/// Aplica el drop-in dinámico de recursos systemd para `nas-web` y recarga systemd.
pub fn apply_resources_dropin(runner: &Runner, env_file: &Path, sudo: bool) -> NasResult<()> {
    let budget = ResourceBudget::detect();
    let data_root = resolve_data_root(env_file);
    let dropin = resource_dropin_artifact(&budget, &data_root);
    write_artifact(runner, &dropin, sudo)?;
    daemon_reload(runner, sudo)?;
    Ok(())
}

/// Aplica el despliegue web completo (incluyendo drop-in dinámico de recursos systemd).
pub fn apply_web(runner: &Runner, sudo: bool) -> NasResult<()> {
    for artifact in artifacts() {
        write_artifact(runner, &artifact, sudo)?;
    }
    let budget = ResourceBudget::detect();
    let data_root = resolve_data_root(Path::new("/etc/nas-web.env"));
    write_artifact(runner, &resource_dropin_artifact(&budget, &data_root), sudo)?;
    validate_sudoers(runner, "/etc/sudoers.d/nas-web-rust", sudo)?;
    daemon_reload(runner, sudo)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn artefactos_web_incluyen_rutas_esperadas() {
        let binding = artifacts();
        let paths: Vec<&str> = binding.iter().map(|a| a.path.as_str()).collect();
        assert!(paths.contains(&"/etc/systemd/system/nas-web.service"));
        assert!(paths.contains(&"/etc/nginx/sites-available/nas-web"));
        assert!(paths.contains(&"/etc/sudoers.d/nas-web-rust"));
    }

    #[test]
    fn nginx_tiene_proxy_pass_y_50g() {
        assert!(NGINX_CONF.contains("proxy_pass http://127.0.0.1:8080"));
        assert!(NGINX_CONF.contains("client_max_body_size 50G"));
    }

    #[test]
    fn systemd_usa_env_file_y_usuario() {
        assert!(SYSTEMD_UNIT.contains("EnvironmentFile=/etc/nas-web.env"));
        assert!(SYSTEMD_UNIT.contains("User=nas-web"));
    }

    #[test]
    fn env_file_contiene_bind_y_secret() {
        let content = env_file_content(
            "127.0.0.1:8080",
            "admin",
            "secret",
            "hash",
            "/var/lib/nas",
            "/srv/nas",
            "/opt/nas_debian",
        );
        assert!(content.contains("NAS_WEB_BIND=127.0.0.1:8080"));
        assert!(content.contains("NAS_WEB_SECRET=secret"));
    }

    #[test]
    fn sudoers_contiene_helper_y_true() {
        assert!(SUDOERS.contains("nas helper *"));
        assert!(SUDOERS.contains("/usr/bin/true"));
    }

    #[test]
    fn paridad_con_plantillas_packaging() {
        assert_eq!(
            SYSTEMD_UNIT,
            include_str!("../../../../packaging/nas-web.service")
        );
        assert!(SYSTEMD_UNIT.contains("NoNewPrivileges=no"));
        assert!(SYSTEMD_UNIT.contains("-/etc/cron.d -/etc/backup-credentials -/etc/samba"));
        assert_eq!(
            NGINX_CONF,
            include_str!("../../../../packaging/nginx-nas-web.conf")
        );
        assert!(NGINX_CONF.contains("gzip on;"));

        let install_sh = include_str!("../../../../packaging/install.sh");
        let marker = "cat > \"$SUDOERS_FILE\" <<'SUDO'\n";
        let start = install_sh
            .find(marker)
            .map(|idx| idx + marker.len())
            .expect("bloque SUDO en packaging/install.sh");
        let end = install_sh[start..]
            .find("\nSUDO\n")
            .map(|rel| start + rel + 1)
            .expect("cierre SUDO en packaging/install.sh");
        assert_eq!(SUDOERS, &install_sh[start..end]);
    }

    #[test]
    fn dropin_dinamico_escala_con_hardware_y_data_root_personalizado() {
        let small =
            nas_core::SystemResources::from_snapshot(1, None, 1024, 768, None, 0.2, 0.0, 0.0, 0.0);
        let small_budget = ResourceBudget::from_resources(small);
        let small_conf = systemd_resource_dropin(&small_budget, "/srv/nas");
        assert!(small_conf.contains("MemoryHigh=384M"));
        assert!(small_conf.contains("MemoryMax=768M"));
        assert!(small_conf.contains("CPUQuota=100%"));
        assert!(small_conf.contains("TasksMax=256"));
        assert!(!small_conf.contains("ReadWritePaths="));

        let big = nas_core::SystemResources::from_snapshot(
            16, None, 32768, 24576, None, 0.5, 0.0, 0.0, 0.0,
        );
        let big_budget = ResourceBudget::from_resources(big);
        let big_conf = systemd_resource_dropin(&big_budget, "/mnt/almacenamiento");
        assert!(big_conf.contains("MemoryHigh=4096M"));
        assert!(big_conf.contains("MemoryMax=8192M"));
        assert!(big_conf.contains("CPUQuota=800%"));
        assert!(big_conf.contains("TasksMax=1024"));
        assert!(big_conf.contains("ReadWritePaths=/mnt/almacenamiento"));
    }
}
