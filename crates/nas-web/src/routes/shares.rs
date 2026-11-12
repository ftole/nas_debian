use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Router;
use nas_core::{ApiResponse, NasError};
use nas_engine::parsers::smb::parse_shares;
use nas_exec::Runner;
use serde::Deserialize;

use super::common::{
    audit_ip, bad_request, error_response, forbidden, is_admin, require_auth, server_error,
    ExtQuery,
};
use crate::AppState;

#[derive(Debug, Deserialize)]
pub struct ShareBody {
    pub name: String,
    #[serde(default)]
    pub comment: String,
    #[serde(default)]
    pub scheme: u8,
    #[serde(default)]
    pub hidden: bool,
    #[serde(default)]
    pub unit: String,
    #[serde(default)]
    pub groups: Vec<String>,
    #[serde(default)]
    pub write_group: String,
}

#[derive(Debug, Deserialize)]
pub struct ShareNameBody {
    pub name: String,
}

#[derive(Debug, Deserialize, Default)]
pub struct ShareRepairBody {
    #[serde(default)]
    pub share: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub force: bool,
}

#[derive(Debug, Deserialize)]
pub struct ShareAccessBody {
    pub share: String,
    #[serde(default)]
    pub kind: String,
    pub name: String,
    pub level: String,
}

pub fn share_path(name: &str, unit: &str) -> String {
    if unit.is_empty() || unit == "principal" {
        format!("/srv/nas/{name}")
    } else {
        format!("/srv/nas/unidades/{unit}/{name}")
    }
}

pub fn normalize_group(group: &str) -> String {
    let trimmed = group.trim();
    if trimmed.starts_with('@') {
        trimmed.to_string()
    } else {
        format!("@{trimmed}")
    }
}

pub fn share_spec(body: &ShareBody, path: &str) -> nas_engine::samba::ShareSpec {
    let groups: Vec<String> = body.groups.iter().map(|g| normalize_group(g)).collect();
    let write = if body.write_group.trim().is_empty() {
        None
    } else {
        Some(normalize_group(&body.write_group))
    };

    let read_only = body.scheme == 3;
    let guest_ok = body.scheme == 4;
    let write_list = if body.scheme == 2 {
        write.into_iter().collect()
    } else if body.scheme == 1 {
        groups.clone()
    } else {
        Vec::new()
    };

    nas_engine::samba::ShareSpec {
        name: body.name.trim().to_string(),
        path: path.to_string(),
        read_only,
        browseable: !body.hidden,
        guest_ok,
        valid_users: if guest_ok { Vec::new() } else { groups },
        write_list: if read_only || guest_ok {
            Vec::new()
        } else {
            write_list
        },
        comment: body.comment.clone(),
    }
}

pub fn validate_share_spec(spec: &nas_engine::samba::ShareSpec) -> Result<(), NasError> {
    nas_engine::samba::validate_name(&spec.name)?;
    nas_engine::samba::validate_path(&spec.path)?;
    nas_engine::samba::validate_comment(&spec.comment)?;
    for entry in spec.valid_users.iter().chain(spec.write_list.iter()) {
        nas_engine::samba::validate_principal(entry)?;
    }
    Ok(())
}

pub fn stage_and_apply(conf: &str) -> Result<(), String> {
    let stamp = chrono::Local::now().format("%Y%m%d%H%M%S%3f");
    let dir = std::path::Path::new("/var/lib/nas/staging");
    std::fs::create_dir_all(dir).map_err(|error| error.to_string())?;
    let staged = dir.join(format!("smbconf_{stamp}"));
    std::fs::write(&staged, conf).map_err(|error| error.to_string())?;

    let staged_str = staged.to_string_lossy().to_string();
    let output = Runner::new()
        .run(
            &nas_exec::CommandSpec::new("/usr/local/bin/nas")
                .args(["helper", "samba-apply", &staged_str])
                .sudo(nas_exec::SudoMode::Passwordless)
                .timeout(std::time::Duration::from_secs(60)),
        )
        .map_err(|error| error.to_string())?;

    if output.is_success() {
        Ok(())
    } else {
        let _ = std::fs::remove_file(&staged);
        Err(if output.stderr.is_empty() {
            output.stdout
        } else {
            output.stderr
        })
    }
}

