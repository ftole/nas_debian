//! Helpers para gestión de terminales interactivas aisladas sobre tmux.

use std::path::{Path, PathBuf};
use std::time::Duration;

use nas_core::{NasError, NasResult};
use nas_exec::{CommandSpec, Runner};

use crate::helpers::accounts::ensure_manageable_existing_user;
use crate::terminal::is_valid_geometry;

/// Valida y prepara el entorno de socket para el terminal del usuario.
fn prepare_terminal_socket(runner: &Runner, user: &str) -> NasResult<(u32, String, PathBuf)> {
    let uid = ensure_manageable_existing_user(runner, user)?;
    let group = runner
        .run(&CommandSpec::new("id").args(["-gn", user]))
        .map(|o| o.stdout.trim().to_string())
        .unwrap_or_else(|_| user.to_string());

    let sock_dir = PathBuf::from(format!("/run/nas-terminal/{uid}"));
    let _ = std::fs::create_dir_all("/run/nas-terminal");
    let _ = std::fs::create_dir_all(&sock_dir);

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ =
            std::fs::set_permissions("/run/nas-terminal", std::fs::Permissions::from_mode(0o755));
        let _ = std::fs::set_permissions(&sock_dir, std::fs::Permissions::from_mode(0o700));
    }
    let sock_dir_str = sock_dir.to_string_lossy().to_string();
    let _ = runner.run(&CommandSpec::new("chown").args([format!("{user}:{group}"), sock_dir_str]));

    let sock = sock_dir.join("tmux.sock");
    Ok((uid, group, sock))
}

fn resolve_home_dir(runner: &Runner, user: &str) -> String {
    if let Ok(out) = runner.run(&CommandSpec::new("getent").args(["passwd", user])) {
        if out.is_success() {
            let parts: Vec<&str> = out.stdout.trim().split(':').collect();
            if parts.len() >= 6 {
                let home = parts[5];
                if Path::new(home).is_dir() {
                    return home.to_string();
                }
            }
        }
    }
    if Path::new("/srv/nas").is_dir() {
        "/srv/nas".to_string()
    } else {
        "/".to_string()
    }
}

/// Inicia la sesión tmux para el usuario especificado.
pub fn start_terminal(
    runner: &Runner,
    user: &str,
    cols: Option<u16>,
    rows: Option<u16>,
) -> NasResult<String> {
    let (_, group, sock) = prepare_terminal_socket(runner, user)?;
    let cols = cols.unwrap_or(200);
    let rows = rows.unwrap_or(50);
    if !is_valid_geometry(cols, rows) {
        return Err(NasError::Invalid(
            "dimensiones de terminal inválidas".into(),
        ));
    }

    let session = format!("nas-web-{user}");
    let unit_name = format!("nas-pty-{user}");
    let home = resolve_home_dir(runner, user);

    // Comprobar si ya existe la sesión
    let check = runner.run(
        &CommandSpec::new("tmux")
            .args(["-S", &sock.to_string_lossy(), "has-session", "-t", &session])
            .timeout(Duration::from_secs(5)),
    );
    if check.map(|o| o.is_success()).unwrap_or(false) {
        return Ok(format!("session={session}"));
    }

    let has_systemd = Path::new("/run/systemd/private").exists();
    if has_systemd {
        let _ = runner.run(
            &CommandSpec::new("systemctl").args(["reset-failed", &format!("{unit_name}.service")]),
        );
        runner.run(
            &CommandSpec::new("systemd-run")
                .args([
                    "--quiet",
                    &format!("--unit={unit_name}"),
                    "--property=Type=forking",
                    &format!("--property=User={user}"),
                    &format!("--property=Group={group}"),
                    "--property=UMask=0022",
                    &format!("--property=WorkingDirectory={home}"),
                    &format!("--setenv=HOME={home}"),
                    &format!("--setenv=USER={user}"),
                    &format!("--setenv=LOGNAME={user}"),
                    "--setenv=SHELL=/bin/bash",
                    "--setenv=TERM=xterm-256color",
                    "--setenv=PAGER=cat",
                    "--setenv=SYSTEMD_PAGER=cat",
                    "--setenv=PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin",
                    "/usr/bin/tmux",
                    "-S",
                    &sock.to_string_lossy(),
                    "new-session",
                    "-d",
                    "-s",
                    &session,
                    "-x",
                    &cols.to_string(),
                    "-y",
                    &rows.to_string(),
                    "-c",
                    &home,
                    "/bin/bash",
                    "-l",
                ])
                .timeout(Duration::from_secs(15)),
        )?.ensure_success()?;
    } else {
        runner
            .run(
                &CommandSpec::new("runuser")
                    .args([
                        "-u",
                        user,
                        "--",
                        "env",
                        &format!("HOME={home}"),
                        &format!("USER={user}"),
                        &format!("LOGNAME={user}"),
                        "SHELL=/bin/bash",
                        "PAGER=cat",
                        "SYSTEMD_PAGER=cat",
                        "TERM=xterm-256color",
                        "tmux",
                        "-S",
                        &sock.to_string_lossy(),
                        "new-session",
                        "-d",
                        "-s",
                        &session,
                        "-x",
                        &cols.to_string(),
                        "-y",
                        &rows.to_string(),
                        "-c",
                        &home,
                        "/bin/bash",
                        "-l",
                    ])
                    .timeout(Duration::from_secs(15)),
            )?
            .ensure_success()?;
    }

    Ok(format!("session={session}"))
}

