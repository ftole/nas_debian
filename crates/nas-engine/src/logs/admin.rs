//! Interpretación de la auditoría de acciones administrativas del panel web.

use super::{epoch_to_iso, humanize, LogEntry, LogSource};

/// Fila de auditoría del panel web.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AdminEvent<'a> {
    /// Marca de tiempo Unix (segundos).
    pub ts: i64,
    /// Usuario que ejecutó la acción.
    pub username: &'a str,
    /// Acción registrada (`login`, `file_delete`, ...).
    pub action: &'a str,
    /// Objeto afectado.
    pub target: &'a str,
    /// Resultado (`SUCCESS`, `FAILED`, `REQUESTED`, ...).
    pub result: &'a str,
    /// Detalle libre.
    pub detail: &'a str,
    /// IP de origen del cliente (vacía en registros históricos).
    #[allow(dead_code)]
    pub origin_ip: &'a str,
}

/// Interpreta filas de la auditoría del panel web.
pub fn parse_admin_events(events: &[AdminEvent<'_>]) -> Vec<LogEntry> {
    events
        .iter()
        .map(|event| {
            let timestamp = epoch_to_iso(event.ts);
            let raw = format!(
                "{} {} {} {} {} {}",
                event.action,
                event.target,
                event.result,
                event.username,
                event.detail,
                event.origin_ip
            );
            let mut entry = LogEntry::new(LogSource::Admin, timestamp, raw.trim());
            entry.user = event.username.to_string();
            entry.ip = event.origin_ip.to_string();
            entry.target = event.target.to_string();
            entry.origin = if event.action.starts_with("terminal") {
                "Terminal web".to_string()
            } else {
                "Panel web".to_string()
            };

            let (label, sentence) = admin_sentence(event.action, event.username, event.target);

            let human = if event.detail.trim().is_empty() {
                sentence
            } else {
                format!("{sentence} — {}", event.detail.trim())
            };

            let mut entry = entry.finish(event.action, label, human);
            entry = match event.result.to_ascii_uppercase().as_str() {
                "FAILED" | "ERROR" => entry.fail(),
                "REQUESTED" | "STARTED" | "PENDING" => entry.notice().warn(),
                _ => entry,
            };
            entry
        })
        .collect()
}

fn admin_sentence(action: &str, user: &str, target: &str) -> (&'static str, String) {
    let actor = if user.is_empty() {
        "Un administrador"
    } else {
        user
    };
    let object = if target.is_empty() || target == "-" {
        "el panel"
    } else {
        target
    };
    match action {
        "login" => {
            let place = if target == "api" {
                "la API del panel"
            } else {
                "el panel web"
            };
            ("Inició sesión", format!("{actor} inició sesión en {place}"))
        }
        "logout" => ("Cerró sesión", format!("{actor} cerró sesión en el panel")),
        "backup_run" => (
            "Ejecutó respaldo",
            format!("{actor} inició manualmente el respaldo «{object}»"),
        ),
        "backup_stop" => (
            "Detuvo respaldo",
            format!("{actor} detuvo el respaldo «{object}»"),
        ),
        "backup_create" => (
            "Creó tarea de respaldo",
            format!("{actor} creó la tarea de respaldo «{object}»"),
        ),
        "backup_update" => (
            "Modificó tarea de respaldo",
            format!("{actor} modificó la tarea de respaldo «{object}»"),
        ),
        "backup_delete" => (
            "Eliminó tarea de respaldo",
            format!("{actor} eliminó la tarea de respaldo «{object}»"),
        ),
        "backup_restore" => (
            "Restauró copia",
            format!("{actor} restauró una copia del respaldo «{object}»"),
        ),
        "backup_freeze" => (
            "Marcó copia permanente",
            format!("{actor} marcó como permanente una copia de «{object}»"),
        ),
        "file_delete" => (
            "Eliminó archivo",
            format!("{actor} eliminó «{object}» del almacenamiento"),
        ),
        "file_upload" => (
            "Subió archivo",
            format!("{actor} subió «{object}» al almacenamiento"),
        ),
        "file_download" => ("Descargó archivo", format!("{actor} descargó «{object}»")),
        "file_mkdir" => (
            "Creó carpeta",
            format!("{actor} creó la carpeta «{object}»"),
        ),
        "file_rename" => ("Renombró archivo", format!("{actor} renombró «{object}»")),
        "share_create" => (
            "Creó recurso compartido",
            format!("{actor} creó el recurso compartido «{object}»"),
        ),
        "share_update" => (
            "Modificó recurso",
            format!("{actor} modificó el recurso compartido «{object}»"),
        ),
        "share_delete" => (
            "Eliminó recurso",
            format!("{actor} eliminó el recurso compartido «{object}»"),
        ),
        "share_access" => (
            "Cambió permisos",
            format!("{actor} cambió los permisos del recurso «{object}»"),
        ),
        "share_acl_repair" => (
            "Reparó permisos",
            format!("{actor} reparó los permisos de los recursos compartidos"),
        ),
        "user_create" => (
            "Creó usuario",
            format!("{actor} creó el usuario «{object}»"),
        ),
        "user_update" => (
            "Modificó usuario",
            format!("{actor} modificó el usuario «{object}»"),
        ),
        "user_toggle" => (
            "Activó o desactivó usuario",
            format!("{actor} cambió el estado del usuario «{object}»"),
        ),
        "user_delete" => (
            "Eliminó usuario",
            format!("{actor} eliminó el usuario «{object}»"),
        ),
        "group_create" => ("Creó grupo", format!("{actor} creó el grupo «{object}»")),
        "group_delete" => (
            "Eliminó grupo",
            format!("{actor} eliminó el grupo «{object}»"),
        ),
        "domain_join" => (
            "Unió al dominio",
            format!("{actor} unió el servidor al dominio «{object}»"),
        ),
        "domain_leave" => (
            "Salió del dominio",
            format!("{actor} retiró el servidor del dominio"),
        ),
        "terminal_start" => (
            "Abrió terminal",
            format!("{actor} abrió una terminal web ({object})"),
        ),
        "terminal_kill" => (
            "Cerró terminal",
            format!("{actor} cerró la terminal web ({object})"),
        ),
        "system_reboot" => (
            "Solicitó reinicio",
            format!("{actor} solicitó reiniciar el servidor"),
        ),
        "system_update" => (
            "Actualizó el sistema",
            format!("{actor} lanzó una actualización del sistema"),
        ),
        other => (
            "Acción administrativa",
            format!("{actor} ejecutó «{}» sobre «{object}»", humanize(other)),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn traduce_acciones_del_panel() {
        let events = [
            AdminEvent {
                ts: 1_793_834_688,
                username: "admin",
                action: "file_delete",
                target: "SISTEMAS/sdfdsf.txt",
                result: "SUCCESS",
                detail: "",
                origin_ip: "",
            },
            AdminEvent {
                ts: 1_793_832_965,
                username: "admin",
                action: "backup_run",
                target: "didi_admin",
                result: "SUCCESS",
                detail: "tarea didi_admin iniciada",
                origin_ip: "",
            },
        ];
        let entries = parse_admin_events(&events);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].action_label, "Eliminó archivo");
        assert!(entries[0].human.contains("eliminó «SISTEMAS/sdfdsf.txt»"));
        assert_eq!(entries[1].action_label, "Ejecutó respaldo");
        assert!(entries[1].human.contains("tarea didi_admin iniciada"));
    }
}