pub fn apply_acl(path: &str, principal: &str, level: &str) -> Result<(), String> {
    let output = Runner::new()
        .run(
            &nas_exec::CommandSpec::new("/usr/local/bin/nas")
                .args([
                    "helper",
                    "share-acl",
                    path,
                    principal.trim_start_matches('@'),
                    level,
                ])
                .sudo(nas_exec::SudoMode::Passwordless)
                .timeout(std::time::Duration::from_secs(120)),
        )
        .map_err(|error| error.to_string())?;

    if output.is_success() {
        Ok(())
    } else {
        Err(if output.stderr.is_empty() {
            output.stdout
        } else {
            output.stderr
        })
    }
}

pub fn create_share_dir(name: &str, unit: &str) -> Result<(), String> {
    let output = Runner::new()
        .run(
            &nas_exec::CommandSpec::new("/usr/local/bin/nas")
                .args([
                    "helper",
                    "share-dir",
                    name,
                    if unit.is_empty() { "principal" } else { unit },
                ])
                .sudo(nas_exec::SudoMode::Passwordless)
                .timeout(std::time::Duration::from_secs(30)),
        )
        .map_err(|error| error.to_string())?;

    if output.is_success() {
        Ok(())
    } else {
        Err(if output.stderr.is_empty() {
            output.stdout
        } else {
            output.stderr
        })
    }
}

pub fn apply_spec_acls(spec: &nas_engine::samba::ShareSpec) {
    for group in &spec.valid_users {
        let level = if spec.write_list.contains(group) {
            "write"
        } else {
            "read"
        };
        if apply_acl(&spec.path, group, level).is_err() {
            eprintln!("aviso: no se pudo aplicar ACL de {group} en {}", spec.path);
        }
    }
    for group in spec
        .write_list
        .iter()
        .filter(|entry| !spec.valid_users.contains(entry))
    {
        if apply_acl(&spec.path, group, "write").is_err() {
            eprintln!("aviso: no se pudo aplicar ACL de {group} en {}", spec.path);
        }
    }
}

/// Revoca las ACLs POSIX de los principais retirados de un recurso.
pub fn revoke_removed_acls(
    previous: &nas_engine::parsers::smb::Share,
    spec: &nas_engine::samba::ShareSpec,
) {
    let keep: std::collections::HashSet<&str> = spec
        .valid_users
        .iter()
        .chain(spec.write_list.iter())
        .map(|entry| entry.trim_start_matches('@'))
        .chain(std::iter::once("grp_samba"))
        .collect();

    let mut removed: Vec<String> = Vec::new();
    for entry in previous
        .valid_users
        .iter()
        .chain(previous.write_list.iter())
    {
        let name = entry.trim_start_matches('@').to_string();
        if !name.is_empty() && !keep.contains(name.as_str()) && !removed.contains(&name) {
            removed.push(name);
        }
    }

    for name in removed {
        let ok = apply_acl(&spec.path, &name, "none");
        if ok.is_err() {
            eprintln!(
                "aviso: no se pudo revocar la ACL de {name} en {}",
                spec.path
            );
        }
    }
}

pub fn acl_entries(runner: &Runner, path: &str) -> Option<String> {
    runner
        .run(
            &nas_exec::CommandSpec::new("getfacl")
                .args(["-p", path])
                .timeout(std::time::Duration::from_secs(15)),
        )
        .ok()
        .filter(|output| output.is_success())
        .map(|output| output.stdout)
}

pub fn acl_has(text: &str, entry: &str, is_group: bool, perms: &str, default: bool) -> bool {
    let prefix = match (default, is_group) {
        (true, true) => "default:group:",
        (false, true) => "group:",
        (true, false) => "default:user:",
        (false, false) => "user:",
    };
    text.lines().any(|line| {
        let line = line.trim();
        line.strip_prefix(prefix)
            .and_then(|rest| rest.split_once(':'))
            .is_some_and(|(name, value)| name == entry && value.trim().starts_with(perms))
    })
}

