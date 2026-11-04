//! Generación validada de tareas de respaldo (runner + cron + credenciales).

use nas_core::{NasError, NasResult};
use serde::Deserialize;

/// Protocolo del origen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Proto {
    /// Recurso compartido Windows (CIFS).
    Cifs,
    /// Servidor Linux remoto por SSH.
    Ssh,
    /// Ruta local del servidor.
    Local,
}

/// Definición de una tarea de respaldo enviada por el panel.
#[derive(Debug, Clone, Deserialize)]
pub struct BackupSpec {
    /// Identificador de la tarea.
    pub id: String,
    /// Protocolo (`cifs`, `ssh`, `local`).
    /// Protocolo del origen (`cifs`, `ssh` o `local`).
    pub proto: String,
    /// IP o host del origen.
    #[serde(default)]
    pub ip: String,
    /// Recurso compartido CIFS.
    #[serde(default)]
    pub share: String,
    /// Ruta remota o local.
    #[serde(default)]
    pub path: String,
    /// Usuario del origen.
    #[serde(default)]
    pub user: String,
    /// Contraseña del origen (se guarda en credenciales 0600).
    #[serde(default)]
    pub password: String,
    /// Expresión cron (5 campos).
    /// Expresión cron de 5 campos.
    pub cron: String,
    /// Snapshots a conservar.
    pub retention: u32,
    /// Unidad de destino (`principal` o nombre de unidad).
    #[serde(default)]
    pub dest_unit: String,
    /// Puerto SSH del origen.
    #[serde(default)]
    pub port: Option<u16>,
    /// Clave privada SSH (contenido PEM o ruta al archivo de clave).
    #[serde(default, alias = "key_file", alias = "key_path", alias = "private_key")]
    pub ssh_key: Option<String>,
    /// Generar respaldo permanente inicial y mensual automático (Golden Snapshot).
    #[serde(default = "default_true")]
    pub auto_permanent: bool,
}

fn default_true() -> bool {
    true
}

