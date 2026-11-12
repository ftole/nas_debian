use axum::extract::{Query, State};
use axum::http::HeaderMap;
use axum::response::{IntoResponse, Response};
use nas_core::ApiResponse;
use nas_engine::parsers::smb::parse_shares;
use nas_exec::Runner;
use serde::Deserialize;

use super::{apply_acl, normalize_group, share_path, stage_and_apply};
use crate::extractors::{AdminUser, AuthenticatedUser};
use crate::routes::common::{
    audit_ip, bad_request, error_response, forbidden, server_error, ExtQuery,
};
use crate::AppState;

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

pub async fn shares_access(State(state): State<AppState>, _user: AuthenticatedUser) -> Response {
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

/// Ajusta el nivel de acceso de un grupo o usuario sobre un recurso.
pub async fn share_access_update(
    State(state): State<AppState>,
    admin: AdminUser,
    headers: HeaderMap,
    axum::Json(body): axum::Json<ShareAccessBody>,
) -> Response {
    let user = admin.user().username().to_string();
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
    admin: AdminUser,
    headers: HeaderMap,
    axum::Json(body): axum::Json<ShareRepairBody>,
) -> Response {
    let user = admin.user().username().to_string();
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
    _user: AuthenticatedUser,
    Query(query): Query<ExtQuery>,
) -> Response {
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
