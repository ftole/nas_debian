use std::process::ExitCode;

use nas_core::{ApiResponse, NasError, NasResult};
use nas_exec::{CommandSpec, Runner};

use super::common::print_json;

pub fn storage_list(json: bool) -> NasResult<ExitCode> {
    let output = Runner::new().run(&CommandSpec::new("lsblk").args([
        "-J",
        "-o",
        "NAME,SIZE,TYPE,MOUNTPOINT,FSTYPE,MODEL,ROTA",
    ]))?;
    if !output.is_success() {
        return Err(NasError::Command {
            command: "lsblk".into(),
            code: output.code,
            stderr: output.stderr,
        });
    }

    let devices = nas_engine::parsers::lsblk::parse(&output.stdout)
        .map_err(|error| NasError::Invalid(format!("lsblk inválido: {error}")))?;

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
