use std::process::ExitCode;

use nas_core::{ApiResponse, NasPaths, NasResult, ServerRole};
use nas_exec::{CommandSpec, Runner};
use serde::Serialize;

use super::common::print_json;

#[derive(Debug, Serialize)]
pub struct Check {
    pub name: String,
    pub ok: bool,
    pub detail: String,
}

pub fn diag(json: bool) -> NasResult<ExitCode> {
    let paths = NasPaths::from_env();
    let runner = Runner::new();
    let role = ServerRole::from_file(&paths.role_file)?.unwrap_or(ServerRole::ArquivosBackup);

    let mut checks = Vec::new();

    checks.push(Check {
        name: "role".to_string(),
        ok: paths.role_file.exists(),
        detail: format!("{} ({})", role.as_str(), role.label()),
    });
    checks.push(Check {
        name: "samba_conf".to_string(),
        ok: paths.samba_conf.exists(),
        detail: paths.samba_conf.display().to_string(),
    });
    checks.push(Check {
        name: "data_root".to_string(),
        ok: paths.data_root.exists(),
        detail: paths.data_root.display().to_string(),
    });
    checks.push(Check {
        name: "state_dir".to_string(),
        ok: paths.state_dir.exists(),
        detail: paths.state_dir.display().to_string(),
    });

    for service in ["smbd", "nginx"] {
        let active = runner
            .run(&CommandSpec::new("systemctl").args(["is-active", service]))
            .map(|out| out.stdout == "active")
            .unwrap_or(false);
        checks.push(Check {
            name: format!("service:{service}"),
            ok: active,
            detail: if active { "activo" } else { "inactivo" }.to_string(),
        });
    }

    let all_ok = checks.iter().all(|check| check.ok);

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
