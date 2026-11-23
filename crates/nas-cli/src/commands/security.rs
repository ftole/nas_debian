use std::process::ExitCode;

use nas_core::{ApiResponse, NasResult};
use nas_engine::security::{harden, status, HardenOptions};
use nas_exec::Runner;

use super::common::print_json;

pub fn security_status(json: bool) -> NasResult<ExitCode> {
    let runner = Runner::new();
    let st = status(&runner);
    if json {
        print_json(&ApiResponse::ok(st, "Estado de seguridad del servidor"))?;
    } else {
        println!("ESTADO DE SEGURIDAD DEL SERVIDOR NAS");
        println!(
            "  Cortafuegos     : {} ({})",
            if st.firewall_active {
                "activo"
            } else {
                "inactivo"
            },
            st.firewall_backend
        );
        println!(
            "  Fail2ban        : {} ({})",
            if st.fail2ban_active {
                "activo"
            } else {
                "inactivo"
            },
            if st.fail2ban_configured {
                "nas.local"
            } else {
                "sin perfil NAS"
            }
        );
        println!(
            "  Parches auto OS : {}",
            if st.unattended_upgrades_configured {
                "habilitado"
            } else {
                "no configurado"
            }
        );
        println!(
            "  Auditd / Reglas : {} / {}",
            if st.auditd_active {
                "activo"
            } else {
                "inactivo"
            },
            if st.audit_rules_present {
                "nas.rules"
            } else {
                "sin reglas NAS"
            }
        );
        println!(
            "  AIDE FIM        : {} / {}",
            if st.aide_rules_present {
                "75_aide_nas"
            } else {
                "sin exclusiones"
            },
            if st.aide_db_initialized {
                "base inicializada"
            } else {
                "sin base inicial"
            }
        );
        println!(
            "  nas-web systemd : {}",
            if st.systemd_hardened {
                "sincronizado (sin AmbientCapabilities)"
            } else {
                "desactualizado"
            }
        );
        println!(
            "  Sudoers acotado : {}",
            if st.sudoers_synced {
                "sincronizado"
            } else {
                "desactualizado"
            }
        );
        println!(
            "  Máscara ACPI    : {}",
            if st.sleep_masked {
                "enmascarado"
            } else {
                "no enmascarado"
            }
        );
        println!(
            "  SSH hardening   : {}",
            if st.ssh_hardened {
                "99-nas-hardening.conf"
            } else {
                "por defecto Debian"
            }
        );
    }
    Ok(ExitCode::SUCCESS)
}

pub fn security_harden(options: HardenOptions, json: bool) -> NasResult<ExitCode> {
    let runner = Runner::new();
    let report = harden(&runner, &options)?;
    if json {
        print_json(&ApiResponse::ok(
            report,
            if options.apply {
                "Endurecimiento de seguridad aplicado"
            } else {
                "Simulación de endurecimiento (dry-run)"
            },
        ))?;
    } else {
        println!(
            "{}",
            if report.applied {
                "ENDURECIMIENTO DE SEGURIDAD APLICADO"
            } else {
                "PLAN DE ENDURECIMIENTO DE SEGURIDAD (dry-run)"
            }
        );
        for action in &report.actions {
            println!("  [OK] {action}");
        }
        for warn in &report.warnings {
            println!("  [!]  {warn}");
        }
    }
    Ok(ExitCode::SUCCESS)
}