impl BackupSpec {
    /// Valida la tarea y devuelve el protocolo normalizado.
    pub fn validate(&self) -> NasResult<Proto> {
        let id = self.id.trim();
        if id.is_empty() || id.len() > 32 {
            return Err(NasError::Invalid("identificador inválido".into()));
        }
        if !id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        {
            return Err(NasError::Invalid(
                "el identificador solo admite letras, números, '_' y '-'".into(),
            ));
        }

        let proto = match self.proto.as_str() {
            "cifs" => Proto::Cifs,
            "ssh" => Proto::Ssh,
            "local" => Proto::Local,
            other => {
                return Err(NasError::Invalid(format!("protocolo inválido: {other}")));
            }
        };

        if self
            .password
            .chars()
            .any(|c| c == '\r' || c == '\n' || c == '\0')
        {
            return Err(NasError::Invalid(
                "la contraseña no puede contener saltos de línea ni bytes nulos".into(),
            ));
        }

        let unit = self.dest_unit.trim();
        if !unit.is_empty()
            && unit != "principal"
            && (unit.len() > 32
                || !unit
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-'))
        {
            return Err(NasError::Invalid("unidad de destino inválida".into()));
        }

        match proto {
            Proto::Cifs => {
                if !valid_host(&self.ip) {
                    return Err(NasError::Invalid("IP o host del origen inválido".into()));
                }
                let share = self.share.trim();
                let user = self.user.trim();
                if share.is_empty() || user.is_empty() {
                    return Err(NasError::Invalid("CIFS requiere recurso y usuario".into()));
                }
                if share.len() > 128
                    || share.contains('/')
                    || share.contains('\\')
                    || share.chars().any(char::is_control)
                {
                    return Err(NasError::Invalid("nombre de recurso CIFS inválido".into()));
                }
                if !valid_cifs_user(user) {
                    return Err(NasError::Invalid("usuario CIFS inválido".into()));
                }
            }
            Proto::Ssh => {
                if !valid_host(&self.ip) {
                    return Err(NasError::Invalid("IP o host del origen inválido".into()));
                }
                let path = self.path.trim();
                let user = self.user.trim();
                if !path.starts_with('/')
                    || path.contains("..")
                    || path.chars().any(char::is_control)
                    || user.is_empty()
                {
                    return Err(NasError::Invalid(
                        "SSH requiere ruta absoluta segura y usuario".into(),
                    ));
                }
                if !valid_ssh_user(user) {
                    return Err(NasError::Invalid("usuario SSH inválido".into()));
                }
                if let Some(key) = self.ssh_key.as_deref().filter(|s| !s.trim().is_empty()) {
                    let key = key.trim();
                    if key.starts_with("-----BEGIN") {
                        nas_ssh::validate_private_key(key)?;
                    } else if key.starts_with('/') {
                        if key.contains("..") || key.chars().any(char::is_control) {
                            return Err(NasError::Invalid(
                                "ruta de clave privada SSH inválida".into(),
                            ));
                        }
                    } else {
                        return Err(NasError::Invalid(
                            "la clave SSH debe ser contenido PEM (-----BEGIN ...) o una ruta absoluta (/...)".into(),
                        ));
                    }
                }
            }
            Proto::Local => {
                let path = self.path.trim();
                if !path.starts_with('/')
                    || path.contains("..")
                    || path.chars().any(char::is_control)
                {
                    return Err(NasError::Invalid(
                        "la ruta local debe ser absoluta y segura".into(),
                    ));
                }
            }
        }

        if self.retention == 0 || self.retention > 365 {
            return Err(NasError::Invalid(
                "retención debe estar entre 1 y 365".into(),
            ));
        }
        if !valid_cron(&self.cron) {
            return Err(NasError::Invalid("expresión cron inválida".into()));
        }

        Ok(proto)
    }

    /// Raíz del repositorio histórico según la unidad seleccionada.
    pub fn destination_root(&self) -> String {
        let unit = self.dest_unit.trim();
        if unit.is_empty() || unit == "principal" {
            "/srv/nas/BACKUPS_HISTORICOS".to_string()
        } else {
            format!("/srv/nas/unidades/{unit}/BACKUPS_HISTORICOS")
        }
    }

    /// Directorio de destino.
    pub fn destination(&self) -> String {
        format!("{}/{}", self.destination_root(), self.id.trim())
    }

    /// Runner bash listo para instalar, compatible con `BackupTask::parse`.
    pub fn render_runner(&self, proto: Proto) -> String {
        let id = self.id.trim();
        let dest_root = self.destination_root();
        let dest = self.destination();
        let cred = match proto {
            Proto::Cifs | Proto::Ssh => format!("/etc/backup-credentials/{id}.cred"),
            Proto::Local => String::new(),
        };
        let key_file = match proto {
            Proto::Ssh => {
                if let Some(k) = self.ssh_key.as_deref().filter(|s| !s.trim().is_empty()) {
                    let k = k.trim();
                    if k.starts_with("-----BEGIN") {
                        format!("/etc/backup-credentials/{id}.id_ed25519")
                    } else {
                        k.to_string()
                    }
                } else {
                    String::new()
                }
            }
            _ => String::new(),
        };
        let port = self.port.unwrap_or(22);

        let (src_type, src_ip, src_share, src_path, src_user) = match proto {
            Proto::Cifs => (
                "cifs",
                self.ip.trim(),
                self.share.trim(),
                "",
                self.user.trim(),
            ),
            Proto::Ssh => (
                "ssh",
                self.ip.trim(),
                "",
                self.path.trim().trim_end_matches('/'),
                self.user.trim(),
            ),
            Proto::Local => ("local", "", "", self.path.trim().trim_end_matches('/'), ""),
        };

        let source_setup = match proto {
            Proto::Cifs => {
                "mkdir -p \"$MOUNT\"\nmount -t cifs \"//$SRC_IP/$SRC_SHARE\" \"$MOUNT\" -o \"credentials=$CRED,ro,iocharset=utf8,vers=3.1.1,noserverino,cache=none,soft,nobrl\" 2>> \"$LOG\"\nSRC=\"$MOUNT/\"".to_string()
            }
            Proto::Ssh => "SRC=\"$SRC_USER@$SRC_IP:${SRC_PATH:-/}/\"".to_string(),
            Proto::Local => "SRC=\"${SRC_PATH:-/}/\"".to_string(),
        };

        let rsync = match proto {
            Proto::Ssh => {
                "mkdir -p /root/.ssh\nchmod 0700 /root/.ssh\nRSYNC_RC=0\nif [ -n \"$KEY_FILE\" ] && [ -f \"$KEY_FILE\" ]; then\n    rsync -aAXH --info=progress2 --numeric-ids --safe-links --timeout=300 \"${LINK_ARGS[@]}\" -e \"ssh -i $KEY_FILE -p $SRC_PORT -o StrictHostKeyChecking=accept-new -o UserKnownHostsFile=/root/.ssh/known_hosts_backup\" -- \"$SRC\" \"$STAGE\" 2>> \"$LOG\" | tr '\\r' '\\n' | update_progress || RSYNC_RC=${PIPESTATUS[0]}\nelif [ -f \"/etc/backup-credentials/$TASK.id_ed25519\" ]; then\n    rsync -aAXH --info=progress2 --numeric-ids --safe-links --timeout=300 \"${LINK_ARGS[@]}\" -e \"ssh -i /etc/backup-credentials/$TASK.id_ed25519 -p $SRC_PORT -o StrictHostKeyChecking=accept-new -o UserKnownHostsFile=/root/.ssh/known_hosts_backup\" -- \"$SRC\" \"$STAGE\" 2>> \"$LOG\" | tr '\\r' '\\n' | update_progress || RSYNC_RC=${PIPESTATUS[0]}\nelse\n    SSHPASS=\"\"\n    if [ -f \"$CRED\" ]; then\n        IFS= read -r SSHPASS < \"$CRED\" || true\n    fi\n    export SSHPASS\n    sshpass -e rsync -aAXH --info=progress2 --numeric-ids --safe-links --timeout=300 \"${LINK_ARGS[@]}\" -e \"ssh -p $SRC_PORT -o StrictHostKeyChecking=accept-new -o UserKnownHostsFile=/root/.ssh/known_hosts_backup\" -- \"$SRC\" \"$STAGE\" 2>> \"$LOG\" | tr '\\r' '\\n' | update_progress || RSYNC_RC=${PIPESTATUS[0]}\n    unset SSHPASS\nfi".to_string()
            }
            _ => {
                "RSYNC_RC=0\nrsync -aAXH --info=progress2 --numeric-ids --safe-links --timeout=300 \"${LINK_ARGS[@]}\" -- \"$SRC\" \"$STAGE\" 2>> \"$LOG\" | tr '\\r' '\\n' | update_progress || RSYNC_RC=${PIPESTATUS[0]}".to_string()
            }
        };

        format!(
            r#"#!/bin/bash
set -euo pipefail
TASK={q_id}
SOURCE_TYPE={q_type}
SRC_IP={q_ip}
SRC_SHARE={q_share}
SRC_PATH={q_path}
SRC_USER={q_user}
SRC_PORT={port}
RETENTION={retention}
AUTO_PERMANENT={auto_permanent}
DEST_ROOT={q_dest_root}
LOG_DIR='/srv/nas/LOGS_BACKUP'
CRED_FILE={q_cred}
KEY_FILE={q_key}
MODE="$SOURCE_TYPE"
CRED="$CRED_FILE"
BKP_DIR="${{BKP_DIR:-{q_dest}}}"
PERM_DIR="${{DEST_ROOT/BACKUPS_HISTORICOS/BACKUPS_PERMANENTES}}/$TASK"
LOG="${{LOG_FILE:-$LOG_DIR/backup_${{TASK}}.log}}"
MOUNT="${{MOUNT_ROOT:-/mnt/backup_sources}}/$TASK"
PROG_FILE="/run/nas-backup-$TASK.progress"
DATE_STR=$(date +%Y-%m-%d_%H%M%S)
START_TIME=$(date +%s)
STAGE="$BKP_DIR/.inprogress_$DATE_STR"
FINAL="$BKP_DIR/snapshot_$DATE_STR"
SNAPSHOT_OK=false

log() {{ printf '[%s] [%s] %s\n' "$(date '+%F %T')" "$TASK" "$*" >> "$LOG"; }}
cleanup() {{
    local status=$?
    rm -f "$PROG_FILE" "${{PROG_FILE}}.tmp."* 2>/dev/null || true
    if mountpoint -q "$MOUNT" 2>/dev/null; then umount "$MOUNT" || true; fi
    if [ "$SNAPSHOT_OK" != "true" ] && [ "$status" -ne 0 ]; then
        log "ERROR: se descarta el snapshot parcial (codigo de salida $status)"
        rm -rf -- "$STAGE" "$FINAL"
    elif [ -d "$STAGE" ]; then
        rm -rf -- "$STAGE"
    fi
    exit "$status"
}}
trap cleanup EXIT
trap 'exit 143' TERM
trap 'exit 130' INT

update_progress() {{
    local b pct spd rem rest num_pct diff elap tmp_file
    while read -r b pct spd rem rest; do
        [ -z "$pct" ] && continue
        num_pct="${{pct%%%}}"
        num_pct="${{num_pct//[^0-9]/}}"
        [ -z "$num_pct" ] && continue
        diff=$(($(date +%s) - START_TIME))
        elap=$(printf '%02d:%02d:%02d' $((diff/3600)) $(((diff%3600)/60)) $((diff%60)))
        tmp_file="${{PROG_FILE}}.tmp.$$"
        printf '{{"percent":%d,"speed":"%s","elapsed":"%s","remaining":"%s","bytes":"%s"}}\n' \
            "$num_pct" "${{spd:-0.00kB/s}}" "$elap" "${{rem:---:--:--}}" "${{b//,/}}" > "$tmp_file" 2>/dev/null || true
        chmod 0644 "$tmp_file" 2>/dev/null || true
        mv -f "$tmp_file" "$PROG_FILE" 2>/dev/null || true
    done
}}

mkdir -p "$BKP_DIR" "$LOG_DIR" "$PERM_DIR"
chown root:grp_samba "$PERM_DIR" 2>/dev/null || true
chmod 2770 "$PERM_DIR" 2>/dev/null || true
exec 9>"${{LOCK_DIR:-/var/lock}}/backup_$TASK.lock"
flock -n 9 || {{ log "=== BACKUP OMITIDO: ya hay una ejecucion en curso ($DATE_STR) ==="; exit 0; }}

read -r AVAIL PCT < <(df -Pk "$BKP_DIR" | awk 'NR==2 {{gsub("%","",$5); print $4, $5}}')
if [ "${{AVAIL:-0}}" -lt 2097152 ] || [ "${{PCT:-0}}" -gt 95 ]; then
    log "=== ABORTADO: espacio libre insuficiente en $BKP_DIR ($AVAIL KB, $PCT%) ==="
    exit 1
fi
if [ "${{PCT:-0}}" -gt 85 ]; then
    log "alerta preventiva: ocupación del volumen en $PCT%"
fi

mapfile -t PREV_SNAPS < <(find "$BKP_DIR" -maxdepth 1 -type d -name 'snapshot_*' | LC_ALL=C sort)
LINK_ARGS=()
if [ "${{#PREV_SNAPS[@]}}" -gt 0 ]; then
    LAST="${{PREV_SNAPS[$((${{#PREV_SNAPS[@]}} - 1))]}}"
    LINK_ARGS=("--link-dest=$LAST")
fi

mkdir -p "$STAGE"
{source_setup}

log "inicio ($MODE): $SRC"
printf '{{"percent":0,"speed":"0.00kB/s","elapsed":"00:00:00","remaining":"--:--:--","bytes":"0"}}\n' > "$PROG_FILE" 2>/dev/null || true
chmod 0644 "$PROG_FILE" 2>/dev/null || true
{rsync}
if [ "$RSYNC_RC" -ne 0 ] && [ "$RSYNC_RC" -ne 24 ]; then
    log "ERROR: en replicación rsync (código $RSYNC_RC)"
    exit "$RSYNC_RC"
fi
mv "$STAGE" "$FINAL"
SNAPSHOT_OK=true
if btrfs subvolume show "$FINAL" >/dev/null 2>&1; then btrfs property set "$FINAL" ro true 2>/dev/null || true; fi
chmod -R a-w "$FINAL" 2>/dev/null || true

if [ "$AUTO_PERMANENT" = "true" ]; then
    HAS_PERM=false
    if [ -d "$PERM_DIR" ] && find "$PERM_DIR" -mindepth 1 -maxdepth 1 -type d -name "FULL_*" 2>/dev/null | grep -q .; then
        HAS_PERM=true
    fi

    if [ "$HAS_PERM" != "true" ]; then
        PERM_DEST="$PERM_DIR/FULL_${{DATE_STR}}_INICIAL"
        if cp -al "$FINAL" "$PERM_DEST" 2>/dev/null; then
            printf '{{"task":"%s","name":"%s","path":"%s","date":"%s","tag":"INICIAL","size_bytes":0,"files_count":0}}\n' \
                "$TASK" "FULL_${{DATE_STR}}_INICIAL" "$PERM_DEST" "$DATE_STR" > "$PERM_DEST/.freeze_meta.json" 2>/dev/null || true
            chmod -R a-w "$PERM_DEST" 2>/dev/null || true
            chattr +i "$PERM_DEST" 2>/dev/null || true
            log "congelación inicial automática: FULL_${{DATE_STR}}_INICIAL"
        fi
    else
        DAY_OF_MONTH=$(date +%d)
        if [ "$DAY_OF_MONTH" = "01" ]; then
            MONTH_TAG=$(date +%Y-%m)
            if ! find "$PERM_DIR" -maxdepth 1 -type d -name "FULL_${{MONTH_TAG}}*" 2>/dev/null | grep -q .; then
                PERM_DEST="$PERM_DIR/FULL_${{DATE_STR}}_MENSUAL"
                if cp -al "$FINAL" "$PERM_DEST" 2>/dev/null; then
                    printf '{{"task":"%s","name":"%s","path":"%s","date":"%s","tag":"MENSUAL","size_bytes":0,"files_count":0}}\n' \
                        "$TASK" "FULL_${{DATE_STR}}_MENSUAL" "$PERM_DEST" "$DATE_STR" > "$PERM_DEST/.freeze_meta.json" 2>/dev/null || true
                    chmod -R a-w "$PERM_DEST" 2>/dev/null || true
                    chattr +i "$PERM_DEST" 2>/dev/null || true
                    log "congelación mensual automática: FULL_${{DATE_STR}}_MENSUAL"
                fi
            fi
        fi
    fi
fi

chattr +i "$FINAL" 2>/dev/null || true
log "snapshot completado: $FINAL"

mapfile -t ALL_SNAPS < <(find "$BKP_DIR" -maxdepth 1 -type d -name 'snapshot_*' | LC_ALL=C sort)
TOTAL_SNAPS="${{#ALL_SNAPS[@]}}"
if [ "$TOTAL_SNAPS" -gt "$RETENTION" ]; then
    TO_REMOVE=$((TOTAL_SNAPS - RETENTION))
    for ((i = 0; i < TO_REMOVE; i++)); do
        OLD="${{ALL_SNAPS[$i]}}"
        chattr -R -i "$OLD" 2>/dev/null || true
        chmod -R u+w "$OLD" 2>/dev/null || true
        if rm -rf -- "$OLD"; then
            log "retención: eliminado $(basename "$OLD")"
        fi
    done
    if [ -d "$PERM_DIR" ]; then
        find "$PERM_DIR" -mindepth 1 -maxdepth 1 -type d -name 'FULL_*' -exec chattr +i {{}} + 2>/dev/null || true
    fi
fi
"#,
            q_id = shell_single_quote(id),
            q_type = shell_single_quote(src_type),
            q_ip = shell_single_quote(src_ip),
            q_share = shell_single_quote(src_share),
            q_path = shell_single_quote(src_path),
            q_user = shell_single_quote(src_user),
            port = port,
            retention = self.retention,
            auto_permanent = self.auto_permanent,
            q_dest_root = shell_single_quote(&dest_root),
            q_cred = shell_single_quote(&cred),
            q_key = shell_single_quote(&key_file),
            q_dest = shell_single_quote(&dest),
        )
    }

    /// Entrada de cron.d lista para instalar.
    pub fn render_cron(&self) -> String {
        format!(
            "SHELL=/bin/bash\nPATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin\nMAILTO=\"\"\n{} root /usr/local/bin/backup_{}.sh\n",
            self.cron.trim(),
            self.id.trim()
        )
    }

    /// Archivo de credenciales 0600 (CIFS: clave=valor con soporte de dominio; SSH: clave literal sin evaluación shell).
    pub fn render_cred(&self, proto: Proto) -> String {
        match proto {
            Proto::Cifs => {
                let user = self.user.trim();
                let password = &self.password;
                if let Some((domain, account)) = split_domain_user(user) {
                    format!("domain={domain}\nusername={account}\npassword={password}\n")
                } else {
                    format!("username={user}\npassword={password}\n")
                }
            }
            Proto::Ssh => {
                if self.password.is_empty() {
                    String::new()
                } else {
                    format!("{}\n", self.password)
                }
            }
            Proto::Local => String::new(),
        }
    }

    /// Contenido de la clave privada SSH para guardar en `/etc/backup-credentials/<id>.id_ed25519` (0600 root:root).
    pub fn render_key(&self, proto: Proto) -> Option<String> {
        if proto != Proto::Ssh {
            return None;
        }
        let key = self.ssh_key.as_deref()?.trim();
        if key.starts_with("-----BEGIN") {
            let mut content = key.to_string();
            if !content.ends_with('\n') {
                content.push('\n');
            }
            Some(content)
        } else {
            None
        }
    }
}

fn split_domain_user(user: &str) -> Option<(&str, &str)> {
    user.split_once('\\')
        .or_else(|| user.split_once('/'))
        .filter(|(domain, account)| !domain.is_empty() && !account.is_empty())
}

fn valid_host(host: &str) -> bool {
    let host = host.trim();
    if host.is_empty() || host.len() > 253 || host.starts_with('-') || host.contains("..") {
        return false;
    }
    host.chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == ':')
}

