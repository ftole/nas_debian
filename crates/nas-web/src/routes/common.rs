use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use axum::http::header::COOKIE;
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
pub use nas_core::ApiResponse;
use nas_core::NasError;
use nas_exec::Runner;
use serde::Deserialize;

use crate::AppState;

static HOSTNAME_CACHE: OnceLock<String> = OnceLock::new();
static SERVER_IP_CACHE: OnceLock<Mutex<Option<(Instant, String)>>> = OnceLock::new();
const SERVER_IP_TTL: Duration = Duration::from_secs(30);

pub fn hostname() -> String {
    HOSTNAME_CACHE
        .get_or_init(|| {
            std::fs::read_to_string("/etc/hostname")
                .map(|value| value.trim().to_string())
                .ok()
                .filter(|value| !value.is_empty())
                .unwrap_or_else(|| "SRV-NAS".to_string())
        })
        .clone()
}

pub fn server_ip() -> String {
    let lock = SERVER_IP_CACHE.get_or_init(|| Mutex::new(None));
    if let Ok(guard) = lock.lock() {
        if let Some((captured_at, ip)) = guard.as_ref() {
            if captured_at.elapsed() < SERVER_IP_TTL {
                return ip.clone();
            }
        }
    }

    let runner = Runner::new();
    let resolved = if let Some(ip) = nas_engine::primary_ip(&runner) {
        ip
    } else {
        std::net::UdpSocket::bind("0.0.0.0:0")
            .and_then(|socket| {
                socket.connect("10.255.255.255:1")?;
                socket.local_addr()
            })
            .map(|addr| addr.ip().to_string())
            .ok()
            .filter(|ip| !ip.starts_with("127."))
            .unwrap_or_else(|| "10.10.1.2".to_string())
    };

    if let Ok(mut guard) = lock.lock() {
        *guard = Some((Instant::now(), resolved.clone()));
    }
    resolved
}

fn cached_host_ip_snapshot(allow_stale: bool) -> Option<(String, String)> {
    let host = HOSTNAME_CACHE.get()?.clone();
    let lock = SERVER_IP_CACHE.get_or_init(|| Mutex::new(None));
    let guard = lock.try_lock().ok()?;
    let (captured_at, ip) = guard.as_ref()?;
    if allow_stale || captured_at.elapsed() < SERVER_IP_TTL {
        Some((host, ip.clone()))
    } else {
        None
    }
}

pub fn cached_host_ip_nonblocking() -> (String, String) {
    if let Some(pair) = cached_host_ip_snapshot(true) {
        return pair;
    }
    let host = HOSTNAME_CACHE
        .get()
        .cloned()
        .unwrap_or_else(|| "SRV-NAS".to_string());
    let ip = SERVER_IP_CACHE
        .get_or_init(|| Mutex::new(None))
        .try_lock()
        .ok()
        .and_then(|guard| guard.as_ref().map(|(_, ip)| ip.clone()))
        .unwrap_or_else(|| "10.10.1.2".to_string());
    (host, ip)
}

pub async fn cached_host_ip() -> (String, String) {
    if let Some(pair) = cached_host_ip_snapshot(false) {
        return pair;
    }
    tokio::task::spawn_blocking(|| (hostname(), server_ip()))
        .await
        .unwrap_or_else(|_| ("SRV-NAS".to_string(), "10.10.1.2".to_string()))
}

/// Registra auditoría con la IP de origen automáticamente extraída de las cabeceras
/// sin bloquear el reactor asíncrono.
pub async fn audit_ip(
    state: &AppState,
    headers: &HeaderMap,
    username: &str,
    action: &str,
    target: &str,
    result: &str,
    detail: &str,
) {
    let ip = extract_client_ip(headers);
    let username = username.to_string();
    let action = action.to_string();
    let target = target.to_string();
    let result = result.to_string();
    let detail = detail.to_string();
    if let Err(error) = state
        .run_db(move |db| {
            db.append_audit_ip(&username, &action, &target, &result, &detail, Some(&ip))
        })
        .await
    {
        eprintln!("aviso: no se pudo registrar auditoría con origen: {error}");
    }
}

