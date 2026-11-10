use axum::http::header::COOKIE;
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use chrono::Utc;
pub use nas_core::ApiResponse;
use nas_core::NasError;
use nas_exec::Runner;
use serde::Deserialize;

use crate::AppState;

pub fn hostname() -> String {
    std::fs::read_to_string("/etc/hostname")
        .map(|value| value.trim().to_string())
        .ok()
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "SRV-NAS".to_string())
}

pub fn server_ip() -> String {
    let runner = Runner::new();
    if let Some(ip) = nas_engine::primary_ip(&runner) {
        return ip;
    }
    std::net::UdpSocket::bind("0.0.0.0:0")
        .and_then(|socket| {
            socket.connect("10.255.255.255:1")?;
            socket.local_addr()
        })
        .map(|addr| addr.ip().to_string())
        .ok()
        .filter(|ip| !ip.starts_with("127."))
        .unwrap_or_else(|| "10.10.1.2".to_string())
}

/// Registra auditoría con la IP de origen automáticamente extraída de las cabeceras.
pub fn audit_ip(
    state: &AppState,
    headers: &HeaderMap,
    username: &str,
    action: &str,
    target: &str,
    result: &str,
    detail: &str,
) {
    let ip = extract_client_ip(headers);
    if let Err(error) =
        state
            .auth
            .db()
            .append_audit_ip(username, action, target, result, detail, Some(&ip))
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

pub fn require_auth(state: &AppState, headers: &HeaderMap) -> Result<String, Box<Response>> {
    match state
        .auth
        .authenticate(cookie_header(headers), Utc::now().timestamp())
    {
        Ok(Some(user)) => Ok(user),
        Ok(None) => Err(Box::new(unauthorized())),
        Err(error) => Err(Box::new(server_error(&error.to_string()))),
    }
}

pub fn unauthorized() -> Response {
    (
        StatusCode::UNAUTHORIZED,
        axum::Json(ApiResponse::<serde_json::Value>::error(
            "sesión no válida o expirada",
        )),
    )
        .into_response()
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

pub fn is_admin(state: &AppState, user: &str) -> bool {
    state
        .auth
        .db()
        .user_role(user)
        .ok()
        .flatten()
        .is_some_and(|role| matches!(role.as_str(), "admin" | "superadmin"))
}

pub fn is_superadmin(state: &AppState, user: &str) -> bool {
    if configured_superadmin().as_deref() == Some(user) {
        return true;
    }
    state
        .auth
        .db()
        .user_role(user)
        .ok()
        .flatten()
        .is_some_and(|role| role == "superadmin")
}

pub fn configured_superadmin() -> Option<String> {
    std::fs::read_to_string("/etc/nas/superadmin")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

pub fn run_json(command: nas_exec::CommandSpec) -> Result<String, String> {
    let output = Runner::new()
        .run(&command)
        .map_err(|error| error.to_string())?;
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
