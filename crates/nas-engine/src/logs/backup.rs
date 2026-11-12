//! Interpretación de las bitácoras de respaldos (`LOGS_BACKUP`).

use super::{humanize, normalize_timestamp, LogEntry, LogSource};

/// Interpreta la bitácora de respaldos (formato con etiquetas y formato antiguo).
pub fn parse_backup_log(text: &str) -> Vec<LogEntry> {
    text.lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            if !trimmed.starts_with('[') {
                return None;
            }
            let rest = trimmed.strip_prefix('[')?;
            let (timestamp, rest) = rest.split_once(']')?;
            let timestamp = normalize_timestamp(timestamp);
            let rest = rest.trim_start();
            let task = if rest.starts_with('[') {
                let inner = rest.strip_prefix('[')?;
                let (task, after) = inner.split_once(']')?;
                (task.trim().to_string(), after.trim_start())
            } else {
                (String::new(), rest)
            };

            let mut entry = LogEntry::new(LogSource::Backup, timestamp, trimmed);
            entry.origin = "Tarea programada".to_string();
            entry.user = if task.0.is_empty() {
                "sistema".to_string()
            } else {
                task.0.clone()
            };
            entry.target = task.0.clone();

            // Formato estructurado: [tarea] [ETIQUETA] [nivel] mensaje
            if let Some(inner) = task.1.strip_prefix('[') {
                if let Some((tag, after_tag)) = inner.split_once(']') {
                    let after_tag = after_tag.trim_start();
                    let (level, message) = if let Some(inner) = after_tag.strip_prefix('[') {
                        match inner.split_once(']') {
                            Some((level, message)) => (level.trim(), message.trim()),
                            None => ("info", after_tag),
                        }
                    } else {
                        ("info", after_tag)
                    };
                    let (label, human) = backup_tag_sentence(tag.trim(), &task.0, message);
                    let mut entry = entry.finish(tag.trim(), label, human);
                    let lowered = level.to_ascii_lowercase();
                    if lowered.starts_with("err") || lowered.contains("error") {
                        entry = entry.fail();
                    } else if lowered.starts_with("warn") {
                        entry = entry.warn();
                    } else if lowered.starts_with("notice") {
                        entry = entry.notice();
                    }
                    return Some(entry);
                }
            }

            // Formato antiguo: [tarea] mensaje libre
            let message = task.1;
            let (label, human, failed) = backup_legacy_sentence(&task.0, message);
            let mut entry = entry.finish("respaldo", label, human);
            if failed {
                entry = entry.fail();
            } else if message.contains("descart") {
                entry = entry.warn();
            }
            Some(entry)
        })
        .collect()
}

fn backup_tag_sentence(tag: &str, task: &str, message: &str) -> (&'static str, String) {
    let task = if task.is_empty() { "sin nombre" } else { task };
    match tag {
        "LOCK_ACQUIRED" => (
            "Inició respaldo",
            format!("El respaldo «{task}» comenzó su ejecución"),
        ),
        "SPACE_CHECK" => (
            "Verificó espacio",
            format!(
                "Comprobó el espacio libre antes de copiar: {}",
                strip_prefix(message)
            ),
        ),
        "MOUNT_SUCCESS" => (
            "Origen conectado",
            format!("Conectó correctamente con el origen remoto del respaldo «{task}»"),
        ),
        "MOUNT_FAILED" => (
            "Error de conexión",
            format!("No se pudo conectar con el origen del respaldo «{task}»: {message}"),
        ),
        "RSYNC_COMPLETED" => (
            "Sincronización completada",
            format!("Copió y sincronizó los archivos del respaldo «{task}»"),
        ),
        "RSYNC_FAILED" => (
            "Error de copia",
            format!("Falló la copia de archivos del respaldo «{task}»: {message}"),
        ),
        "SNAPSHOT_PROMOTED" => (
            "Copia guardada",
            format!("Guardó una nueva copia de seguridad del respaldo «{task}»"),
        ),
        "BACKUP_COMPLETED" => (
            "Respaldo completado",
            format!("El respaldo «{task}» terminó correctamente"),
        ),
        "BACKUP_FAILED" => (
            "Respaldo fallido",
            format!("El respaldo «{task}» falló: {message}"),
        ),
        "RETENTION_PRUNED" | "SNAPSHOT_PRUNED" => (
            "Limpió copias antiguas",
            format!("Eliminó copias antiguas del respaldo «{task}» según la retención: {message}"),
        ),
        other => (
            "Evento de respaldo",
            format!(
                "El respaldo «{task}» registró «{}»: {message}",
                humanize(other)
            ),
        ),
    }
}

