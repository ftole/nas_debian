//! Clasificación de estado de tareas de respaldo a partir de sus bitácoras y snapshots físicos.

/// Comprueba si existe físicamente al menos un snapshot en el disco de destino.
fn has_physical_snapshot(dest_root: &str, id: &str) -> bool {
    let dest_dir = std::path::Path::new(dest_root).join(id);
    let Ok(entries) = std::fs::read_dir(&dest_dir) else {
        return false;
    };
    entries.flatten().any(|e| {
        e.path().is_dir()
            && e.file_name()
                .to_str()
                .map(|name| name.starts_with("snapshot_"))
                .unwrap_or(false)
    })
}

/// Clasifica el estado de una tarea de respaldo evaluando hasta 50 líneas del log.
pub fn classify_backup_status(
    running: bool,
    log_content: Option<&str>,
    dest_root: &str,
    id: &str,
) -> &'static str {
    if running {
        return "En curso";
    }

    let Some(text) = log_content else {
        return "PENDIENTE";
    };

    let all_lines: Vec<&str> = text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    if all_lines.is_empty() {
        return "PENDIENTE";
    }

    let recent = if all_lines.len() > 50 {
        &all_lines[all_lines.len() - 50..]
    } else {
        &all_lines[..]
    };

    let mut last_error_idx: Option<usize> = None;
    let mut last_cancel_idx: Option<usize> = None;
    let mut last_success_idx: Option<usize> = None;
    let mut last_start_idx: Option<usize> = None;

    for (idx, line) in recent.iter().enumerate() {
        let upper = line.to_uppercase();
        let mut line_is_error = false;
        let mut line_is_cancel = false;

        if upper.contains("CANCELADO")
            || upper.contains("CANCELADA")
            || upper.contains("DETENIDO")
            || upper.contains("DETENIDA")
            || upper.contains("CODIGO DE SALIDA 143")
            || upper.contains("CÓDIGO DE SALIDA 143")
            || upper.contains("CODIGO 143")
            || upper.contains("CÓDIGO 143")
            || upper.contains("CODIGO DE SALIDA 130")
            || upper.contains("CÓDIGO DE SALIDA 130")
        {
            line_is_cancel = true;
        }

        if upper.contains("ERROR")
            || upper.contains("FALLO")
            || upper.contains("FAILED")
            || upper.contains("ABORTADO")
            || upper.contains("OMITIDO")
            || upper.contains("DESCARTA EL SNAPSHOT")
        {
            line_is_error = true;
        }

        for needle in [
            "CODIGO DE SALIDA ",
            "CÓDIGO DE SALIDA ",
            "CODIGO ",
            "CÓDIGO ",
        ] {
            if let Some(pos) = upper.find(needle) {
                let rest = &upper[pos + needle.len()..];
                let num_str: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
                if let Ok(code) = num_str.parse::<i32>() {
                    if code == 143 || code == 130 {
                        line_is_cancel = true;
                    } else if code == 24 || code == 23 {
                        // Advertencias no fatales (archivos desaparecidos o en uso)
                    } else if code != 0 {
                        line_is_error = true;
                    }
                }
            }
        }

        if line_is_cancel {
            last_cancel_idx = Some(idx);
        } else if line_is_error {
            last_error_idx = Some(idx);
        }

        if upper.contains("INICIO (")
            || upper.contains("INICIANDO BACKUP")
            || upper.contains("=== BACKUP OMITIDO")
            || upper.contains("=== ABORTADO")
            || upper.contains("=== RESPALDO CANCELADO")
        {
            last_start_idx = Some(idx);
        }

        if upper.contains("SNAPSHOT COMPLETADO:")
            || upper.contains("BACKUP_COMPLETED")
            || upper.contains("BACKUP FINALIZADO CON ÉXITO")
            || upper.contains("BACKUP FINALIZADO CON EXITO")
        {
            last_success_idx = Some(idx);
        }
    }

    if let Some(cancel_idx) = last_cancel_idx {
        let is_after_success = last_success_idx.map_or(true, |succ| cancel_idx >= succ);
        let is_after_error = last_error_idx.map_or(true, |err| cancel_idx >= err);
        if is_after_success && is_after_error {
            return "Cancelada";
        }
    }

    match last_success_idx {
        None => "Error",
        Some(succ_idx) => {
            if let Some(err_idx) = last_error_idx {
                if err_idx > succ_idx {
                    return "Error";
                }
            }
            if let Some(start_idx) = last_start_idx {
                if start_idx > succ_idx {
                    return "Error";
                }
            }
            if has_physical_snapshot(dest_root, id) {
                "OK"
            } else {
                "Error"
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::classify_backup_status;

    #[test]
    fn clasifica_estados_de_respaldo() {
        assert_eq!(classify_backup_status(true, None, "/tmp", "t1"), "En curso");
        assert_eq!(
            classify_backup_status(false, None, "/tmp", "t1"),
            "PENDIENTE"
        );
        assert_eq!(
            classify_backup_status(false, Some(""), "/tmp", "t1"),
            "PENDIENTE"
        );
        assert_eq!(
            classify_backup_status(false, Some("   \n\n  "), "/tmp", "t1"),
            "PENDIENTE"
        );

        let err_log = "[2026-10-29 10:00:00] [t1] ERROR: se descarta el snapshot parcial (codigo de salida 1)";
        assert_eq!(
            classify_backup_status(false, Some(err_log), "/tmp", "t1"),
            "Error"
        );

        let abort_log = "[2026-10-29 10:00:00] [t1] === ABORTADO: espacio libre insuficiente ===";
        assert_eq!(
            classify_backup_status(false, Some(abort_log), "/tmp", "t1"),
            "Error"
        );

        let rsync_err = "[2026-10-29 10:00:00] [t1] ERROR: en replicación rsync (código 12)";
        assert_eq!(
            classify_backup_status(false, Some(rsync_err), "/tmp", "t1"),
            "Error"
        );

        let cancel_log = "[2026-10-29 10:00:00] [t1] === RESPALDO CANCELADO: la ejecución fue detenida manualmente por el usuario ===";
        assert_eq!(
            classify_backup_status(false, Some(cancel_log), "/tmp", "t1"),
            "Cancelada"
        );

        let term_log = "[2026-10-29 10:00:00] [t1] ERROR: se descarta el snapshot parcial (codigo de salida 143)";
        assert_eq!(
            classify_backup_status(false, Some(term_log), "/tmp", "t1"),
            "Cancelada"
        );

        let ok_log = "[2026-10-29 10:00:00] [t1] snapshot completado: /tmp/snapshot_2026";
        assert_eq!(
            classify_backup_status(false, Some(ok_log), "/tmp/nonexistent_backup_dir", "t1"),
            "Error"
        );

        let tmp_root = std::env::temp_dir().join(format!("test_bkp_ok_{}", std::process::id()));
        let task_dir = tmp_root.join("t1");
        let snap_dir = task_dir.join("snapshot_2026-10-29_100000");
        std::fs::create_dir_all(&snap_dir).expect("snap_dir");
        assert_eq!(
            classify_backup_status(false, Some(ok_log), tmp_root.to_str().unwrap(), "t1"),
            "OK"
        );

        let warning_23_log = "[2026-10-29 10:00:00] [t1] ADVERTENCIA: algunos ficheros en uso o bloqueados por usuarios no se pudieron leer (codigo 23: el resto de ficheros se respaldo con exito)\n[2026-10-29 10:01:00] [t1] snapshot completado: /tmp/snapshot_2026";
        assert_eq!(
            classify_backup_status(
                false,
                Some(warning_23_log),
                tmp_root.to_str().unwrap(),
                "t1"
            ),
            "OK"
        );

        let multi_log = "[2026-10-28 10:00:00] [t1] ERROR: se descarta el snapshot parcial (codigo de salida 1)\n\
                         [2026-10-29 10:00:00] [t1] inicio (cifs): //server/share\n\
                         [2026-10-29 10:00:05] [t1] snapshot completado: /tmp/snapshot_2026";
        assert_eq!(
            classify_backup_status(false, Some(multi_log), tmp_root.to_str().unwrap(), "t1"),
            "OK"
        );

        let fail_after_ok = "[2026-10-28 10:00:05] [t1] snapshot completado: /tmp/snapshot_2026\n\
                             [2026-10-29 10:00:00] [t1] inicio (cifs): //server/share\n\
                             [2026-10-29 10:00:01] [t1] ERROR: se descarta el snapshot parcial (codigo de salida 32)";
        assert_eq!(
            classify_backup_status(false, Some(fail_after_ok), tmp_root.to_str().unwrap(), "t1"),
            "Error"
        );

        let interrupted_after_ok =
            "[2026-10-28 10:00:05] [t1] snapshot completado: /tmp/snapshot_2026\n\
                                    [2026-10-29 10:00:00] [t1] inicio (cifs): //server/share";
        assert_eq!(
            classify_backup_status(
                false,
                Some(interrupted_after_ok),
                tmp_root.to_str().unwrap(),
                "t1"
            ),
            "Error"
        );

        let server_fmt_ok =
            "[2026-10-28 23:00:11] [t1] [BACKUP_COMPLETED] [notice] Respaldo finalizado con exito";
        assert_eq!(
            classify_backup_status(false, Some(server_fmt_ok), tmp_root.to_str().unwrap(), "t1"),
            "OK"
        );

        let server_fmt_err = "[2026-10-27 23:00:15] [t1] [BACKUP_COMPLETED] [notice] Respaldo finalizado con exito\n\
                              mount error(115): could not connect to 10.2.0.10\n\
                              [2026-10-28 23:00:21] [t1] [MOUNT_FAILED] [err] Error al montar recurso CIFS\n\
                              === se descarta el snapshot parcial ===\n\
                              [2026-10-28 23:00:21] [t1] [BACKUP_FAILED] [err] Respaldo fallido";
        assert_eq!(
            classify_backup_status(
                false,
                Some(server_fmt_err),
                tmp_root.to_str().unwrap(),
                "t1"
            ),
            "Error"
        );

        let _ = std::fs::remove_dir_all(&tmp_root);
    }
}
