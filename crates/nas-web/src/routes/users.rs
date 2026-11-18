mod groups;

use std::collections::HashMap;

use axum::extract::State;
use axum::http::HeaderMap;
use axum::response::{IntoResponse, Response};
use axum::Router;
use nas_core::ApiResponse;
use nas_exec::Runner;
use serde::Deserialize;

use super::common::{audit_ip, bad_request, configured_superadmin, forbidden, server_error};
use crate::AppState;

pub use groups::{group_create, group_delete, group_rename, groups, user_groups};

#[derive(Debug, Deserialize)]
pub struct UserBody {
    pub username: String,
    #[serde(default)]
    pub full_name: String,
    #[serde(default)]
    pub password: String,
    #[serde(default)]
    pub groups: Vec<String>,
    #[serde(default)]
    pub is_admin: bool,
    #[serde(default)]
    pub can_web: bool,
    #[serde(default)]
    pub samba_enabled: bool,
    #[serde(default)]
    pub is_superadmin: bool,
}

#[derive(Debug, Deserialize)]
pub struct ToggleUserBody {
    pub username: String,
    #[serde(default)]
    pub enabled: bool,
}

#[derive(Debug, Deserialize)]
pub struct MemberBody {
    pub username: String,
    pub group: String,
    #[serde(default)]
    pub action: String,
}

pub fn valid_username(name: &str) -> bool {
    let mut chars = name.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_lowercase() || c == '_')
        && name.len() <= 32
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
}

pub fn valid_grp(name: &str) -> bool {
    name.starts_with("grp_")
        && name.len() <= 32
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-')
}

pub fn is_master_grp(name: &str) -> bool {
    matches!(
        name,
        "grp_samba" | "grp_web" | "grp_superadmin" | "grp_backups"
    )
}

pub fn is_protected_posix_account(username: &str) -> bool {
    let (users, _) = nas_engine::users::load_local();
    let found = users.into_iter().find(|u| u.name == username);
    let uid = found.as_ref().map(|u| u.uid);
    if nas_engine::users::is_protected_system_account(username, uid) {
        return true;
    }
    found.is_some_and(|u| u.system)
}

pub fn is_target_superadmin(state: &AppState, username: &str) -> bool {
    if configured_superadmin().as_deref() == Some(username) {
        return true;
    }
    if state
        .auth
        .db()
        .user_role(username)
        .ok()
        .flatten()
        .as_deref()
        == Some("superadmin")
    {
        return true;
    }
    let (_, groups) = nas_engine::users::load_local();
    groups
        .into_iter()
        .find(|g| g.name == "grp_superadmin")
        .is_some_and(|g| g.members.iter().any(|m| m == username))
}

