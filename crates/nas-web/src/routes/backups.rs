mod actions;
mod helpers;

use axum::extract::State;
use axum::http::HeaderMap;
use axum::response::{IntoResponse, Response};
use axum::Router;
use nas_core::ApiResponse;
use nas_exec::Runner;
use serde::Deserialize;

use super::common::{audit_ip, bad_request, error_response, forbidden, server_error};
use crate::extractors::{AdminUser, AuthenticatedUser};
use crate::AppState;

pub use actions::{
    backup_freeze, backup_freeze_id, backup_logs_id, backup_permanent_delete, backup_restore,
    backup_run_id, backup_stop_id, backups_permanent,
};
pub use helpers::{
    backup_action_id, clean_helper_error, last_snapshot, last_snapshot_path, run_backup_helper,
    sanitize_error_message,
};

#[derive(Debug, Deserialize)]
pub struct BackupRunBody {
    pub path: String,
    #[serde(default)]
    pub confirm: bool,
}

#[derive(Debug, Deserialize)]
pub struct BackupTestBody {
    #[serde(default)]
    pub proto: String,
    #[serde(default)]
    pub ip: String,
    #[serde(default)]
    pub share: String,
    #[serde(default)]
    pub path: String,
    #[serde(default)]
    pub port: Option<u16>,
    #[serde(default)]
    pub user: String,
    #[serde(default)]
    pub password: String,
    #[serde(default, alias = "key_file", alias = "key_path", alias = "private_key")]
    pub ssh_key: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct BackupIdBody {
    pub id: String,
}

async fn install_backup(
    state: AppState,
    admin: AdminUser,
    headers: HeaderMap,
    body: nas_engine::backup_spec::BackupSpec,
    actor_action: &'static str,
) -> Response {
    let actor = admin.user().username().to_string();
    if !state.role.has_backups() {
        return forbidden("el rol ARCHIVOS no expone tareas de respaldo");
    }

    if let Err(error) = body.validate() {
        return error_response(error);
    }
    let id = body.id.trim().to_string();
    let spec_json = match serde_json::to_string(&body) {
        Ok(json) => json,
        Err(error) => return server_error(&error.to_string()),
    };

    let result = state
        .run_system_blocking(move || run_backup_helper(&["install-spec"], Some(&spec_json)))
        .await
        .unwrap_or_else(|_| Err("instalación interrumpida".to_string()));

    match result {
        Ok(_) => {
            audit_ip(&state, &headers, &actor, actor_action, &id, "SUCCESS", "").await;
            axum::Json(ApiResponse::<serde_json::Value>::ok_empty(format!(
                "Tarea [{id}] programada"
            )))
            .into_response()
        }
        Err(error) => server_error(&error),
    }
}

/// Lista las tareas de respaldo instaladas con su último snapshot.
pub async fn backups(State(state): State<AppState>, _user: AuthenticatedUser) -> Response {
    if !state.role.has_backups() {
        return forbidden("el rol ARCHIVOS no expone tareas de respaldo");
    }

    let dir = state.backup_dir.clone();
    let discovered = match state
        .run_system_blocking(move || {
            let tasks = nas_engine::tasks::BackupTask::discover(&dir).unwrap_or_default();
            let running_ids = crate::legacy::running_backup_task_ids();
            let mut disk_cache = std::collections::HashMap::new();
            for task in &tasks {
                disk_cache
                    .entry(task.destination_root.clone())
                    .or_insert_with(|| {
                        crate::legacy::calculate_destination_disk(&task.destination_root)
                    });
            }
            (tasks, running_ids, disk_cache)
        })
        .await
    {
        Ok(tuple) => tuple,
        Err(_) => return server_error("consulta de respaldos interrumpida"),
    };

    let (tasks, running_ids, disk_cache) = discovered;
    let running_shared = std::sync::Arc::new(running_ids);
    let disk_shared = std::sync::Arc::new(disk_cache);

    let mut handles = Vec::with_capacity(tasks.len());
    for (idx, task) in tasks.into_iter().enumerate() {
        let sem = state.system_ops.clone();
        let running_ref = running_shared.clone();
        let disk_ref = disk_shared.clone();
        handles.push(tokio::spawn(async move {
            let _permit = sem.acquire_owned().await.ok();
            let row = tokio::task::spawn_blocking(move || {
                let mut local_disk = std::collections::HashMap::new();
                if let Some(cached) = disk_ref.get(&task.destination_root) {
                    local_disk.insert(task.destination_root.clone(), cached.clone());
                }
                let last = last_snapshot(&task);
                let mut row = crate::legacy::backup_row_with_context(
                    &task,
                    &mut local_disk,
                    Some(running_ref.as_ref()),
                );
                row["task"] = serde_json::json!(task);
                row["last_snapshot"] = serde_json::json!(last);
                row
            })
            .await
            .unwrap_or_else(|_| serde_json::json!({}));
            (idx, row)
        }));
    }

    let mut indexed = Vec::with_capacity(handles.len());
    for handle in handles {
        if let Ok(pair) = handle.await {
            indexed.push(pair);
        }
    }
    indexed.sort_by_key(|(idx, _)| *idx);
    let list: Vec<serde_json::Value> = indexed.into_iter().map(|(_, row)| row).collect();
    axum::Json(ApiResponse::ok(list, "Tareas de respaldo")).into_response()
}

/// Plan de ejecución de una tarea de respaldo.
pub async fn backup_plan(
    State(state): State<AppState>,
    _user: AuthenticatedUser,
    axum::Json(body): axum::Json<crate::routes::files::PathBody>,
) -> Response {
    let dir = state.backup_dir.clone();
    let id = body.path;
    match state
        .run_system_blocking(move || {
            let tasks = nas_engine::tasks::BackupTask::discover(&dir)?;
            let task = tasks.into_iter().find(|task| task.id == id).ok_or_else(|| {
                nas_core::NasError::NotFound(format!("tarea '{id}' no encontrada"))
            })?;

            let problems = task.validate();
            if !problems.is_empty() {
                return Err(nas_core::NasError::Invalid(problems.join("; ")));
            }

            let previous = last_snapshot_path(&task);
            let plan = task.plan(previous.as_deref(), &chrono::Local::now());
            let steps: Vec<String> = plan
                .iter()
                .map(|step| nas_core::shell::quote_all(step))
                .collect();

            Ok::<_, nas_core::NasError>(serde_json::json!({
                "task": task,
                "last_snapshot": previous.map(|path| path.display().to_string()),
                "steps": steps,
            }))
        })
        .await
    {
        Ok(Ok(payload)) => axum::Json(ApiResponse::ok(payload, "Plan de respaldo")).into_response(),
        Ok(Err(error)) => error_response(error),
        Err(_) => server_error("planificación interrumpida"),
    }
}

/// Ejecuta una tarea de respaldo con las guardas del motor.
pub async fn backup_run(
    State(state): State<AppState>,
    admin: AdminUser,
    headers: HeaderMap,
    axum::Json(body): axum::Json<BackupRunBody>,
) -> Response {
    let user = admin.user().username().to_string();

    if !nas_engine::backup_control::valid_id(&body.path) {
        return bad_request("identificador de tarea inválido");
    }
    if !body.confirm {
        return forbidden("la ejecución real exige confirm: true");
    }

    let id = body.path.clone();
    let outcome = state
        .run_heavy_blocking(move || nas_engine::backup_control::run(&Runner::new(), &id))
        .await
        .unwrap_or_else(|_| Err("ejecución interrumpida".to_string()));

    match outcome {
        Ok(message) => {
            audit_ip(
                &state,
                &headers,
                &user,
                "backup_run",
                &body.path,
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
            audit_ip(
                &state,
                &headers,
                &user,
                "backup_run",
                &body.path,
                "FAILED",
                &error,
            )
            .await;
            error_response(nas_core::NasError::Invalid(error))
        }
    }
}

pub async fn backups_running(State(state): State<AppState>, _user: AuthenticatedUser) -> Response {
    match state
        .run_system_blocking(|| {
            let output = Runner::new().run(
                &nas_exec::CommandSpec::new("pgrep")
                    .args(["-af", "backup_|rsync"])
                    .timeout(std::time::Duration::from_secs(10)),
            );
            let mut tasks: Vec<String> = Vec::new();
            if let Ok(output) = output {
                for line in output.stdout.lines() {
                    if let Some(name) = line.split("backup_").nth(1) {
                        let id = name
                            .split(|c: char| !c.is_ascii_alphanumeric() && c != '-' && c != '_')
                            .next()
                            .unwrap_or("");
                        if !id.is_empty() && !tasks.iter().any(|t| t == id) {
                            tasks.push(id.to_string());
                        }
                    }
                }
            }
            let running = !tasks.is_empty();
            serde_json::json!({ "running": running, "tasks": tasks })
        })
        .await
    {
        Ok(payload) => {
            axum::Json(ApiResponse::ok(payload, "Respaldos en ejecución")).into_response()
        }
        Err(_) => server_error("consulta de respaldos interrumpida"),
    }
}

/// Crea una tarea de respaldo.
pub async fn backup_create(
    State(state): State<AppState>,
    admin: AdminUser,
    headers: HeaderMap,
    axum::Json(mut body): axum::Json<nas_engine::backup_spec::BackupSpec>,
) -> Response {
    body.preserve_existing_auth = false;
    install_backup(state, admin, headers, body, "backup_create").await
}

/// Actualiza (reinstala) una tarea de respaldo.
pub async fn backup_update(
    State(state): State<AppState>,
    admin: AdminUser,
    headers: HeaderMap,
    axum::Json(mut body): axum::Json<nas_engine::backup_spec::BackupSpec>,
) -> Response {
    body.preserve_existing_auth = true;
    install_backup(state, admin, headers, body, "backup_update").await
}

/// Elimina una tarea de respaldo (conserva los snapshots).
pub async fn backup_delete(
    State(state): State<AppState>,
    admin: AdminUser,
    headers: HeaderMap,
    axum::Json(body): axum::Json<BackupIdBody>,
) -> Response {
    let actor = admin.user().username().to_string();
    if !state.role.has_backups() {
        return forbidden("el rol ARCHIVOS no expone tareas de respaldo");
    }
    if body.id.is_empty()
        || !body
            .id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        return bad_request("identificador inválido");
    }

    let id = body.id.clone();
    let result = state
        .run_system_blocking(move || run_backup_helper(&["delete", &id], None))
        .await
        .unwrap_or_else(|_| Err("borrado interrumpido".to_string()));

    match result {
        Ok(_) => {
            audit_ip(
                &state,
                &headers,
                &actor,
                "backup_delete",
                &body.id,
                "SUCCESS",
                "",
            )
            .await;
            axum::Json(ApiResponse::<serde_json::Value>::ok_empty(
                "Tarea eliminada",
            ))
            .into_response()
        }
        Err(error) => server_error(&error),
    }
}

/// Prueba la conectividad del origen de una tarea.
pub async fn backup_test(
    State(state): State<AppState>,
    _admin: AdminUser,
    axum::Json(body): axum::Json<BackupTestBody>,
) -> Response {
    if !state.role.has_backups() {
        return forbidden("el rol ARCHIVOS no expone tareas de respaldo");
    }

    let proto = body.proto.clone();
    let host = body.ip.clone();
    let target = if body.proto == "cifs" {
        body.share.clone()
    } else {
        body.path.clone()
    };
    let port = body.port.unwrap_or(22).to_string();
    let user_name = body.user.clone();
    let auth = if let Some(key) = body.ssh_key.filter(|k| !k.trim().is_empty()) {
        key
    } else {
        body.password.clone()
    };

    let result = state
        .run_heavy_blocking(move || {
            run_backup_helper(
                &["test", &proto, &host, &target, &port, &user_name],
                Some(&auth),
            )
        })
        .await
        .unwrap_or_else(|_| Err("prueba interrumpida".to_string()));

    match result {
        Ok(output) => axum::Json(ApiResponse::ok(
            serde_json::json!({ "output": output }),
            "Conexión verificada",
        ))
        .into_response(),
        Err(error) => server_error(&error),
    }
}

/// Router del dominio de respaldos.
pub fn router() -> Router<crate::AppState> {
    use axum::routing::{get, post};
    Router::new()
        .route("/api/backups", get(backups).post(backup_create))
        .route("/api/backups/plan", post(backup_plan))
        .route("/api/backups/run", post(backup_run))
        .route("/api/backups/running", get(backups_running))
        .route("/api/backups/{id}/run", post(backup_run_id))
        .route("/api/backups/{id}/stop", post(backup_stop_id))
        .route("/api/backups/{id}/logs", get(backup_logs_id))
        .route("/api/backups/test", post(backup_test))
        .route("/api/backups/update", post(backup_update))
        .route("/api/backups/delete", post(backup_delete))
        .route("/api/backups/permanent", get(backups_permanent))
        .route("/api/backups/{id}/freeze", post(backup_freeze_id))
        .route("/api/backups/freeze", post(backup_freeze))
        .route("/api/backups/restore", post(backup_restore))
        .route(
            "/api/backups/permanent/delete",
            post(backup_permanent_delete),
        )
        .route("/api/backups/", post(backup_create))
}