/// Recursos compartidos declarados en `smb.conf`.
pub async fn shares(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if let Err(response) = require_auth(&state, &headers) {
        return *response;
    }

    if !state.role.has_shares() {
        return (
            StatusCode::FORBIDDEN,
            axum::Json(ApiResponse::<serde_json::Value>::error(
                "el rol BACKUP no expone recursos compartidos",
            )),
        )
            .into_response();
    }

    let path = state.paths.samba_conf.clone();
    match tokio::task::spawn_blocking(move || std::fs::read_to_string(&path)).await {
        Ok(Ok(text)) => {
            let shares: Vec<serde_json::Value> = parse_shares(&text)
                .into_iter()
                .map(|share| {
                    let scheme_name = if share.guest_ok {
                        "Acceso público / Invitados"
                    } else if share.read_only {
                        "Solo lectura estricta"
                    } else if !share.write_list.is_empty() {
                        "Lectura general + escritura exclusiva"
                    } else {
                        "Lectura y escritura por grupo"
                    };
                    serde_json::json!({
                        "name": share.name,
                        "path": share.path,
                        "read_only": share.read_only,
                        "browseable": share.browseable,
                        "hidden": !share.browseable,
                        "guest_ok": share.guest_ok,
                        "valid_users": share.valid_users,
                        "write_list": share.write_list,
                        "comment": share.comment,
                        "scheme_name": scheme_name,
                        "scheme": if share.guest_ok { 4 } else if share.read_only { 3 } else if !share.write_list.is_empty() { 2 } else { 1 },
                    })
                })
                .collect();
            axum::Json(ApiResponse::ok(shares, "Recursos compartidos")).into_response()
        }
        Ok(Err(error)) => server_error(&format!("no se pudo leer smb.conf: {error}")),
        Err(_) => server_error("lectura de smb.conf interrumpida"),
    }
}

pub async fn shares_access(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if let Err(response) = require_auth(&state, &headers) {
        return *response;
    }
    if !state.role.has_shares() {
        return forbidden("el rol BACKUP no expone recursos compartidos");
    }
    let path = state.paths.samba_conf.clone();

    match tokio::task::spawn_blocking(move || {
        let conf = std::fs::read_to_string(&path).unwrap_or_default();
        let parsed = nas_engine::parsers::smb::parse_shares(&conf);
        let shares: Vec<String> = parsed.iter().map(|share| share.name.clone()).collect();

        let level_for =
            |share: &nas_engine::parsers::smb::Share, principal: &str| -> &'static str {
                let entry = if principal.starts_with('@') {
                    principal.to_string()
                } else {
                    format!("@{principal}")
                };
                if share.guest_ok {
                    "read"
                } else if share
                    .write_list
                    .iter()
                    .any(|item| item == &entry || item == principal)
                {
                    "write"
                } else if share
                    .valid_users
                    .iter()
                    .any(|item| item == &entry || item == principal)
                {
                    "read"
                } else {
                    "none"
                }
            };

        let (mut users, groups) = nas_engine::users::load_local();
        users.retain(|user| !user.system);
        let admin_members: std::collections::HashSet<String> = groups
            .iter()
            .filter(|group| matches!(group.name.as_str(), "sudo" | "grp_samba" | "grp_superadmin"))
            .flat_map(|group| group.members.clone())
            .collect();
        let groups: Vec<serde_json::Value> = groups
            .into_iter()
            .filter(|group| group.name.starts_with("grp_"))
            .map(|group| {
                let mut row = serde_json::Map::new();
                row.insert("name".into(), serde_json::json!(group.name));
                for share in &parsed {
                    row.insert(
                        share.name.clone(),
                        serde_json::json!(level_for(share, &format!("@{}", group.name))),
                    );
                }
                serde_json::Value::Object(row)
            })
            .collect();
        let users: Vec<serde_json::Value> = users
            .into_iter()
            .map(|user| {
                let mut row = serde_json::Map::new();
                row.insert("name".into(), serde_json::json!(user.name));
                let is_admin = admin_members.contains(&user.name);
                row.insert("is_admin".into(), serde_json::json!(is_admin));
                for share in &parsed {
                    row.insert(
                        share.name.clone(),
                        serde_json::json!(level_for(share, &user.name)),
                    );
                }
                serde_json::Value::Object(row)
            })
            .collect();
        serde_json::json!({ "shares": shares, "groups": groups, "users": users })
    })
    .await
    {
        Ok(payload) => axum::Json(ApiResponse::ok(payload, "Mapa de acceso")).into_response(),
        Err(_) => server_error("mapa de acceso interrumpido"),
    }
}

