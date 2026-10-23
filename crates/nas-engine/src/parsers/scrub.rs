//! Parser de `btrfs scrub status`.

use std::sync::OnceLock;

use regex::Regex;
use serde::Serialize;

/// Estado de la auditoría criptográfica Btrfs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ScrubStatus {
    /// Indica si hay un scrub en curso.
    pub running: bool,
    /// Estado normalizado (`running`, `finished`, `aborted`, `never run`).
    pub status: String,
    /// Duración reportada.
    pub duration: Option<String>,
    /// Total a verificar.
    pub total_to_scrub: Option<String>,
    /// Velocidad reportada.
    pub rate: Option<String>,
    /// Resumen de errores.
    pub error_summary: Option<String>,
    /// Tiempo restante estimado.
    pub time_left: Option<String>,
    /// Resumen legible para la interfaz.
    pub summary: String,
}

fn patterns() -> &'static Patterns {
    static CACHE: OnceLock<Patterns> = OnceLock::new();
    CACHE.get_or_init(|| Patterns {
        status: Regex::new(r"(?i)Status:\s+([A-Za-z ]+)").expect("regex de estado"),
        duration: Regex::new(r"(?i)Duration:\s+([^\r\n]+)").expect("regex de duración"),
        total: Regex::new(r"(?i)Total to scrub:\s+([^\r\n]+)").expect("regex de total"),
        rate: Regex::new(r"(?i)Rate:\s+([^\r\n]+)").expect("regex de velocidad"),
        errors: Regex::new(r"(?i)Error summary:\s+([^\r\n]+)").expect("regex de errores"),
        eta: Regex::new(r"(?i)Time left:\s+([^\r\n]+)").expect("regex de tiempo restante"),
    })
}

struct Patterns {
    status: Regex,
    duration: Regex,
    total: Regex,
    rate: Regex,
    errors: Regex,
    eta: Regex,
}

fn capture(regex: &Regex, text: &str) -> Option<String> {
    regex
        .captures(text)
        .map(|caps| caps[1].trim().to_string())
        .filter(|value| !value.is_empty())
}

/// Interpreta la salida de `btrfs scrub status /srv/nas`.
pub fn parse_scrub_status(text: &str) -> ScrubStatus {
    let re = patterns();
    let raw_status = capture(&re.status, text).unwrap_or_else(|| "never run".to_string());
    let normalized = raw_status.to_lowercase();

    let status = if normalized.contains("running") {
        "running".to_string()
    } else if normalized.contains("finished") {
        "finished".to_string()
    } else if normalized.contains("aborted")
        || normalized.contains("canceled")
        || normalized.contains("cancelled")
    {
        "aborted".to_string()
    } else {
        "never run".to_string()
    };

    let running = status == "running";
    let duration = capture(&re.duration, text);
    let rate = capture(&re.rate, text);
    let errors = capture(&re.errors, text);
    let time_left = capture(&re.eta, text);

    let summary = if running {
        format!(
            "Scrub en curso ({}). Tiempo restante estimado: {}",
            rate.clone().unwrap_or_else(|| "N/D".to_string()),
            time_left
                .clone()
                .unwrap_or_else(|| "calculando...".to_string())
        )
    } else if status == "finished" {
        format!(
            "Scrub finalizado ({}). Errores: {}",
            duration.clone().unwrap_or_else(|| "N/D".to_string()),
            errors.clone().unwrap_or_else(|| "0".to_string())
        )
    } else {
        format!("Estado: {status}")
    };

    ScrubStatus {
        running,
        status,
        duration,
        total_to_scrub: capture(&re.total, text),
        rate,
        error_summary: errors,
        time_left,
        summary,
    }
}

#[cfg(test)]
mod tests {
    use super::parse_scrub_status;

    const RUNNING: &str = "\
UUID:             6ada7be3-3698-4a1c-9e01-112233445566
Scrub started:    Thu Oct 22 15:00:00 2026
Status:           running
Duration:         0:03:12
Time left:        0:12:34
ETA:              Thu Oct 22 15:16:00 2026
Total to scrub:   64.00GiB
Rate:             120.00MiB/s
Error summary:    no errors found
";

    const FINISHED: &str = "\
Scrub started:    Wed Oct 21 02:00:00 2026
Status:           finished
Duration:         0:01:15
Total to scrub:   64.00GiB
Rate:             512.00MiB/s
Error summary:    no errors found
";

    const NEVER: &str = "Scrub started:    none\nStatus:           never run\n";

    #[test]
    fn detecta_scrub_en_curso() {
        let status = parse_scrub_status(RUNNING);
        assert!(status.running);
        assert_eq!(status.status, "running");
        assert_eq!(status.rate.as_deref(), Some("120.00MiB/s"));
        assert_eq!(status.time_left.as_deref(), Some("0:12:34"));
        assert!(status.summary.contains("en curso"));
    }

    #[test]
    fn detecta_scrub_finalizado() {
        let status = parse_scrub_status(FINISHED);
        assert!(!status.running);
        assert_eq!(status.status, "finished");
        assert_eq!(status.duration.as_deref(), Some("0:01:15"));
        assert!(status.summary.contains("finalizado"));
    }

    #[test]
    fn detecta_scrub_nunca_ejecutado() {
        let status = parse_scrub_status(NEVER);
        assert!(!status.running);
        assert_eq!(status.status, "never run");
    }

    #[test]
    fn salida_vacia_no_paniquea() {
        let status = parse_scrub_status("");
        assert!(!status.running);
        assert_eq!(status.status, "never run");
    }
}
