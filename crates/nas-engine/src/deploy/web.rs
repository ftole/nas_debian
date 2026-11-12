//! Despliegue del panel web (nas-web), Nginx, helpers root y sudoers.

use nas_core::{NasError, NasResult};
use nas_exec::{CommandSpec, Runner, SudoMode};

use crate::deploy::Artifact;

/// Unidad systemd del servicio nas-web.
pub const SYSTEMD_UNIT: &str = "[Unit]\nDescription=Panel web NAS (Rust/axum)\nAfter=network-online.target\nWants=network-online.target\n\n[Service]\nType=simple\nUser=nas-web\nGroup=nas-web\nSupplementaryGroups=grp_samba\nEnvironmentFile=/etc/nas-web.env\nExecStart=/usr/local/bin/nas-web\nRestart=on-failure\nRestartSec=2\nTimeoutStopSec=10\n\nProtectSystem=yes\nProtectHome=no\nPrivateTmp=yes\nReadWritePaths=/var/lib/nas /srv/nas /usr/local/bin\nUMask=0007\n\n[Install]\nWantedBy=multi-user.target\n";

/// Configuracion de Nginx.
pub const NGINX_CONF: &str = "# Host virtual del panel NAS (backend Rust en 127.0.0.1:8080)\nserver {\n    listen 80 default_server;\n    server_name _;\n    return 301 https://$host$request_uri;\n}\n\nserver {\n    listen 443 ssl default_server;\n    server_name _;\n\n    ssl_certificate /etc/ssl/certs/nas-web.crt;\n    ssl_certificate_key /etc/ssl/private/nas-web.key;\n    ssl_protocols TLSv1.2 TLSv1.3;\n    ssl_ciphers HIGH:!aNULL:!MD5;\n\n    add_header Strict-Transport-Security \"max-age=31536000; includeSubDomains\" always;\n\n    client_max_body_size 50G;\n    client_body_timeout 3600s;\n\n    location / {\n        proxy_pass http://127.0.0.1:8080;\n        proxy_http_version 1.1;\n        proxy_set_header Host $host;\n        proxy_set_header X-Real-IP $remote_addr;\n        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;\n        proxy_set_header X-Forwarded-Proto $scheme;\n        proxy_read_timeout 3600s;\n        proxy_send_timeout 3600s;\n        proxy_buffering off;\n        proxy_request_buffering off;\n    }\n}\n";

/// Reglas sudo acotadas.
pub const SUDOERS: &str = "nas-web ALL=(root) NOPASSWD: /usr/bin/systemctl restart nas-web, /usr/local/bin/nas helper *, /usr/local/sbin/nas-samba-apply /var/lib/nas/staging/smbconf_*, /usr/local/sbin/nas-storage-apply *, /usr/bin/true\n";

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
    let staged = std::env::temp_dir().join(format!("nas-web-artifact-{}", std::process::id()));
    std::fs::write(&staged, &artifact.content)
        .map_err(|e| NasError::Denied(format!("no se pudo preparar {}: {e}", staged.display())))?;
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
    let out = runner.run(&spec)?;
    let _ = std::fs::remove_file(&staged);
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

/// Aplica el despliegue web completo.
pub fn apply_web(runner: &Runner, sudo: bool) -> NasResult<()> {
    for artifact in artifacts() {
        write_artifact(runner, &artifact, sudo)?;
    }
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
}
