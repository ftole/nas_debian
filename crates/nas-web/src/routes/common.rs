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

pub async fn cached_host_ip() -> (String, String) {
    tokio::task::spawn_blocking(|| (hostname(), server_ip()))
        .await
        .unwrap_or_else(|_| ("SRV-NAS".to_string(), "10.10.1.2".to_string()))
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
    let output = Runner::new()
        .run(&spec)
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
