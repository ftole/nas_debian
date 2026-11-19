//! Políticas de respaldo: nombres de snapshots, retención y control de espacio.

use std::path::Path;

use chrono::{DateTime, Local};

/// Nombres de snapshot aceptados por el repositorio histórico.
pub const SNAPSHOT_PREFIX: &str = "snapshot_";
/// Prefijo de los directorios temporales de staging atómico.
pub const STAGING_PREFIX: &str = ".inprogress_";
/// Espacio libre mínimo exigido antes de iniciar una copia (2 GiB).
pub const MIN_FREE_BYTES: u64 = 2 * 1024 * 1024 * 1024;
/// Techo máximo de reserva exigida en volúmenes grandes (32 GiB).
pub const MAX_HYBRID_FREE_BYTES: u64 = 32 * 1024 * 1024 * 1024;
/// Umbral de advertencia de ocupación (%).
pub const WARN_USED_PERCENT: f64 = 85.0;
/// Umbral de cancelación de ocupación (%).
pub const ABORT_USED_PERCENT: f64 = 95.0;

/// Decisión sobre el espacio disponible antes de copiar.
#[derive(Debug, Clone, PartialEq)]
pub enum SpaceDecision {
    /// Hay espacio suficiente.
    Ok,
    /// Hay espacio, pero conviene avisar en la bitácora.
    Warn {
        /// Porcentaje de ocupación actual.
        used_percent: f64,
    },
    /// No se debe iniciar la copia.
    Abort {
        /// Motivo legible.
        reason: String,
    },
}

/// Nombre del directorio de snapshot para una fecha dada.
pub fn snapshot_dir_name(now: &DateTime<Local>) -> String {
    format!("{SNAPSHOT_PREFIX}{}", now.format("%Y-%m-%d_%H%M%S"))
}

/// Nombre del directorio temporal de staging para una fecha dada.
pub fn staging_dir_name(now: &DateTime<Local>) -> String {
    format!("{STAGING_PREFIX}{}", now.format("%Y-%m-%d_%H%M%S"))
}

/// Selecciona los snapshots que deben eliminarse para respetar la retención.
///
/// Equivale a `ls -1 | sort | head -n -RETENTION`: conserva los más recientes
/// (comparación lexicográfica, válida porque el formato de fecha es ordenable).
pub fn retention_victims(existing: &[String], retention: usize) -> Vec<String> {
    let mut sorted: Vec<String> = existing.to_vec();
    sorted.sort();

    if retention == 0 {
        return sorted;
    }
    if sorted.len() <= retention {
        return Vec::new();
    }

    let surplus = sorted.len() - retention;
    sorted.into_iter().take(surplus).collect()
}

/// Calcula el espacio libre mínimo híbrido según el tamaño total del volumen:
/// 2% del volumen acotado entre `MIN_FREE_BYTES` (2 GiB) y `MAX_HYBRID_FREE_BYTES` (32 GiB).
pub fn min_free_bytes_for_volume(total_bytes: u64) -> u64 {
    if total_bytes == 0 {
        MIN_FREE_BYTES
    } else {
        (total_bytes / 50).clamp(MIN_FREE_BYTES, MAX_HYBRID_FREE_BYTES)
    }
}

/// Porcentaje máximo de ocupación permitido antes de abortar según el tamaño del volumen.
pub fn abort_used_percent_for_volume(total_bytes: u64) -> f64 {
    if total_bytes >= 2 * 1024 * 1024 * 1024 * 1024 {
        98.0
    } else if total_bytes >= 512 * 1024 * 1024 * 1024 {
        97.0
    } else {
        ABORT_USED_PERCENT
    }
}

/// Evalúa si se puede iniciar o continuar una copia aplicando umbral híbrido según el volumen.
pub fn evaluate_space_for_volume(
    total_bytes: u64,
    free_bytes: u64,
    used_percent: f64,
) -> SpaceDecision {
    if total_bytes == 0 {
        return evaluate_space(free_bytes, used_percent);
    }
    let abort_pct = abort_used_percent_for_volume(total_bytes);
    if used_percent > abort_pct {
        return SpaceDecision::Abort {
            reason: format!(
                "ocupación del {used_percent:.1}% superior al límite híbrido del {abort_pct:.0}%"
            ),
        };
    }

    let min_free = min_free_bytes_for_volume(total_bytes);
    if free_bytes < min_free {
        return SpaceDecision::Abort {
            reason: format!(
                "espacio libre insuficiente: {} MiB (mínimo híbrido {} MiB)",
                free_bytes / 1024 / 1024,
                min_free / 1024 / 1024,
            ),
        };
    }

    let warn_pct = (abort_pct - 8.0).max(WARN_USED_PERCENT);
    if used_percent > warn_pct {
        return SpaceDecision::Warn { used_percent };
    }

    SpaceDecision::Ok
}