fn valid_ssh_user(user: &str) -> bool {
    !user.is_empty()
        && user.len() <= 64
        && !user.starts_with('-')
        && user
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
}

fn valid_cifs_user(user: &str) -> bool {
    !user.is_empty()
        && user.len() <= 64
        && !user.starts_with('-')
        && user
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | '\\' | '/' | '@'))
}

fn valid_cron(expr: &str) -> bool {
    let parts: Vec<&str> = expr.split_whitespace().collect();
    parts.len() == 5
        && parts.iter().all(|part| {
            !part.is_empty()
                && part
                    .chars()
                    .all(|c| c.is_ascii_digit() || matches!(c, '*' | '/' | ',' | '-' | ' '))
        })
}

fn shell_single_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

#[cfg(test)]
mod tests {
    use super::{BackupSpec, Proto};
    use crate::tasks::{BackupTask, SourceKind};

    fn cifs() -> BackupSpec {
        BackupSpec {
            id: "contabilidad".into(),
            proto: "cifs".into(),
            ip: "10.10.1.50".into(),
            share: "Contabilidad$".into(),
            path: String::new(),
            user: "backup".into(),
            password: "Clave123#".into(),
            cron: "0 2 * * *".into(),
            retention: 20,
            dest_unit: "principal".into(),
            port: None,
            ssh_key: None,
            auto_permanent: true,
        }
    }

