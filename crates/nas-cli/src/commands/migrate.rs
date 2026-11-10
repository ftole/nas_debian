use std::path::{Path, PathBuf};
use std::process::ExitCode;

use nas_core::{ApiResponse, NasResult};

use super::common::print_json;

#[derive(Debug, Clone, serde::Serialize)]
pub struct LegacyItem {
    pub path: String,
    pub replacement: &'static str,
    pub wave: &'static str,
    pub present: bool,
}

pub fn legacy_inventory(root: &Path) -> Vec<LegacyItem> {
    let items: [(&str, &str, &str); 10] = [
        ("web", "nas-web", "A"),
        ("bin/nas-cli", "nas (CLI Rust)", "A"),
        ("src/asistente.sh", "nas-tui", "A"),
        ("src/modules", "nas-tui", "A"),
        ("nas_admin.py", "nasctl", "A"),
        ("test_remote.py", "nas-ssh + nas-dev", "A"),
        ("src/core/deploy.sh", "nas deploy", "B"),
        ("src/core/updater.sh", "nas update", "B"),
        ("src/core/uninstall.sh", "nas uninstall", "B"),
        ("install.sh", "packaging/install.sh", "B"),
    ];

    items
        .iter()
        .map(|(path, replacement, wave)| LegacyItem {
            path: (*path).to_string(),
            replacement,
            wave,
            present: root.join(path).exists(),
        })
        .collect()
}

pub fn migrate_status(json: bool) -> NasResult<ExitCode> {
    let root = std::env::var("NAS_REPO_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("."));
    let items = legacy_inventory(&root);
    let pending = items.iter().filter(|item| item.present).count();

    if json {
        print_json(&ApiResponse::ok(
            serde_json::json!({
                "root": root.display().to_string(),
                "pending": pending,
                "items": items,
            }),
            "Estado del retiro del stack anterior",
        ))?;
        return Ok(ExitCode::SUCCESS);
    }

    println!("RETIRO DEL STACK ANTERIOR (raíz: {})", root.display());
    println!(
        "{:<26} {:<22} {:<5} ESTADO",
        "COMPONENTE", "SUSTITUTO", "OLA"
    );
    for item in &items {
        println!(
            "{:<26} {:<22} {:<5} {}",
            item.path,
            item.replacement,
            item.wave,
            if item.present { "presente" } else { "retirado" }
        );
    }
    println!();
    println!("Pendientes de retiro: {pending}");

    Ok(ExitCode::SUCCESS)
}
