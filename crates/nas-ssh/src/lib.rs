//! Transporte remoto sobre OpenSSH.
//!
//! Se apoya en el cliente `ssh`/`scp` del sistema (presente en Debian y Windows),
//! con autenticación por clave, `BatchMode` (sin prompts interactivos), política
//! `StrictHostKeyChecking=accept-new` y archivo `known_hosts` dedicado.
//!
//! Cada argumento remoto se cotiza individualmente antes de unirse: el shell del
//! servidor sólo recibe tokens ya escapados, nunca texto interpolado.

#![forbid(unsafe_code)]
#![deny(missing_docs)]

use std::path::{Path, PathBuf};
use std::time::Duration;

use nas_core::{NasError, NasResult};
use nas_exec::{CommandOutput, CommandSpec, Runner};

/// Destino SSH con su política de seguridad.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SshTarget {
    /// Host o dirección.
    pub host: String,
    /// Puerto.
    pub port: u16,
    /// Usuario.
    pub user: String,
    /// Archivo `known_hosts` dedicado al host.
    pub known_hosts: Option<PathBuf>,
    /// Clave privada a utilizar.
    pub identity: Option<PathBuf>,
    /// Timeout de conexión en segundos.
    pub connect_timeout_secs: u64,
}

impl SshTarget {
    /// Crea un destino con valores por defecto seguros.
    pub fn new(host: impl Into<String>, port: u16, user: impl Into<String>) -> Self {
        Self {
            host: host.into(),
            port,
            user: user.into(),
            known_hosts: None,
            identity: None,
            connect_timeout_secs: 10,
        }
    }

    /// Fija el archivo `known_hosts` específico del host.
    pub fn with_known_hosts(mut self, path: impl Into<PathBuf>) -> Self {
        self.known_hosts = Some(path.into());
        self
    }

    /// Fija la clave privada.
    pub fn with_identity(mut self, path: impl Into<PathBuf>) -> Self {
        self.identity = Some(path.into());
        self
    }

    /// Ajusta el timeout de conexión.
    pub fn with_connect_timeout(mut self, seconds: u64) -> Self {
        self.connect_timeout_secs = seconds;
        self
    }

    /// Forma `usuario@host` usada por `ssh` y `scp`.
    pub fn destination(&self) -> String {
        format!("{}@{}", self.user, self.host)
    }

    /// Opciones comunes de OpenSSH compartidas por `ssh` y `scp`.
    fn common_options(&self) -> Vec<String> {
        let mut args: Vec<String> = vec![
            "-o".into(),
            "BatchMode=yes".into(),
            "-o".into(),
            "StrictHostKeyChecking=accept-new".into(),
            "-o".into(),
            format!("ConnectTimeout={}", self.connect_timeout_secs),
        ];

        if let Some(known_hosts) = &self.known_hosts {
            args.push("-o".into());
            args.push(format!(
                "UserKnownHostsFile={}",
                known_hosts.to_string_lossy()
            ));
        }

        if let Some(identity) = &self.identity {
            args.push("-i".into());
            args.push(identity.to_string_lossy().to_string());
        }

        args
    }

    /// Opciones de `ssh` (incluye el puerto con `-p`).
    pub fn ssh_args(&self) -> Vec<String> {
        let mut args = self.common_options();
        args.push("-p".into());
        args.push(self.port.to_string());
        args
    }

    /// Opciones de `scp` (incluye el puerto con `-P`).
    pub fn scp_args(&self) -> Vec<String> {
        let mut args = self.common_options();
        args.push("-q".into());
        args.push("-P".into());
        args.push(self.port.to_string());
        args
    }
}

pub use nas_core::shell::{quote, quote_all};

/// Antepone `sudo -n --` a una invocación remota.
pub fn with_sudo(args: &[String]) -> Vec<String> {
    let mut prefixed = vec!["sudo".to_string(), "-n".to_string(), "--".to_string()];
    prefixed.extend(args.iter().cloned());
    prefixed
}

/// Ejecutor remoto abstraído para permitir pruebas sin red.
pub trait RemoteExecutor {
    /// Ejecuta un comando remoto compuesto por argumentos ya separados.
    fn run(&self, target: &SshTarget, argv: &[String]) -> NasResult<CommandOutput>;

