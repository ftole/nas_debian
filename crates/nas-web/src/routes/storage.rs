use axum::{
    extract::State,
    http::HeaderMap,
    response::{IntoResponse, Response},
};
use nas_core::NasError;
use nas_exec::Runner;
use serde::Deserialize;

use crate::{
    AppState,
    routes::common::{
        ApiResponse, audit_ip, error_response, forbidden, is_admin, require_auth, run_json,
        server_error,
    },
};

/// Cuerpo para inspeccionar un dispositivo.
#[derive(Debug, Deserialize)]
pub struct DeviceBody {
    pub device: String,
}

/// Cuerpo de una acción de almacenamiento.
#[derive(Debug, Deserialize)]
pub struct StorageActionBody {
    #[serde(default)]
    pub device: String,
    #[serde(default)]
    pub disk: String,
    #[serde(default)]
    pub fstype: String,
    #[serde(default)]
    pub confirm: String,
    #[serde(default)]
    pub unmount: bool,
    #[serde(default)]
    pub unit_id: String,
    #[serde(default)]
    pub subvolume: String,
    #[serde(default)]
    pub vg: String,
    #[serde(default)]
    pub lv: String,
    #[serde(default)]
    pub size: String,
}

fn collect_storage_payload(paths: &nas_core::NasPaths) -> Result<serde_json::Value, NasError> {
    let runner = Runner::new();
    let overview = nas_engine::storage::overview_for(paths, &runner).unwrap_or_else(|_| {
        nas_engine::storage::StorageOverview {
            path: paths.data_root.to_string_lossy().to_string(),
            filesystem: "no montado".to_string(),
            device: None,
            total_gb: 0.0,
            used_gb: 0.0,
            free_gb: 0.0,
            usage_percent: 0.0,
            is_btrfs: false,
            supports_trim: false,
            device_type: "Desconocido".to_string(),
        }
    });
    let disks = nas_engine::storage::disks(&runner).unwrap_or_default();
    let disks: Vec<serde_json::Value> = disks
        .iter()
        .map(|disk| {
            let mut row = serde_json::to_value(disk).unwrap_or_default();
            let is_part = disk.children_count == 0
                && (disk.fstype.as_deref().is_some_and(|fs| !fs.is_empty())
                    || disk.name.chars().last().is_some_and(|c| c.is_ascii_digit()));
            row["type"] = serde_json::json!(if is_part { "part" } else { "disk" });
            row["mount"] = serde_json::json!(disk.mountpoint.clone().unwrap_or_default());
            row["children_count"] = serde_json::json!(disk.children_count);
            row["is_os_disk"] = serde_json::json!(disk.protected);
            row
        })
        .collect();
    Ok(serde_json::json!({
        "overview": overview,
        "disks": disks,
    }))
}

/// Resumen del volumen de datos.
pub async fn storage(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if let Err(response) = require_auth(&state, &headers) {
        return *response;
    }

    let paths = state.paths.clone();
    match tokio::task::spawn_blocking(move || collect_storage_payload(&paths)).await {
        Ok(Ok(payload)) => axum::Json(ApiResponse::ok(payload, "Almacenamiento")).into_response(),
        Ok(Err(error)) => error_response(error),
        Err(_) => server_error("consulta de almacenamiento interrumpida"),
    }
}

/// Discos físicos detectados y resumen de capacidad (contrato consumido por `loadStorage()`).
pub async fn storage_disks(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if let Err(response) = require_auth(&state, &headers) {
        return *response;
    }

    let paths = state.paths.clone();
    match tokio::task::spawn_blocking(move || collect_storage_payload(&paths)).await {
        Ok(Ok(payload)) => {
            axum::Json(ApiResponse::ok(payload, "Discos detectados")).into_response()
        }
        Ok(Err(error)) => error_response(error),
        Err(_) => server_error("inventario de discos interrumpido"),
    }
}

