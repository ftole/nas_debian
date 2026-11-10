use std::process::ExitCode;

use nas_core::{ApiResponse, NasResult};
use nas_exec::{CommandSpec, Runner};
use serde::Serialize;

use super::common::print_json;

#[derive(Debug, Serialize)]
pub struct ServiceStatus {
    pub service: String,
    pub active: bool,
}

pub fn services(json: bool) -> NasResult<ExitCode> {
    let runner = Runner::new();
    let list: Vec<ServiceStatus> = ["smbd", "nmbd", "wsdd2", "nginx"]
        .into_iter()
        .map(|service| ServiceStatus {
            service: service.to_string(),
            active: runner
                .run(&CommandSpec::new("systemctl").args(["is-active", service]))
                .map(|out| out.stdout == "active")
                .unwrap_or(false),
        })
        .collect();

    if json {
        print_json(&ApiResponse::ok(list, "Estado de servicios"))?;
        return Ok(ExitCode::SUCCESS);
    }

    println!("DEMONIOS DEL SISTEMA");
    for item in &list {
        let state = if item.active { "activo" } else { "inactivo" };
        println!("{:<10} {state}", item.service);
    }

    Ok(ExitCode::SUCCESS)
}
