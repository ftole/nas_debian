//! Interpretación de la bitácora de auditoría de Samba (`full_audit`).

use super::{normalize_timestamp, LogEntry, LogSource};

/// Interpreta la auditoría de Samba (`user|ip|equipo|recurso|operación|resultado|ruta`).
pub fn parse_samba_audit(text: &str) -> Vec<LogEntry> {
    text.lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                return None;
            }
            let (head, payload) = trimmed.split_once("smbd_audit:")?;
            let mut parts = payload.trim().splitn(7, '|');
            let user = parts.next().unwrap_or_default().trim().to_string();
            let ip = parts.next().unwrap_or_default().trim().to_string();
            let machine = parts.next().unwrap_or_default().trim().to_string();
            let share = parts.next().unwrap_or_default().trim().to_string();
            let operation = parts.next().unwrap_or_default().trim().to_string();
            let result = parts.next().unwrap_or_default().trim().to_string();
            let path = parts.next().unwrap_or_default().trim().to_string();

            let timestamp = normalize_timestamp(head.split_whitespace().next().unwrap_or_default());
            let mut entry = LogEntry::new(LogSource::SambaAudit, timestamp, trimmed);
            entry.user = user.clone();
            entry.ip = ip.clone();
            entry.origin = "Equipo remoto".to_string();
            entry.origin_detail = if machine.is_empty() {
                ip.clone()
            } else {
                machine.clone()
            };

            let object = if path.is_empty() {
                share.clone()
            } else {
                path.clone()
            };
            entry.target = object.clone();

            let (label, sentence) =
                samba_sentence(&operation, &user, &machine, &ip, &share, &object);

            let failed = !result.eq_ignore_ascii_case("ok")
                && !result.eq_ignore_ascii_case("success")
                && !result.is_empty();

            let human = if failed {
                format!(
                    "No se pudo completar la operación de Samba: {sentence} (resultado: {result})"
                )
            } else {
                sentence
            };

            let mut entry = entry.finish(&operation, label, human);
            if failed {
                entry = entry.fail();
            }
            Some(entry)
        })
        .collect()
}

fn samba_sentence(
    operation: &str,
    user: &str,
    machine: &str,
    ip: &str,
    share: &str,
    object: &str,
) -> (&'static str, String) {
    let actor = if user.is_empty() { "Un usuario" } else { user };
    let origin = match (machine.is_empty(), ip.is_empty()) {
        (false, false) => format!(" desde el equipo {machine} ({ip})"),
        (true, false) => format!(" desde {ip}"),
        (false, true) => format!(" desde el equipo {machine}"),
        (true, true) => String::new(),
    };

    match operation {
        "connect" => (
            "Conexión",
            format!("{actor} se conectó al recurso «{share}»{origin}"),
        ),
        "disconnect" => (
            "Desconexión",
            format!("{actor} cerró la conexión con el recurso «{share}»"),
        ),
        "open" | "openat" => (
            "Abrió archivo",
            format!("{actor} abrió «{object}» en el recurso «{share}»{origin}"),
        ),
        "unlinkat" | "unlink" => (
            "Eliminó archivo",
            format!("{actor} eliminó «{object}» del recurso «{share}»{origin}"),
        ),
        "renameat" | "rename" => (
            "Renombró",
            format!("{actor} renombró «{object}» en el recurso «{share}»{origin}"),
        ),
        "mkdirat" | "mkdir" => (
            "Creó carpeta",
            format!("{actor} creó la carpeta «{object}» en el recurso «{share}»{origin}"),
        ),
        "chmod" | "chmodat" => (
            "Cambió permisos",
            format!("{actor} cambió los permisos de «{object}»{origin}"),
        ),
        "chown" | "chownat" => (
            "Cambió propietario",
            format!("{actor} cambió el propietario de «{object}»{origin}"),
        ),
        "" => (
            "Actividad de archivos",
            format!("{actor} realizó una operación en el recurso «{share}»{origin}"),
        ),
        other => (
            "Actividad de archivos",
            format!("{actor} realizó «{other}» sobre «{object}»{origin}"),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn describe_accesos_samba() {
        let text = "2026-11-03T17:35:22.862852-06:00 tsdebian smbd_audit: sistemas|10.10.1.250|sis-frank|CONTABILIDAD|openat|ok|CONTABILIDAD/presupuesto.xlsx";
        let entries = parse_samba_audit(text);
        assert_eq!(entries.len(), 1);
        let entry = &entries[0];
        assert_eq!(entry.source, LogSource::SambaAudit);
        assert_eq!(entry.user, "sistemas");
        assert_eq!(entry.ip, "10.10.1.250");
        assert_eq!(entry.status, "SUCCESS");
        assert!(entry
            .human
            .contains("abrió «CONTABILIDAD/presupuesto.xlsx»"));
        assert!(entry.human.contains("sis-frank"));
    }

    #[test]
    fn marca_fallos_samba() {
        let text = "2026-11-03T17:35:22-06:00 tsdebian smbd_audit: sistemas|10.10.1.250|sis-frank|CONTABILIDAD|openat|fail|CONTABLE.xlsx";
        let entries = parse_samba_audit(text);
        assert_eq!(entries[0].status, "FAILED");
        assert_eq!(entries[0].level, "error");
        assert!(entries[0].human.starts_with("No se pudo completar"));
    }
}