    /// Sube un archivo local a una ruta remota.
    fn upload(&self, target: &SshTarget, local: &Path, remote: &str) -> NasResult<()>;
}

/// Implementación basada en los binarios `ssh` y `scp` del sistema.
#[derive(Debug, Clone)]
pub struct OpenSsh {
    runner: Runner,
}

impl Default for OpenSsh {
    fn default() -> Self {
        Self::new()
    }
}

impl OpenSsh {
    /// Crea el ejecutor con un `Runner` limpio.
    pub fn new() -> Self {
        Self {
            runner: Runner::new(),
        }
    }

    /// Timeout aplicado a cada invocación.
    fn timeout(target: &SshTarget) -> Duration {
        Duration::from_secs(target.connect_timeout_secs.max(10) * 6)
    }
}

impl RemoteExecutor for OpenSsh {
    fn run(&self, target: &SshTarget, argv: &[String]) -> NasResult<CommandOutput> {
        if argv.is_empty() {
            return Err(NasError::Invalid("comando remoto vacío".into()));
        }

        let mut args = target.ssh_args();
        args.push(target.destination());
        args.push(quote_all(argv));

        self.runner.run(
            &CommandSpec::new("ssh")
                .args(args)
                .timeout(Self::timeout(target)),
        )
    }

    fn upload(&self, target: &SshTarget, local: &Path, remote: &str) -> NasResult<()> {
        let mut args = target.scp_args();
        args.push(local.to_string_lossy().to_string());
        args.push(format!("{}:{remote}", target.destination()));

        let output = self.runner.run(
            &CommandSpec::new("scp")
                .args(args)
                .timeout(Self::timeout(target)),
        )?;

        if output.is_success() {
            Ok(())
        } else {
            Err(NasError::Command {
                command: format!("scp {}", local.display()),
                code: output.code,
                stderr: output.stderr,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{quote, quote_all, with_sudo, SshTarget};

    #[test]
    fn cotiza_argumentos_peligrosos() {
        assert_eq!(quote("simple"), "simple");
        assert_eq!(quote("/var/www/nas-web"), "/var/www/nas-web");
        assert_eq!(quote("hola mundo"), "'hola mundo'");
        assert_eq!(quote("a;rm -rf /"), "'a;rm -rf /'");
        assert_eq!(quote("it's"), "'it'\\''s'");
        assert_eq!(quote(""), "''");
    }

    #[test]
    fn une_argumentos_cotizados() {
        let args = vec![
            "mkdir".to_string(),
            "-p".to_string(),
            "/tmp/a b".to_string(),
        ];
        assert_eq!(quote_all(&args), "mkdir -p '/tmp/a b'");
    }

    #[test]
    fn antepone_sudo_sin_password_interactiva() {
        let args = vec![
            "systemctl".to_string(),
            "reload".to_string(),
            "nginx".to_string(),
        ];
        assert_eq!(
            with_sudo(&args),
            vec!["sudo", "-n", "--", "systemctl", "reload", "nginx"]
        );
    }

    #[test]
    fn opciones_ssh_incluyen_politica_estricta() {
        let target = SshTarget::new("10.10.1.222", 2222, "nasdev")
            .with_known_hosts("/home/dev/.ssh/known_hosts.d/lab-dev");
        let args = target.ssh_args();
        let joined = args.join(" ");

        assert!(joined.contains("BatchMode=yes"));
        assert!(joined.contains("StrictHostKeyChecking=accept-new"));
        assert!(joined.contains("UserKnownHostsFile=/home/dev/.ssh/known_hosts.d/lab-dev"));
        assert!(joined.contains("-p 2222"));
        assert_eq!(target.destination(), "nasdev@10.10.1.222");
    }

    #[test]
    fn incluye_clave_privada_si_se_indica() {
        let target = SshTarget::new("h", 22, "u").with_identity("/home/u/.ssh/id_ed25519");
        assert!(target
            .ssh_args()
            .join(" ")
            .contains("-i /home/u/.ssh/id_ed25519"));
    }
}
