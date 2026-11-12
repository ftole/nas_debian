//! Normalización de bitácoras en descripciones legibles.
//!
//! El panel web muestra cuatro fuentes de registro muy distintas: la auditoría
//! de Samba (`full_audit`), la auditoría del panel (SQLite), los runners de
//! respaldo y `journald`. Este módulo las convierte a una forma común
//! (`LogEntry`) con una frase en español pensada para usuarios no técnicos,
//! conservando siempre la línea original en `raw` para auditoría.

use std::collections::HashMap;

use chrono::TimeZone;
use serde::{Deserialize, Serialize};

mod admin;
mod backup;
mod journal;
mod samba;

pub use admin::{parse_admin_events, AdminEvent};
pub use backup::{parse_backup_log, rsync_code_hint};
pub use journal::parse_journal;
pub use samba::parse_samba_audit;

/// Origen de un registro.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LogSource {
    /// Auditoría de acceso a archivos de Samba (`/var/log/samba/audit.log`).
    SambaAudit,
    /// Auditoría de acciones del panel web (tabla `web_audit`).
    Admin,
    /// Runners de respaldo (`LOGS_BACKUP`).
    Backup,
    /// Eventos del sistema en `journald`.
    System,
}

impl LogSource {
    /// Identificador estable que consume el panel web.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::SambaAudit => "samba_audit",
            Self::Admin => "admin",
            Self::Backup => "backup",
            Self::System => "system",
        }
    }

    /// Interpreta el identificador enviado por el panel.
    ///
    /// El valor `all` (o cualquier valor desconocido como `panel`) no mapean a
    /// una única fuente: el endpoint lo interpreta como el conjunto por defecto
    /// (admin + backup + samba_audit), que excluye el ruido interno del kernel.
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "samba_audit" | "samba" => Some(Self::SambaAudit),
            "admin" | "web" => Some(Self::Admin),
            "backup" | "backups" => Some(Self::Backup),
            "system" | "sistema" => Some(Self::System),
            _ => None,
        }
    }

    /// Etiqueta corta en español.
    pub fn label(self) -> &'static str {
        match self {
            Self::SambaAudit => "Samba",
            Self::Admin => "Admin",
            Self::Backup => "Respaldos",
            Self::System => "Sistema",
        }
    }

    /// Color por defecto del distintivo de cada fuente.
    pub fn badge(self) -> &'static str {
        match self {
            Self::SambaAudit => "purple",
            Self::Admin => "blue",
            Self::Backup => "ok",
            Self::System => "gray",
        }
    }
}

/// Entrada de bitácora normalizada, lista para el panel web.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LogEntry {
    /// Marca de tiempo normalizada (`AAAA-MM-DDTHH:MM:SS`).
    pub timestamp: String,
    /// Fuente del registro.
    pub source: LogSource,
    /// Código técnico corto de la acción (`openat`, `file_delete`, ...).
    pub action: String,
    /// Etiqueta legible de la acción para la columna de estado.
    pub action_label: String,
    /// Objeto afectado (archivo, recurso, tarea, servicio).
    pub target: String,
    /// Usuario o proceso responsable.
    pub user: String,
    /// Dirección IP de origen cuando se conoce.
    pub ip: String,
    /// Resultado (`SUCCESS`, `FAILED`, `WARNING`).
    pub status: String,
    /// Nivel de severidad (`info`, `notice`, `warning`, `error`).
    pub level: String,
    /// Color del distintivo en la interfaz (`blue`, `purple`, `ok`, `warn`, `err`, `gray`).
    pub badge: String,
    /// Frase completa en español que describe lo ocurrido.
    pub human: String,
    /// Origen legible del evento (`Panel web`, `Terminal web`, `Equipo remoto`, ...).
    pub origin: String,
    /// Equipo o host donde se originó el evento.
    pub origin_detail: String,
    /// Código de error real del sistema (`rsync 11`, `mount error(112)`, `NT_STATUS_...`).
    pub code: String,
    /// Línea original sin transformar.
    pub raw: String,
    /// Número de repeticiones agrupadas.
    pub count: u32,
    /// Marca de tiempo de la repetición más antigua del grupo.
    pub first_seen: String,
    /// Marca de tiempo de la repetición más reciente del grupo.
    pub last_seen: String,
}

impl LogEntry {
    pub(super) fn new(source: LogSource, timestamp: String, raw: &str) -> Self {
        Self {
            timestamp: timestamp.clone(),
            source,
            action: String::new(),
            action_label: String::new(),
            target: String::new(),
            user: String::new(),
            ip: String::new(),
            status: "SUCCESS".to_string(),
            level: "info".to_string(),
            badge: source.badge().to_string(),
            human: String::new(),
            origin: String::new(),
            origin_detail: String::new(),
            code: String::new(),
            raw: raw.to_string(),
            count: 1,
            first_seen: timestamp.clone(),
            last_seen: timestamp,
        }
    }

