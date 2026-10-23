//! Políticas de respaldo: nombres de snapshots, retención y control de espacio.

use chrono::{DateTime, Local};

/// Nombres de snapshot aceptados por el repositorio histórico.
pub const SNAPSHOT_PREFIX: &str = "snapshot_";
/// Prefijo de los directorios temporales de staging atómico.
pub const STAGING_PREFIX: &str = ".inprogress_";
/// Espacio libre mínimo exigido antes de iniciar una copia (2 GiB).
pub const MIN_FREE_BYTES: u64 = 2 * 1024 * 1024 * 1024;
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
    let line = output.lines().nth(1)?;
    let fields: Vec<&str> = line.split_whitespace().collect();
    if fields.len() < 5 {
        return None;
    }

    let available_kb: u64 = fields[3].parse().ok()?;
    let used_percent: f64 = fields[4].trim_end_matches('%').parse().ok()?;

    Some(DfUsage {
        free_bytes: available_kb.saturating_mul(1024),
        used_percent,
    })
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
}
