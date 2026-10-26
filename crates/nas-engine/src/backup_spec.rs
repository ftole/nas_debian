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

        match proto {
            Proto::Cifs => {
                if !valid_host(&self.ip) {
                    return Err(NasError::Invalid("IP o host del origen inválido".into()));
                }
                if self.share.trim().is_empty() || self.user.trim().is_empty() {
                    return Err(NasError::Invalid("CIFS requiere recurso y usuario".into()));
                }
            }
            Proto::Ssh => {
                if !valid_host(&self.ip) {
                    return Err(NasError::Invalid("IP o host del origen inválido".into()));
                }
                if !self.path.starts_with('/') || self.user.trim().is_empty() {
                    return Err(NasError::Invalid(
                        "SSH requiere ruta absoluta y usuario".into(),
                    ));
                }
            }
            Proto::Local => {
                if !self.path.starts_with('/') {
                    return Err(NasError::Invalid("la ruta local debe ser absoluta".into()));
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

    /// Directorio de destino.
    pub fn destination(&self) -> String {
        if self.dest_unit.is_empty() || self.dest_unit == "principal" {
            format!("/srv/nas/BACKUPS_HISTORICOS/{}", self.id)
        } else {
            format!(
                "/srv/nas/unidades/{}/BACKUPS_HISTORICOS/{}",
                self.dest_unit, self.id
            )
        }
    }

    /// Runner bash listo para instalar.
    pub fn render_runner(&self, proto: Proto) -> String {
        let dest = self.destination();
        let cred = format!("/etc/backup-credentials/{}.cred", self.id);
        let port = self.port.unwrap_or(22);

        let source_setup = match proto {
            Proto::Cifs => format!(
                "mkdir -p \"$MOUNT\"\n    mount -t cifs \"//{ip}/{share}\" \"$MOUNT\" -o \"credentials=$CRED,ro,iocharset=utf8,vers=3.1.1,noserverino,cache=none,soft,nobrl\"\n    SRC=\"$MOUNT/\"",
                ip = shell_quote(&self.ip),
                share = shell_quote(&self.share)
            ),
            Proto::Ssh => format!(
                "SRC=\"{}@{}:{}/\"",
                shell_quote(&self.user),
                shell_quote(&self.ip),
                shell_quote(self.path.trim_end_matches('/'))
            ),
            Proto::Local => format!("SRC=\"{}/\"", shell_quote(self.path.trim_end_matches('/'))),
        };

        let rsync = match proto {
            Proto::Ssh => format!(
                "if [ -f \"$CRED\" ]; then . \"$CRED\"; fi\n    SSHPASS=\"${{SSH_PASSWORD:-}}\" sshpass -e rsync -aAXH --numeric-ids --timeout=300 $LINK -e \"ssh -p {port} -o StrictHostKeyChecking=accept-new -o UserKnownHostsFile=/root/.ssh/known_hosts_backup\" \"$SRC\" \"$STAGE\"",
                port = port
            ),
            _ => "rsync -aAXH --numeric-ids --timeout=300 $LINK \"$SRC\" \"$STAGE\"".to_string(),
        };

        format!(
            r#"#!/bin/bash
set -euo pipefail
TASK="{id}"
RETENTION={retention}
MODE="{mode}"
CRED="{cred}"
BKP_DIR="{dest}"
LOG="/srv/nas/LOGS_BACKUP/backup_{id}.log"
MOUNT="/mnt/backup_sources/{id}"
DATE_STR=$(date +%Y-%m-%d_%H%M%S)
STAGE="$BKP_DIR/.inprogress_$DATE_STR"
FINAL="$BKP_DIR/snapshot_$DATE_STR"

log() {{ printf '[%s] [%s] %s\n' "$(date '+%F %T')" "$TASK" "$*" >> "$LOG"; }}
cleanup() {{
    if mountpoint -q "$MOUNT" 2>/dev/null; then umount "$MOUNT" || true; fi
    if [ -d "$STAGE" ]; then rm -rf "$STAGE"; fi
}}
trap cleanup EXIT TERM INT

mkdir -p "$BKP_DIR" /srv/nas/LOGS_BACKUP
exec 9>"/var/lock/backup_$TASK.lock"
flock -n 9 || {{ log "omitido: otra ejecución en curso"; exit 0; }}

read -r AVAIL PCT < <(df -Pk "$BKP_DIR" | awk 'NR==2 {{gsub("%","",$5); print $4, $5}}')
if [ "${{AVAIL:-0}}" -lt 2097152 ] || [ "${{PCT:-0}}" -gt 95 ]; then
    log "abortado: espacio insuficiente ($AVAIL KB, $PCT%)"
    exit 1
fi

LAST=$(find "$BKP_DIR" -maxdepth 1 -type d -name 'snapshot_*' | sort | tail -n 1)
LINK=""
[ -n "$LAST" ] && LINK="--link-dest=$LAST"

LINK="$LINK"
{source_setup}

log "inicio ($MODE): $SRC"
{rsync}
mv "$STAGE" "$FINAL"
log "snapshot completado: $FINAL"

COUNT=$(find "$BKP_DIR" -maxdepth 1 -type d -name 'snapshot_*' | wc -l)
if [ "$COUNT" -gt "$RETENTION" ]; then
    find "$BKP_DIR" -maxdepth 1 -type d -name 'snapshot_*' | sort | head -n -"$RETENTION" | while read -r OLD; do
        rm -rf "$OLD" && log "retención: eliminado $(basename "$OLD")"
    done
fi
"#,
            id = self.id.trim(),
            retention = self.retention,
            mode = self.proto,
            dest = dest,
            cred = cred,
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

    /// Archivo de credenciales (CIFS: clave=valor; SSH: variable shell).
    pub fn render_cred(&self, proto: Proto) -> String {
        match proto {
            Proto::Cifs => format!(
                "username={}\npassword={}\n",
                self.user.trim(),
                self.password.trim()
            ),
            Proto::Ssh => format!("SSH_PASSWORD={}\n", shell_quote(&self.password)),
            Proto::Local => String::new(),
        }
    }
}

fn valid_host(host: &str) -> bool {
    let host = host.trim();
    if host.is_empty() || host.len() > 253 || host.contains("..") {
        return false;
    }
    host.chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == ':')
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

fn shell_quote(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

#[cfg(test)]
mod tests {
    use super::{BackupSpec, Proto};

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
}
