mod ops;

use axum::{
    extract::{Query, State},
    response::{IntoResponse, Response},
    Router,
};
use nas_core::NasError;
use nas_exec::Runner;
use serde::Deserialize;

use crate::{
    extractors::{AdminUser, AuthenticatedUser},
    routes::common::{error_response, run_json, server_error, ApiResponse},
    AppState,
};

pub use ops::{
    storage_add_unit, storage_create_pool, storage_extend_pool, storage_extend_pool_multi,
    storage_format, storage_fstab_sanitize, storage_lvm, storage_remove_device, storage_subvolume,
    storage_umount_unit, storage_wipe,
};

/// Parámetro opcional `?unit=` para scrub o TRIM sobre unidades adicionales.
#[derive(Debug, Deserialize, Default)]
pub struct UnitQuery {
    #[serde(default)]
    pub unit: Option<String>,
}

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
            data_root_on_os_disk: true,
            is_readonly: false,
            pool_devices: Vec::new(),
            data_profile: None,
            metadata_profile: None,
        }
    });
    let lsblk_devs = nas_engine::storage::lsblk_devices_with_uuid(&runner).unwrap_or_default();
    let btrfs_pools = nas_engine::storage::btrfs_pools_from_lsblk(&lsblk_devs);
    let fstab_audit = nas_engine::storage::audit_system_fstab(&runner);
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
        "btrfs_pools": btrfs_pools,
        "fstab_audit": fstab_audit,
    }))
}

/// Resumen del volumen de datos.
pub async fn storage(State(state): State<AppState>, _user: AuthenticatedUser) -> Response {
    let paths = state.paths.clone();
    match state
        .run_system_blocking(move || collect_storage_payload(&paths))
        .await
    {
        Ok(Ok(payload)) => axum::Json(ApiResponse::ok(payload, "Almacenamiento")).into_response(),
        Ok(Err(error)) => error_response(error),
        Err(_) => server_error("consulta de almacenamiento interrumpida"),
    }
}

/// Discos físicos detectados y resumen de capacidad (contrato consumido por `loadStorage()`).
pub async fn storage_disks(State(state): State<AppState>, _user: AuthenticatedUser) -> Response {
    let paths = state.paths.clone();
    match state
        .run_system_blocking(move || collect_storage_payload(&paths))
        .await
    {
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
    _admin: AdminUser,
    axum::Json(body): axum::Json<DeviceBody>,
) -> Response {
    let device = body.device;
    match state
        .run_heavy_blocking(move || nas_engine::storage::inspect(&Runner::new(), &device))
        .await
    {
        Ok(Ok(payload)) => {
            axum::Json(ApiResponse::ok(payload, "Inspección de disco")).into_response()
        }
        Ok(Err(error)) => error_response(error),
        Err(_) => server_error("inspección interrumpida"),
    }
}

pub async fn storage_units(State(state): State<AppState>, _user: AuthenticatedUser) -> Response {
    let paths = state.paths.clone();
    match state
        .run_system_blocking(move || {
            let runner = Runner::new();
            let mut units = Vec::new();
            let mut main_on_os = false;
            if let Ok(overview) = nas_engine::storage::overview_for(&paths, &runner) {
                main_on_os = overview.data_root_on_os_disk;
                units.push(serde_json::json!({
                    "id": "principal",
                    "name": format!("Unidad Principal ({})", overview.path),
                    "path": overview.path,
                    "is_main": true,
                    "device": overview.device,
                    "filesystem": overview.filesystem,
                    "total_gb": overview.total_gb,
                    "used_gb": overview.used_gb,
                    "free_gb": overview.free_gb,
                    "usage_percent": overview.usage_percent,
                    "is_btrfs": overview.is_btrfs,
                    "supports_trim": overview.supports_trim,
                    "is_readonly": overview.is_readonly,
                    "data_root_on_os_disk": overview.data_root_on_os_disk,
                    "pool_devices": overview.pool_devices,
                    "data_profile": overview.data_profile,
                    "metadata_profile": overview.metadata_profile,
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
                    if !nas_engine::storage::is_mountpoint(&unit_path) {
                        continue;
                    }
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
                            "device": overview.device,
                            "filesystem": overview.filesystem,
                            "total_gb": overview.total_gb,
                            "used_gb": overview.used_gb,
                            "free_gb": overview.free_gb,
                            "usage_percent": overview.usage_percent,
                            "is_btrfs": overview.is_btrfs,
                            "supports_trim": overview.supports_trim,
                            "is_readonly": overview.is_readonly,
                            "data_root_on_os_disk": overview.data_root_on_os_disk,
                            "pool_devices": overview.pool_devices,
                            "data_profile": overview.data_profile,
                            "metadata_profile": overview.metadata_profile,
                        }));
                    }
                }
            }
            (units, main_on_os)
        })
        .await
    {
        Ok((units, main_on_os)) => axum::Json(ApiResponse::ok(
            serde_json::json!({
                "units": units,
                "data_root_on_os_disk": main_on_os,
            }),
            "Unidades de almacenamiento",
        ))
        .into_response(),
        Err(_) => server_error("consulta de unidades interrumpida"),
    }
}

