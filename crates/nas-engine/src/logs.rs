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

    /// Interpreta el identificador enviado por el panel (`all` no es fuente).
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
    fn new(source: LogSource, timestamp: String, raw: &str) -> Self {
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
            raw: raw.to_string(),
            count: 1,
            first_seen: timestamp.clone(),
            last_seen: timestamp,
        }
    }

    fn finish(mut self, action: &str, label: &str, human: String) -> Self {
        self.action = action.to_string();
        self.action_label = label.to_string();
        self.human = human.trim().to_string();
        self
    }

    fn fail(mut self) -> Self {
        self.status = "FAILED".to_string();
        self.level = "error".to_string();
        self.badge = "err".to_string();
        self
    }

    fn warn(mut self) -> Self {
        self.status = "WARNING".to_string();
        self.level = "warning".to_string();
        self.badge = "warn".to_string();
        self
    }

    fn notice(mut self) -> Self {
        self.level = "notice".to_string();
        self
    }
}

impl LogSource {
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

fn epoch_to_iso(ts: i64) -> String {
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

/// Interpreta filas de la auditoría del panel web.
pub fn parse_admin_events(events: &[AdminEvent<'_>]) -> Vec<LogEntry> {
    events
        .iter()
        .map(|event| {
            let timestamp = epoch_to_iso(event.ts);
            let raw = format!(
                "{} {} {} {} {}",
                event.action, event.target, event.result, event.username, event.detail
            );
            let mut entry = LogEntry::new(LogSource::Admin, timestamp, raw.trim());
            entry.user = event.username.to_string();
            entry.target = event.target.to_string();

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
            },
            AdminEvent {
                ts: 1_793_832_965,
                username: "admin",
                action: "backup_run",
                target: "didi_admin",
                result: "SUCCESS",
                detail: "tarea didi_admin iniciada",
            },
        ];
        let entries = parse_admin_events(&events);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].action_label, "Eliminó archivo");
        assert!(entries[0].human.contains("eliminó «SISTEMAS/sdfdsf.txt»"));
        assert_eq!(entries[1].action_label, "Ejecutó respaldo");
        assert!(entries[1].human.contains("tarea didi_admin iniciada"));
    }

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
}