    #[test]
    fn valida_tarea_cifs() {
        assert_eq!(cifs().validate().expect("válida"), Proto::Cifs);
        assert_eq!(
            cifs().destination(),
            "/srv/nas/BACKUPS_HISTORICOS/contabilidad"
        );
    }

    #[test]
    fn rechaza_identificadores_y_cron_invalidos() {
        let mut bad = cifs();
        bad.id = "a b".into();
        assert!(bad.validate().is_err());

        let mut bad = cifs();
        bad.cron = "cada día".into();
        assert!(bad.validate().is_err());

        let mut bad = cifs();
        bad.retention = 0;
        assert!(bad.validate().is_err());
    }

    #[test]
    fn generan_runner_cron_y_credenciales() {
        let spec = cifs();
        let runner = spec.render_runner(Proto::Cifs);
        assert!(runner.contains("flock -n 9"));
        assert!(runner.contains("--link-dest"));
        assert!(runner.contains("/srv/nas/BACKUPS_HISTORICOS/contabilidad"));
        assert!(runner.contains("mount -t cifs"));
        assert!(runner.contains("chmod -R a-w \"$FINAL\""));
        assert!(runner.contains("chattr +i \"$FINAL\""));
        assert!(runner.contains("LOCK_DIR:-/var/lock"));
        assert!(runner.contains("MOUNT_ROOT:-/mnt/backup_sources"));
        assert!(runner.contains("ERROR: se descarta el snapshot parcial"));
        assert!(runner.contains("2>> \"$LOG\""));
        assert!(runner.contains("--info=progress2"));
        assert!(runner.contains("update_progress"));
        assert!(runner.contains("FULL_${DATE_STR}_INICIAL"));
        assert!(runner.contains("AUTO_PERMANENT=true"));

        let cron = spec.render_cron();
        assert!(cron.contains("0 2 * * * root /usr/local/bin/backup_contabilidad.sh"));

        let cred = spec.render_cred(Proto::Cifs);
        assert!(cred.contains("username=backup"));
        assert!(cred.contains("password=Clave123#"));
    }

