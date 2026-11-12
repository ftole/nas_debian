//! Generación validada de tareas de respaldo (runner + cron + credenciales).

use nas_core::{NasError, NasResult};
use serde::{Deserialize, Serialize};

mod render;

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
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupSpec {
    /// Identificador de la tarea.
    pub id: String,
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
    /// Conservar credenciales o clave SSH previas si se omiten durante una actualización.
    #[serde(default)]
    pub preserve_existing_auth: bool,
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
            preserve_existing_auth: false,
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
        assert!(runner.contains("codigo 23"));
        assert!(runner.contains("codigo 24"));
        assert!(runner.contains("nobrl"));
        assert!(runner.contains("BKP_DIR=\"${BKP_DIR:-$DEST_ROOT/$TASK}\""));
        assert!(!runner.contains("BKP_DIR=\"${BKP_DIR:-'"));

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

        assert!(runner.contains("KEY_FILE=''") || runner.contains("KEY_FILE=\"\""));

        let parsed = BackupTask::parse("srv_pwd_test", &runner).expect("parse");
        assert_eq!(parsed.key_file, None);

        let plan = parsed.plan(None, &chrono::Local::now());
        let has_sshpass = plan
            .iter()
            .any(|step| step.first().is_some_and(|arg| arg == "sshpass"));
        assert!(
            has_sshpass,
            "una tarea SSH sin clave debe planificar con sshpass"
        );
    }
}