pub fn run_accounts(args: &[&str], password: Option<&str>) -> Result<String, String> {
    super::common::run_nas_helper("accounts", args, password, 60)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct AccountRuntimeStatus {
    enabled: bool,
    is_samba: bool,
    samba_enabled: bool,
}

fn parse_accounts_status_tsv(tsv: &str) -> HashMap<String, AccountRuntimeStatus> {
    let mut map = HashMap::new();
    for line in tsv.lines() {
        let parts: Vec<&str> = line.split('\t').collect();
        if parts.len() >= 4 {
            let username = parts[0].trim();
            if !username.is_empty() {
                let enabled = parts[1].trim() == "1";
                let is_samba = parts[2].trim() == "1";
                let samba_enabled = is_samba && parts[3].trim() == "1";
                map.insert(
                    username.to_string(),
                    AccountRuntimeStatus {
                        enabled,
                        is_samba,
                        samba_enabled,
                    },
                );
            }
        } else if parts.len() == 3 {
            let username = parts[0].trim();
            if !username.is_empty() {
                let enabled = parts[1].trim() == "1";
                let samba_enabled = parts[2].trim() == "1";
                map.insert(
                    username.to_string(),
                    AccountRuntimeStatus {
                        enabled,
                        is_samba: samba_enabled,
                        samba_enabled,
                    },
                );
            }
        }
    }
    map
}

/// Usuarios administrables (formato que consume `app.js`).
pub async fn users(
    _user: crate::extractors::AuthenticatedUser,
    State(state): State<AppState>,
) -> Response {
    let db = state.auth.db().clone();
    match state
        .run_system_blocking(move || {
        let (mut users, groups) = nas_engine::users::load_local();
        users.retain(|user| !user.system);

        let status_map = run_accounts(&["status"], None)
            .ok()
            .map(|tsv| parse_accounts_status_tsv(&tsv))
            .filter(|map| !map.is_empty());

        let fallback_samba = if status_map.is_none() {
            Runner::new()
                .run(
                    &nas_exec::CommandSpec::new("pdbedit")
                        .args(["-L", "-s"])
                        .sudo(nas_exec::SudoMode::Passwordless)
                        .timeout(std::time::Duration::from_secs(15)),
                )
                .ok()
                .map(|output| {
                    output
                        .stdout
                        .lines()
                        .filter_map(|line| line.split(':').next().map(str::to_string))
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default()
        } else {
            Vec::new()
        };

        let list: Vec<serde_json::Value> = users
            .into_iter()
            .map(|user| {
                let member_of: Vec<String> = groups
                    .iter()
                    .filter(|group| group.members.iter().any(|member| member == &user.name))
                    .map(|group| group.name.clone())
                    .collect();
                let role = db.user_role(&user.name).ok().flatten();
                let is_superadmin = role.as_deref() == Some("superadmin");
                let is_admin = matches!(role.as_deref(), Some("admin") | Some("superadmin"));
                let can_web = role.is_some();
                let runtime = status_map
                    .as_ref()
                    .and_then(|map| map.get(&user.name).copied())
                    .unwrap_or_else(|| {
                        let in_samba = fallback_samba.iter().any(|name| name == &user.name);
                        AccountRuntimeStatus {
                            enabled: true,
                            is_samba: in_samba,
                            samba_enabled: in_samba,
                        }
                    });
                serde_json::json!({
                    "username": user.name,
                    "full_name": user.full_name,
                    "uid": user.uid,
                    "shell": user.shell,
                    "home": user.home,
                    "groups": member_of,
                    "is_admin": is_admin,
                    "is_superadmin": is_superadmin,
                    "can_web": can_web,
                    "samba_enabled": runtime.samba_enabled,
                    "is_samba": runtime.is_samba,
                    "enabled": runtime.enabled,
                })
            })
            .collect();
        list
    })
    .await
    {
        Ok(list) => axum::Json(ApiResponse::ok(list, "Usuarios")).into_response(),
        Err(_) => server_error("inventario de usuarios interrumpido"),
    }
}

/// Crea un usuario del sistema.
pub async fn user_create(
    admin: crate::extractors::AdminUser,
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::Json(body): axum::Json<UserBody>,
) -> Response {
    let actor = admin.username().to_string();
    let check_name = body.username.clone();
    let protected = state
        .run_system_blocking(move || is_protected_posix_account(&check_name))
        .await
        .unwrap_or(true);
    if !valid_username(&body.username)
        || protected
        || !body.groups.iter().all(|group| valid_grp(group))
    {
        return bad_request("usuario o grupo inválido o reservado");
    }
    if body.is_superadmin && !admin.is_superadmin() {
        return forbidden("solo el superadministrador puede otorgar rango de superadministrador");
    }
    if body.password.len() < 6 {
        return bad_request("la contraseña debe tener al menos 6 caracteres");
    }
    if body.password.chars().any(char::is_control) {
        return bad_request("la contraseña contiene caracteres de control no permitidos");
    }

    let username = body.username.clone();
    let groups = body.groups.join(",");
    let samba = if body.samba_enabled { "1" } else { "0" };
    let full_name = body.full_name.clone();
    let grant_admin = body.is_admin || body.is_superadmin;
    let can_web = body.can_web || grant_admin;
    let password = body.password.clone();

    let result = state
        .run_system_blocking(move || {
            let hash = if can_web {
                Some(
                    crate::Auth::hash_password(&password)
                        .map_err(|err| format!("no se pudo generar el hash web: {err}"))?,
                )
            } else {
                None
            };
            run_accounts(
                &[
                    "create-user",
                    &username,
                    &full_name,
                    "-",
                    if grant_admin { "1" } else { "0" },
                    if can_web { "1" } else { "0" },
                    samba,
                    &groups,
                ],
                Some(&password),
            )?;
            Ok::<_, String>(hash)
        })
        .await
        .unwrap_or_else(|_| Err("creación interrumpida".to_string()));

    match result {
        Ok(maybe_hash) => {
            if can_web {
                let Some(hash) = maybe_hash else {
                    let rollback_user = body.username.clone();
                    let _ = state
                        .run_system_blocking(move || {
                            let _ = run_accounts(&["delete-user", &rollback_user], None);
                        })
                        .await;
                    return server_error("no se pudo generar el hash de acceso web");
                };
                let role = if body.is_superadmin {
                    "superadmin"
                } else if body.is_admin {
                    "admin"
                } else {
                    "operator"
                };
                let uname = body.username.clone();
                if let Err(err) = state
                    .run_db(move |db| db.upsert_user(&uname, &hash, role))
                    .await
                {
                    let rollback_user = body.username.clone();
                    let _ = state
                        .run_system_blocking(move || {
                            let _ = run_accounts(&["delete-user", &rollback_user], None);
                        })
                        .await;
                    return server_error(&format!(
                        "no se pudo registrar el acceso web en SQLite: {err}"
                    ));
                }
            }
            audit_ip(
                &state,
                &headers,
                &actor,
                "user_create",
                &body.username,
                "SUCCESS",
                "",
            )
            .await;
            axum::Json(ApiResponse::<serde_json::Value>::ok_empty("Usuario creado")).into_response()
        }
        Err(error) => server_error(&error),
    }
}

/// Actualiza un usuario existente.
pub async fn user_update(
    admin: crate::extractors::AdminUser,
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::Json(body): axum::Json<UserBody>,
) -> Response {
    let actor = admin.username().to_string();
    let check_name = body.username.clone();
    let state_for_check = state.clone();
    let (protected, target_is_super) = state
        .run_system_blocking(move || {
            (
                is_protected_posix_account(&check_name),
                is_target_superadmin(&state_for_check, &check_name),
            )
        })
        .await
        .unwrap_or((true, false));
    if !valid_username(&body.username)
        || protected
        || !body.groups.iter().all(|group| valid_grp(group))
    {
        return bad_request("usuario o grupo inválido o reservado");
    }
    if (target_is_super || body.is_superadmin) && !admin.is_superadmin() {
        return forbidden("no se puede modificar el rango del superadministrador");
    }
    if !body.password.is_empty() {
        if body.password.len() < 6 {
            return bad_request("la contraseña debe tener al menos 6 caracteres");
        }
        if body.password.chars().any(char::is_control) {
            return bad_request("la contraseña contiene caracteres de control no permitidos");
        }
    }

    let username = body.username.clone();
    let groups = body.groups.join(",");
    let samba = if body.samba_enabled { "1" } else { "0" };
    let full_name = body.full_name.clone();
    let password = body.password.clone();
    let grant_admin = target_is_super || body.is_superadmin || body.is_admin;
    let can_web = target_is_super || body.can_web || grant_admin;

    let hash_password = if can_web && !body.password.is_empty() {
        Some(body.password.clone())
    } else {
        None
    };

    let result = state
        .run_system_blocking(move || {
            let pass_mode = if password.is_empty() {
                "--keep-password"
            } else {
                "-"
            };
            let secret = if password.is_empty() {
                None
            } else {
                Some(password.as_str())
            };
            run_accounts(
                &[
                    "update-user",
                    &username,
                    &full_name,
                    pass_mode,
                    if grant_admin { "1" } else { "0" },
                    if can_web { "1" } else { "0" },
                    samba,
                    &groups,
                ],
                secret,
            )?;
            let web_hash = match hash_password {
                Some(pw) => Some(
                    crate::Auth::hash_password(&pw)
                        .map_err(|err| format!("no se pudo generar el hash web: {err}"))?,
                ),
                None => None,
            };
            Ok::<_, String>(web_hash)
        })
        .await
        .unwrap_or_else(|_| Err("actualización interrumpida".to_string()));

    match result {
        Ok(web_hash) => {
            let role = if target_is_super || body.is_superadmin {
                "superadmin"
            } else if body.is_admin {
                "admin"
            } else {
                "operator"
            };
            let uname = body.username.clone();
            let has_new_password = !body.password.is_empty();
            let db_res = state
                .run_db(move |db| {
                    if can_web {
                        if has_new_password {
                            if let Some(hash) = web_hash {
                                db.upsert_user(&uname, &hash, role)?;
                            }
                        } else if let Some(existing_hash) = db.user_hash(&uname)? {
                            db.upsert_user(&uname, &existing_hash, role)?;
                        }
                    } else {
                        db.delete_user(&uname)?;
                    }
                    if has_new_password {
                        let _ = db.delete_user_sessions(&uname);
                    }
                    Ok(())
                })
                .await;
            if let Err(err) = db_res {
                return server_error(&format!(
                    "no se pudo actualizar el acceso web en SQLite: {err}"
                ));
            }
            audit_ip(
                &state,
                &headers,
                &actor,
                "user_update",
                &body.username,
                "SUCCESS",
                "",
            )
            .await;
            axum::Json(ApiResponse::<serde_json::Value>::ok_empty(
                "Usuario actualizado",
            ))
            .into_response()
        }
        Err(error) => server_error(&error),
    }
}

/// Activa o suspende una cuenta.
pub async fn user_toggle(
    admin: crate::extractors::AdminUser,
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::Json(body): axum::Json<ToggleUserBody>,
) -> Response {
    let actor = admin.username().to_string();
    let check_name = body.username.clone();
    let state_for_check = state.clone();
    let (protected, target_is_super) = state
        .run_system_blocking(move || {
            (
                is_protected_posix_account(&check_name),
                is_target_superadmin(&state_for_check, &check_name),
            )
        })
        .await
        .unwrap_or((true, false));
    if !valid_username(&body.username) || protected {
        return bad_request("usuario inválido o reservado del sistema");
    }
    if !body.enabled && (body.username == actor || target_is_super) {
        return forbidden("no se puede suspender la cuenta propia ni al superadministrador");
    }

    let username = body.username.clone();
    let enabled = if body.enabled { "1" } else { "0" };
    let result = state
        .run_system_blocking(move || run_accounts(&["toggle-user", &username, enabled], None))
        .await
        .unwrap_or_else(|_| Err("cambio de estado interrumpido".to_string()));

    match result {
        Ok(_) => {
            if !body.enabled {
                let uname = body.username.clone();
                let _ = state
                    .run_db(move |db| db.delete_user_sessions(&uname).map(|_| ()))
                    .await;
            }
            audit_ip(
                &state,
                &headers,
                &actor,
                "user_toggle",
                &body.username,
                "SUCCESS",
                "",
            )
            .await;
            axum::Json(ApiResponse::<serde_json::Value>::ok_empty(
                "Estado actualizado",
            ))
            .into_response()
        }
        Err(error) => server_error(&error),
    }
}

/// Elimina una cuenta del sistema.
pub async fn user_delete(
    admin: crate::extractors::AdminUser,
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::Json(body): axum::Json<ToggleUserBody>,
) -> Response {
    let actor = admin.username().to_string();
    let check_name = body.username.clone();
    let state_for_check = state.clone();
    let (protected, target_is_super) = state
        .run_system_blocking(move || {
            (
                is_protected_posix_account(&check_name),
                is_target_superadmin(&state_for_check, &check_name),
            )
        })
        .await
        .unwrap_or((true, false));
    if !valid_username(&body.username) || protected {
        return bad_request("usuario inválido o protegido del sistema");
    }
    if body.username == actor || target_is_super {
        return forbidden("no se puede eliminar la cuenta propia ni al superadministrador");
    }

    let username = body.username.clone();
    let result = state
        .run_system_blocking(move || run_accounts(&["delete-user", &username], None))
        .await
        .unwrap_or_else(|_| Err("borrado interrumpido".to_string()));

    match result {
        Ok(_) => {
            let uname = body.username.clone();
            if let Err(err) = state.run_db(move |db| db.delete_user(&uname)).await {
                return server_error(&format!(
                    "cuenta eliminada del sistema, pero falló SQLite: {err}"
                ));
            }
            audit_ip(
                &state,
                &headers,
                &actor,
                "user_delete",
                &body.username,
                "SUCCESS",
                "",
            )
            .await;
            axum::Json(ApiResponse::<serde_json::Value>::ok_empty(
                "Usuario eliminado",
            ))
            .into_response()
        }
        Err(error) => server_error(&error),
    }
}

/// Router del dominio de usuarios e identidad.
pub fn router() -> Router<crate::AppState> {
    use axum::routing::{get, post};
    Router::new()
        .route("/api/users", get(users).post(user_create))
        .route("/api/users/update", post(user_update))
        .route("/api/users/toggle", post(user_toggle))
        .route("/api/users/groups", post(user_groups))
        .route("/api/users/delete", post(user_delete))
        .route("/api/groups", get(groups).post(group_create))
        .route("/api/groups/rename", post(group_rename))
        .route("/api/groups/delete", post(group_delete))
}

#[cfg(test)]
mod tests {
    use super::{is_protected_posix_account, parse_accounts_status_tsv, AccountRuntimeStatus};

    #[test]
    fn nas_es_cuenta_reservada_protegida() {
        assert!(is_protected_posix_account("nas"));
        assert!(is_protected_posix_account("nas-web"));
        assert!(is_protected_posix_account("root"));
    }

    #[test]
    fn parsea_tsv_de_estado_cuatro_y_tres_columnas() {
        let sample = "\
operador1\t1\t0\t0
empleado1\t1\t1\t1
suspendido\t0\t1\t0
legado\t1\t0
";
        let map = parse_accounts_status_tsv(sample);
        assert_eq!(
            map.get("operador1"),
            Some(&AccountRuntimeStatus {
                enabled: true,
                is_samba: false,
                samba_enabled: false,
            })
        );
        assert_eq!(
            map.get("empleado1"),
            Some(&AccountRuntimeStatus {
                enabled: true,
                is_samba: true,
                samba_enabled: true,
            })
        );
        assert_eq!(
            map.get("suspendido"),
            Some(&AccountRuntimeStatus {
                enabled: false,
                is_samba: true,
                samba_enabled: false,
            })
        );
        assert_eq!(
            map.get("legado"),
            Some(&AccountRuntimeStatus {
                enabled: true,
                is_samba: false,
                samba_enabled: false,
            })
        );
    }
}