/// Envía teclas o texto a la sesión tmux del usuario.
pub fn send_terminal_keys(runner: &Runner, user: &str, keys: &str, enter: bool) -> NasResult<()> {
    let (_, _, sock) = prepare_terminal_socket(runner, user)?;
    let session = format!("nas-web-{user}");

    runner
        .run(
            &CommandSpec::new("runuser")
                .args([
                    "-u",
                    user,
                    "--",
                    "tmux",
                    "-S",
                    &sock.to_string_lossy(),
                    "send-keys",
                    "-t",
                    &session,
                    "-l",
                    "--",
                    keys,
                ])
                .timeout(Duration::from_secs(10)),
        )?
        .ensure_success()?;

    if enter {
        let _ = runner.run(&CommandSpec::new("runuser").args([
            "-u",
            user,
            "--",
            "tmux",
            "-S",
            &sock.to_string_lossy(),
            "send-keys",
            "-t",
            &session,
            "Enter",
        ]));
    }
    Ok(())
}

/// Envía una tecla especial (Enter, Tab, C-c, etc.) a la sesión tmux.
pub fn send_terminal_special_key(runner: &Runner, user: &str, key_name: &str) -> NasResult<()> {
    let (_, _, sock) = prepare_terminal_socket(runner, user)?;
    let session = format!("nas-web-{user}");

    let tmux_key = match key_name.trim() {
        "ENTER" | "Enter" => "Enter",
        "TAB" | "Tab" => "Tab",
        "CTRL_C" | "C-c" => "C-c",
        "CTRL_D" | "C-d" => "C-d",
        "CTRL_L" | "C-l" => "C-l",
        "ESC" | "Escape" => "Escape",
        "BSpace" | "BACKSPACE" => "BSpace",
        "Up" => "Up",
        "Down" => "Down",
        "Left" => "Left",
        "Right" => "Right",
        other => return Err(NasError::Invalid(format!("tecla no soportada: '{other}'"))),
    };

    runner
        .run(&CommandSpec::new("runuser").args([
            "-u",
            user,
            "--",
            "tmux",
            "-S",
            &sock.to_string_lossy(),
            "send-keys",
            "-t",
            &session,
            tmux_key,
        ]))?
        .ensure_success()?;
    Ok(())
}

/// Captura la pantalla del terminal y la ruta de trabajo actual.
pub fn capture_terminal(runner: &Runner, user: &str) -> NasResult<String> {
    let (_, _, sock) = prepare_terminal_socket(runner, user)?;
    let session = format!("nas-web-{user}");

    let cap_out = runner.run(
        &CommandSpec::new("runuser")
            .args([
                "-u",
                user,
                "--",
                "tmux",
                "-S",
                &sock.to_string_lossy(),
                "capture-pane",
                "-e",
                "-t",
                &session,
                "-p",
            ])
            .timeout(Duration::from_secs(10)),
    )?;

    let cwd = runner
        .run(&CommandSpec::new("runuser").args([
            "-u",
            user,
            "--",
            "tmux",
            "-S",
            &sock.to_string_lossy(),
            "display-message",
            "-p",
            "-F",
            "#{pane_current_path}",
            "-t",
            &session,
        ]))
        .map(|o| o.stdout.trim().to_string())
        .unwrap_or_else(|_| "/srv/nas".to_string());

    let cwd = if cwd.is_empty() {
        "/srv/nas".to_string()
    } else {
        cwd
    };
    Ok(format!("{}\n___NAS_CWD___{cwd}", cap_out.stdout))
}

/// Redimensiona la ventana del terminal.
pub fn resize_terminal(runner: &Runner, user: &str, cols: u16, rows: u16) -> NasResult<()> {
    if !is_valid_geometry(cols, rows) {
        return Err(NasError::Invalid(
            "dimensiones de terminal inválidas".into(),
        ));
    }
    let (_, _, sock) = prepare_terminal_socket(runner, user)?;
    let session = format!("nas-web-{user}");

    let _ = runner.run(&CommandSpec::new("runuser").args([
        "-u",
        user,
        "--",
        "tmux",
        "-S",
        &sock.to_string_lossy(),
        "resize-window",
        "-t",
        &session,
        "-x",
        &cols.to_string(),
        "-y",
        &rows.to_string(),
    ]));
    Ok(())
}

/// Termina la sesión del terminal del usuario.
pub fn kill_terminal(runner: &Runner, user: &str) -> NasResult<()> {
    let (_, _, sock) = prepare_terminal_socket(runner, user)?;
    let session = format!("nas-web-{user}");
    let unit_name = format!("nas-pty-{user}");

    let _ = runner.run(&CommandSpec::new("runuser").args([
        "-u",
        user,
        "--",
        "tmux",
        "-S",
        &sock.to_string_lossy(),
        "kill-session",
        "-t",
        &session,
    ]));

    let _ =
        runner.run(&CommandSpec::new("systemctl").args(["stop", &format!("{unit_name}.service")]));
    let _ = runner.run(
        &CommandSpec::new("systemctl").args(["reset-failed", &format!("{unit_name}.service")]),
    );

    Ok(())
}

/// Comprueba si existe la sesión tmux para el usuario.
pub fn terminal_exists(runner: &Runner, user: &str) -> NasResult<bool> {
    let (_, _, sock) = prepare_terminal_socket(runner, user)?;
    let session = format!("nas-web-{user}");
    let out = runner.run(
        &CommandSpec::new("runuser")
            .args([
                "-u",
                user,
                "--",
                "tmux",
                "-S",
                &sock.to_string_lossy(),
                "has-session",
                "-t",
                &session,
            ])
            .timeout(Duration::from_secs(5)),
    )?;
    Ok(out.is_success())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validacion_de_geometria() {
        assert!(is_valid_geometry(80, 24));
        assert!(is_valid_geometry(200, 50));
        assert!(!is_valid_geometry(5, 5));
        assert!(!is_valid_geometry(1000, 24));
    }
}
