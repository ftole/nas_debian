use axum::extract::{Query, State};
use axum::http::HeaderMap;
use axum::response::{IntoResponse, Response};
use axum::Router;
use nas_core::{ApiResponse, NasError};
use nas_exec::Runner;
use serde::Deserialize;

use super::common::{
    audit_ip, bad_request, error_response, forbidden, is_admin, require_auth, server_error,
    ExtQuery,
};
use crate::AppState;

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

pub fn last_snapshot(task: &nas_engine::tasks::BackupTask) -> Option<String> {
    last_snapshot_path(task).map(|path| path.display().to_string())
}

pub fn last_snapshot_path(task: &nas_engine::tasks::BackupTask) -> Option<std::path::PathBuf> {
    let entries = std::fs::read_dir(task.task_dir()).ok()?;
    let mut snapshots: Vec<std::path::PathBuf> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("snapshot_"))
        })
        .collect();
    snapshots.sort();
    snapshots.pop()
}

pub fn run_backup_helper(args: &[&str], password: Option<&str>) -> Result<String, String> {
    let mut cmd_args = vec!["helper", "backup-manage"];
    cmd_args.extend_from_slice(args);
    let mut spec = nas_exec::CommandSpec::new("/usr/local/bin/nas")
        .args(cmd_args)
        .sudo(nas_exec::SudoMode::Passwordless)
        .timeout(std::time::Duration::from_secs(60));
    if let Some(password) = password {
        spec = spec.stdin_secret(nas_exec::SecretInput::new(password));
    }

    let output = Runner::new()
        .run(&spec)
        .map_err(|error| error.to_string())?;
    if output.is_success() {
        Ok(output.stdout)
    } else {
        let raw = if output.stderr.is_empty() {
            output.stdout
        } else {
            output.stderr
        };
        Err(clean_helper_error(&raw))
    }
}

pub fn clean_helper_error(raw: &str) -> String {
    let trimmed = raw.trim();
    if let Ok(val) = serde_json::from_str::<serde_json::Value>(trimmed) {
        if let Some(msg) = val
            .get("message")
            .and_then(|v| v.as_str())
            .or_else(|| val.get("error").and_then(|v| v.as_str()))
        {
            return sanitize_error_message(msg);
        }
    }
    sanitize_error_message(trimmed)
}

pub fn sanitize_error_message(msg: &str) -> String {
    let clean = if let Some((head, _)) = msg.split_once("\nRefer to the mount.cifs") {
        head.trim()
    } else if let Some((head, _)) = msg.split_once("Refer to the mount.cifs") {
        head.trim()
    } else {
        msg.trim()
    };

    let clean = clean.strip_prefix("operación denegada: ").unwrap_or(clean);

    if clean.contains("mount error(13): Permission denied") {
        "Fallo de conexión CIFS: Acceso denegado (usuario o contraseña incorrectos, o permisos insuficientes en el recurso compartido).".to_string()
    } else if clean.contains("mount error(112): Host is down")
        || clean.contains("mount error(113): No route to host")
    {
        "Fallo de conexión CIFS: Host no disponible (servidor remoto apagado o fuera de red)."
            .to_string()
    } else if clean.contains("mount error(111): Connection refused") {
        "Fallo de conexión CIFS: Conexión rechazada (puerto CIFS/SMB 445 no disponible)."
            .to_string()
    } else if clean.contains("mount error(6): No such device or address")
        || clean.contains("mount error(2): No such file or directory")
    {
        "Fallo de conexión CIFS: Recurso compartido no encontrado en el servidor remoto."
            .to_string()
    } else if clean.contains("mount error(115): Operation now in progress") {
        "Fallo de conexión CIFS: Tiempo de espera agotado al conectar al servidor remoto."
            .to_string()
    } else {
        clean.to_string()
    }
}

pub fn stage_backup_files(
    spec: &nas_engine::backup_spec::BackupSpec,
    proto: nas_engine::backup_spec::Proto,
) -> Result<(), String> {
    let dir = std::path::Path::new("/var/lib/nas/staging");
    std::fs::create_dir_all(dir).map_err(|error| error.to_string())?;
    let id = spec.id.trim();

    std::fs::write(
        dir.join(format!("nas_backup_{id}.sh")),
        spec.render_runner(proto),
    )
    .map_err(|error| error.to_string())?;
    std::fs::write(
        dir.join(format!("nas_backup_{id}.cron")),
        spec.render_cron(),
    )
    .map_err(|error| error.to_string())?;
    let cred = spec.render_cred(proto);
    if !cred.is_empty() {
        std::fs::write(dir.join(format!("nas_backup_{id}.cred")), cred)
            .map_err(|error| error.to_string())?;
    } else {
        let _ = std::fs::remove_file(dir.join(format!("nas_backup_{id}.cred")));
    }
    if let Some(key_content) = spec.render_key(proto) {
        std::fs::write(dir.join(format!("nas_backup_{id}.id_ed25519")), key_content)
            .map_err(|error| error.to_string())?;
    } else {
        let _ = std::fs::remove_file(dir.join(format!("nas_backup_{id}.id_ed25519")));
    }
    Ok(())
}

