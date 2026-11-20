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
    extractors::AdminUser,
    routes::common::{audit_ip, error_response, ApiResponse},
    AppState,
};

/// Cuerpo para ampliar el pool con varios discos a la vez.
#[derive(Debug, Deserialize)]
pub struct StorageMultiBody {
    #[serde(default)]
    pub devices: Vec<String>,
    #[serde(default)]
    pub confirm: String,
    #[serde(default)]
    pub target: String,
    #[serde(default)]
    pub metadata_raid1: bool,
}

/// Cuerpo para crear un pool multi-disco o unidad.
#[derive(Debug, Deserialize)]
pub struct CreatePoolBody {
    #[serde(default)]
    pub target: String,
    #[serde(default)]
    pub mode: String,
    #[serde(default)]
    pub devices: Vec<String>,
    #[serde(default)]
    pub confirm: String,
    #[serde(default)]
    pub unmount: bool,
}

/// Cuerpo para desmontar una unidad adicional.
#[derive(Debug, Deserialize)]
pub struct UmountUnitBody {
    #[serde(default)]
    pub unit_id: String,
    #[serde(default)]
    pub confirm: String,
}

/// Cuerpo para limpiar firmas de disco (`wipefs`).
#[derive(Debug, Deserialize)]
pub struct WipeDiskBody {
    #[serde(default)]
    pub device: String,
    #[serde(default)]
    pub confirm: String,
    #[serde(default)]
    pub unmount: bool,
}

/// Cuerpo para sanear `/etc/fstab`.
#[derive(Debug, Deserialize)]
pub struct SanitizeFstabBody {
    #[serde(default)]
    pub confirm: String,
}

/// Cuerpo para retirar un miembro de un pool Btrfs (`btrfs device remove`).
#[derive(Debug, Deserialize)]
pub struct RemovePoolDeviceBody {
    #[serde(default)]
    pub target: String,
    #[serde(default)]
    pub device: String,
    #[serde(default)]
    pub confirm: String,
}

async fn audit_storage(
    state: &AppState,
    headers: &HeaderMap,
    actor: &str,
    action: &str,
    target: &str,
    status: &str,
    detail: &str,
) {
    audit_ip(state, headers, actor, action, target, status, detail).await;
}

async fn storage_operation<F>(
    state: AppState,
    admin: AdminUser,
    headers: HeaderMap,
    action: &'static str,
    target: String,
    operation: F,
) -> Response
where
    F: FnOnce() -> Result<String, String> + Send + 'static,
{
    let actor = admin.user().username().to_string();

    let audited_target = target.clone();
    let outcome = state
        .run_heavy_blocking(operation)
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
            )
            .await;
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
            )
            .await;
            error_response(NasError::Invalid(error))
        }
    }
}

/// Formatea y monta un dispositivo en `/srv/nas`.
pub async fn storage_format(
    State(state): State<AppState>,
    admin: AdminUser,
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
    storage_operation(state, admin, headers, "storage_format", target, move || {
        nas_engine::storage_ops::format_device(&Runner::new(), &device, &fstype, &confirm, unmount)
    })
    .await
}

