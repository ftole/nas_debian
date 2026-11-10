use std::process::ExitCode;

use nas_core::{ApiResponse, NasResult};
use nas_engine::collect_status;
use nas_exec::Runner;

use super::common::print_json;

pub fn status(json: bool) -> NasResult<ExitCode> {
    let runner = Runner::new();
    let metrics = collect_status(&runner);

    if json {
        print_json(&ApiResponse::ok(metrics, "Métricas del sistema"))?;
        return Ok(ExitCode::SUCCESS);
    }

    println!("SERVIDOR NAS — ESTADO DEL SISTEMA");
    println!("Hostname        : {}", metrics.hostname);
    println!("Sistema         : {}", metrics.os);
    println!("Kernel          : {}", metrics.kernel);
    println!("Uptime          : {}", metrics.uptime);
    println!(
        "CPU             : {} ({:.1}%)",
        metrics.cpu_model, metrics.cpu_usage_pct
    );
    println!(
        "Memoria         : {:.2} GiB / {:.2} GiB ({:.1}%)",
        metrics.ram_used_gb, metrics.ram_total_gb, metrics.ram_usage_pct
    );
    println!(
        "IP primaria     : {}",
        metrics.ip.as_deref().unwrap_or("N/D")
    );
    println!(
        "Hora            : {} {} (UTC{:+})",
        metrics.server_time,
        metrics.timezone_abbr,
        metrics.timezone_offset / 3600
    );

    Ok(ExitCode::SUCCESS)
}