async fn install_backup(
    state: AppState,
    headers: HeaderMap,
    body: nas_engine::backup_spec::BackupSpec,
    actor_action: &'static str,
) -> Response {
    let actor = match require_auth(&state, &headers) {
        Ok(user) => user,
        Err(response) => return *response,
    };
    if !is_admin(&state, &actor) {
        return forbidden("se requieren privilegios de administrador");
    }
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

pub fn backup_action_id(id: &str) -> Option<String> {
    nas_engine::backup_control::valid_id(id).then(|| id.trim().to_string())
}

/// Lista las tareas de respaldo instaladas con su último snapshot.
pub async fn backups(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let user = match require_auth(&state, &headers) {
        Ok(user) => user,
        Err(response) => return *response,
    };
    if !state.role.has_backups() {
        return forbidden("el rol ARCHIVOS no expone tareas de respaldo");
    }
    let _ = &user;

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
    headers: HeaderMap,
    axum::Json(body): axum::Json<crate::routes::files::PathBody>,
) -> Response {
    if let Err(response) = require_auth(&state, &headers) {
        return *response;
    }

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
    headers: HeaderMap,
    axum::Json(body): axum::Json<BackupRunBody>,
) -> Response {
    let user = match require_auth(&state, &headers) {
        Ok(user) => user,
        Err(response) => return *response,
    };
    if !is_admin(&state, &user) {
        return forbidden("se requieren privilegios de administrador");
    }

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

pub async fn backups_running(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if let Err(response) = require_auth(&state, &headers) {
        return *response;
    }

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
    headers: HeaderMap,
    axum::Json(body): axum::Json<nas_engine::backup_spec::BackupSpec>,
) -> Response {
    install_backup(state, headers, body, "backup_create").await
}

/// Actualiza (reinstala) una tarea de respaldo.
pub async fn backup_update(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::Json(body): axum::Json<nas_engine::backup_spec::BackupSpec>,
) -> Response {
    install_backup(state, headers, body, "backup_update").await
}

/// Elimina una tarea de respaldo (conserva los snapshots).
pub async fn backup_delete(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::Json(body): axum::Json<BackupIdBody>,
) -> Response {
    let actor = match require_auth(&state, &headers) {
        Ok(user) => user,
        Err(response) => return *response,
    };
    if !is_admin(&state, &actor) {
        return forbidden("se requieren privilegios de administrador");
    }
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
    headers: HeaderMap,
    axum::Json(body): axum::Json<BackupTestBody>,
) -> Response {
    let actor = match require_auth(&state, &headers) {
        Ok(user) => user,
        Err(response) => return *response,
    };
    if !is_admin(&state, &actor) {
        return forbidden("se requieren privilegios de administrador");
    }
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

/// Ejecuta una tarea de respaldo en segundo plano.
pub async fn backup_run_id(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> Response {
    let actor = match require_auth(&state, &headers) {
        Ok(user) => user,
        Err(response) => return *response,
    };
    if !is_admin(&state, &actor) {
        return forbidden("se requieren privilegios de administrador");
    }
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
    headers: HeaderMap,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> Response {
    let actor = match require_auth(&state, &headers) {
        Ok(user) => user,
        Err(response) => return *response,
    };
    if !is_admin(&state, &actor) {
        return forbidden("se requieren privilegios de administrador");
    }
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
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::extract::Path(id): axum::extract::Path<String>,
    Query(query): Query<ExtQuery>,
) -> Response {
    if let Err(response) = require_auth(&state, &headers) {
        return *response;
    }
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
    headers: HeaderMap,
    Query(query): Query<BackupPermanentQuery>,
) -> Response {
    if let Err(response) = require_auth(&state, &headers) {
        return *response;
    }
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
    headers: HeaderMap,
    axum::extract::Path(id): axum::extract::Path<String>,
    axum::Json(body): axum::Json<FreezeBody>,
) -> Response {
    let actor = match require_auth(&state, &headers) {
        Ok(user) => user,
        Err(response) => return *response,
    };
    if !is_admin(&state, &actor) {
        return forbidden("se requieren privilegios de administrador");
    }
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
    headers: HeaderMap,
    axum::Json(body): axum::Json<FreezeBody>,
) -> Response {
    let actor = match require_auth(&state, &headers) {
        Ok(user) => user,
        Err(response) => return *response,
    };
    if !is_admin(&state, &actor) {
        return forbidden("se requieren privilegios de administrador");
    }
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
    headers: HeaderMap,
    axum::Json(body): axum::Json<RestoreBackupBody>,
) -> Response {
    let actor = match require_auth(&state, &headers) {
        Ok(user) => user,
        Err(response) => return *response,
    };
    if !is_admin(&state, &actor) {
        return forbidden("se requieren privilegios de administrador");
    }
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
    headers: HeaderMap,
    axum::Json(body): axum::Json<DeletePermanentBody>,
) -> Response {
    let actor = match require_auth(&state, &headers) {
        Ok(user) => user,
        Err(response) => return *response,
    };
    if !is_admin(&state, &actor) {
        return forbidden("se requieren privilegios de administrador");
    }
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
