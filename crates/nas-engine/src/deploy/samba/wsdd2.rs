//! Configuración del servicio `wsdd2` para descubrimiento en redes Windows.

use std::time::Duration;

use nas_core::{NasError, NasResult};
use nas_exec::{CommandSpec, Runner};

/// Genera el contenido del override de `wsdd2`.
pub fn wsdd2_override_content(netbios: &str, workgroup: &str) -> String {
    format!(
        "[Service]\nExecStart=\nExecStart=/usr/bin/wsdd2 -N {netbios} -G {workgroup} -H {netbios}\n",
        netbios = netbios,
        workgroup = workgroup,
    )
}

/// Alias de `wsdd2_override_content`.
pub fn wsdd2_override(netbios: &str, workgroup: &str) -> String {
    wsdd2_override_content(netbios, workgroup)
}

/// Configura el override de `wsdd2` en el sistema.
pub fn configure_wsdd2(runner: &Runner, netbios: &str, workgroup: &str) -> NasResult<String> {
    let default_content = format!(
        "WSDD2_OPTS=\"-N {netbios} -G {workgroup} -H {netbios}\"\n",
        netbios = netbios,
        workgroup = workgroup,
    );
    let override_content = wsdd2_override_content(netbios, workgroup);

    runner
        .run(
            &CommandSpec::new("mkdir")
                .args(["-p", "/etc/systemd/system/wsdd2.service.d"])
                .timeout(Duration::from_secs(20)),
        )?
        .ensure_success()?;

    for (path, content) in [
        ("/etc/default/wsdd2", default_content.as_str()),
        (
            "/etc/systemd/system/wsdd2.service.d/override.conf",
            override_content.as_str(),
        ),
    ] {
        let staged = std::env::temp_dir().join(format!(
            "nas-wsdd2-{}-{}",
            std::process::id(),
            path.replace('/', "_")
        ));
        std::fs::write(&staged, content)
            .map_err(|e| NasError::Denied(format!("no se pudo preparar override de wsdd2: {e}")))?;
        let res = runner.run(
            &CommandSpec::new("install")
                .args([
                    "-m",
                    "0644",
                    "-o",
                    "root",
                    "-g",
                    "root",
                    &staged.to_string_lossy(),
                    path,
                ])
                .sudo(nas_exec::SudoMode::Passwordless)
                .timeout(Duration::from_secs(30)),
        );
        let _ = std::fs::remove_file(&staged);
        match res {
            Ok(out) if !out.is_success() => {
                return Err(NasError::Denied(format!(
                    "no se pudo escribir {path}: {}",
                    out.stderr
                )));
            }
            Err(e) => {
                return Err(NasError::Denied(format!("no se pudo escribir {path}: {e}")));
            }
            _ => {}
        }
    }

    let _ = runner.run(
        &CommandSpec::new("systemctl")
            .args(["daemon-reload"])
            .timeout(Duration::from_secs(30)),
    );

    Ok("wsdd2 configurado".to_string())
}

/// Alias de `configure_wsdd2`.
pub fn setup_wsdd2(runner: &Runner, netbios: &str, workgroup: &str) -> NasResult<String> {
    configure_wsdd2(runner, netbios, workgroup)
}

/// Alias de `configure_wsdd2`.
pub fn configure_wsdd(runner: &Runner, netbios: &str, workgroup: &str) -> NasResult<String> {
    configure_wsdd2(runner, netbios, workgroup)
}

/// Alias de `configure_wsdd2`.
pub fn wsdd2_configure(runner: &Runner, netbios: &str, workgroup: &str) -> NasResult<String> {
    configure_wsdd2(runner, netbios, workgroup)
}
