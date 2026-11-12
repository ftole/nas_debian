//! Pruebas de conectividad CIFS y SSH para tareas de respaldo.

use std::path::Path;
use std::time::Duration;

use nas_core::{NasError, NasResult};
use nas_exec::{CommandSpec, Runner};

use super::write_secure_file;

fn is_valid_host(host: &str) -> bool {
    let host = host.trim();
    !host.is_empty()
        && host.len() <= 253
        && !host.starts_with('-')
        && !host.contains("..")
        && host
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | ':' | '-'))
}

fn is_valid_port(port: u16) -> bool {
    port >= 1
}

fn is_valid_ssh_user(user: &str) -> bool {
    let user = user.trim();
    !user.is_empty()
        && user.len() <= 64
        && !user.starts_with('-')
        && user
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
}

fn is_valid_cifs_user(user: &str) -> bool {
    let user = user.trim();
    !user.is_empty()
        && user.len() <= 64
        && !user.starts_with('-')
        && user
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '\\' | '/' | '@' | '-'))
}

/// Prueba de conectividad CIFS.
pub fn test_cifs_conn(
    runner: &Runner,
    host: &str,
    share: &str,
    user: &str,
    password: &str,
) -> NasResult<String> {
    if !is_valid_host(host) {
        return Err(NasError::Invalid("host inválido".into()));
    }
    if share.is_empty() || share.contains('/') || share.contains('\\') {
        return Err(NasError::Invalid("nombre de recurso CIFS inválido".into()));
    }
    if !is_valid_cifs_user(user) {
        return Err(NasError::Invalid("usuario CIFS inválido".into()));
    }

    let stamp = chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0);
    let temp_dir = std::env::temp_dir().join(format!("cifs_test_{}_{stamp}", std::process::id()));
    let temp_cred = std::env::temp_dir().join(format!("cifs_cred_{}_{stamp}", std::process::id()));
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        let mut builder = std::fs::DirBuilder::new();
        builder.recursive(true);
        builder.mode(0o700);
        let _ = builder.create(&temp_dir);
    }
    #[cfg(not(unix))]
    let _ = std::fs::create_dir_all(&temp_dir);

    let cred_content = if let Some((dom, acc)) = user.split_once('\\') {
        format!("domain={dom}\nusername={acc}\npassword={password}\n")
    } else if let Some((dom, acc)) = user.split_once('/') {
        format!("domain={dom}\nusername={acc}\npassword={password}\n")
    } else {
        format!("username={user}\npassword={password}\n")
    };
    write_secure_file(&temp_cred, &cred_content)?;

    let opts = format!(
        "credentials={},ro,iocharset=utf8,vers=3.1.1,noserverino,cache=none,soft,nobrl",
        temp_cred.display()
    );
    let target = format!("//{host}/{share}");
    let temp_dir_str = temp_dir.to_string_lossy().to_string();
    let mount_res = runner.run(
        &CommandSpec::new("mount")
            .args(["-t", "cifs", &target, &temp_dir_str, "-o", &opts])
            .timeout(Duration::from_secs(15)),
    );

    let _ = runner.run(&CommandSpec::new("umount").args([&temp_dir_str]));
    let _ = std::fs::remove_file(&temp_cred);
    let _ = std::fs::remove_dir(&temp_dir);

    match mount_res {
        Ok(out) if out.is_success() => Ok(format!("conexión CIFS verificada: {target}")),
        Ok(out) => Err(NasError::Denied(clean_cifs_mount_error(&out.stderr))),
        Err(e) => Err(NasError::Denied(format!("error al ejecutar mount: {e}"))),
    }
}

pub(super) fn clean_cifs_mount_error(stderr: &str) -> String {
    let clean = if let Some((head, _)) = stderr.split_once("\nRefer to the mount.cifs") {
        head.trim()
    } else if let Some((head, _)) = stderr.split_once("Refer to the mount.cifs") {
        head.trim()
    } else {
        stderr.trim()
    };

    if clean.contains("mount error(13): Permission denied") {
        "falló conexión CIFS: Acceso denegado (usuario o contraseña incorrectos, o permisos insuficientes en el recurso compartido)".to_string()
    } else if clean.contains("mount error(112): Host is down")
        || clean.contains("mount error(113): No route to host")
    {
        "falló conexión CIFS: Host no disponible (servidor remoto apagado o inaccesible en la red)"
            .to_string()
    } else if clean.contains("mount error(111): Connection refused") {
        "falló conexión CIFS: Conexión rechazada (puerto CIFS/SMB 445 cerrado o inaccesible)"
            .to_string()
    } else if clean.contains("mount error(6): No such device or address")
        || clean.contains("mount error(2): No such file or directory")
    {
        "falló conexión CIFS: Recurso compartido no encontrado en el servidor remoto".to_string()
    } else if clean.contains("mount error(115): Operation now in progress") {
        "falló conexión CIFS: Tiempo de espera agotado al conectar al servidor remoto".to_string()
    } else {
        format!("falló conexión CIFS: {clean}")
    }
}

