use axum::{
    extract::State,
    http::HeaderMap,
    response::{IntoResponse, Response},
};
use nas_core::NasError;
use nas_exec::Runner;
use serde::Deserialize;

use super::StorageActionBody;
use crate::{
    routes::common::{audit_ip, error_response, forbidden, is_admin, require_auth, ApiResponse},
    AppState,
};

/// Cuerpo para ampliar el pool con varios discos a la vez.
#[derive(Debug, Deserialize)]
pub struct StorageMultiBody {
    #[serde(default)]
    pub devices: Vec<String>,
    #[serde(default)]
    pub confirm: String,
}

fn audit_storage(
    state: &AppState,
    headers: &HeaderMap,
    actor: &str,
    action: &str,
    target: &str,
    status: &str,
    detail: &str,
) {
    audit_ip(state, headers, actor, action, target, status, detail);
}

async fn storage_operation<F>(
    state: AppState,
    headers: HeaderMap,
    action: &'static str,
    target: String,
    operation: F,
) -> Response
where
    F: FnOnce() -> Result<String, String> + Send + 'static,
{
    let actor = match require_auth(&state, &headers) {
        Ok(user) => user,
        Err(response) => return *response,
    };
    if !is_admin(&state, &actor) {
        return forbidden("se requieren privilegios de administrador");
    }

    let audited_target = target.clone();
    let outcome = tokio::task::spawn_blocking(operation)
        .await
        .unwrap_or_else(|_| Err("operación interrumpida".to_string()));

    match outcome {
        Ok(message) => {
            audit_storage(
                &state,
                &headers,
                &actor,
                action,
                &audited_target,
                "SUCCESS",
                &message,
            );
            axum::Json(ApiResponse::ok(
                serde_json::json!({ "message": message }),
                message,
            ))
            .into_response()
        }
        Err(error) => {
            audit_storage(
                &state,
                &headers,
                &actor,
                action,
                &audited_target,
                "FAILED",
                &error,
            );
            error_response(NasError::Invalid(error))
        }
    }
}

/// Formatea y monta un dispositivo en `/srv/nas`.
pub async fn storage_format(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::Json(body): axum::Json<StorageActionBody>,
) -> Response {
    let target = body.device.clone();
    let device = body.device;
    let fstype = if body.fstype.is_empty() {
        "ext4".to_string()
    } else {
        body.fstype
    };
    let confirm = body.confirm;
    let unmount = body.unmount;
    storage_operation(state, headers, "storage_format", target, move || {
        nas_engine::storage_ops::format_device(&Runner::new(), &device, &fstype, &confirm, unmount)
    })
    .await
}

/// Crea un volumen LVM y lo monta en `/srv/nas`.
pub async fn storage_lvm(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::Json(body): axum::Json<StorageActionBody>,
) -> Response {
    let target = body.disk.clone();
    storage_operation(state, headers, "storage_lvm", target, move || {
        nas_engine::storage_ops::lvm_create(
            &Runner::new(),
            &body.disk,
            &body.vg,
            &body.lv,
            &body.size,
            &body.fstype,
            &body.confirm,
            body.unmount,
        )
    })
    .await
}

/// Crea un subvolumen Btrfs.
pub async fn storage_subvolume(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::Json(body): axum::Json<StorageActionBody>,
) -> Response {
    let target = body.subvolume.clone();
    storage_operation(state, headers, "storage_subvolume", target, move || {
        nas_engine::storage_ops::create_subvolume(
            &Runner::new(),
            &body.device,
            &body.subvolume,
            &body.confirm,
        )
    })
    .await
}

/// Añade una unidad adicional montada en `/srv/nas/unidades/<id>`.
pub async fn storage_add_unit(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::Json(body): axum::Json<StorageActionBody>,
) -> Response {
    let target = body.unit_id.clone();
    let fstype = if body.fstype.is_empty() {
        "ext4".to_string()
    } else {
        body.fstype
    };
    storage_operation(state, headers, "storage_add_unit", target, move || {
        nas_engine::storage_ops::add_unit(
            &Runner::new(),
            &body.device,
            &body.unit_id,
            &fstype,
            &body.confirm,
        )
    })
    .await
}

/// Extiende el pool LVM que contiene `/srv/nas`.
pub async fn storage_extend_pool(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::Json(body): axum::Json<StorageActionBody>,
) -> Response {
    let target = body.device.clone();
    storage_operation(state, headers, "storage_extend_pool", target, move || {
        nas_engine::storage_ops::extend_pool(&Runner::new(), &body.device, &body.confirm)
    })
    .await
}

/// Amplía el pool LVM que contiene `/srv/nas` usando varios discos.
pub async fn storage_extend_pool_multi(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::Json(body): axum::Json<StorageMultiBody>,
) -> Response {
    let actor = match require_auth(&state, &headers) {
        Ok(user) => user,
        Err(response) => return *response,
    };
    if !is_admin(&state, &actor) {
        return forbidden("se requieren privilegios de administrador");
    }

    let devices = body.devices.clone();
    let confirm = body.confirm.clone();
    let target = devices.join(", ");
    let outcome = tokio::task::spawn_blocking(move || {
        let runner = Runner::new();
        let mut results: Vec<serde_json::Value> = Vec::new();
        let mut failed = false;
        for device in &devices {
            match nas_engine::storage_ops::extend_pool(&runner, device, &confirm) {
                Ok(message) => results.push(serde_json::json!({
                    "device": device,
                    "success": true,
                    "message": message,
                })),
                Err(error) => {
                    failed = true;
                    results.push(serde_json::json!({
                        "device": device,
                        "success": false,
                        "message": error,
                    }));
                    break;
                }
            }
        }
        Ok::<_, String>((results, failed))
    })
    .await
    .unwrap_or_else(|_| Err("operación interrumpida".to_string()));

    match outcome {
        Ok((results, failed)) => {
            let status = if failed { "FAILED" } else { "SUCCESS" };
            audit_storage(
                &state,
                &headers,
                &actor,
                "storage_extend_pool_multi",
                &target,
                status,
                &format!("{:?}", results),
            );
            let message = if failed {
                "Ampliación parcial: revisa los resultados por disco."
            } else {
                "Pool ampliado correctamente con todos los discos seleccionados."
            };
            axum::Json(ApiResponse::ok(
                serde_json::json!({ "message": message, "results": results, "failed": failed }),
                message,
            ))
            .into_response()
        }
        Err(error) => {
            audit_storage(
                &state,
                &headers,
                &actor,
                "storage_extend_pool_multi",
                &target,
                "FAILED",
                &error,
            );
            error_response(NasError::Invalid(error))
        }
    }
}
