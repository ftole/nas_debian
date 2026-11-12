use axum::{
    extract::State,
    http::HeaderMap,
    response::{IntoResponse, Response},
    Router,
};
use nas_exec::Runner;
use serde::Deserialize;

use crate::{
    extractors::{AuthenticatedUser, SuperAdminUser},
    routes::common::{audit_ip, bad_request, server_error, ApiResponse},
    AppState,
};

/// Cuerpo para iniciar la sesión de terminal.
#[derive(Debug, Deserialize)]
#[serde(default)]
pub struct TerminalSessionBody {
    pub cols: u16,
    pub rows: u16,
}

impl Default for TerminalSessionBody {
    fn default() -> Self {
        Self {
            cols: 200,
            rows: 50,
        }
    }
}

/// Cuerpo para enviar texto o teclas especiales al terminal.
#[derive(Debug, Deserialize)]
#[serde(default)]
pub struct TerminalSendBody {
    pub data: String,
    pub enter: bool,
    pub raw: bool,
}

impl Default for TerminalSendBody {
    fn default() -> Self {
        Self {
            data: String::new(),
            enter: true,
            raw: false,
        }
    }
}

/// Cuerpo para redimensionar el terminal.
#[derive(Debug, Deserialize)]
pub struct TerminalResizeBody {
    pub cols: u16,
    pub rows: u16,
}

fn is_interactive_posix_user(user: &nas_engine::users::UserEntry) -> bool {
    !user.system
        && !nas_engine::users::is_protected_system_account(&user.name, Some(user.uid))
        && (1000..60_000).contains(&user.uid)
        && !user.shell.ends_with("nologin")
        && !user.shell.ends_with("/false")
}

/// Resuelve la cuenta POSIX no-root bajo la cual correrá `tmux` para el superadministrador.
fn resolve_terminal_posix_user(web_user: &str) -> Result<String, String> {
    let (users, groups) = nas_engine::users::load_local();
    if let Some(found) = users
        .iter()
        .find(|u| u.name == web_user && is_interactive_posix_user(u))
    {
        return Ok(found.name.clone());
    }

    if let Ok(designated) = std::fs::read_to_string("/etc/nas/superadmin") {
        let designated = designated.trim();
        if let Some(found) = users
            .iter()
            .find(|u| u.name == designated && is_interactive_posix_user(u))
        {
            return Ok(found.name.clone());
        }
    }

    for group_name in ["grp_superadmin", "sudo"] {
        if let Some(group) = groups.iter().find(|g| g.name == group_name) {
            for member in &group.members {
                if let Some(found) = users
                    .iter()
                    .find(|u| u.name == *member && is_interactive_posix_user(u))
                {
                    return Ok(found.name.clone());
                }
            }
        }
    }

    Err(
        "no existe una cuenta POSIX interactiva de superadministrador (verifique /etc/nas/superadmin)"
            .to_string(),
    )
}

/// Inicia (o reutiliza) la sesión de terminal del usuario.
pub async fn terminal_session(
    State(state): State<AppState>,
    superadmin: SuperAdminUser,
    headers: HeaderMap,
    axum::Json(body): axum::Json<TerminalSessionBody>,
) -> Response {
    if !nas_engine::terminal::is_valid_geometry(body.cols, body.rows) {
        return bad_request("geometría del terminal fuera de rango");
    }

    let user = superadmin.user().username().to_string();
    let web_user = user.clone();
    let cols = body.cols;
    let rows = body.rows;
    let outcome = tokio::task::spawn_blocking(move || {
        let posix_user = resolve_terminal_posix_user(&web_user)?;
        nas_engine::terminal::start(&Runner::new(), &posix_user, cols, rows)
    })
    .await
    .unwrap_or_else(|_| Err("apertura de terminal interrumpida".to_string()));

    match outcome {
        Ok(session) => {
            audit_ip(
                &state,
                &headers,
                &user,
                "terminal_start",
                &session.name,
                "SUCCESS",
                "",
            );
            axum::Json(ApiResponse::ok(session, "Sesión de terminal lista")).into_response()
        }
        Err(error) => {
            audit_ip(
                &state,
                &headers,
                &user,
                "terminal_start",
                "terminal",
                "FAILED",
                &error,
            );
            server_error(&error)
        }
    }
}