/// Crea un recurso compartido.
pub async fn share_create(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::Json(body): axum::Json<ShareBody>,
) -> Response {
    let user = match require_auth(&state, &headers) {
        Ok(user) => user,
        Err(response) => return *response,
    };
    if !is_admin(&state, &user) {
        return forbidden("se requieren privilegios de administrador");
    }
    if !state.role.has_shares() {
        return forbidden("el rol BACKUP no expone recursos compartidos");
    }
    let unit = body.unit.trim().to_string();
    if !unit.is_empty() && unit != "principal" && !nas_engine::storage_ops::valid_name(&unit) {
        return bad_request("unidad de almacenamiento inválida");
    }

    let path = share_path(body.name.trim(), &unit);
    let spec = share_spec(&body, &path);
    if let Err(error) = validate_share_spec(&spec) {
        return error_response(error);
    }
    let share_name = spec.name.clone();
    let conf_path = state.paths.samba_conf.clone();

    let outcome = tokio::task::spawn_blocking(move || -> Result<(), String> {
        let conf = std::fs::read_to_string(&conf_path).map_err(|e| e.to_string())?;
        let updated =
            nas_engine::samba::upsert_share(&conf, &spec).map_err(|error| error.to_string())?;
        create_share_dir(&spec.name, &unit)?;
        stage_and_apply(&updated)?;
        apply_spec_acls(&spec);
        Ok(())
    })
    .await
    .unwrap_or_else(|_| Err("creación interrumpida".to_string()));

    match outcome {
        Ok(()) => {
            audit_ip(
                &state,
                &headers,
                &user,
                "share_create",
                &share_name,
                "SUCCESS",
                "",
            );
            axum::Json(ApiResponse::<serde_json::Value>::ok_empty("Recurso creado")).into_response()
        }
        Err(error) => {
            audit_ip(
                &state,
                &headers,
                &user,
                "share_create",
                &share_name,
                "FAILED",
                &error,
            );
            server_error(&error)
        }
    }
}

/// Actualiza un recurso compartido existente.
pub async fn share_update(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::Json(body): axum::Json<ShareBody>,
) -> Response {
    let user = match require_auth(&state, &headers) {
        Ok(user) => user,
        Err(response) => return *response,
    };
    if !is_admin(&state, &user) {
        return forbidden("se requieren privilegios de administrador");
    }
    if !state.role.has_shares() {
        return forbidden("el rol BACKUP no expone recursos compartidos");
    }
    if let Err(error) = nas_engine::samba::validate_name(body.name.trim()) {
        return error_response(error);
    }
    if let Err(error) = nas_engine::samba::validate_comment(&body.comment) {
        return error_response(error);
    }

    let share_name = body.name.trim().to_string();
    let conf_path = state.paths.samba_conf.clone();

    let outcome = tokio::task::spawn_blocking(move || -> Result<(), String> {
        let conf = std::fs::read_to_string(&conf_path).map_err(|e| e.to_string())?;
        let existing = nas_engine::parsers::smb::parse_shares(&conf)
            .into_iter()
            .find(|share| share.name.eq_ignore_ascii_case(body.name.trim()))
            .ok_or_else(|| format!("no existe el recurso '{}'", body.name))?;
        let preserved = existing.clone();
        let path = existing
            .path
            .unwrap_or_else(|| share_path(body.name.trim(), "principal"));
        let spec = share_spec(&body, &path);
        validate_share_spec(&spec).map_err(|error| error.to_string())?;
        let updated =
            nas_engine::samba::upsert_share(&conf, &spec).map_err(|error| error.to_string())?;
        stage_and_apply(&updated)?;
        apply_spec_acls(&spec);
        revoke_removed_acls(&preserved, &spec);
        Ok(())
    })
    .await
    .unwrap_or_else(|_| Err("actualización interrumpida".to_string()));

    match outcome {
        Ok(()) => {
            audit_ip(
                &state,
                &headers,
                &user,
                "share_update",
                &share_name,
                "SUCCESS",
                "",
            );
            axum::Json(ApiResponse::<serde_json::Value>::ok_empty(
                "Recurso actualizado",
            ))
            .into_response()
        }
        Err(error) => {
            audit_ip(
                &state,
                &headers,
                &user,
                "share_update",
                &share_name,
                "FAILED",
                &error,
            );
            server_error(&error)
        }
    }
}

