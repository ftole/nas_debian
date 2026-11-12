use axum::extract::{Query, State};
use axum::http::HeaderMap;
use axum::response::{IntoResponse, Response};
use nas_core::{ApiResponse, NasError};
use nas_exec::Runner;
use serde::Deserialize;

use super::backup_action_id;
use crate::extractors::{AdminUser, AuthenticatedUser};
use crate::routes::common::{
    audit_ip, bad_request, error_response, forbidden, server_error, ExtQuery,
};
use crate::AppState;

#[derive(Debug, Deserialize, Default)]
pub struct BackupPermanentQuery {
    pub task: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
pub struct FreezeBody {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub tag: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
pub struct RestoreBackupBody {
    pub id: String,
    pub snapshot: String,
    #[serde(default)]
    pub target: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
pub struct DeletePermanentBody {
    pub id: String,
    pub name: String,
}

/// Ejecuta una tarea de respaldo en segundo plano.
pub async fn backup_run_id(
    State(state): State<AppState>,
    admin: AdminUser,
    headers: HeaderMap,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> Response {
    let actor = admin.user().username().to_string();
    if !state.role.has_backups() {
        return forbidden("el rol ARCHIVOS no expone tareas de respaldo");
    }
    let Some(id) = backup_action_id(&id) else {
        return bad_request("identificador de tarea inválido");
    };
    let task = id.clone();
    let outcome =
        tokio::task::spawn_blocking(move || nas_engine::backup_control::run(&Runner::new(), &task))
            .await
            .unwrap_or_else(|_| Err("ejecución interrumpida".to_string()));

    match outcome {
        Ok(message) => {
            audit_ip(
                &state,
                &headers,
                &actor,
                "backup_run",
                &id,
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
                &actor,
                "backup_run",
                &id,
                "FAILED",
                &error,
            );
            error_response(NasError::Invalid(error))
        }
    }
}

/// Detiene la ejecución de una tarea de respaldo.
pub async fn backup_stop_id(
    State(state): State<AppState>,
    admin: AdminUser,
    headers: HeaderMap,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> Response {
    let actor = admin.user().username().to_string();
    if !state.role.has_backups() {
        return forbidden("el rol ARCHIVOS no expone tareas de respaldo");
    }
    let Some(id) = backup_action_id(&id) else {
        return bad_request("identificador de tarea inválido");
    };
    let task = id.clone();
    let outcome = tokio::task::spawn_blocking(move || {
        nas_engine::backup_control::stop(&Runner::new(), &task)
    })
    .await
    .unwrap_or_else(|_| Err("detención interrumpida".to_string()));

    match outcome {
        Ok(message) => {
            audit_ip(
                &state,
                &headers,
                &actor,
                "backup_stop",
                &id,
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
                &actor,
                "backup_stop",
                &id,
                "FAILED",
                &error,
            );
            error_response(NasError::Invalid(error))
        }
    }
}

/// Devuelve las últimas líneas de la bitácora de una tarea.
pub async fn backup_logs_id(
    _user: AuthenticatedUser,
    axum::extract::Path(id): axum::extract::Path<String>,
    Query(query): Query<ExtQuery>,
) -> Response {
    let Some(id) = backup_action_id(&id) else {
        return bad_request("identificador de tarea inválido");
    };
    let lines = query.limit.unwrap_or(200);
    let task = id.clone();
    let outcome = tokio::task::spawn_blocking(move || {
        nas_engine::backup_control::logs(&Runner::new(), &task, lines)
    })
    .await
    .unwrap_or_else(|_| Err("lectura de bitácora interrumpida".to_string()));

    match outcome {
        Ok(output) => axum::Json(ApiResponse::ok(
            serde_json::json!({ "id": id, "output": output, "logs": output }),
            "Bitácora de la tarea",
        ))
        .into_response(),
        Err(error) => error_response(NasError::Invalid(error)),
    }
}

/// Lista los respaldos permanentes inmutables (Golden Snapshots).
pub async fn backups_permanent(
    State(state): State<AppState>,
    _user: AuthenticatedUser,
    Query(query): Query<BackupPermanentQuery>,
) -> Response {
    if !state.role.has_backups() {
        return forbidden("el rol ARCHIVOS no expone respaldos permanentes");
    }
    let task = query.task;
    match tokio::task::spawn_blocking(move || {
        nas_engine::helpers::backup::list_permanent_backups(task.as_deref())
    })
    .await
    {
        Ok(Ok(list)) => axum::Json(ApiResponse::ok(list, "Respaldos permanentes")).into_response(),
        Ok(Err(error)) => error_response(error),
        Err(_) => server_error("consulta interrumpida"),
    }
}

/// Congela el último snapshot de una tarea como respaldo permanente (por ID en URL).
pub async fn backup_freeze_id(
    State(state): State<AppState>,
    admin: AdminUser,
    headers: HeaderMap,
    axum::extract::Path(id): axum::extract::Path<String>,
    axum::Json(body): axum::Json<FreezeBody>,
) -> Response {
    let actor = admin.user().username().to_string();
    if !state.role.has_backups() {
        return forbidden("el rol ARCHIVOS no expone tareas de respaldo");
    }
    let Some(id) = backup_action_id(&id) else {
        return bad_request("identificador de tarea inválido");
    };
    let tag = body.tag;
    let task = id.clone();
    let outcome = tokio::task::spawn_blocking(move || {
        nas_engine::backup_control::freeze(&Runner::new(), &task, tag.as_deref())
    })
    .await
    .unwrap_or_else(|_| Err("congelación interrumpida".to_string()));

    match outcome {
        Ok(msg) => {
            audit_ip(
                &state,
                &headers,
                &actor,
                "backup_freeze",
                &id,
                "SUCCESS",
                &msg,
            );
            axum::Json(ApiResponse::ok(serde_json::json!({ "message": msg }), msg)).into_response()
        }
        Err(err) => {
            audit_ip(
                &state,
                &headers,
                &actor,
                "backup_freeze",
                &id,
                "FAILED",
                &err,
            );
            error_response(NasError::Invalid(err))
        }
    }
}

/// Congela el último snapshot de una tarea como respaldo permanente (por cuerpo JSON).
pub async fn backup_freeze(
    State(state): State<AppState>,
    admin: AdminUser,
    headers: HeaderMap,
    axum::Json(body): axum::Json<FreezeBody>,
) -> Response {
    let actor = admin.user().username().to_string();
    if !state.role.has_backups() {
        return forbidden("el rol ARCHIVOS no expone tareas de respaldo");
    }
    let Some(id) = backup_action_id(&body.id) else {
        return bad_request("identificador de tarea inválido");
    };
    let tag = body.tag;
    let task = id.clone();
    let outcome = tokio::task::spawn_blocking(move || {
        nas_engine::backup_control::freeze(&Runner::new(), &task, tag.as_deref())
    })
    .await
    .unwrap_or_else(|_| Err("congelación interrumpida".to_string()));

    match outcome {
        Ok(msg) => {
            audit_ip(
                &state,
                &headers,
                &actor,
                "backup_freeze",
                &id,
                "SUCCESS",
                &msg,
            );
            axum::Json(ApiResponse::ok(serde_json::json!({ "message": msg }), msg)).into_response()
        }
        Err(err) => {
            audit_ip(
                &state,
                &headers,
                &actor,
                "backup_freeze",
                &id,
                "FAILED",
                &err,
            );
            error_response(NasError::Invalid(err))
        }
    }
}

/// Restaura un snapshot o respaldo permanente hacia el origen de red o destino local.
pub async fn backup_restore(
    State(state): State<AppState>,
    admin: AdminUser,
    headers: HeaderMap,
    axum::Json(body): axum::Json<RestoreBackupBody>,
) -> Response {
    let actor = admin.user().username().to_string();
    if !state.role.has_backups() {
        return forbidden("el rol ARCHIVOS no expone tareas de respaldo");
    }
    let Some(id) = backup_action_id(&body.id) else {
        return bad_request("identificador de tarea inválido");
    };
    let snap = body.snapshot;
    let tgt = body.target;
    let task = id.clone();
    let outcome = tokio::task::spawn_blocking(move || {
        nas_engine::backup_control::restore(&Runner::new(), &task, &snap, tgt.as_deref())
    })
    .await
    .unwrap_or_else(|_| Err("restauración interrumpida".to_string()));

    match outcome {
        Ok(msg) => {
            audit_ip(
                &state,
                &headers,
                &actor,
                "backup_restore",
                &id,
                "SUCCESS",
                &msg,
            );
            axum::Json(ApiResponse::ok(serde_json::json!({ "message": msg }), msg)).into_response()
        }
        Err(err) => {
            audit_ip(
                &state,
                &headers,
                &actor,
                "backup_restore",
                &id,
                "FAILED",
                &err,
            );
            error_response(NasError::Invalid(err))
        }
    }
}

/// Elimina un respaldo permanente inmutable.
pub async fn backup_permanent_delete(
    State(state): State<AppState>,
    admin: AdminUser,
    headers: HeaderMap,
    axum::Json(body): axum::Json<DeletePermanentBody>,
) -> Response {
    let actor = admin.user().username().to_string();
    if !state.role.has_backups() {
        return forbidden("el rol ARCHIVOS no expone tareas de respaldo");
    }
    let Some(id) = backup_action_id(&body.id) else {
        return bad_request("identificador de tarea inválido");
    };
    let name = body.name;
    let task = id.clone();
    let outcome = tokio::task::spawn_blocking(move || {
        nas_engine::backup_control::delete_permanent(&Runner::new(), &task, &name)
    })
    .await
    .unwrap_or_else(|_| Err("eliminación interrumpida".to_string()));

    match outcome {
        Ok(msg) => {
            audit_ip(
                &state,
                &headers,
                &actor,
                "backup_permanent_delete",
                &id,
                "SUCCESS",
                &msg,
            );
            axum::Json(ApiResponse::ok(serde_json::json!({ "message": msg }), msg)).into_response()
        }
        Err(err) => {
            audit_ip(
                &state,
                &headers,
                &actor,
                "backup_permanent_delete",
                &id,
                "FAILED",
                &err,
            );
            error_response(NasError::Invalid(err))
        }
    }
}