    #[test]
    fn valida_ssh_y_local() {
        let mut ssh = cifs();
        ssh.proto = "ssh".into();
        ssh.path = "/var/www".into();
        ssh.user = "bk".into();
        assert_eq!(ssh.validate().expect("ssh"), Proto::Ssh);
        assert!(ssh.render_runner(Proto::Ssh).contains("sshpass -e rsync"));

        let mut local = cifs();
        local.proto = "local".into();
        local.path = "/datos".into();
        assert_eq!(local.validate().expect("local"), Proto::Local);

        let mut bad = cifs();
        bad.proto = "ftp".into();
        assert!(bad.validate().is_err());
    }

    #[test]
    fn runner_generado_es_parseable_por_backup_task_en_todos_los_protocolos() {
        let mut cifs_spec = cifs();
        cifs_spec.user = "EADCOL\\svc_backup".into();
        cifs_spec.dest_unit = "disco2".into();
        let proto = cifs_spec.validate().expect("cifs válida");
        let parsed =
            BackupTask::parse("contabilidad", &cifs_spec.render_runner(proto)).expect("parse cifs");
        assert_eq!(parsed.kind, SourceKind::Cifs);
        assert_eq!(parsed.source_ip.as_deref(), Some("10.10.1.50"));
        assert_eq!(parsed.share.as_deref(), Some("Contabilidad$"));
        assert_eq!(parsed.user.as_deref(), Some("EADCOL\\svc_backup"));
        assert_eq!(
            parsed.destination_root,
            "/srv/nas/unidades/disco2/BACKUPS_HISTORICOS"
        );
        assert!(parsed.validate().is_empty());

        let cred_cifs = cifs_spec.render_cred(Proto::Cifs);
        assert!(cred_cifs.contains("domain=EADCOL\nusername=svc_backup\npassword=Clave123#\n"));

        let mut ssh_spec = cifs();
        ssh_spec.id = "srv_linux".into();
        ssh_spec.proto = "ssh".into();
        ssh_spec.path = "/var/www/app".into();
        ssh_spec.user = "operador".into();
        ssh_spec.port = Some(2222);
        ssh_spec.password = "$(id > /tmp/pwned)".into();
        let ssh_proto = ssh_spec.validate().expect("ssh válida");
        let ssh_runner = ssh_spec.render_runner(ssh_proto);
        assert!(!ssh_runner.contains(". \"$CRED\""));
        assert_eq!(ssh_spec.render_cred(Proto::Ssh), "$(id > /tmp/pwned)\n");
        let parsed_ssh = BackupTask::parse("srv_linux", &ssh_runner).expect("parse ssh");
        assert_eq!(parsed_ssh.kind, SourceKind::Ssh);
        assert_eq!(parsed_ssh.source_ip.as_deref(), Some("10.10.1.50"));
        assert_eq!(parsed_ssh.path.as_deref(), Some("/var/www/app"));
        assert_eq!(parsed_ssh.user.as_deref(), Some("operador"));
        assert_eq!(parsed_ssh.port, Some(2222));
        assert!(parsed_ssh.validate().is_empty());

        let mut local_spec = cifs();
        local_spec.id = "local_etc".into();
        local_spec.proto = "local".into();
        local_spec.path = "/etc/samba".into();
        let local_proto = local_spec.validate().expect("local válida");
        let parsed_local = BackupTask::parse("local_etc", &local_spec.render_runner(local_proto))
            .expect("parse local");
        assert_eq!(parsed_local.kind, SourceKind::Local);
        assert_eq!(parsed_local.path.as_deref(), Some("/etc/samba"));
        assert!(parsed_local.validate().is_empty());
    }

