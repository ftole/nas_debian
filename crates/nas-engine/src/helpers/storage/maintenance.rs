//! Mantenimiento de almacenamiento: scrubs Btrfs, saneamiento de fstab y write-cache.

use std::path::Path;
use std::time::Duration;

use nas_core::{NasError, NasResult};
use nas_exec::{CommandSpec, Runner};

use crate::storage_ops::{confirm_ok, valid_device, valid_name};

pub(in crate::helpers::storage) fn register_btrfs_scrub_for_target(
    target: &str,
    unit_id: Option<&str>,
) {
    let (cron_path, minute_hour) = match unit_id {
        Some(id) if valid_name(id) => (
            format!("/etc/cron.d/nas-btrfs-scrub-unit-{id}"),
            "0 3 1 * *",
        ),
        _ => ("/etc/cron.d/nas-btrfs-scrub".to_string(), "0 2 1 * *"),
    };
    let content = format!(
        "SHELL=/bin/bash\nPATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin\n{minute_hour} root nice -n 19 ionice -c 3 btrfs scrub start -B {target} >/dev/null 2>&1\n"
    );
    let _ = std::fs::write(&cron_path, content);
}

pub(in crate::helpers::storage) fn remove_btrfs_scrub_for_unit(unit_id: &str) {
    if valid_name(unit_id) {
        let cron_path = format!("/etc/cron.d/nas-btrfs-scrub-unit-{unit_id}");
        let _ = std::fs::remove_file(cron_path);
    }
}

/// Sanea `/etc/fstab` retirando entradas `/dev/...` huérfanas y normalizando `nofail` en `/srv/nas*`.
pub fn sanitize_fstab(runner: &Runner, confirm: &str) -> NasResult<String> {
    if !confirm_ok(confirm) && confirm != "SI-SANEAR" {
        return Err(NasError::Denied(
            "confirmación requerida (SI-FORMATEAR o SI-SANEAR)".into(),
        ));
    }
    let fstab_path = Path::new("/etc/fstab");
    if !fstab_path.exists() {
        return Ok("no existe /etc/fstab".into());
    }
    let content = std::fs::read_to_string(fstab_path).map_err(|e| NasError::io(fstab_path, e))?;
    let (cleaned, changes) =
        crate::deploy::sanitize_fstab_content(&content, &|src| Path::new(src).exists());
    if changes == 0 {
        return Ok("/etc/fstab ya está saneado (sin cambios)".into());
    }
    if let Ok(staged) = crate::deploy::write_unique_temp_file("nas-fstab-verify", &cleaned) {
        let pre = runner.run(
            &CommandSpec::new("findmnt")
                .args(["--verify", "--tab-file", &staged.to_string_lossy()])
                .timeout(Duration::from_secs(10)),
        );
        let _ = std::fs::remove_file(&staged);
        if let Ok(out) = pre {
            let combined = format!("{}\n{}", out.stdout, out.stderr);
            if !out.is_success() && combined.to_ascii_lowercase().contains("parse error") {
                return Err(NasError::Invalid(format!(
                    "findmnt --verify rechazó la sintaxis propuesta de fstab: {}",
                    combined.trim()
                )));
            }
        }
    }
    crate::deploy::install_fstab_content(runner, &cleaned)?;
    let verify_note = match runner.run(
        &CommandSpec::new("findmnt")
            .args(["--verify", "--fstab"])
            .timeout(Duration::from_secs(10)),
    ) {
        Ok(out) if out.is_success() => String::new(),
        Ok(out) => {
            let detail = if out.stderr.trim().is_empty() {
                out.stdout.trim()
            } else {
                out.stderr.trim()
            };
            format!(" (aviso findmnt --verify: {detail})")
        }
        Err(_) => String::new(),
    };
    Ok(format!(
        "/etc/fstab saneado ({changes} corrección(es), respaldo previo guardado){verify_note}"
    ))
}

fn find_by_id_basename_for_disk(disk_path: &str) -> Option<String> {
    let target_canon = std::fs::canonicalize(disk_path).ok()?;
    let entries = std::fs::read_dir("/dev/disk/by-id").ok()?;
    let mut best: Option<String> = None;
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if name.contains("-part") {
            continue;
        }
        if let Ok(resolved) = std::fs::canonicalize(entry.path()) {
            if resolved == target_canon {
                if name.starts_with("wwn-") || name.starts_with("nvme-eui.") {
                    return Some(name);
                }
                if best.is_none() {
                    best = Some(name);
                }
            }
        }
    }
    best
}

/// Activa o desactiva la caché volátil de escritura de un disco (`hdparm -W0`/`-W1`)
/// y persiste una regla udev para mantener la configuración tras cada reinicio.
pub fn set_write_cache(
    runner: &Runner,
    confirm: &str,
    device: &str,
    enabled: bool,
) -> NasResult<String> {
    if !confirm_ok(confirm) && confirm != "SI-APLICAR" {
        return Err(NasError::Denied(
            "confirmación requerida (SI-APLICAR o SI-FORMATEAR)".into(),
        ));
    }
    if !valid_device(device) {
        return Err(NasError::Invalid("dispositivo inválido".into()));
    }
    let disk_path = crate::storage_ops::parent_disk(runner, device)
        .unwrap_or_else(|| device.trim().to_string());
    if !valid_device(&disk_path) {
        return Err(NasError::Invalid("disco físico inválido".into()));
    }
    let disk_name = disk_path.trim_start_matches("/dev/");
    if disk_name.is_empty() || disk_name.contains('/') {
        return Err(NasError::Invalid(
            "solo los discos físicos directos admiten control hdparm write-cache".into(),
        ));
    }

    crate::deploy::deps::ensure_packages(runner, &["hdparm"], false)?;

    let flag = if enabled { "-W1" } else { "-W0" };
    runner
        .run(
            &CommandSpec::new("hdparm")
                .args([flag, &disk_path])
                .timeout(Duration::from_secs(10)),
        )?
        .ensure_success()?;

    let by_id = find_by_id_basename_for_disk(&disk_path);
    let rule_content =
        crate::storage::render_write_cache_udev_rule(by_id.as_deref(), disk_name, enabled);
    let rule_path = format!("/etc/udev/rules.d/65-nas-writecache-{disk_name}.rules");
    std::fs::create_dir_all("/etc/udev/rules.d")
        .map_err(|e| NasError::io(Path::new("/etc/udev/rules.d"), e))?;
    std::fs::write(&rule_path, rule_content).map_err(|e| NasError::io(Path::new(&rule_path), e))?;
    let _ = runner.run(&CommandSpec::new("udevadm").args(["control", "--reload-rules"]));

    let mode_text = if enabled {
        "ACTIVADA (rendimiento estándar)"
    } else {
        "DESACTIVADA (protección máxima ante cortes)"
    };
    Ok(format!(
        "caché volátil de {disk_path} {mode_text}; regla persistente en {rule_path}"
    ))
}
