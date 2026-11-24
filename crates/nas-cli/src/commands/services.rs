use std::process::ExitCode;

use nas_core::{ApiResponse, NasResult};
use nas_exec::Runner;

use super::common::print_json;

pub fn services(json: bool) -> NasResult<ExitCode> {
    let list =
        nas_engine::services::collect(&Runner::new(), nas_engine::services::DEFAULT_SERVICES);

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