    pub(super) fn finish(mut self, action: &str, label: &str, human: String) -> Self {
        self.action = action.to_string();
        self.action_label = label.to_string();
        self.human = human.trim().to_string();
        self.code = detect_code(&self.raw);
        self
    }

    pub(super) fn fail(mut self) -> Self {
        self.status = "FAILED".to_string();
        self.level = "error".to_string();
        self.badge = "err".to_string();
        self
    }

    pub(super) fn warn(mut self) -> Self {
        self.status = "WARNING".to_string();
        self.level = "warning".to_string();
        self.badge = "warn".to_string();
        self
    }

    pub(super) fn notice(mut self) -> Self {
        self.level = "notice".to_string();
        self
    }
}

/// Normaliza una marca de tiempo a `AAAA-MM-DDTHH:MM:SS` recortando zona y fracciones.
pub fn normalize_timestamp(raw: &str) -> String {
    let chars: Vec<char> = raw.trim().chars().take(19).collect();
    if chars.len() == 19 && chars[4] == '-' && chars[7] == '-' && chars[10] == ' ' {
        let mut out = String::with_capacity(19);
        out.extend(chars[..10].iter());
        out.push('T');
        out.extend(chars[11..].iter());
        out
    } else {
        chars.into_iter().collect()
    }
}

/// Extrae el código de error real que lanza el sistema, si existe.
fn detect_code(text: &str) -> String {
    let lower = text.to_lowercase();
    if let Some(position) = lower.find("nt_status_") {
        let code: String = lower[position..]
            .chars()
            .take_while(|character| character.is_ascii_alphanumeric() || *character == '_')
            .collect();
        if !code.is_empty() {
            return code.to_uppercase();
        }
    }
    if let Some(position) = lower.find("mount error(") {
        let rest = &lower[position + "mount error(".len()..];
        if let Some(end) = rest.find(')') {
            return format!("mount error({})", &rest[..end]);
        }
    }
    if lower.contains("rsync") {
        for marker in ["(código ", "(codigo "] {
            if let Some(position) = lower.find(marker) {
                let rest = &lower[position + marker.len()..];
                let digits: String = rest
                    .chars()
                    .take_while(|character| character.is_ascii_digit())
                    .collect();
                if !digits.is_empty() {
                    return format!("rsync {digits}");
                }
            }
        }
    }
    for marker in ["exit code ", "exit "] {
        if let Some(position) = lower.find(marker) {
            let rest = &lower[position + marker.len()..];
            let digits: String = rest
                .chars()
                .take_while(|character| character.is_ascii_digit())
                .collect();
            if !digits.is_empty() {
                return format!("exit {digits}");
            }
        }
    }
    String::new()
}

pub(super) fn epoch_to_iso(ts: i64) -> String {
    chrono::Local
        .timestamp_opt(ts, 0)
        .single()
        .map(|value| value.format("%Y-%m-%dT%H:%M:%S").to_string())
        .unwrap_or_else(|| ts.to_string())
}

/// Ordena las entradas de la más reciente a la más antigua.
pub fn sort_recent(entries: &mut [LogEntry]) {
    entries.sort_by(|left, right| right.timestamp.cmp(&left.timestamp));
}

/// Filtra entradas por texto libre sobre la frase legible y la línea original.
pub fn filter_query(entries: Vec<LogEntry>, query: &str) -> Vec<LogEntry> {
    let needle = query.trim().to_lowercase();
    if needle.is_empty() {
        return entries;
    }
    entries
        .into_iter()
        .filter(|entry| {
            let haystack = format!(
                "{} {} {} {} {} {} {}",
                entry.human,
                entry.raw,
                entry.user,
                entry.ip,
                entry.target,
                entry.action_label,
                entry.action
            )
            .to_lowercase();
            haystack.contains(&needle)
        })
        .collect()
}

/// Agrupa eventos idénticos sumando repeticiones y conservando el rango temporal.
pub fn group_repeated(entries: Vec<LogEntry>) -> Vec<LogEntry> {
    let mut index: HashMap<String, usize> = HashMap::new();
    let mut grouped: Vec<LogEntry> = Vec::new();

    for entry in entries {
        let key = format!(
            "{}|{}|{}|{}|{}|{}",
            entry.source.as_str(),
            entry.action,
            entry.target,
            entry.status,
            entry.user,
            entry.ip
        );
        if let Some(position) = index.get(&key).copied() {
            let existing = &mut grouped[position];
            existing.count += 1;
            existing.first_seen = entry.timestamp.clone();
            continue;
        }
        index.insert(key, grouped.len());
        grouped.push(entry);
    }

    grouped
}

