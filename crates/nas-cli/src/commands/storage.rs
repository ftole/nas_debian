use std::process::ExitCode;

use nas_core::{ApiResponse, NasResult};
use nas_exec::Runner;

use super::common::print_json;

pub fn storage_list(json: bool) -> NasResult<ExitCode> {
    let devices = nas_engine::storage::lsblk_devices_with_uuid(&Runner::new())?;

    if json {
        print_json(&ApiResponse::ok(&devices, "Dispositivos de bloque"))?;
        return Ok(ExitCode::SUCCESS);
    }

    println!("DISPOSITIVOS DE BLOQUE");
    for device in &devices {
        for entry in device.flatten() {
            let mount = entry.mountpoint.as_deref().unwrap_or("-");
            let fs = entry.fstype.as_deref().unwrap_or("-");
            let kind = if entry.rotational == Some(true) {
                "HDD"
            } else {
                "SSD"
            };
            println!(
                "{:<16} {:<8} {:<6} {:<8} {:<18} {}",
                entry.name, entry.size, entry.kind, fs, mount, kind
            );
        }
    }

    Ok(ExitCode::SUCCESS)
}