/// Elimina un recurso compartido (conserva los archivos).
pub async fn share_delete(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::Json(body): axum::Json<ShareNameBody>,
) -> Response {
    let user = match require_auth(&state, &headers) {
        Ok(user) => user,
        Err(response) => return *response,
    };
    if !is_admin(&state, &user) {
        return forbidden("se requieren privilegios de administrador");
    }
    if !state.role.has_shares() {
        return forbidden("el rol BACKUP no expone recursos compartidos");
    }
    if let Err(error) = nas_engine::samba::validate_name(body.name.trim()) {
        return error_response(error);
    }

    let name = body.name.trim().to_string();
    let conf_path = state.paths.samba_conf.clone();
    let outcome = tokio::task::spawn_blocking(move || -> Result<(), String> {
        let conf = std::fs::read_to_string(&conf_path).map_err(|e| e.to_string())?;
        let updated =
            nas_engine::samba::remove_share(&conf, &name).map_err(|error| error.to_string())?;
        stage_and_apply(&updated)
    })
    .await
    .unwrap_or_else(|_| Err("borrado interrumpido".to_string()));

    match outcome {
        Ok(()) => {
            audit_ip(
                &state,
                &headers,
                &user,
                "share_delete",
                &body.name,
                "SUCCESS",
                "",
            );
            axum::Json(ApiResponse::<serde_json::Value>::ok_empty(
                "Recurso eliminado",
            ))
            .into_response()
        }
        Err(error) => {
            audit_ip(
                &state,
                &headers,
                &user,
                "share_delete",
                &body.name,
                "FAILED",
                &error,
            );
            server_error(&error)
        }
    }
}