    #[test]
    fn rechaza_opciones_ssh_en_usuario_y_saltos_de_linea() {
        let mut bad_ssh = cifs();
        bad_ssh.proto = "ssh".into();
        bad_ssh.path = "/datos".into();
        bad_ssh.user = "-oProxyCommand=id".into();
        assert!(bad_ssh.validate().is_err());

        let mut bad_pwd = cifs();
        bad_pwd.password = "linea1\nusername=otro".into();
        assert!(bad_pwd.validate().is_err());

        let mut bad_unit = cifs();
        bad_unit.dest_unit = "../etc".into();
        assert!(bad_unit.validate().is_err());
    }

    #[test]
    fn soporta_autenticacion_por_clave_ssh_ed25519() {
        let mut ssh_key_spec = cifs();
        ssh_key_spec.id = "srv_ed25519".into();
        ssh_key_spec.proto = "ssh".into();
        ssh_key_spec.path = "/opt/data".into();
        ssh_key_spec.user = "ansible".into();
        let key_data = "-----BEGIN OPENSSH PRIVATE KEY-----\nb3BlbnNzaC1rZXktdjEAAAAABG5vbmUAAAAEbm9uZQAAAAAAAAABAAAAMwAAAAtzc2gtZW\nQyNTUxOQAAACD012345678901234567890123456789012345678901234567890123456\n-----END OPENSSH PRIVATE KEY-----\n";
        ssh_key_spec.ssh_key = Some(key_data.into());

        let proto = ssh_key_spec.validate().expect("ssh válido");
        assert_eq!(proto, Proto::Ssh);

        let key_content = ssh_key_spec.render_key(proto).expect("clave");
        assert_eq!(key_content, key_data);

        let runner = ssh_key_spec.render_runner(proto);
        assert!(runner.contains("KEY_FILE='/etc/backup-credentials/srv_ed25519.id_ed25519'"));
        assert!(runner.contains("ssh -i $KEY_FILE -p $SRC_PORT"));
        assert!(runner.contains("ssh -i /etc/backup-credentials/$TASK.id_ed25519 -p $SRC_PORT"));
        assert!(runner.contains("sshpass -e rsync"));

        // Clave malformada es rechazada
        let mut bad_key = ssh_key_spec.clone();
        bad_key.ssh_key = Some("clave-invalida-demasiado-corta".into());
        assert!(bad_key.validate().is_err());
    }

    #[test]
    fn tarea_ssh_sin_clave_usa_sshpass_y_no_define_key_file() {
        let mut ssh_spec = cifs();
        ssh_spec.id = "srv_pwd_test".into();
        ssh_spec.proto = "ssh".into();
        ssh_spec.path = "/var/log".into();
        ssh_spec.user = "sistemas".into();
        ssh_spec.password = "secreta".into();
        ssh_spec.ssh_key = None;

        let proto = ssh_spec.validate().expect("ssh válido");
        let runner = ssh_spec.render_runner(proto);

        // El runner no debe definir KEY_FILE con una ruta ficticia si no hay clave
        assert!(runner.contains("KEY_FILE=''") || runner.contains("KEY_FILE=\"\""));

        let parsed = BackupTask::parse("srv_pwd_test", &runner).expect("parse");
        assert_eq!(parsed.key_file, None);

        let plan = parsed.plan(None, &chrono::Local::now());
        // Debe usar sshpass porque no tiene clave configurada
        let has_sshpass = plan
            .iter()
            .any(|step| step.first().is_some_and(|arg| arg == "sshpass"));
        assert!(
            has_sshpass,
            "una tarea SSH sin clave debe planificar con sshpass"
        );
    }
}