/// Extrae la IP del cliente respetando proxys (`X-Real-IP`, `X-Forwarded-For`).
pub fn extract_client_ip(headers: &HeaderMap) -> String {
    if let Some(real_ip) = headers.get("x-real-ip").and_then(|v| v.to_str().ok()) {
        let trimmed = real_ip.trim();
        if !trimmed.is_empty() {
            return trimmed.to_string();
        }
    }
    if let Some(forwarded) = headers.get("x-forwarded-for").and_then(|v| v.to_str().ok()) {
        if let Some(first) = forwarded.split(',').next() {
            let trimmed = first.trim();
            if !trimmed.is_empty() {
                return trimmed.to_string();
            }
        }
    }
    "127.0.0.1".to_string()
}

pub fn random_hex() -> String {
    let mut buffer = [0u8; 8];
    let _ = getrandom::getrandom(&mut buffer);
    hex::encode(buffer)
}

pub fn bad_request(message: &str) -> Response {
    (
        StatusCode::BAD_REQUEST,
        axum::Json(ApiResponse::<serde_json::Value>::error(message)),
    )
        .into_response()
}

pub fn error_response(error: NasError) -> Response {
    let status = match &error {
        NasError::Denied(_) => StatusCode::FORBIDDEN,
        NasError::NotFound(_) => StatusCode::NOT_FOUND,
        NasError::Invalid(_) => StatusCode::BAD_REQUEST,
        NasError::Io { source, .. } if source.kind() == std::io::ErrorKind::NotFound => {
            StatusCode::NOT_FOUND
        }
        NasError::Io { source, .. }
            if source.kind() == std::io::ErrorKind::PermissionDenied
                || source.raw_os_error() == Some(1)
                || source.raw_os_error() == Some(13) =>
        {
            StatusCode::FORBIDDEN
        }
        _ => StatusCode::INTERNAL_SERVER_ERROR,
    };

    (
        status,
        axum::Json(ApiResponse::<serde_json::Value>::error(error.to_string())),
    )
        .into_response()
}

pub fn cookie_header(headers: &HeaderMap) -> &str {
    headers
        .get(COOKIE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
}

pub fn server_error(message: &str) -> Response {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        axum::Json(ApiResponse::<serde_json::Value>::error(message)),
    )
        .into_response()
}

pub fn forbidden(message: &str) -> Response {
    (
        StatusCode::FORBIDDEN,
        axum::Json(ApiResponse::<serde_json::Value>::error(message)),
    )
        .into_response()
}