/// Envía texto o teclas especiales a la sesión del usuario.
pub async fn terminal_send(
    superadmin: SuperAdminUser,
    axum::Json(body): axum::Json<TerminalSendBody>,
) -> Response {
    if !nas_engine::terminal::is_valid_input(&body.data) {
        return bad_request("entrada inválida para el terminal");
    }

    let web_user = superadmin.user().username().to_string();
    let data = body.data;
    let enter = body.enter && !body.raw;
    let raw = body.raw;
    let outcome = tokio::task::spawn_blocking(move || {
        let posix_user = resolve_terminal_posix_user(&web_user)?;
        let name = nas_engine::terminal::session_name(&posix_user);
        let runner = Runner::new();
        if !raw && nas_engine::terminal::is_allowed_key(&data) {
            nas_engine::terminal::send_key(&runner, &posix_user, &name, &data)
        } else {
            nas_engine::terminal::send(&runner, &posix_user, &name, &data, enter)
        }
    })
    .await
    .unwrap_or_else(|_| Err("envío interrumpido".to_string()));

    match outcome {
        Ok(()) => axum::Json(ApiResponse::<serde_json::Value>::ok_empty(
            "Entrada enviada",
        ))
        .into_response(),
        Err(error) => server_error(&error),
    }
}

/// Captura el panel visible del terminal.
pub async fn terminal_capture(superadmin: SuperAdminUser) -> Response {
    let web_user = superadmin.user().username().to_string();
    let outcome = tokio::task::spawn_blocking(move || {
        let posix_user = resolve_terminal_posix_user(&web_user)?;
        let name = nas_engine::terminal::session_name(&posix_user);
        let (output, cwd) =
            nas_engine::terminal::capture_details(&Runner::new(), &posix_user, &name)?;
        Ok::<_, String>((output, cwd, posix_user, name))
    })
    .await
    .unwrap_or_else(|_| Err("captura interrumpida".to_string()));

    match outcome {
        Ok((output, cwd, posix_user, name)) => axum::Json(ApiResponse::ok(
            serde_json::json!({
                "output": output,
                "cwd": cwd,
                "user": posix_user,
                "session": name,
            }),
            "Captura del terminal",
        ))
        .into_response(),
        Err(error) => server_error(&error),
    }
}

/// Redimensiona el terminal.
pub async fn terminal_resize(
    superadmin: SuperAdminUser,
    axum::Json(body): axum::Json<TerminalResizeBody>,
) -> Response {
    if !nas_engine::terminal::is_valid_geometry(body.cols, body.rows) {
        return bad_request("geometría del terminal fuera de rango");
    }

    let web_user = superadmin.user().username().to_string();
    let cols = body.cols;
    let rows = body.rows;
    let outcome = tokio::task::spawn_blocking(move || {
        let posix_user = resolve_terminal_posix_user(&web_user)?;
        let name = nas_engine::terminal::session_name(&posix_user);
        nas_engine::terminal::resize(&Runner::new(), &posix_user, &name, cols, rows)
    })
    .await
    .unwrap_or_else(|_| Err("redimensionado interrumpido".to_string()));

    match outcome {
        Ok(()) => axum::Json(ApiResponse::<serde_json::Value>::ok_empty(
            "Terminal redimensionado",
        ))
        .into_response(),
        Err(error) => server_error(&error),
    }
}

/// Termina la sesión de terminal del usuario.
pub async fn terminal_kill(
    State(state): State<AppState>,
    superadmin: SuperAdminUser,
    headers: HeaderMap,
) -> Response {
    let user = superadmin.user().username().to_string();
    let web_user = user.clone();
    let outcome = tokio::task::spawn_blocking(move || {
        let posix_user = resolve_terminal_posix_user(&web_user)?;
        let name = nas_engine::terminal::session_name(&posix_user);
        nas_engine::terminal::kill(&Runner::new(), &posix_user, &name)?;
        Ok::<_, String>(name)
    })
    .await
    .unwrap_or_else(|_| Err("cierre interrumpido".to_string()));

    match outcome {
        Ok(name) => {
            audit_ip(
                &state,
                &headers,
                &user,
                "terminal_kill",
                &name,
                "SUCCESS",
                "",
            );
            axum::Json(ApiResponse::<serde_json::Value>::ok_empty("Sesión cerrada")).into_response()
        }
        Err(error) => server_error(&error),
    }
}

/// Historial de comandos del terminal (aún sin persistencia propia).
pub async fn terminal_history(_user: AuthenticatedUser) -> Response {
    axum::Json(ApiResponse::ok(
        Vec::<String>::new(),
        "Historial de terminal",
    ))
    .into_response()
}

/// Router del dominio de terminal.
pub fn router() -> Router<crate::AppState> {
    use axum::routing::{get, post};
    Router::new()
        .route("/api/terminal/session", post(terminal_session))
        .route("/api/terminal/send", post(terminal_send))
        .route("/api/terminal/capture", get(terminal_capture))
        .route("/api/terminal/resize", post(terminal_resize))
        .route("/api/terminal/kill", post(terminal_kill))
        .route("/api/terminal/history", get(terminal_history))
}