fn backup_legacy_sentence(task: &str, message: &str) -> (&'static str, String, bool) {
    let task = if task.is_empty() { "sin nombre" } else { task };
    let lowered = message.to_lowercase();
    if lowered.starts_with("inicio") {
        let origin = if lowered.contains("(cifs)") {
            "el recurso compartido de Windows"
        } else if lowered.contains("(ssh)") {
            "el servidor Linux remoto"
        } else {
            "el origen local"
        };
        return (
            "Inició respaldo",
            format!("El respaldo «{task}» comenzó a copiar desde {origin}"),
            false,
        );
    }
    if lowered.contains("error en replicación rsync")
        || lowered.contains("error en replicacion rsync")
    {
        let hint = rsync_code_hint(message);
        return (
            "Error de copia",
            format!("Falló la copia del respaldo «{task}»: {hint}"),
            true,
        );
    }
    if lowered.contains("se descarta") {
        return (
            "Copia descartada",
            format!("Descartó la copia incompleta de «{task}» para no dejar datos a medias"),
            false,
        );
    }
    if lowered.starts_with("fin") || lowered.contains("completado") {
        return (
            "Respaldo completado",
            format!("El respaldo «{task}» terminó correctamente"),
            false,
        );
    }
    (
        "Evento de respaldo",
        format!("El respaldo «{task}» registró: {message}"),
        false,
    )
}

/// Traduce un código de salida de `rsync` a una explicación en español.
pub fn rsync_code_hint(text: &str) -> String {
    let code = extract_number(text);
    let reason = match code {
        Some(1) => "error interno de rsync al armar la orden",
        Some(2) => "el servidor remoto usa una versión incompatible de rsync",
        Some(3) => "no pudo leer los archivos de origen",
        Some(4) => "operación no soportada por el sistema de archivos",
        Some(5) => "el cliente y el servidor no se entendieron (protocolo)",
        Some(6) => "no pudo actualizar su archivo de registro",
        Some(10) => "se perdió la conexión con el origen durante la copia",
        Some(11) => "error de lectura o escritura en el origen",
        Some(12) => "se corrompió el flujo de datos entre equipos",
        Some(13) => "fallo en las comprobaciones de integridad",
        Some(14) => "fallo de comunicación interna entre procesos",
        Some(20) => "la copia fue interrumpida por una señal",
        Some(21) => "error del sistema al esperar un proceso",
        Some(22) => "el equipo no tiene memoria suficiente para continuar",
        Some(23) => "la copia quedó parcial por errores en algunos archivos",
        Some(24) => "algunos archivos desaparecieron del origen durante la copia",
        Some(25) => "se alcanzó el límite máximo de borrado configurado",
        Some(30) => "el origen dejó de responder (tiempo de espera agotado)",
        Some(35) => "el servidor de respaldo no respondió a tiempo",
        Some(other) => return format!("código de error {other} de rsync (ver registro técnico)"),
        None => return "error de sincronización (ver registro técnico)".to_string(),
    };
    match code {
        Some(code) => format!("{reason} (rsync {code})"),
        None => reason.to_string(),
    }
}

fn extract_number(text: &str) -> Option<u32> {
    let start = text.find(|c: char| c.is_ascii_digit())?;
    let digits: String = text[start..]
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect();
    digits.parse().ok()
}

fn strip_prefix(message: &str) -> String {
    message
        .split_once(':')
        .map(|(_, rest)| rest.trim().to_string())
        .unwrap_or_else(|| message.trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interpreta_bitacora_estructurada_de_respaldos() {
        let text = "\
[2026-11-04 16:55:42] [didi] [LOCK_ACQUIRED] [info] Bloqueo de ejecucion exclusivo adquirido
[2026-11-04 16:55:42] [didi] [SPACE_CHECK] [info] Espacio verificado: 39428 MB libres (84% en uso)
[2026-11-04 16:55:48] [didi] [SNAPSHOT_PROMOTED] [notice] Snapshot promovido: /srv/nas/x
[2026-11-04 16:55:48] [didi] [BACKUP_COMPLETED] [notice] Respaldo CIFS didi finalizado con exito";
        let entries = parse_backup_log(text);
        assert_eq!(entries.len(), 4);
        assert_eq!(entries[0].action_label, "Inició respaldo");
        assert!(entries[0].human.contains("«didi»"));
        assert_eq!(entries[1].action_label, "Verificó espacio");
        assert!(entries[1].human.contains("39428 MB libres"));
        assert_eq!(entries[3].action_label, "Respaldo completado");
        assert_eq!(entries[3].level, "notice");
    }

    #[test]
    fn interpreta_bitacora_antigua_y_explica_rsync() {
        let text = "\
[2026-10-28 23:00:01] [Creditas] inicio (cifs): /mnt/backup_sources/Creditas/
[2026-10-28 23:11:54] [Creditas] error en replicación rsync (código 11)
[2026-10-28 23:11:54] [Creditas] === se descarta el snapshot parcial ===";
        let entries = parse_backup_log(text);
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].action_label, "Inició respaldo");
        assert!(entries[0].human.contains("recurso compartido de Windows"));
        assert_eq!(entries[1].status, "FAILED");
        assert!(entries[1]
            .human
            .contains("error de lectura o escritura en el origen"));
        assert!(entries[1].human.contains("rsync 11"));
        assert_eq!(entries[2].action_label, "Copia descartada");
    }
}