pub fn configured_superadmin() -> Option<String> {
    std::fs::read_to_string("/etc/nas/superadmin")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// Evalúa si un usuario es superadmin dados los orígenes ya resueltos.
pub fn is_superadmin_from_sources(
    username: &str,
    configured_super: Option<&str>,
    db_role: Option<&str>,
    in_superadmin_group: bool,
) -> bool {
    configured_super == Some(username) || db_role == Some("superadmin") || in_superadmin_group
}

fn is_member_in_group_text(group_text: &str, group_name: &str, username: &str) -> bool {
    group_text.lines().any(|line| {
        let mut parts = line.split(':');
        if parts.next() != Some(group_name) {
            return false;
        }
        parts
            .nth(2)
            .is_some_and(|members| members.split(',').any(|m| m.trim() == username))
    })
}

/// Comprueba membresía POSIX en `grp_superadmin` leyendo `/etc/group`.
pub fn is_in_superadmin_group(username: &str) -> bool {
    let Ok(group_text) = std::fs::read_to_string("/etc/group") else {
        return false;
    };
    is_member_in_group_text(&group_text, "grp_superadmin", username)
}

/// Resuelve el rol efectivo (`"superadmin"`, `"admin"`, `"operator"`, ...) de forma homogénea.
pub fn resolve_effective_role(db: &crate::db::Db, username: &str) -> String {
    let configured = configured_superadmin();
    let db_role = db.user_role(username).ok().flatten();
    if is_superadmin_from_sources(username, configured.as_deref(), db_role.as_deref(), false)
        || is_in_superadmin_group(username)
    {
        "superadmin".to_string()
    } else {
        db_role.unwrap_or_else(|| "operator".to_string())
    }
}

/// Determina si `username` tiene privilegios efectivos de superadministrador.
pub fn is_user_superadmin(db: &crate::db::Db, username: &str) -> bool {
    resolve_effective_role(db, username) == "superadmin"
}

pub fn run_json(command: nas_exec::CommandSpec) -> Result<String, String> {
    let runner = match &command.stdin_data {
        Some(secret) => Runner::new().with_secret(secret.expose()),
        None => Runner::new(),
    };
    let output = runner.run(&command).map_err(|error| error.to_string())?;
    if output.is_success() {
        Ok(output.stdout)
    } else {
        Err(if output.stderr.is_empty() {
            output.stdout
        } else {
            output.stderr
        })
    }
}

/// Invoca un subcomando de `/usr/local/bin/nas helper` mediante `sudo -n`.
pub fn run_nas_helper(
    subcommand: &str,
    args: &[&str],
    stdin_secret: Option<&str>,
    timeout_secs: u64,
) -> Result<String, String> {
    let mut cmd_args = Vec::with_capacity(2 + args.len());
    cmd_args.push("helper");
    cmd_args.push(subcommand);
    cmd_args.extend_from_slice(args);
    let mut spec = nas_exec::CommandSpec::new("/usr/local/bin/nas")
        .args(cmd_args)
        .sudo(nas_exec::SudoMode::Passwordless)
        .timeout(Duration::from_secs(timeout_secs));
    if let Some(secret) = stdin_secret {
        spec = spec.stdin_secret(nas_exec::SecretInput::new(secret));
    }
    run_json(spec)
}

pub fn safe_content_disposition(disposition: &str, filename: &str) -> HeaderValue {
    let ascii_name: String = filename
        .chars()
        .map(|c| {
            if c.is_ascii_graphic() && c != '"' && c != '\\' && c != ';' && c != '/' {
                c
            } else if c.is_ascii_whitespace() {
                ' '
            } else {
                '_'
            }
        })
        .collect();
    let ascii_name = if ascii_name.trim().is_empty() {
        "archivo.bin"
    } else {
        ascii_name.trim()
    };

    let mut encoded = String::new();
    for byte in filename.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            encoded.push(byte as char);
        } else {
            use std::fmt::Write;
            let _ = write!(encoded, "%{:02X}", byte);
        }
    }

    let header_val =
        format!("{disposition}; filename=\"{ascii_name}\"; filename*=UTF-8''{encoded}");
    HeaderValue::from_str(&header_val)
        .unwrap_or_else(|_| HeaderValue::from_static("attachment; filename=\"descarga.bin\""))
}

pub fn deserialize_string_or_int<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = serde_json::Value::deserialize(deserializer)?;
    match value {
        serde_json::Value::String(text) => Ok(text),
        serde_json::Value::Number(num) => Ok(num.to_string()),
        serde_json::Value::Null => Ok(String::new()),
        other => Err(serde::de::Error::custom(format!(
            "identificador inválido: {other}"
        ))),
    }
}

#[derive(Debug, Deserialize, Default)]
#[allow(dead_code)]
pub struct ExtQuery {
    #[serde(default)]
    pub limit: Option<usize>,
    #[serde(default)]
    pub source: Option<String>,
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub share: Option<String>,
    #[serde(default)]
    pub root: Option<String>,
    #[serde(default)]
    pub q: Option<String>,
    #[serde(default)]
    pub grouped: Option<u8>,
}

#[cfg(test)]
mod tests {
    use super::{is_member_in_group_text, is_superadmin_from_sources};

    #[test]
    fn is_member_in_group_text_detecta_miembros() {
        let group = "root:x:0:\ngrp_superadmin:x:1002:alice,bob\n";
        assert!(is_member_in_group_text(group, "grp_superadmin", "alice"));
        assert!(is_member_in_group_text(group, "grp_superadmin", "bob"));
        assert!(!is_member_in_group_text(group, "grp_superadmin", "charlie"));
        assert!(!is_member_in_group_text(group, "grp_admin", "alice"));
    }

    #[test]
    fn is_superadmin_from_sources_respeta_tres_origenes() {
        assert!(is_superadmin_from_sources(
            "alice",
            Some("alice"),
            None,
            false
        ));
        assert!(is_superadmin_from_sources(
            "alice",
            None,
            Some("superadmin"),
            false
        ));
        assert!(is_superadmin_from_sources(
            "alice",
            None,
            Some("admin"),
            true
        ));
        assert!(!is_superadmin_from_sources(
            "alice",
            Some("bob"),
            Some("admin"),
            false
        ));
    }
}
