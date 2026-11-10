use std::process::ExitCode;

use nas_core::{ApiResponse, NasResult};

use super::common::print_json;

pub fn users_list(json: bool) -> NasResult<ExitCode> {
    let (mut users, mut groups) = nas_engine::users::load_local();
    users.retain(|user| !user.system);
    groups.retain(|group| group.name.starts_with("grp_"));

    if json {
        print_json(&ApiResponse::ok(
            serde_json::json!({ "users": users, "groups": groups }),
            "Usuarios y grupos",
        ))?;
        return Ok(ExitCode::SUCCESS);
    }

    println!("USUARIOS DEL PANEL ({})", users.len());
    for user in &users {
        println!(
            "{:<20} uid={:<6} gid={:<6} {:<24} {}",
            user.name, user.uid, user.gid, user.home, user.shell
        );
    }
    println!();
    println!("GRUPOS CORPORATIVOS ({})", groups.len());
    for group in &groups {
        println!(
            "{:<20} gid={:<6} {}",
            group.name,
            group.gid,
            group.members.join(", ")
        );
    }

    Ok(ExitCode::SUCCESS)
}