/// Inspección de un dispositivo concreto.
pub async fn storage_inspect(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::Json(body): axum::Json<DeviceBody>,
) -> Response {
    let user = match require_auth(&state, &headers) {
        Ok(user) => user,
        Err(response) => return *response,
    };
    if !is_admin(&state, &user) {
        return forbidden("se requieren privilegios de administrador");
    }

    let device = body.device;
    match tokio::task::spawn_blocking(move || nas_engine::storage::inspect(&Runner::new(), &device))
        .await
    {
        Ok(Ok(payload)) => {
            axum::Json(ApiResponse::ok(payload, "Inspección de disco")).into_response()
        }
        Ok(Err(error)) => error_response(error),
        Err(_) => server_error("inspección interrumpida"),
    }
}

pub async fn storage_units(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if let Err(response) = require_auth(&state, &headers) {
        return *response;
    }
    let paths = state.paths.clone();
    match tokio::task::spawn_blocking(move || {
        let runner = Runner::new();
        let mut units = Vec::new();
        if let Ok(overview) = nas_engine::storage::overview_for(&paths, &runner) {
            units.push(serde_json::json!({
                "id": "principal",
                "name": format!("Unidad Principal ({})", overview.path),
                "path": overview.path,
                "is_main": true,
                "filesystem": overview.filesystem,
                "total_gb": overview.total_gb,
                "used_gb": overview.used_gb,
                "free_gb": overview.free_gb,
                "usage_percent": overview.usage_percent,
            }));
        }

        let extra_dir = paths.data_root.join("unidades");
        if let Ok(entries) = std::fs::read_dir(&extra_dir) {
            let mut dirs: Vec<std::path::PathBuf> = entries
                .flatten()
                .map(|entry| entry.path())
                .filter(|path| path.is_dir())
                .collect();
            dirs.sort();
            for unit_path in dirs {
                let Some(id) = unit_path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .filter(|id| nas_engine::storage_ops::valid_name(id))
                else {
                    continue;
                };
                let mut unit_paths = (*paths).clone();
                unit_paths.data_root = unit_path.clone();
                if let Ok(overview) = nas_engine::storage::overview_for(&unit_paths, &runner) {
                    units.push(serde_json::json!({
                        "id": id,
                        "name": format!("Unidad {id} ({})", overview.path),
                        "path": overview.path,
                        "is_main": false,
                        "filesystem": overview.filesystem,
                        "total_gb": overview.total_gb,
                        "used_gb": overview.used_gb,
                        "free_gb": overview.free_gb,
                        "usage_percent": overview.usage_percent,
                    }));
                }
            }
        }
        units
    })
    .await
    {
        Ok(units) => axum::Json(ApiResponse::ok(
            serde_json::json!({ "units": units }),
            "Unidades de almacenamiento",
        ))
        .into_response(),
        Err(_) => server_error("consulta de unidades interrumpida"),
    }
}

/// Subvolúmenes Btrfs (vacío si el filesystem no es Btrfs).
pub async fn storage_subvolumes(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if let Err(response) = require_auth(&state, &headers) {
        return *response;
    }
    let paths = state.paths.clone();
    match tokio::task::spawn_blocking(move || {
        let runner = Runner::new();
        let is_btrfs = nas_engine::storage::overview_for(&paths, &runner)
            .map(|o| o.is_btrfs)
            .unwrap_or(false);
        if !is_btrfs {
            return Vec::<serde_json::Value>::new();
        }
        let target = paths.data_root.to_string_lossy().to_string();
        runner
            .run(
                &nas_exec::CommandSpec::new("btrfs")
                    .args(["subvolume", "list", &target])
                    .sudo(nas_exec::SudoMode::Passwordless)
                    .timeout(std::time::Duration::from_secs(15)),
            )
            .ok()
            .filter(|out| out.is_success())
            .map(|out| {
                out.stdout
                    .lines()
                    .filter_map(|line| {
                        let (_, rel) = line.split_once(" path ")?;
                        let id = line.split_whitespace().nth(1).unwrap_or_default();
                        Some(serde_json::json!({
                            "id": id,
                            "name": rel.trim(),
                            "path": format!("{}/{}", target.trim_end_matches('/'), rel.trim()),
                        }))
                    })
                    .collect()
            })
            .unwrap_or_default()
    })
    .await
    {
        Ok(subvolumes) => axum::Json(ApiResponse::ok(
            serde_json::json!({ "subvolumes": subvolumes }),
            "Subvolúmenes",
        ))
        .into_response(),
        Err(_) => server_error("consulta de subvolúmenes interrumpida"),
    }
}

