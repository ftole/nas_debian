use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use axum::{
    extract::{Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Router,
};
use serde::Deserialize;

use crate::{
    extractors::{AdminUser, AuthenticatedUser},
    routes::{
        common::{
            audit_ip, bad_request, deserialize_string_or_int, error_response, extract_client_ip,
            forbidden, server_error, ApiResponse, ExtQuery,
        },
        files::file_root_for_async,
    },
    AppState,
};

const PRIV_DEL_MAX_FAILURES: usize = 5;
const PRIV_DEL_WINDOW: Duration = Duration::from_secs(300);

fn priv_del_failures() -> &'static Mutex<HashMap<String, Vec<Instant>>> {
    static STORE: OnceLock<Mutex<HashMap<String, Vec<Instant>>>> = OnceLock::new();
    STORE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn priv_del_retry_after(key: &str, now: Instant) -> Option<u64> {
    let mut guard = priv_del_failures().lock().ok()?;
    let list = guard.get_mut(key)?;
    list.retain(|&t| now.duration_since(t) < PRIV_DEL_WINDOW);
    if list.len() >= PRIV_DEL_MAX_FAILURES {
        let oldest = list.first().copied().unwrap_or(now);
        let rem = PRIV_DEL_WINDOW
            .saturating_sub(now.duration_since(oldest))
            .as_secs()
            .max(1);
        Some(rem)
    } else {
        None
    }
}

fn priv_del_record_failure(key: &str, now: Instant) {
    if let Ok(mut guard) = priv_del_failures().lock() {
        let list = guard.entry(key.to_string()).or_default();
        list.retain(|&t| now.duration_since(t) < PRIV_DEL_WINDOW);
        list.push(now);
    }
}

fn priv_del_clear(key: &str) {
    if let Ok(mut guard) = priv_del_failures().lock() {
        guard.remove(key);
    }
}

/// Cuerpo para borrado privilegiado con credenciales del SO (`root` o `sudo`).
#[derive(Debug, Deserialize)]
pub struct PrivilegedDeleteBody {
    #[serde(default)]
    pub root: Option<String>,
    #[serde(default)]
    pub path: String,
    #[serde(default)]
    pub auth_user: String,
    #[serde(default)]
    pub auth_password: String,
    #[serde(default)]
    pub confirm: String,
}

/// Cuerpo para borrado (papelera o definitivo).
#[derive(Debug, Deserialize)]
pub struct DeleteBody {
    #[serde(default)]
    pub root: Option<String>,
    #[serde(default)]
    pub path: String,
    #[serde(default)]
    pub permanent: bool,
}

#[derive(Debug, Deserialize, Default)]
pub struct TrashBody {
    #[serde(default, deserialize_with = "deserialize_string_or_int")]
    pub id: String,
    #[serde(default)]
    pub root: Option<String>,
}

/// Envía un elemento a la papelera interna o lo borra definitivamente.
pub async fn files_delete(
    State(state): State<AppState>,
    user: AuthenticatedUser,
    headers: HeaderMap,
    axum::Json(body): axum::Json<DeleteBody>,
) -> Response {
    if body.permanent && !user.is_admin() {
        return forbidden("se requieren privilegios de administrador");
    }
    let root = match file_root_for_async(&state, body.root.as_deref()).await {
        Ok(root) => root,
        Err(response) => return *response,
    };
    let path = body.path;

    let client = user.clone();
    let audit_path = path.clone();
    let permanent = body.permanent;

    let outcome = state
        .run_system_blocking(move || {
            if permanent {
                root.delete_permanent(&path).map(|_| None)
            } else {
                root.trash_with_actor(&path, &client).map(Some)
            }
        })
        .await;

    match outcome {
        Ok(Ok(target)) => {
            let action = if permanent {
                "file_delete_permanent"
            } else {
                "file_delete"
            };
            audit_ip(
                &state,
                &headers,
                user.username(),
                action,
                &audit_path,
                "SUCCESS",
                "",
            )
            .await;
            let msg = if permanent {
                "Elemento eliminado definitivamente"
            } else {
                "Elemento movido a la papelera"
            };
            axum::Json(ApiResponse::ok(
                serde_json::json!({
                    "trash": target.map(|t| t.display().to_string()),
                    "permanent": permanent,
                }),
                msg,
            ))
            .into_response()
        }
        Ok(Err(error)) => error_response(error),
        Err(_) => server_error("borrado interrumpido"),
    }
}

/// Lista la papelera interna.
pub async fn files_trash(
    State(state): State<AppState>,
    _user: AuthenticatedUser,
    Query(query): Query<ExtQuery>,
) -> Response {
    let root = match file_root_for_async(&state, query.root.as_deref()).await {
        Ok(root) => root,
        Err(response) => return *response,
    };

    match state
        .run_system_blocking(move || root.trash_entries())
        .await
    {
        Ok(Ok(items)) => axum::Json(ApiResponse::ok(
            crate::legacy::trash_listing(&items),
            "Papelera",
        ))
        .into_response(),
        Ok(Err(error)) => error_response(error),
        Err(_) => server_error("papelera interrumpida"),
    }
}

/// Restaura un elemento de la papelera.
pub async fn trash_restore(
    State(state): State<AppState>,
    _user: AuthenticatedUser,
    axum::Json(body): axum::Json<TrashBody>,
) -> Response {
    let root = match file_root_for_async(&state, body.root.as_deref()).await {
        Ok(root) => root,
        Err(response) => return *response,
    };
    match state
        .run_system_blocking(move || root.restore_trash(&body.id))
        .await
    {
        Ok(Ok(name)) => axum::Json(ApiResponse::ok(
            serde_json::json!({ "restored": name }),
            "Elemento restaurado",
        ))
        .into_response(),
        Ok(Err(error)) => error_response(error),
        Err(_) => server_error("restauración interrumpida"),
    }
}

/// Elimina definitivamente un elemento de la papelera.
pub async fn trash_delete(
    State(state): State<AppState>,
    _admin: AdminUser,
    axum::Json(body): axum::Json<TrashBody>,
) -> Response {
    let root = match file_root_for_async(&state, body.root.as_deref()).await {
        Ok(root) => root,
        Err(response) => return *response,
    };
    match state
        .run_system_blocking(move || root.delete_trash(&body.id))
        .await
    {
        Ok(Ok(())) => axum::Json(ApiResponse::<serde_json::Value>::ok_empty(
            "Elemento eliminado",
        ))
        .into_response(),
        Ok(Err(error)) => error_response(error),
        Err(_) => server_error("borrado interrumpido"),
    }
}

/// Vacía la papelera.
pub async fn trash_empty(
    State(state): State<AppState>,
    _admin: AdminUser,
    body: axum::body::Bytes,
) -> Response {
    let trash_body: Option<TrashBody> = if body.is_empty() {
        None
    } else {
        serde_json::from_slice(&body).ok()
    };
    let root_param = trash_body.as_ref().and_then(|b| b.root.as_deref());
    let root = match file_root_for_async(&state, root_param).await {
        Ok(root) => root,
        Err(response) => return *response,
    };
    match state.run_system_blocking(move || root.empty_trash()).await {
        Ok(Ok(removed)) => axum::Json(ApiResponse::ok(
            serde_json::json!({ "removed": removed }),
            "Papelera vaciada",
        ))
        .into_response(),
        Ok(Err(error)) => error_response(error),
        Err(_) => server_error("vaciado interrumpido"),
    }
}

/// Restaura todos los elementos de la papelera.
pub async fn trash_restore_all(
    State(state): State<AppState>,
    _user: AuthenticatedUser,
    body: axum::body::Bytes,
) -> Response {
    let trash_body: Option<TrashBody> = if body.is_empty() {
        None
    } else {
        serde_json::from_slice(&body).ok()
    };
    let root_param = trash_body.as_ref().and_then(|b| b.root.as_deref());
    let root = match file_root_for_async(&state, root_param).await {
        Ok(root) => root,
        Err(response) => return *response,
    };
    match state
        .run_system_blocking(move || {
            let entries = root.trash_entries()?;
            let mut restored = 0u32;
            for entry in entries {
                if root.restore_trash(&entry.id).is_ok() {
                    restored += 1;
                }
            }
            Ok::<_, nas_core::NasError>(restored)
        })
        .await
    {
        Ok(Ok(restored)) => axum::Json(ApiResponse::ok(
            serde_json::json!({ "restored": restored }),
            "Papelera restaurada",
        ))
        .into_response(),
        Ok(Err(error)) => error_response(error),
        Err(_) => server_error("restauración interrumpida"),
    }
}

/// Elimina un elemento con credenciales OS (`root` o usuario `sudo`), sin alterar permisos/ACLs.
pub async fn files_delete_privileged(
    State(state): State<AppState>,
    admin: AdminUser,
    headers: HeaderMap,
    axum::Json(body): axum::Json<PrivilegedDeleteBody>,
) -> Response {
    let ip = extract_client_ip(&headers);
    let rate_key = format!("{ip}:{}", admin.username());
    let now = Instant::now();
    if let Some(retry_after) = priv_del_retry_after(&rate_key, now) {
        let mut resp = (
            StatusCode::TOO_MANY_REQUESTS,
            axum::Json(ApiResponse::<serde_json::Value>::error(format!(
                "demasiados intentos fallidos; espera {retry_after}s antes de reintentar"
            ))),
        )
            .into_response();
        if let Ok(val) = axum::http::HeaderValue::from_str(&retry_after.to_string()) {
            resp.headers_mut()
                .insert(axum::http::header::RETRY_AFTER, val);
        }
        return resp;
    }

    if body.confirm.trim() != "SI-ELIMINAR" {
        return bad_request("confirmación requerida: escribe SI-ELIMINAR");
    }
    let rel_path = body.path.trim().to_string();
    if rel_path.is_empty() {
        return bad_request("ruta requerida");
    }
    let auth_user = body.auth_user.trim().to_string();
    if !nas_engine::helpers::share::is_valid_os_username(&auth_user) {
        return bad_request("usuario del sistema inválido");
    }
    if body.auth_password.is_empty()
        || body.auth_password.contains('\0')
        || body.auth_password.contains('\n')
        || body.auth_password.contains('\r')
    {
        return bad_request("contraseña del sistema inválida");
    }

    let root = match file_root_for_async(&state, body.root.as_deref()).await {
        Ok(root) => root,
        Err(response) => return *response,
    };
    if let Some(desc) = root.is_protected_path(&rel_path) {
        return forbidden(&format!(
            "operación no permitida: no se puede eliminar sobre {desc}"
        ));
    }
    let root_check = root.clone();
    let rel_check = rel_path.clone();
    match state
        .run_system_blocking(move || {
            let resolved = root_check.resolve(&rel_check)?;
            if resolved == root_check.root() {
                return Err(nas_core::NasError::Denied(
                    "no se puede eliminar el directorio raíz".into(),
                ));
            }
            Ok::<_, nas_core::NasError>(())
        })
        .await
    {
        Ok(Ok(())) => {}
        Ok(Err(error)) => return error_response(error),
        Err(_) => return server_error("validación de ruta interrumpida"),
    }

    let payload_json = serde_json::json!({
        "root": body.root,
        "path": rel_path,
        "auth_user": auth_user,
        "auth_password": body.auth_password,
        "web_user": admin.username(),
        "origin_ip": ip,
    })
    .to_string();
    let secret_pw = body.auth_password.clone();

    let outcome = state
        .run_system_blocking(move || {
            let runner = nas_exec::Runner::new().with_secret(&secret_pw);
            let spec = nas_exec::CommandSpec::new("/usr/local/bin/nas")
                .args(["helper", "files-delete"])
                .sudo(nas_exec::SudoMode::Passwordless)
                .stdin_secret(nas_exec::SecretInput::new(payload_json))
                .timeout(Duration::from_secs(30));
            let out = runner.run(&spec)?;
            if out.is_success() {
                Ok::<_, nas_core::NasError>(out.stdout)
            } else {
                let msg = if out.stderr.trim().is_empty() {
                    out.stdout.trim().to_string()
                } else {
                    out.stderr.trim().to_string()
                };
                Err(nas_core::NasError::Denied(msg))
            }
        })
        .await;

    let detail = format!("os_user={auth_user}");
    match outcome {
        Ok(Ok(msg)) => {
            priv_del_clear(&rate_key);
            audit_ip(
                &state,
                &headers,
                admin.username(),
                "file_delete_privileged",
                &rel_path,
                "SUCCESS",
                &detail,
            )
            .await;
            axum::Json(ApiResponse::ok(
                serde_json::json!({ "path": rel_path, "os_user": auth_user }),
                if msg.trim().is_empty() {
                    "Elemento eliminado con privilegios"
                } else {
                    msg.trim()
                },
            ))
            .into_response()
        }
        Ok(Err(error)) => {
            priv_del_record_failure(&rate_key, Instant::now());
            audit_ip(
                &state,
                &headers,
                admin.username(),
                "file_delete_privileged",
                &rel_path,
                "FAILED",
                &detail,
            )
            .await;
            error_response(error)
        }
        Err(_) => {
            priv_del_record_failure(&rate_key, Instant::now());
            server_error("eliminación privilegiada interrumpida")
        }
    }
}

/// Router del dominio de papelera.
pub fn router() -> Router<crate::AppState> {
    use axum::routing::{get, post};
    Router::new()
        .route("/api/files/trash", get(files_trash))
        .route("/api/files/trash/restore", post(trash_restore))
        .route("/api/files/trash/restore-all", post(trash_restore_all))
        .route("/api/files/trash/delete", post(trash_delete))
        .route("/api/files/trash/empty", post(trash_empty))
        .route("/api/files/delete", post(files_delete))
        .route(
            "/api/files/delete-privileged",
            post(files_delete_privileged),
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rate_limit_borrado_privilegiado_bloquea_tras_cinco_fallos() {
        let key = "127.0.0.1:admin_test_rl";
        priv_del_clear(key);
        let now = Instant::now();
        for _ in 0..5 {
            assert!(priv_del_retry_after(key, now).is_none());
            priv_del_record_failure(key, now);
        }
        assert!(priv_del_retry_after(key, now).is_some());
        priv_del_clear(key);
        assert!(priv_del_retry_after(key, now).is_none());
    }
}