/// Subvolúmenes Btrfs (vacío si el filesystem no es Btrfs).
pub async fn storage_subvolumes(
    State(state): State<AppState>,
    _user: AuthenticatedUser,
) -> Response {
    let paths = state.paths.clone();
    match state
        .run_system_blocking(move || {
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

fn normalized_unit_id(raw: Option<&str>) -> Option<String> {
    let trimmed = raw.unwrap_or("").trim();
    if trimmed.is_empty() || trimmed == "principal" {
        None
    } else {
        Some(trimmed.to_string())
    }
}

/// Estado del scrub Btrfs (responde `200` degradado si el volumen no es Btrfs).
pub async fn storage_scrub(
    State(state): State<AppState>,
    _user: AuthenticatedUser,
    Query(query): Query<UnitQuery>,
) -> Response {
    let paths = state.paths.clone();
    let unit = normalized_unit_id(query.unit.as_deref());
    if let Some(ref u) = unit {
        if !nas_engine::storage_ops::valid_name(u) {
            return error_response(NasError::Invalid("identificador de unidad inválido".into()));
        }
    }
    match state
        .run_system_blocking(move || {
            let runner = Runner::new();
            let mut unit_paths = (*paths).clone();
            if let Some(ref u) = unit {
                unit_paths.data_root = paths.data_root.join("unidades").join(u);
            }
            let is_btrfs = nas_engine::storage::overview_for(&unit_paths, &runner)
                .map(|o| o.is_btrfs)
                .unwrap_or(false);
            if !is_btrfs {
                return serde_json::json!({
                    "running": false,
                    "status": "no disponible",
                    "errors": 0,
                    "success": true,
                    "output": "El volumen seleccionado no utiliza Btrfs.",
                });
            }
            let res = if let Some(ref u) = unit {
                nas_engine::storage_ops::unit_scrub_status(&runner, u)
            } else {
                let target = paths.data_root.to_string_lossy().to_string();
                run_json(
                    nas_exec::CommandSpec::new("btrfs")
                        .args(["scrub", "status", &target])
                        .sudo(nas_exec::SudoMode::Passwordless)
                        .timeout(std::time::Duration::from_secs(15)),
                )
            };
            match res {
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

/// Inicia el scrub Btrfs en `/srv/nas` o una unidad `/srv/nas/unidades/<id>`.
pub async fn storage_scrub_start(
    State(state): State<AppState>,
    _admin: AdminUser,
    Query(query): Query<UnitQuery>,
) -> Response {
    let unit = normalized_unit_id(query.unit.as_deref());
    if let Some(ref u) = unit {
        if !nas_engine::storage_ops::valid_name(u) {
            return error_response(NasError::Invalid("identificador de unidad inválido".into()));
        }
    }
    let target = state.paths.data_root.to_string_lossy().to_string();
    match state
        .run_heavy_blocking(move || {
            if let Some(ref u) = unit {
                nas_engine::storage_ops::unit_scrub_start(&Runner::new(), u)
            } else {
                run_json(
                    nas_exec::CommandSpec::new("btrfs")
                        .args(["scrub", "start", &target])
                        .sudo(nas_exec::SudoMode::Passwordless)
                        .timeout(std::time::Duration::from_secs(30)),
                )
            }
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

/// Ejecuta fstrim sobre `/srv/nas` o una unidad `/srv/nas/unidades/<id>`.
pub async fn storage_trim(
    State(state): State<AppState>,
    _admin: AdminUser,
    Query(query): Query<UnitQuery>,
) -> Response {
    let unit = normalized_unit_id(query.unit.as_deref());
    if let Some(ref u) = unit {
        if !nas_engine::storage_ops::valid_name(u) {
            return error_response(NasError::Invalid("identificador de unidad inválido".into()));
        }
    }
    let target = state.paths.data_root.to_string_lossy().to_string();
    match state
        .run_heavy_blocking(move || {
            if let Some(ref u) = unit {
                nas_engine::storage_ops::unit_trim(&Runner::new(), u)
            } else {
                run_json(
                    nas_exec::CommandSpec::new("fstrim")
                        .args(["-v", &target])
                        .sudo(nas_exec::SudoMode::Passwordless)
                        .timeout(std::time::Duration::from_secs(120)),
                )
            }
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

/// Router del dominio de almacenamiento.
pub fn router() -> Router<crate::AppState> {
    use axum::routing::{get, post};
    Router::new()
        .route("/api/storage", get(storage))
        .route("/api/storage/disks", get(storage_disks))
        .route("/api/storage/inspect", post(storage_inspect))
        .route("/api/storage/format", post(storage_format))
        .route("/api/storage/lvm", post(storage_lvm))
        .route("/api/storage/subvolume", post(storage_subvolume))
        .route("/api/storage/add-unit", post(storage_add_unit))
        .route("/api/storage/umount-unit", post(storage_umount_unit))
        .route("/api/storage/create-pool", post(storage_create_pool))
        .route("/api/storage/remove-device", post(storage_remove_device))
        .route("/api/storage/wipe", post(storage_wipe))
        .route("/api/storage/fstab-sanitize", post(storage_fstab_sanitize))
        .route("/api/storage/extend-pool", post(storage_extend_pool))
        .route(
            "/api/storage/extend-pool-multi",
            post(storage_extend_pool_multi),
        )
        .route("/api/storage/units", get(storage_units))
        .route("/api/storage/subvolumes", get(storage_subvolumes))
        .route(
            "/api/storage/scrub",
            get(storage_scrub).post(storage_scrub_start),
        )
        .route("/api/storage/trim", post(storage_trim))
}