/// Estado del scrub Btrfs (responde `200` degradado si `/srv/nas` no es Btrfs).
pub async fn storage_scrub(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if let Err(response) = require_auth(&state, &headers) {
        return *response;
    }

    let paths = state.paths.clone();
    match tokio::task::spawn_blocking(move || {
        let runner = Runner::new();
        let is_btrfs = nas_engine::storage::overview_for(&paths, &runner)
            .map(|o| o.is_btrfs)
            .unwrap_or(false);
        if !is_btrfs {
            return serde_json::json!({
                "running": false,
                "status": "no disponible",
                "errors": 0,
                "success": true,
                "output": "El volumen principal no utiliza Btrfs.",
            });
        }
        let target = paths.data_root.to_string_lossy().to_string();
        match run_json(
            nas_exec::CommandSpec::new("btrfs")
                .args(["scrub", "status", &target])
                .sudo(nas_exec::SudoMode::Passwordless)
                .timeout(std::time::Duration::from_secs(15)),
        ) {
            Ok(text) => {
                let status = nas_engine::parsers::scrub::parse_scrub_status(&text);
                let mut payload = serde_json::to_value(&status).unwrap_or_default();
                payload["success"] = serde_json::json!(true);
                payload["output"] = serde_json::json!(text);
                payload
            }
            Err(error) => serde_json::json!({
                "running": false,
                "status": "no disponible",
                "errors": 0,
                "success": true,
                "output": error,
            }),
        }
    })
    .await
    {
        Ok(payload) => axum::Json(ApiResponse::ok(payload, "Estado de scrub")).into_response(),
        Err(_) => server_error("consulta de scrub interrumpida"),
    }
}

/// Inicia el scrub Btrfs.
pub async fn storage_scrub_start(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let user = match require_auth(&state, &headers) {
        Ok(user) => user,
        Err(response) => return *response,
    };
    if !is_admin(&state, &user) {
        return forbidden("se requieren privilegios de administrador");
    }
    let target = state.paths.data_root.to_string_lossy().to_string();
    match tokio::task::spawn_blocking(move || {
        run_json(
            nas_exec::CommandSpec::new("btrfs")
                .args(["scrub", "start", &target])
                .sudo(nas_exec::SudoMode::Passwordless)
                .timeout(std::time::Duration::from_secs(30)),
        )
    })
    .await
    {
        Ok(Ok(text)) => axum::Json(ApiResponse::ok(
            serde_json::json!({ "output": text }),
            "Scrub iniciado",
        ))
        .into_response(),
        Ok(Err(error)) => server_error(&error),
        Err(_) => server_error("inicio de scrub interrumpido"),
    }
}

/// Ejecuta fstrim sobre el volumen.
pub async fn storage_trim(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let user = match require_auth(&state, &headers) {
        Ok(user) => user,
        Err(response) => return *response,
    };
    if !is_admin(&state, &user) {
        return forbidden("se requieren privilegios de administrador");
    }
    let target = state.paths.data_root.to_string_lossy().to_string();
    match tokio::task::spawn_blocking(move || {
        run_json(
            nas_exec::CommandSpec::new("fstrim")
                .args(["-v", &target])
                .sudo(nas_exec::SudoMode::Passwordless)
                .timeout(std::time::Duration::from_secs(120)),
        )
    })
    .await
    {
        Ok(Ok(text)) => axum::Json(ApiResponse::ok(
            serde_json::json!({ "output": text }),
            "TRIM ejecutado",
        ))
        .into_response(),
        Ok(Err(error)) => server_error(&error),
        Err(_) => server_error("TRIM interrumpido"),
    }
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