/// Prueba de conectividad SSH con contraseña o clave privada SSH (Ed25519/RSA).
pub fn test_ssh_conn(
    runner: &Runner,
    host: &str,
    port: u16,
    user: &str,
    auth: &str,
) -> NasResult<String> {
    if !is_valid_host(host) {
        return Err(NasError::Invalid("host inválido".into()));
    }
    if !is_valid_port(port) {
        return Err(NasError::Invalid("puerto SSH inválido".into()));
    }
    if !is_valid_ssh_user(user) {
        return Err(NasError::Invalid("usuario SSH inválido".into()));
    }

    let _ = std::fs::create_dir_all("/root/.ssh");
    let trimmed_auth = auth.trim();

    let (spec, temp_guard) = if trimmed_auth.starts_with("-----BEGIN") {
        nas_ssh::validate_private_key(trimmed_auth)?;
        let stamp = chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0);
        let temp_key = std::env::temp_dir().join(format!("ssh_key_{}_{stamp}", std::process::id()));
        let mut key_content = trimmed_auth.to_string();
        if !key_content.ends_with('\n') {
            key_content.push('\n');
        }
        write_secure_file(&temp_key, &key_content)?;

        let cmd = CommandSpec::new("ssh")
            .args([
                "-i",
                &temp_key.to_string_lossy(),
                "-p",
                &port.to_string(),
                "-l",
                user,
                "-o",
                "StrictHostKeyChecking=accept-new",
                "-o",
                "UserKnownHostsFile=/root/.ssh/known_hosts_backup",
                "-o",
                "ConnectTimeout=10",
                "-o",
                "BatchMode=yes",
                "--",
                host,
                "true",
            ])
            .timeout(Duration::from_secs(15));
        (cmd, Some(temp_key))
    } else if !trimmed_auth.is_empty() && Path::new(trimmed_auth).is_file() {
        let cmd = CommandSpec::new("ssh")
            .args([
                "-i",
                trimmed_auth,
                "-p",
                &port.to_string(),
                "-l",
                user,
                "-o",
                "StrictHostKeyChecking=accept-new",
                "-o",
                "UserKnownHostsFile=/root/.ssh/known_hosts_backup",
                "-o",
                "ConnectTimeout=10",
                "-o",
                "BatchMode=yes",
                "--",
                host,
                "true",
            ])
            .timeout(Duration::from_secs(15));
        (cmd, None)
    } else {
        let mut spec = CommandSpec::new("sshpass")
            .args([
                "-e",
                "ssh",
                "-p",
                &port.to_string(),
                "-l",
                user,
                "-o",
                "StrictHostKeyChecking=accept-new",
                "-o",
                "UserKnownHostsFile=/root/.ssh/known_hosts_backup",
                "-o",
                "ConnectTimeout=10",
                "--",
                host,
                "true",
            ])
            .timeout(Duration::from_secs(15));
        spec = spec.env("SSHPASS", auth);
        (spec, None)
    };

    let res = runner.run(&spec);
    if let Some(path) = temp_guard {
        let _ = std::fs::remove_file(path);
    }
    let res = res?;
    if res.is_success() {
        Ok(format!("conexión SSH verificada: {user}@{host}:{port}"))
    } else {
        Err(NasError::Denied(format!(
            "falló conexión SSH: {}",
            res.stderr
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clean_cifs_mount_error_humaniza_y_limpia() {
        let raw = "mount error(13): Permission denied\nRefer to the mount.cifs(8) manual page (e.g. man mount.cifs) and kernel log messages (dmesg)";
        let msg = clean_cifs_mount_error(raw);
        assert!(msg.contains("Acceso denegado"));
        assert!(!msg.contains("Refer to the mount.cifs"));

        let host_down = "mount error(112): Host is down\nRefer to the mount.cifs...";
        let msg2 = clean_cifs_mount_error(host_down);
        assert!(msg2.contains("Host no disponible"));

        let unknown = "mount error(999): Unknown error\nRefer to the mount.cifs...";
        let msg3 = clean_cifs_mount_error(unknown);
        assert_eq!(msg3, "falló conexión CIFS: mount error(999): Unknown error");
    }

    #[test]
    fn test_ssh_conn_valida_entradas_y_clave_pem() {
        let runner = Runner::new();
        assert!(test_ssh_conn(&runner, "-invalido", 22, "root", "pwd").is_err());
        assert!(test_ssh_conn(&runner, "10.10.1.1", 22, "-user", "pwd").is_err());
        let bad_pem =
            "-----BEGIN OPENSSH PRIVATE KEY-----\ncorto\n-----END OPENSSH PRIVATE KEY-----";
        assert!(test_ssh_conn(&runner, "10.10.1.1", 22, "root", bad_pem).is_err());
    }
}
