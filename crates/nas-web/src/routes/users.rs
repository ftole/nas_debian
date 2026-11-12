use axum::extract::State;
use axum::http::HeaderMap;
use axum::response::{IntoResponse, Response};
use axum::Router;
use nas_core::ApiResponse;
use nas_exec::Runner;
use serde::Deserialize;

use super::common::{
    audit_ip, bad_request, configured_superadmin, forbidden, is_admin, require_auth, server_error,
};
use crate::AppState;

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

#[derive(Debug, Deserialize)]
pub struct GroupBody {
    pub name: String,
}

#[derive(Debug, Deserialize)]
pub struct RenameGroupBody {
    pub old: String,
    #[serde(default)]
    pub new: String,
    #[serde(default)]
    pub new_name: String,
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
    let mut cmd_args = vec!["helper", "accounts"];
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
        Err(if output.stderr.is_empty() {
            output.stdout
        } else {
            output.stderr
        })
    }
}

/// Usuarios administrables (formato que consume `app.js`).
pub async fn users(
    _user: crate::extractors::AuthenticatedUser,
    State(state): State<AppState>,
) -> Response {
    let db = state.auth.db().clone();
    match tokio::task::spawn_blocking(move || {
        let (mut users, groups) = nas_engine::users::load_local();
        users.retain(|user| !user.system);

        let samba_users = Runner::new()
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
            .unwrap_or_default();

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
                let is_samba = samba_users.iter().any(|name| name == &user.name);
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
                    "samba_enabled": is_samba,
                    "is_samba": is_samba,
                    "enabled": true,
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

/// Grupos corporativos (formato que consume `app.js`).
pub async fn groups(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if let Err(response) = require_auth(&state, &headers) {
        return *response;
    }

    match tokio::task::spawn_blocking(|| {
        let (_, groups) = nas_engine::users::load_local();
        let list: Vec<serde_json::Value> = groups
            .into_iter()
            .filter(|group| group.name.starts_with("grp_"))
            .map(|group| {
                let is_special = matches!(
                    group.name.as_str(),
                    "grp_samba" | "grp_web" | "grp_superadmin" | "grp_backups"
                );
                serde_json::json!({
                    "name": group.name,
                    "gid": group.gid,
                    "members": group.members,
                    "is_special": is_special,
                    "is_master": matches!(
                        group.name.as_str(),
                        "grp_samba" | "grp_superadmin" | "grp_backups" | "grp_web"
                    ),
                })
            })
            .collect();
        list
    })
    .await
    {
        Ok(list) => axum::Json(ApiResponse::ok(list, "Grupos")).into_response(),
        Err(_) => server_error("inventario de grupos interrumpido"),
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
    if !valid_username(&body.username)
        || is_protected_posix_account(&body.username)
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

    let username = body.username.clone();
    let groups = body.groups.join(",");
    let samba = if body.samba_enabled { "1" } else { "0" };
    let full_name = body.full_name.clone();
    let grant_admin = body.is_admin || body.is_superadmin;
    let can_web = body.can_web || grant_admin;
    let password = body.password.clone();

    let result = tokio::task::spawn_blocking(move || {
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
        )
    })
    .await
    .unwrap_or_else(|_| Err("creación interrumpida".to_string()));

    match result {
        Ok(_) => {
            if can_web {
                if let Ok(hash) = crate::Auth::hash_password(&body.password) {
                    let role = if body.is_superadmin {
                        "superadmin"
                    } else if body.is_admin {
                        "admin"
                    } else {
                        "operator"
                    };
                    let _ = state.auth.db().upsert_user(&body.username, &hash, role);
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
            );
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
    if !valid_username(&body.username)
        || is_protected_posix_account(&body.username)
        || !body.groups.iter().all(|group| valid_grp(group))
    {
        return bad_request("usuario o grupo inválido o reservado");
    }
    let target_is_super = is_target_superadmin(&state, &body.username);
    if (target_is_super || body.is_superadmin) && !admin.is_superadmin() {
        return forbidden("no se puede modificar el rango del superadministrador");
    }
    if !body.password.is_empty() && body.password.len() < 6 {
        return bad_request("la contraseña debe tener al menos 6 caracteres");
    }

    let username = body.username.clone();
    let groups = body.groups.join(",");
    let samba = if body.samba_enabled { "1" } else { "0" };
    let full_name = body.full_name.clone();
    let password = body.password.clone();
    let grant_admin = target_is_super || body.is_superadmin || body.is_admin;
    let can_web = target_is_super || body.can_web || grant_admin;

    let result = tokio::task::spawn_blocking(move || {
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
        )
    })
    .await
    .unwrap_or_else(|_| Err("actualización interrumpida".to_string()));

    match result {
        Ok(_) => {
            let role = if target_is_super || body.is_superadmin {
                "superadmin"
            } else if body.is_admin {
                "admin"
            } else {
                "operator"
            };
            if can_web {
                if !body.password.is_empty() {
                    if let Ok(hash) = crate::Auth::hash_password(&body.password) {
                        let _ = state.auth.db().upsert_user(&body.username, &hash, role);
                    }
                } else if let Ok(Some(existing_hash)) = state.auth.db().user_hash(&body.username) {
                    let _ = state
                        .auth
                        .db()
                        .upsert_user(&body.username, &existing_hash, role);
                }
            } else {
                let _ = state.auth.db().delete_user(&body.username);
            }
            if !body.password.is_empty() {
                let _ = state.auth.db().delete_user_sessions(&body.username);
            }
            audit_ip(
                &state,
                &headers,
                &actor,
                "user_update",
                &body.username,
                "SUCCESS",
                "",
            );
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
    if !valid_username(&body.username) || is_protected_posix_account(&body.username) {
        return bad_request("usuario inválido o reservado del sistema");
    }
    if !body.enabled && (body.username == actor || is_target_superadmin(&state, &body.username)) {
        return forbidden("no se puede suspender la cuenta propia ni al superadministrador");
    }

    let username = body.username.clone();
    let enabled = if body.enabled { "1" } else { "0" };
    let result = tokio::task::spawn_blocking(move || {
        run_accounts(&["toggle-user", &username, enabled], None)
    })
    .await
    .unwrap_or_else(|_| Err("cambio de estado interrumpido".to_string()));

    match result {
        Ok(_) => {
            if !body.enabled {
                let _ = state.auth.db().delete_user_sessions(&body.username);
            }
            audit_ip(
                &state,
                &headers,
                &actor,
                "user_toggle",
                &body.username,
                "SUCCESS",
                "",
            );
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
    if !valid_username(&body.username) || is_protected_posix_account(&body.username) {
        return bad_request("usuario inválido o protegido del sistema");
    }
    if body.username == actor || is_target_superadmin(&state, &body.username) {
        return forbidden("no se puede eliminar la cuenta propia ni al superadministrador");
    }

    let username = body.username.clone();
    let result =
        tokio::task::spawn_blocking(move || run_accounts(&["delete-user", &username], None))
            .await
            .unwrap_or_else(|_| Err("borrado interrumpido".to_string()));

    match result {
        Ok(_) => {
            let _ = state.auth.db().delete_user(&body.username);
            audit_ip(
                &state,
                &headers,
                &actor,
                "user_delete",
                &body.username,
                "SUCCESS",
                "",
            );
            axum::Json(ApiResponse::<serde_json::Value>::ok_empty(
                "Usuario eliminado",
            ))
            .into_response()
        }
        Err(error) => server_error(&error),
    }
}

/// Añade o quita un miembro de un grupo.
pub async fn user_groups(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::Json(body): axum::Json<MemberBody>,
) -> Response {
    let actor = match require_auth(&state, &headers) {
        Ok(user) => user,
        Err(response) => return *response,
    };
    if !is_admin(&state, &actor) {
        return forbidden("se requieren privilegios de administrador");
    }
    if !valid_username(&body.username)
        || is_protected_posix_account(&body.username)
        || !valid_grp(&body.group)
    {
        return bad_request("usuario o grupo inválido");
    }
    if body.group == "grp_superadmin" {
        return forbidden("el grupo grp_superadmin es inmutable");
    }

    let action = if body.action == "add" {
        "add-member"
    } else {
        "remove-member"
    };
    let username = body.username.clone();
    let group = body.group.clone();
    let result =
        tokio::task::spawn_blocking(move || run_accounts(&[action, &username, &group], None))
            .await
            .unwrap_or_else(|_| Err("membresía interrumpida".to_string()));

    match result {
        Ok(_) => axum::Json(ApiResponse::<serde_json::Value>::ok_empty(
            "Membresía actualizada",
        ))
        .into_response(),
        Err(error) => server_error(&error),
    }
}

/// Crea un grupo corporativo.
pub async fn group_create(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::Json(body): axum::Json<GroupBody>,
) -> Response {
    let actor = match require_auth(&state, &headers) {
        Ok(user) => user,
        Err(response) => return *response,
    };
    if !is_admin(&state, &actor) {
        return forbidden("se requieren privilegios de administrador");
    }
    let raw = body.name.trim();
    let clean = raw.trim_start_matches("grp_");
    let name = format!("grp_{clean}");
    if clean.is_empty() || !valid_grp(&name) {
        return bad_request(
            "el grupo debe tener un nombre válido (letras minúsculas, números, guiones y hasta 32 caracteres)",
        );
    }
    if is_master_grp(&name) {
        return bad_request("no se puede crear un grupo maestro reservado del sistema");
    }

    let created = name.clone();
    let result =
        tokio::task::spawn_blocking(move || run_accounts(&["create-group", &created], None))
            .await
            .unwrap_or_else(|_| Err("creación interrumpida".to_string()));

    match result {
        Ok(_) => {
            audit_ip(
                &state,
                &headers,
                &actor,
                "group_create",
                &name,
                "SUCCESS",
                "",
            );
            axum::Json(ApiResponse::<serde_json::Value>::ok_empty("Grupo creado")).into_response()
        }
        Err(error) => server_error(&error),
    }
}

/// Renombra un grupo.
pub async fn group_rename(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::Json(body): axum::Json<RenameGroupBody>,
) -> Response {
    let actor = match require_auth(&state, &headers) {
        Ok(user) => user,
        Err(response) => return *response,
    };
    if !is_admin(&state, &actor) {
        return forbidden("se requieren privilegios de administrador");
    }
    let raw_new = if body.new.is_empty() {
        body.new_name.trim()
    } else {
        body.new.trim()
    };
    let clean_new = raw_new.trim_start_matches("grp_");
    let new_name = format!("grp_{clean_new}");
    if clean_new.is_empty()
        || !valid_grp(&body.old)
        || !valid_grp(&new_name)
        || is_master_grp(&body.old)
        || is_master_grp(&new_name)
    {
        return bad_request("nombre de grupo inválido o grupo maestro protegido");
    }

    let old = body.old.clone();
    let target = new_name.clone();
    let result =
        tokio::task::spawn_blocking(move || run_accounts(&["rename-group", &old, &target], None))
            .await
            .unwrap_or_else(|_| Err("renombrado interrumpido".to_string()));

    match result {
        Ok(_) => axum::Json(ApiResponse::<serde_json::Value>::ok_empty(
            "Grupo renombrado",
        ))
        .into_response(),
        Err(error) => server_error(&error),
    }
}

/// Elimina un grupo corporativo.
pub async fn group_delete(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::Json(body): axum::Json<GroupBody>,
) -> Response {
    let actor = match require_auth(&state, &headers) {
        Ok(user) => user,
        Err(response) => return *response,
    };
    if !is_admin(&state, &actor) {
        return forbidden("se requieren privilegios de administrador");
    }
    let name = body.name.trim().to_string();
    if !valid_grp(&name) || is_master_grp(&name) {
        return bad_request("grupo inválido o grupo maestro protegido");
    }

    let target = name.clone();
    let result =
        tokio::task::spawn_blocking(move || run_accounts(&["delete-group", &target], None))
            .await
            .unwrap_or_else(|_| Err("borrado interrumpido".to_string()));

    match result {
        Ok(_) => {
            audit_ip(
                &state,
                &headers,
                &actor,
                "group_delete",
                &name,
                "SUCCESS",
                "",
            );
            axum::Json(ApiResponse::<serde_json::Value>::ok_empty(
                "Grupo eliminado",
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