/// Ajusta el nivel de acceso de un grupo o usuario sobre un recurso.
pub async fn share_access_update(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::Json(body): axum::Json<ShareAccessBody>,
) -> Response {
    let user = match require_auth(&state, &headers) {
        Ok(user) => user,
        Err(response) => return *response,
    };
    if !is_admin(&state, &user) {
        return forbidden("se requieren privilegios de administrador");
    }
    if !state.role.has_shares() {
        return forbidden("el rol BACKUP no expone recursos compartidos");
    }
    if !matches!(body.level.as_str(), "none" | "read" | "write") {
        return bad_request("nivel de acceso inválido");
    }
    if let Err(error) = nas_engine::samba::validate_name(body.share.trim()) {
        return error_response(error);
    }

    let share_name = body.share.trim().to_string();
    let level_name = body.level.clone();
    let principal = if matches!(body.kind.as_str(), "user" | "users") {
        body.name.trim().to_string()
    } else {
        normalize_group(&body.name)
    };
    if let Err(error) = nas_engine::samba::validate_principal(&principal) {
        return error_response(error);
    }
    let conf_path = state.paths.samba_conf.clone();

    let outcome = tokio::task::spawn_blocking(move || -> Result<(), String> {
        let conf = std::fs::read_to_string(&conf_path).map_err(|e| e.to_string())?;
        let mut shares = nas_engine::parsers::smb::parse_shares(&conf);
        let share = shares
            .iter_mut()
            .find(|share| share.name.eq_ignore_ascii_case(body.share.trim()))
            .ok_or_else(|| format!("no existe el recurso '{}'", body.share))?;

        let path = share
            .path
            .clone()
            .unwrap_or_else(|| share_path(&share.name, "principal"));
        let entry = principal.clone();

        match body.level.as_str() {
            "none" => {
                share.valid_users.retain(|item| item != &entry);
                share.write_list.retain(|item| item != &entry);
            }
            "read" => {
                if !share.valid_users.contains(&entry) {
                    share.valid_users.push(entry.clone());
                }
                share.write_list.retain(|item| item != &entry);
            }
            _ => {
                if !share.valid_users.contains(&entry) {
                    share.valid_users.push(entry.clone());
                }
                if !share.write_list.contains(&entry) {
                    share.write_list.push(entry.clone());
                }
            }
        }

        let spec = nas_engine::samba::ShareSpec {
            name: share.name.clone(),
            path,
            read_only: share.read_only,
            browseable: share.browseable,
            guest_ok: share.guest_ok,
            valid_users: share.valid_users.clone(),
            write_list: share.write_list.clone(),
            comment: share.comment.clone().unwrap_or_default(),
        };
        let updated =
            nas_engine::samba::upsert_share(&conf, &spec).map_err(|error| error.to_string())?;
        stage_and_apply(&updated)?;
        let acl_level = if body.level == "none" {
            "none"
        } else {
            body.level.as_str()
        };
        apply_acl(&spec.path, &principal, acl_level)?;
        Ok(())
    })
    .await
    .unwrap_or_else(|_| Err("cambio de acceso interrumpido".to_string()));

    match outcome {
        Ok(()) => {
            audit_ip(
                &state,
                &headers,
                &user,
                "share_access",
                &share_name,
                "SUCCESS",
                &level_name,
            );
            axum::Json(ApiResponse::<serde_json::Value>::ok_empty(
                "Permiso actualizado",
            ))
            .into_response()
        }
        Err(error) => {
            audit_ip(
                &state,
                &headers,
                &user,
                "share_access",
                &share_name,
                "FAILED",
                &error,
            );
            server_error(&error)
        }
    }
}

/// Reaplica las ACLs de todos los recursos según `smb.conf`.
pub async fn share_access_repair(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::Json(body): axum::Json<ShareRepairBody>,
) -> Response {
    let user = match require_auth(&state, &headers) {
        Ok(user) => user,
        Err(response) => return *response,
    };
    if !is_admin(&state, &user) {
        return forbidden("se requieren privilegios de administrador");
    }
    if !state.role.has_shares() {
        return forbidden("el rol BACKUP no expone recursos compartidos");
    }
    let _ = body.force;
    let target_filter = if body.share.trim().is_empty() {
        body.name.trim().to_string()
    } else {
        body.share.trim().to_string()
    };
    let conf_path = state.paths.samba_conf.clone();

    let outcome = tokio::task::spawn_blocking(move || -> Result<usize, String> {
        let conf = std::fs::read_to_string(&conf_path).map_err(|e| e.to_string())?;
        let shares = nas_engine::parsers::smb::parse_shares(&conf);
        let mut applied = 0;
        for share in shares {
            if !target_filter.is_empty() && !share.name.eq_ignore_ascii_case(&target_filter) {
                continue;
            }
            let Some(path) = share.path.clone() else {
                continue;
            };
            for group in &share.valid_users {
                let level = if share.write_list.contains(group) {
                    "write"
                } else {
                    "read"
                };
                if apply_acl(&path, group, level).is_ok() {
                    applied += 1;
                }
            }
            for group in &share.write_list {
                if !share.valid_users.contains(group) && apply_acl(&path, group, "write").is_ok() {
                    applied += 1;
                }
            }
        }
        Ok(applied)
    })
    .await
    .unwrap_or_else(|_| Err("reparación interrumpida".to_string()));

    match outcome {
        Ok(applied) => {
            audit_ip(
                &state,
                &headers,
                &user,
                "share_acl_repair",
                "shares",
                "SUCCESS",
                "",
            );
            axum::Json(ApiResponse::ok(
                serde_json::json!({ "applied": applied }),
                "ACLs reaplicadas",
            ))
            .into_response()
        }
        Err(error) => {
            audit_ip(
                &state,
                &headers,
                &user,
                "share_acl_repair",
                "shares",
                "FAILED",
                &error,
            );
            server_error(&error)
        }
    }
}