/// Crea un volumen LVM y lo monta en `/srv/nas`.
pub async fn storage_lvm(
    State(state): State<AppState>,
    admin: AdminUser,
    headers: HeaderMap,
    axum::Json(body): axum::Json<StorageActionBody>,
) -> Response {
    let target = body.disk.clone();
    storage_operation(state, admin, headers, "storage_lvm", target, move || {
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
    admin: AdminUser,
    headers: HeaderMap,
    axum::Json(body): axum::Json<StorageActionBody>,
) -> Response {
    let target = body.subvolume.clone();
    storage_operation(
        state,
        admin,
        headers,
        "storage_subvolume",
        target,
        move || {
            nas_engine::storage_ops::create_subvolume(
                &Runner::new(),
                &body.device,
                &body.subvolume,
                &body.confirm,
            )
        },
    )
    .await
}

/// Añade una unidad adicional montada en `/srv/nas/unidades/<id>`.
pub async fn storage_add_unit(
    State(state): State<AppState>,
    admin: AdminUser,
    headers: HeaderMap,
    axum::Json(body): axum::Json<StorageActionBody>,
) -> Response {
    let target = body.unit_id.clone();
    let fstype = if body.fstype.is_empty() {
        "ext4".to_string()
    } else {
        body.fstype
    };
    storage_operation(
        state,
        admin,
        headers,
        "storage_add_unit",
        target,
        move || {
            nas_engine::storage_ops::add_unit(
                &Runner::new(),
                &body.device,
                &body.unit_id,
                &fstype,
                &body.confirm,
            )
        },
    )
    .await
}

/// Extiende un pool existente (LVM o Btrfs) con un disco.
pub async fn storage_extend_pool(
    State(state): State<AppState>,
    admin: AdminUser,
    headers: HeaderMap,
    axum::Json(body): axum::Json<StorageActionBody>,
) -> Response {
    let target = body.device.clone();
    let pool_target = if body.unit_id.trim().is_empty() {
        "principal".to_string()
    } else {
        body.unit_id.trim().to_string()
    };
    storage_operation(
        state,
        admin,
        headers,
        "storage_extend_pool",
        target,
        move || {
            nas_engine::storage_ops::extend_pool_target(
                &Runner::new(),
                &pool_target,
                &body.device,
                &body.confirm,
                false,
            )
        },
    )
    .await
}

/// Amplía un pool existente (LVM o Btrfs) usando varios discos.
pub async fn storage_extend_pool_multi(
    State(state): State<AppState>,
    admin: AdminUser,
    headers: HeaderMap,
    axum::Json(body): axum::Json<StorageMultiBody>,
) -> Response {
    let actor = admin.user().username().to_string();

    let devices = body.devices.clone();
    let confirm = body.confirm.clone();
    let pool_target = if body.target.trim().is_empty() {
        "principal".to_string()
    } else {
        body.target.trim().to_string()
    };
    let metadata_raid1 = body.metadata_raid1;
    let target = format!("{pool_target}: {}", devices.join(", "));
    let outcome = state
        .run_heavy_blocking(move || {
            let runner = Runner::new();
            if !nas_engine::storage_ops::confirm_ok(&confirm) {
                return Err("confirmación requerida (SI-FORMATEAR o SI-DESTRUIR-DATOS)".to_string());
            }
            if devices.is_empty() {
                return Err("selecciona al menos un disco".to_string());
            }
            let mut results: Vec<serde_json::Value> = Vec::new();
            let mut failed = false;
            let total = devices.len();
            for (idx, device) in devices.iter().enumerate() {
                let apply_raid1 = metadata_raid1 && idx + 1 == total;
                match nas_engine::storage_ops::extend_pool_target(
                    &runner,
                    &pool_target,
                    device,
                    &confirm,
                    apply_raid1,
                ) {
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
            )
            .await;
            let message = if failed {
                "Ampliación parcial o rechazada: revisa los resultados por disco."
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
            )
            .await;
            error_response(NasError::Invalid(error))
        }
    }
}

/// Crea un pool unificado (1 o N discos) en `principal` o `/srv/nas/unidades/<id>`.
pub async fn storage_create_pool(
    State(state): State<AppState>,
    admin: AdminUser,
    headers: HeaderMap,
    axum::Json(body): axum::Json<CreatePoolBody>,
) -> Response {
    let pool_target = if body.target.trim().is_empty() {
        "principal".to_string()
    } else {
        body.target.trim().to_string()
    };
    let audit_target = format!("{pool_target} ({})", body.mode);
    storage_operation(
        state,
        admin,
        headers,
        "storage_create_pool",
        audit_target,
        move || {
            nas_engine::storage_ops::create_pool(
                &Runner::new(),
                &pool_target,
                &body.mode,
                &body.devices,
                &body.confirm,
                body.unmount,
            )
        },
    )
    .await
}

/// Desmonta una unidad adicional `/srv/nas/unidades/<id>` y la retira de `/etc/fstab`.
pub async fn storage_umount_unit(
    State(state): State<AppState>,
    admin: AdminUser,
    headers: HeaderMap,
    axum::Json(body): axum::Json<UmountUnitBody>,
) -> Response {
    let unit_id = body.unit_id.clone();
    storage_operation(
        state,
        admin,
        headers,
        "storage_umount_unit",
        unit_id.clone(),
        move || nas_engine::storage_ops::umount_unit(&Runner::new(), &unit_id, &body.confirm),
    )
    .await
}

/// Borra firmas y tabla de particiones de un disco no protegido.
pub async fn storage_wipe(
    State(state): State<AppState>,
    admin: AdminUser,
    headers: HeaderMap,
    axum::Json(body): axum::Json<WipeDiskBody>,
) -> Response {
    let target = body.device.clone();
    storage_operation(state, admin, headers, "storage_wipe", target, move || {
        nas_engine::storage_ops::wipe_disk(
            &Runner::new(),
            &body.device,
            &body.confirm,
            body.unmount,
        )
    })
    .await
}

/// Sanea `/etc/fstab` retirando entradas huérfanas y normalizando `nofail`.
pub async fn storage_fstab_sanitize(
    State(state): State<AppState>,
    admin: AdminUser,
    headers: HeaderMap,
    axum::Json(body): axum::Json<SanitizeFstabBody>,
) -> Response {
    storage_operation(
        state,
        admin,
        headers,
        "storage_fstab_sanitize",
        "/etc/fstab".to_string(),
        move || nas_engine::storage_ops::sanitize_fstab(&Runner::new(), &body.confirm),
    )
    .await
}

/// Retira un dispositivo miembro de un pool Btrfs multi-disco en línea.
pub async fn storage_remove_device(
    State(state): State<AppState>,
    admin: AdminUser,
    headers: HeaderMap,
    axum::Json(body): axum::Json<RemovePoolDeviceBody>,
) -> Response {
    let pool_target = if body.target.trim().is_empty() {
        "principal".to_string()
    } else {
        body.target.trim().to_string()
    };
    let target = format!("{pool_target}: {}", body.device);
    storage_operation(
        state,
        admin,
        headers,
        "storage_remove_device",
        target,
        move || {
            nas_engine::storage_ops::remove_pool_device(
                &Runner::new(),
                &pool_target,
                &body.device,
                &body.confirm,
            )
        },
    )
    .await
}
