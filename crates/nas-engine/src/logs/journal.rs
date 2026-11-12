//! Interpretación de eventos del sistema en `journald`.

use super::{normalize_timestamp, LogEntry, LogSource};

/// Interpreta `journalctl -o short-iso`, descartando ruido y traduciendo eventos.
pub fn parse_journal(text: &str) -> Vec<LogEntry> {
    text.lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                return None;
            }
            let mut parts = trimmed.splitn(3, ' ');
            let timestamp = normalize_timestamp(parts.next().unwrap_or_default());
            let host = parts.next().unwrap_or_default();
            let remainder = parts.next().unwrap_or(trimmed);
            let (unit_raw, message) = match remainder.split_once(':') {
                Some((unit, message)) => (unit.trim(), message.trim()),
                None => ("systemd", remainder.trim()),
            };
            if is_journal_noise(unit_raw, message) {
                return None;
            }
            let unit = unit_base(unit_raw);
            let friendly = friendly_unit(&unit);
            let mut entry = LogEntry::new(LogSource::System, timestamp, trimmed);
            entry.origin = "Sistema".to_string();
            entry.origin_detail = host.to_string();
            entry.target = if host.is_empty() {
                unit.clone()
            } else {
                format!("{host}/{unit}")
            };

            let (label, human) = journal_sentence(&unit, friendly, message);
            let mut entry = entry.finish(unit_raw, label, human);

            if !host.is_empty() {
                entry.ip = extract_ip(message).unwrap_or_default();
            }

            if looks_like_error(message) {
                entry = entry.fail();
            } else if message.starts_with("Warning") || message.contains("warning") {
                entry = entry.warn();
            }
            Some(entry)
        })
        .collect()
}

fn is_journal_noise(unit: &str, message: &str) -> bool {
    let lowered = message.to_lowercase();
    if lowered.contains("debian-sa1") || lowered.contains("sa1 1 1") {
        return true;
    }
    if lowered.contains("session opened for user") || lowered.contains("session closed for user") {
        return true;
    }
    if lowered.contains(".scope") || lowered.contains("new session") {
        return true;
    }
    if lowered.contains("reached target") || lowered.contains("created slice") {
        return true;
    }
    if lowered.contains("pam_unix(cron:session)") {
        return true;
    }
    if unit.starts_with("systemd") && lowered.contains("deactivated successfully") {
        return true;
    }
    false
}

fn unit_base(unit: &str) -> String {
    unit.split('[').next().unwrap_or(unit).trim().to_string()
}

fn friendly_unit(unit: &str) -> &'static str {
    match unit.to_ascii_lowercase().as_str() {
        "sshd" | "sshd-session" => "SSH",
        "smbd" => "Samba",
        "nmbd" => "NetBIOS",
        "wsdd2" => "Descubrimiento de red",
        "nginx" => "Servidor web",
        "nas-web" => "Panel web",
        "cron" | "crond" => "Tareas programadas",
        "sudo" => "Administración",
        "auditd" => "Auditoría del sistema",
        "kernel" => "Núcleo",
        "ufw" => "Cortafuegos",
        "systemd" | "systemd-logind" | "systemd-journald" => "Sistema",
        _ => "Sistema",
    }
}

fn journal_sentence(unit: &str, friendly: &'static str, message: &str) -> (&'static str, String) {
    let lowered = message.to_lowercase();
    if let Some(rest) = message.strip_prefix("Accepted password for ") {
        if let Some((user, tail)) = rest.split_once(" from ") {
            let ip = tail.split_whitespace().next().unwrap_or_default();
            return (
                "Acceso SSH",
                format!("El usuario {user} inició sesión por SSH desde {ip}"),
            );
        }
    }
    if let Some(rest) = message.strip_prefix("Accepted publickey for ") {
        if let Some((user, tail)) = rest.split_once(" from ") {
            let ip = tail.split_whitespace().next().unwrap_or_default();
            return (
                "Acceso SSH",
                format!("El usuario {user} accedió por SSH con llave desde {ip}"),
            );
        }
    }
    if lowered.contains("failed password for") || lowered.contains("invalid user") {
        return (
            "Acceso rechazado",
            format!("Un intento de acceso por SSH fue rechazado: {message}"),
        );
    }
    if unit.eq_ignore_ascii_case("sudo") {
        let command = message.split("COMMAND=").nth(1).unwrap_or(message).trim();
        return (
            "Comando administrativo",
            format!("Se ejecutó un comando con permisos de administrador: {command}"),
        );
    }
    if unit.eq_ignore_ascii_case("cron") {
        let command = message
            .split_once("CMD (")
            .map(|(_, rest)| rest.trim_end_matches(')'))
            .unwrap_or(message);
        return (
            "Tarea programada",
            format!("El planificador ejecutó la tarea: {command}"),
        );
    }
    if let Some(rest) = message.strip_prefix("Started ") {
        return ("Servicio iniciado", format!("Se inició {rest}"));
    }
    if let Some(rest) = message.strip_prefix("Stopped ") {
        return ("Servicio detenido", format!("Se detuvo {rest}"));
    }
    if let Some(rest) = message.strip_prefix("Failed to start ") {
        return ("Fallo de servicio", format!("No se pudo iniciar {rest}"));
    }
    (friendly, format!("{friendly}: {message}"))
}

fn looks_like_error(message: &str) -> bool {
    let lowered = message.to_lowercase();
    if lowered.contains("0 errors") || lowered.contains("no error") || lowered.contains("0 error") {
        return false;
    }
    [
        "failed", "failure", "error", "denied", "refused", "panic", "segfault", "corrupt",
    ]
    .iter()
    .any(|needle| lowered.contains(needle))
}

fn extract_ip(message: &str) -> Option<String> {
    message
        .split_whitespace()
        .find(|token| {
            let mut groups = 0;
            let mut valid = true;
            for part in token.split('.') {
                if part.is_empty() || part.len() > 3 || !part.chars().all(|c| c.is_ascii_digit()) {
                    valid = false;
                    break;
                }
                groups += 1;
            }
            valid && groups == 4
        })
        .map(|token| {
            token
                .trim_matches(|c: char| !c.is_ascii_digit() && c != '.')
                .to_string()
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filtra_ruido_y_traduce_journald() {
        let text = "\
2026-11-04T17:25:01-0600 tsdebian CRON[21517]: (root) CMD (command -v debian-sa1 > /dev/null && debian-sa1 1 1)
2026-11-04T17:24:28-0600 tsdebian sshd-session[21432]: Accepted password for sistemas from 10.10.1.182 port 53096 ssh2
2026-11-04T17:24:28-0600 tsdebian systemd[1]: session-232.scope: Deactivated successfully.
2026-11-04T17:20:00-0600 tsdebian sudo[1234]: sistemas : TTY=pts/0 ; PWD=/home ; USER=root ; COMMAND=/usr/bin/systemctl restart nas-web";
        let entries = parse_journal(text);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].action_label, "Acceso SSH");
        assert!(entries[0].human.contains("sistemas"));
        assert!(entries[0].human.contains("10.10.1.182"));
        assert_eq!(entries[1].action_label, "Comando administrativo");
        assert!(entries[1].human.contains("systemctl restart nas-web"));
    }
}