/// Evalúa si se puede iniciar una copia con el espacio disponible.
pub fn evaluate_space(free_bytes: u64, used_percent: f64) -> SpaceDecision {
    if used_percent > ABORT_USED_PERCENT {
        return SpaceDecision::Abort {
            reason: format!(
                "ocupación del {used_percent:.1}% superior al límite del {ABORT_USED_PERCENT:.0}%"
            ),
        };
    }

    if free_bytes < MIN_FREE_BYTES {
        return SpaceDecision::Abort {
            reason: format!(
                "espacio libre insuficiente: {} MiB (mínimo 2 GiB)",
                free_bytes / 1024 / 1024
            ),
        };
    }

    if used_percent > WARN_USED_PERCENT {
        return SpaceDecision::Warn { used_percent };
    }

    SpaceDecision::Ok
}

/// Uso del sistema de archivos según `df -Pk`.
#[derive(Debug, Clone, PartialEq)]
pub struct DfUsage {
    /// Bytes libres.
    pub free_bytes: u64,
    /// Porcentaje de ocupación.
    pub used_percent: f64,
}

/// Interpreta la salida de `df -Pk <ruta>` (segunda línea).
pub fn parse_df(output: &str) -> Option<DfUsage> {
    parse_df_with_total(output).map(|(_, usage)| usage)
}

/// Interpreta la salida de `df -Pk <ruta>` devolviendo `(total_bytes, DfUsage)`.
pub fn parse_df_with_total(output: &str) -> Option<(u64, DfUsage)> {
    let line = output.lines().nth(1)?;
    let fields: Vec<&str> = line.split_whitespace().collect();
    if fields.len() < 5 {
        return None;
    }

    let total_kb: u64 = fields[1].parse().ok()?;
    let available_kb: u64 = fields[3].parse().ok()?;
    let used_percent: f64 = fields[4].trim_end_matches('%').parse().ok()?;

    Some((
        total_kb.saturating_mul(1024),
        DfUsage {
            free_bytes: available_kb.saturating_mul(1024),
            used_percent,
        },
    ))
}

/// Elimina directorios temporales huérfanos `.inprogress_*` dentro de `task_dir`
/// cuando ninguna ejecución de la tarea mantiene el cerrojo exclusivo.
pub fn clean_orphan_inprogress_dirs(task_dir: &Path) -> usize {
    let Ok(entries) = std::fs::read_dir(task_dir) else {
        return 0;
    };
    let mut removed = 0usize;
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        if !name_str.starts_with(STAGING_PREFIX) {
            continue;
        }
        let path = entry.path();
        if path.is_dir() && std::fs::remove_dir_all(&path).is_ok() {
            removed += 1;
        }
    }
    removed
}

#[cfg(test)]
mod tests {
    use super::{
        evaluate_space, parse_df, retention_victims, snapshot_dir_name, staging_dir_name,
        SpaceDecision, MIN_FREE_BYTES,
    };
    use chrono::TimeZone;

    #[test]
    fn nombra_snapshots_y_staging_con_marca_de_tiempo() {
        let now = chrono::Local
            .with_ymd_and_hms(2026, 10, 22, 15, 4, 9)
            .single()
            .expect("fecha válida");
        assert_eq!(snapshot_dir_name(&now), "snapshot_2026-10-22_150409");
        assert_eq!(staging_dir_name(&now), ".inprogress_2026-10-22_150409");
    }

    #[test]
    fn retencion_conserva_los_mas_recientes() {
        let snaps: Vec<String> = (1..=7)
            .map(|day| format!("snapshot_2026-10-{day:02}_020000"))
            .collect();
        let victims = retention_victims(&snaps, 3);
        assert_eq!(
            victims,
            vec![
                "snapshot_2026-10-01_020000",
                "snapshot_2026-10-02_020000",
                "snapshot_2026-10-03_020000",
                "snapshot_2026-10-04_020000",
            ]
        );
    }

