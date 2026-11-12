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
    sanitize_error_message, stage_backup_files,
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

    let proto = match body.validate() {
        Ok(proto) => proto,
        Err(error) => return error_response(error),
    };
    let id = body.id.trim().to_string();

    if let Err(error) = stage_backup_files(&body, proto) {
        return server_error(&error);
    }

    let helper_id = id.clone();
    let result =
        tokio::task::spawn_blocking(move || run_backup_helper(&["install", &helper_id], None))
            .await
            .unwrap_or_else(|_| Err("instalación interrumpida".to_string()));

    match result {
        Ok(_) => {
            audit_ip(&state, &headers, &actor, actor_action, &id, "SUCCESS", "");
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
    match tokio::task::spawn_blocking(move || {
        let tasks = nas_engine::tasks::BackupTask::discover(&dir).unwrap_or_default();
        let handles: Vec<_> = tasks
            .into_iter()
            .map(|task| {
                std::thread::Builder::new()
                    .name(format!("backup-row-{}", task.id))
                    .spawn(move || {
                        let mut disk_cache = std::collections::HashMap::new();
                        let last = last_snapshot(&task);
                        let mut row =
                            crate::legacy::backup_row_with_metrics(&task, &mut disk_cache);
                        row["task"] = serde_json::json!(task);
                        row["last_snapshot"] = serde_json::json!(last);
                        row
                    })
            })
            .collect();
        handles
            .into_iter()
            .filter_map(Result::ok)
            .map(|joined| joined.join().unwrap_or(serde_json::json!({})))
            .collect::<Vec<_>>()
    })
    .await
    {
        Ok(list) => axum::Json(ApiResponse::ok(list, "Tareas de respaldo")).into_response(),
        Err(_) => server_error("consulta de respaldos interrumpida"),
    }
}

/// Plan de ejecución de una tarea de respaldo.
pub async fn backup_plan(
    State(state): State<AppState>,
    _user: AuthenticatedUser,
    axum::Json(body): axum::Json<crate::routes::files::PathBody>,
) -> Response {
    let dir = state.backup_dir.clone();
    let id = body.path;
    match tokio::task::spawn_blocking(move || {
        let tasks = nas_engine::tasks::BackupTask::discover(&dir)?;
        let task = tasks
            .into_iter()
            .find(|task| task.id == id)
            .ok_or_else(|| nas_core::NasError::NotFound(format!("tarea '{id}' no encontrada")))?;

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
    let outcome =
        tokio::task::spawn_blocking(move || nas_engine::backup_control::run(&Runner::new(), &id))
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
            );
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
            );
            error_response(nas_core::NasError::Invalid(error))
        }
    }
}

pub async fn backups_running(_user: AuthenticatedUser) -> Response {
    match tokio::task::spawn_blocking(|| {
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
    axum::Json(body): axum::Json<nas_engine::backup_spec::BackupSpec>,
) -> Response {
    install_backup(state, admin, headers, body, "backup_create").await
}

/// Actualiza (reinstala) una tarea de respaldo.
pub async fn backup_update(
    State(state): State<AppState>,
    admin: AdminUser,
    headers: HeaderMap,
    axum::Json(body): axum::Json<nas_engine::backup_spec::BackupSpec>,
) -> Response {
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
    let result = tokio::task::spawn_blocking(move || run_backup_helper(&["delete", &id], None))
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
            );
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

    let result = tokio::task::spawn_blocking(move || {
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
