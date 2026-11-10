use std::path::PathBuf;

use axum::{
    extract::{Query, State},
    http::HeaderMap,
    response::{IntoResponse, Response},
};

use crate::{
    AppState,
    routes::common::{ApiResponse, ExtQuery, require_auth, run_json, server_error},
};

/// Logs recientes y legibles: Samba, auditoría del panel, respaldos y journald.
pub async fn logs(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<ExtQuery>,
) -> Response {
    if let Err(response) = require_auth(&state, &headers) {
        return *response;
    }
    let limit = query.limit.unwrap_or(100).clamp(1, 500);
    // Por defecto: SOLO eventos del panel web, respaldos y auditoría de Samba.
    // El ruido interno del kernel (`system`) se muestra sólo si el usuario pide
    // explícitamente `source=system`.
    let default_sources = &["admin", "backup", "samba_audit"];
    let source = query.source.clone().unwrap_or_default();
    let search = query.q.clone().unwrap_or_default();
    let grouped = query.grouped != Some(0);

    let outcome = tokio::task::spawn_blocking(move || {
        let selected = nas_engine::LogSource::parse(&source).map(|choice| vec![choice]);
        let selected = selected.unwrap_or_else(|| {
            default_sources
                .iter()
                .filter_map(|name| nas_engine::LogSource::parse(name))
                .collect()
        });
        let list = selected.clone();
        let wants = move |candidate: nas_engine::LogSource| list.contains(&candidate);
        let window = limit.saturating_mul(4);
        let window_arg = window.to_string();
        let mut entries: Vec<nas_engine::LogEntry> = Vec::new();

        if wants(nas_engine::LogSource::SambaAudit) {
            let path = std::env::var("NAS_SAMBA_AUDIT_LOG")
                .map(PathBuf::from)
                .unwrap_or_else(|_| PathBuf::from("/var/log/samba/audit.log"));
            entries.extend(nas_engine::parse_samba_audit(&read_tail(&path, window)));
        }

        if wants(nas_engine::LogSource::Admin) {
            if let Ok(rows) = state.auth.db().recent_audit(limit) {
                let events: Vec<nas_engine::AdminEvent<'_>> = rows
                    .iter()
                    .map(|row| nas_engine::AdminEvent {
                        ts: row.ts,
                        username: &row.username,
                        action: &row.action,
                        target: &row.target,
                        result: &row.result,
                        detail: &row.detail,
                        origin_ip: &row.origin_ip,
                    })
                    .collect();
                entries.extend(nas_engine::parse_admin_events(&events));
            }
        }

        if wants(nas_engine::LogSource::Backup) {
            let path = std::env::var("NAS_BACKUP_MASTER_LOG")
                .map(PathBuf::from)
                .unwrap_or_else(|_| {
                    state
                        .paths
                        .data_root
                        .join("LOGS_BACKUP")
                        .join("backups_master.log")
                });
            entries.extend(nas_engine::parse_backup_log(&read_tail(&path, window)));
        }

        if wants(nas_engine::LogSource::System) {
            let spec = nas_exec::CommandSpec::new("journalctl")
                .args(["-n", window_arg.as_str(), "--no-pager", "-o", "short-iso"])
                .timeout(std::time::Duration::from_secs(20));
            if let Ok(text) = run_json(spec) {
                entries.extend(nas_engine::parse_journal(&text));
            }
        }

        entries = nas_engine::filter_query(entries, &search);
        nas_engine::sort_recent(&mut entries);
        if grouped {
            entries = nas_engine::group_repeated(entries);
        }
        entries.truncate(limit);

        entries
            .into_iter()
            .filter_map(|entry| {
                let human = entry.human.clone();
                let mut value = serde_json::to_value(entry).ok()?;
                // Compatibilidad con el `app.js` original: usa `message` cuando
                // no hay `human` (p. ej. el resumen del panel de inicio).
                value["message"] = serde_json::Value::String(human);
                Some(value)
            })
            .collect::<Vec<serde_json::Value>>()
    })
    .await;

    match outcome {
        Ok(entries) => {
            axum::Json(ApiResponse::ok(entries, "Registros del sistema")).into_response()
        }
        Err(_) => server_error("lectura de logs interrumpida"),
    }
}

/// Lee únicamente las últimas `max_lines` líneas de un archivo de bitácora.
///
/// Solo se leen los últimos 256 KB del archivo para que el panel pueda
/// refrescarse con frecuencia sin cargar bitácoras históricas completas.
pub fn read_tail(path: &std::path::Path, max_lines: usize) -> String {
    const WINDOW: u64 = 256 * 1024;
    let mut file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(_) => return String::new(),
    };
    let length = file.metadata().map(|meta| meta.len()).unwrap_or(0);
    let start = length.saturating_sub(WINDOW);
    if start > 0 && std::io::Seek::seek(&mut file, std::io::SeekFrom::Start(start)).is_err() {
        return String::new();
    }
    let mut bytes = Vec::new();
    if std::io::Read::read_to_end(&mut file, &mut bytes).is_err() {
        return String::new();
    }
    let buffer = String::from_utf8_lossy(&bytes);
    let text = if start > 0 {
        // Al comenzar a media línea se descarta el primer fragmento.
        buffer.split_once('\n').map_or("", |(_, rest)| rest)
    } else {
        buffer.as_ref()
    };
    let lines: Vec<&str> = text.lines().collect();
    let tail = lines.len().saturating_sub(max_lines.max(1));
    lines[tail..].join("\n")
}