/// Audita las ACL reales de cada recurso contra `smb.conf`.
pub async fn shares_access_audit(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<ExtQuery>,
) -> Response {
    if let Err(response) = require_auth(&state, &headers) {
        return *response;
    }
    if !state.role.has_shares() {
        return forbidden("el rol BACKUP no expone recursos compartidos");
    }
    let path = state.paths.samba_conf.clone();
    let only = query
        .share
        .or(query.path)
        .filter(|value| !value.trim().is_empty());

    match tokio::task::spawn_blocking(move || {
        let conf = std::fs::read_to_string(&path).unwrap_or_default();
        let runner = Runner::new();
        let mut shares = serde_json::Map::new();
        let mut out_of_sync = 0;

        for share in parse_shares(&conf) {
            if let Some(only) = &only {
                if !share.name.eq_ignore_ascii_case(only.trim()) {
                    continue;
                }
            }
            let path = share.path.clone().unwrap_or_default();
            let mut discrepancies = Vec::new();
            let acl = acl_entries(&runner, &path);

            let mut expected: Vec<(String, bool, &'static str)> = Vec::new();
            for entry in &share.valid_users {
                let is_group = entry.starts_with('@');
                let name = entry.trim_start_matches('@').to_string();
                expected.push((name, is_group, "r-x"));
            }
            for entry in &share.write_list {
                let is_group = entry.starts_with('@');
                let name = entry.trim_start_matches('@').to_string();
                if let Some(item) = expected
                    .iter_mut()
                    .find(|(n, g, _)| *n == name && *g == is_group)
                {
                    item.2 = "rwx";
                } else {
                    expected.push((name, is_group, "rwx"));
                }
            }
            if !expected
                .iter()
                .any(|(name, is_group, _)| *is_group && name == "grp_samba")
            {
                expected.push(("grp_samba".to_string(), true, "rwx"));
            }

            match &acl {
                None => discrepancies.push(format!("no se pudo leer la ACL de {path}")),
                Some(text) => {
                    for (name, is_group, perms) in &expected {
                        let kind = if *is_group { "group" } else { "user" };
                        if !acl_has(text, name, *is_group, perms, false) {
                            discrepancies.push(format!("falta {kind}:{name}:{perms}"));
                        }
                        if !acl_has(text, name, *is_group, perms, true) {
                            discrepancies.push(format!("falta default:{kind}:{name}:{perms}"));
                        }
                    }
                }
            }

            if !discrepancies.is_empty() {
                out_of_sync += 1;
            }
            shares.insert(
                share.name.clone(),
                serde_json::json!({
                    "path": path,
                    "in_sync": discrepancies.is_empty(),
                    "discrepancies": discrepancies,
                }),
            );
        }

        let total = shares.len();
        serde_json::json!({
            "total": total,
            "out_of_sync_count": out_of_sync,
            "shares": shares,
        })
    })
    .await
    {
        Ok(payload) => axum::Json(ApiResponse::ok(payload, "Auditoría de ACL")).into_response(),
        Err(_) => server_error("auditoría de ACL interrumpida"),
    }
}

/// Router del dominio de recursos compartidos.
pub fn router() -> Router<crate::AppState> {
    use axum::routing::{get, post};
    Router::new()
        .route("/api/shares", get(shares).post(share_create))
        .route("/api/shares/update", post(share_update))
        .route("/api/shares/delete", post(share_delete))
        .route(
            "/api/shares/access",
            get(shares_access).post(share_access_update),
        )
        .route("/api/shares/access/repair", post(share_access_repair))
        .route("/api/shares/access/audit", get(shares_access_audit))
}