/// Convierte un identificador técnico (`file_delete`) en texto presentable.
pub fn humanize(value: &str) -> String {
    let text = value.replace(['_', '-'], " ");
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => format!("{}{}", first.to_uppercase(), chars.as_str()),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normaliza_marcas_de_tiempo() {
        assert_eq!(
            normalize_timestamp("2026-11-04T17:24:28-0600"),
            "2026-11-04T17:24:28"
        );
        assert_eq!(
            normalize_timestamp("2026-11-03T17:35:22.862852-06:00"),
            "2026-11-03T17:35:22"
        );
        assert_eq!(
            normalize_timestamp("2026-11-04 16:55:42"),
            "2026-11-04T16:55:42"
        );
    }

    #[test]
    fn agrupa_eventos_repetidos() {
        let text_entries = parse_journal(
            "2026-11-04T17:24:28-0600 tsdebian nginx[1]: Started worker\n\
             2026-11-04T17:23:28-0600 tsdebian nginx[1]: Started worker\n",
        );
        let grouped = group_repeated(text_entries);
        assert_eq!(grouped.len(), 1);
        assert_eq!(grouped[0].count, 2);
        assert_eq!(grouped[0].first_seen, "2026-11-04T17:23:28");
        assert_eq!(grouped[0].last_seen, "2026-11-04T17:24:28");
    }

    #[test]
    fn busca_por_texto_libre() {
        let entries = parse_admin_events(&[AdminEvent {
            ts: 1_793_834_688,
            username: "admin",
            action: "file_delete",
            target: "SISTEMAS/logo.jpg",
            result: "SUCCESS",
            detail: "",
            origin_ip: "",
        }]);
        assert_eq!(filter_query(entries.clone(), "logo").len(), 1);
        assert_eq!(filter_query(entries.clone(), "admin").len(), 1);
        assert!(filter_query(entries, "inexistente").is_empty());
    }

    #[test]
    fn humano_simple() {
        assert_eq!(humanize("share_acl_repair"), "Share acl repair");
        assert_eq!(humanize(""), "");
    }

    #[test]
    fn detecta_codigos_de_error_del_sistema() {
        assert_eq!(
            detect_code("[2026-10-28 23:11:54] [Creditas] error en replicación rsync (código 11)"),
            "rsync 11"
        );
        assert_eq!(
            detect_code("mount error(112): Host is down"),
            "mount error(112)"
        );
        assert_eq!(
            detect_code("smbd: open failed NT_STATUS_ACCESS_DENIED"),
            "NT_STATUS_ACCESS_DENIED"
        );
        assert_eq!(detect_code("el script salió con exit code 127"), "exit 127");
        assert_eq!(detect_code("todo correcto"), "");
    }

    #[test]
    fn propaga_el_codigo_real_en_la_entrada() {
        let entries = parse_backup_log(
            "[2026-10-28 23:11:54] [Creditas] error en replicación rsync (código 11)",
        );
        assert_eq!(entries[0].status, "FAILED");
        assert_eq!(entries[0].code, "rsync 11");
        assert!(entries[0].human.contains("error de lectura o escritura"));
    }

    #[test]
    fn identifica_origen_y_equipo_del_evento() {
        let samba = parse_samba_audit(
            "2026-11-03T17:35:22-06:00 tsdebian smbd_audit: sistemas|10.10.1.250|sis-frank|CONTABILIDAD|openat|ok|CONTABILIDAD/x.xlsx",
        );
        assert_eq!(samba[0].origin, "Equipo remoto");
        assert_eq!(samba[0].origin_detail, "sis-frank");

        let terminal = parse_admin_events(&[AdminEvent {
            ts: 0,
            username: "admin",
            action: "terminal_start",
            target: "nas-web-admin",
            result: "SUCCESS",
            detail: "",
            origin_ip: "",
        }]);
        assert_eq!(terminal[0].origin, "Terminal web");

        let panel = parse_admin_events(&[AdminEvent {
            ts: 0,
            username: "admin",
            action: "login",
            target: "web",
            result: "SUCCESS",
            detail: "",
            origin_ip: "",
        }]);
        assert_eq!(panel[0].origin, "Panel web");

        let backup = parse_backup_log("[2026-11-04 16:55:42] [didi] [LOCK_ACQUIRED] [info] x");
        assert_eq!(backup[0].origin, "Tarea programada");

        let system = parse_journal(
            "2026-11-04T17:24:28-0600 tsdebian sshd-session[1]: Accepted password for sistemas from 10.10.1.182 port 1 ssh2",
        );
        assert_eq!(system[0].origin, "Sistema");
        assert_eq!(system[0].origin_detail, "tsdebian");
    }
}
