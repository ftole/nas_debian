use std::process::ExitCode;

use nas_core::{ApiResponse, NasPaths, NasResult};
use nas_exec::Runner;

use super::common::print_json;

pub fn diag(json: bool) -> NasResult<ExitCode> {
    let paths = NasPaths::from_env();
    let runner = Runner::new();
    let checks = nas_engine::diagnostics::collect(&paths, &runner);
    let all_ok = nas_engine::diagnostics::all_ok(&checks);

    if json {
        print_json(&ApiResponse::ok(&checks, "Diagnóstico integral"))?;
    } else {
        println!("DIAGNÓSTICO INTEGRAL");
        for check in &checks {
            let mark = if check.ok { "[OK]" } else { "[FALLO]" };
            println!("{mark:<8} {:<18} {}", check.name, check.detail);
        }
    }

    if all_ok {
        Ok(ExitCode::SUCCESS)
    } else {
        Ok(ExitCode::from(1))
    }
}