    #[test]
    fn retencion_sin_excedente_no_borra_nada() {
        let snaps = vec!["snapshot_2026-10-01_020000".to_string()];
        assert!(retention_victims(&snaps, 5).is_empty());
        assert!(retention_victims(&[], 5).is_empty());
    }

    #[test]
    fn espacio_suficiente_es_ok() {
        assert_eq!(
            evaluate_space(50 * 1024 * 1024 * 1024, 40.0),
            SpaceDecision::Ok
        );
    }

    #[test]
    fn espacio_ajustado_solo_advierte() {
        assert_eq!(
            evaluate_space(50 * 1024 * 1024 * 1024, 88.0),
            SpaceDecision::Warn { used_percent: 88.0 }
        );
    }

    #[test]
    fn interpreta_df_con_espacio_suficiente() {
        let output = "S.ficheros  bloques de 1024  Usados  Disponibles  Capacidad  Montado
/dev/sdb1  1921800000  900000000  1021800000  47%  /srv/nas
";
        let usage = parse_df(output).expect("df válido");
        assert_eq!(usage.free_bytes, 1021800000 * 1024);
        assert!((usage.used_percent - 47.0).abs() < f64::EPSILON);
        assert_eq!(
            evaluate_space(usage.free_bytes, usage.used_percent),
            SpaceDecision::Ok
        );
    }

    #[test]
    fn df_critico_aborta_la_copia() {
        let output = "F 1024-blocks Used Available Capacity Mounted
/dev/sdb1 1000 960 40 96% /srv/nas
";
        let usage = parse_df(output).expect("df válido");
        assert!(matches!(
            evaluate_space(usage.free_bytes, usage.used_percent),
            SpaceDecision::Abort { .. }
        ));
    }

    #[test]
    fn df_invalido_devuelve_none() {
        assert!(parse_df("").is_none());
        assert!(parse_df(
            "solo cabecera
"
        )
        .is_none());
        assert!(parse_df(
            "a b c d e f
"
        )
        .is_none());
    }

    #[test]
    fn espacio_critico_aborta() {
        assert!(matches!(
            evaluate_space(MIN_FREE_BYTES - 1, 40.0),
            SpaceDecision::Abort { .. }
        ));
        assert!(matches!(
            evaluate_space(50 * 1024 * 1024 * 1024, 96.0),
            SpaceDecision::Abort { .. }
        ));
    }

    #[test]
    fn umbral_hibrido_adapta_limites_a_volumenes_grandes_y_pequenos() {
        let tb10: u64 = 10 * 1024 * 1024 * 1024 * 1024;
        // 10 TB al 96% con 400 GiB libres -> solo advierte (no desperdicia 400 GiB).
        assert_eq!(
            super::evaluate_space_for_volume(tb10, 400 * 1024 * 1024 * 1024, 96.0),
            SpaceDecision::Warn { used_percent: 96.0 }
        );
        // 10 TB con 20 GiB libres (< 32 GiB mínimos híbridos) -> aborta.
        assert!(matches!(
            super::evaluate_space_for_volume(tb10, 20 * 1024 * 1024 * 1024, 97.5),
            SpaceDecision::Abort { .. }
        ));
        // 10 TB al 99% -> aborta.
        assert!(matches!(
            super::evaluate_space_for_volume(tb10, 100 * 1024 * 1024 * 1024, 99.0),
            SpaceDecision::Abort { .. }
        ));
        // Volumen pequeño de 20 GiB con 1.5 GiB libres (< 2 GiB piso) -> aborta.
        let gb20: u64 = 20 * 1024 * 1024 * 1024;
        assert!(matches!(
            super::evaluate_space_for_volume(gb20, 1536 * 1024 * 1024, 92.0),
            SpaceDecision::Abort { .. }
        ));
    }

    #[test]
    fn limpia_solo_directorios_inprogress_huerfanos() {
        let root = std::env::temp_dir().join(format!("nas-orphan-inprog-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join(".inprogress_2026-11-18_100000")).unwrap();
        std::fs::write(
            root.join(".inprogress_2026-11-18_100000/archivo.bin"),
            b"abc",
        )
        .unwrap();
        std::fs::create_dir_all(root.join("snapshot_2026-11-18_090000")).unwrap();

        assert_eq!(super::clean_orphan_inprogress_dirs(&root), 1);
        assert!(!root.join(".inprogress_2026-11-18_100000").exists());
        assert!(root.join("snapshot_2026-11-18_090000").exists());

        let _ = std::fs::remove_dir_all(&root);
    }
}
