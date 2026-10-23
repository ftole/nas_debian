//! Ejecución de comandos con argumentos tipados, timeout y redacción.

use std::io::{Read, Write};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use nas_core::{NasError, NasResult};

use crate::redact::Redactor;

/// Valor secreto que solo puede viajar por `stdin`.
#[derive(Clone, PartialEq, Eq)]
pub struct SecretInput(String);

impl SecretInput {
    /// Envuelve un valor secreto.
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// Expone el valor para escribirlo en `stdin` (nunca para registrarlo).
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Debug for SecretInput {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SecretInput(***)")
    }
}

/// Forma de elevación de privilegios solicitada.
#[derive(Debug, Clone)]
pub enum SudoMode {
    /// Sin elevación.
    None,
    /// `sudo -n` (requiere regla NOPASSWD).
    Passwordless,
    /// `sudo -S` con la contraseña escrita por `stdin`.
    WithPassword(SecretInput),
}

/// Descripción declarativa de un comando a ejecutar.
#[derive(Debug, Clone)]
pub struct CommandSpec {
    /// Binario a ejecutar (nunca un shell).
    pub program: String,
    /// Argumentos, uno por elemento.
    pub args: Vec<String>,
    /// Tiempo máximo de ejecución.
    pub timeout: Duration,
    /// Datos opcionales para `stdin` (secretos incluidos).
    pub stdin_data: Option<SecretInput>,
    /// Modo de elevación.
    pub sudo: SudoMode,
    /// Variables de entorno adicionales.
    pub env: Vec<(String, String)>,
}

impl CommandSpec {
    /// Crea una especificación con timeout de 30 segundos.
    pub fn new(program: impl Into<String>) -> Self {
        Self {
            program: program.into(),
            args: Vec::new(),
            timeout: Duration::from_secs(30),
            stdin_data: None,
            sudo: SudoMode::None,
            env: Vec::new(),
        }
    }

    /// Añade un argumento.
    pub fn arg(mut self, arg: impl Into<String>) -> Self {
        self.args.push(arg.into());
        self
    }

    /// Añade varios argumentos.
    pub fn args<I, S>(mut self, args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.args.extend(args.into_iter().map(Into::into));
        self
    }

    /// Ajusta el timeout.
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Define el modo de elevación.
    pub fn sudo(mut self, mode: SudoMode) -> Self {
        self.sudo = mode;
        self
    }

    /// Define datos de entrada por `stdin`.
    pub fn stdin_secret(mut self, secret: SecretInput) -> Self {
        self.stdin_data = Some(secret);
        self
    }

    /// Añade una variable de entorno.
    pub fn env(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.env.push((key.into(), value.into()));
        self
    }

    /// Representación legible y **sin secretos** de la invocación.
    pub fn describe(&self) -> String {
        let mut parts = Vec::with_capacity(self.args.len() + 2);
        if !matches!(self.sudo, SudoMode::None) {
            parts.push("sudo".to_string());
        }
        parts.push(self.program.clone());
        for arg in &self.args {
            if arg.is_empty() {
                parts.push("''".to_string());
            } else if arg.contains(char::is_whitespace) {
                parts.push(format!("'{arg}'"));
            } else {
                parts.push(arg.clone());
            }
        }
        parts.join(" ")
    }
}

/// Resultado de una ejecución.
#[derive(Debug, Clone)]
pub struct CommandOutput {
    /// Código de salida (`-1` si fue terminado por timeout).
    pub code: i32,
    /// Salida estándar redactada.
    pub stdout: String,
    /// Salida de error redactada.
    pub stderr: String,
    /// Indica si se agotó el tiempo límite.
    pub timed_out: bool,
    /// Duración medida.
    pub duration: Duration,
    /// Comando lógico ejecutado (sin secretos).
    pub command: String,
}

impl CommandOutput {
    /// Indica si el comando terminó correctamente.
    pub fn is_success(&self) -> bool {
        self.code == 0 && !self.timed_out
    }

    /// Convierte un fallo en error del dominio.
    pub fn ensure_success(self) -> NasResult<Self> {
        if self.timed_out {
            return Err(NasError::Timeout {
                command: self.command,
                timeout_secs: self.duration.as_secs().max(1),
            });
        }
        if self.code != 0 {
            return Err(NasError::Command {
                command: self.command,
                code: self.code,
                stderr: if self.stderr.is_empty() {
                    self.stdout.clone()
                } else {
                    self.stderr.clone()
                },
            });
        }
        Ok(self)
    }
}

/// Ejecutor con redacción de secretos integrada.
#[derive(Debug, Clone)]
pub struct Runner {
    redactor: Redactor,
}

impl Default for Runner {
    fn default() -> Self {
        Self::new()
    }
}

impl Runner {
    /// Crea un ejecutor sin secretos registrados.
    pub fn new() -> Self {
        Self {
            redactor: Redactor::new(),
        }
    }

    /// Registra un secreto para que nunca aparezca en las salidas capturadas.
    pub fn with_secret(mut self, secret: impl Into<String>) -> Self {
        self.redactor.add_secret(secret);
        self
    }

    /// Redactor asociado.
    pub fn redactor(&self) -> &Redactor {
        &self.redactor
    }

    /// Ejecuta el comando describiéndolo sin secretos.
    pub fn run(&self, spec: &CommandSpec) -> NasResult<CommandOutput> {
        let command_line = spec.describe();
        let started = Instant::now();

        let mut command = match &spec.sudo {
            SudoMode::None => {
                let mut c = Command::new(&spec.program);
                c.args(&spec.args);
                c
            }
            SudoMode::Passwordless => {
                let mut c = Command::new("sudo");
                c.arg("-n")
                    .arg("-p")
                    .arg("")
                    .arg(&spec.program)
                    .args(&spec.args);
                c
            }
            SudoMode::WithPassword(_) => {
                let mut c = Command::new("sudo");
                c.arg("-S")
                    .arg("-p")
                    .arg("")
                    .arg(&spec.program)
                    .args(&spec.args);
                c
            }
        };

        command
            .env_clear()
            .env(
                "PATH",
                "/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin",
            )
            .env("LC_ALL", "C")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        for (key, value) in &spec.env {
            command.env(key, value);
        }

        let mut child = command
            .spawn()
            .map_err(|e| NasError::io(&spec.program, e))?;

        let payload = match (&spec.sudo, &spec.stdin_data) {
            (SudoMode::WithPassword(secret), _) => Some(secret.expose().to_string()),
            (_, Some(secret)) => Some(secret.expose().to_string()),
            _ => None,
        };

        if let Some(mut stdin) = child.stdin.take() {
            if let Some(value) = payload {
                let mut data = value.into_bytes();
                data.push(b'\n');
                stdin
                    .write_all(&data)
                    .map_err(|e| NasError::io(&spec.program, e))?;
            }
        }

        let mut stdout_pipe = child
            .stdout
            .take()
            .ok_or_else(|| NasError::Invalid("stdout no disponible".into()))?;
        let mut stderr_pipe = child
            .stderr
            .take()
            .ok_or_else(|| NasError::Invalid("stderr no disponible".into()))?;

        let stdout_handle = thread::spawn(move || {
            let mut buf = Vec::new();
            let _ = stdout_pipe.read_to_end(&mut buf);
            buf
        });
        let stderr_handle = thread::spawn(move || {
            let mut buf = Vec::new();
            let _ = stderr_pipe.read_to_end(&mut buf);
            buf
        });

        let mut timed_out = false;
        let code;

        loop {
            match child.try_wait() {
                Ok(Some(status)) => {
                    code = status.code().unwrap_or(-1);
                    break;
                }
                Ok(None) => {}
                Err(e) => return Err(NasError::io(&spec.program, e)),
            }

            if started.elapsed() >= spec.timeout {
                let _ = child.kill();
                let _ = child.wait();
                timed_out = true;
                code = -1;
                break;
            }

            thread::sleep(Duration::from_millis(20));
        }

        let stdout_raw = stdout_handle.join().unwrap_or_default();
        let stderr_raw = stderr_handle.join().unwrap_or_default();
        let stdout = self.redactor.redact(&String::from_utf8_lossy(&stdout_raw));
        let stderr = self.redactor.redact(&String::from_utf8_lossy(&stderr_raw));

        Ok(CommandOutput {
            code,
            stdout: stdout.trim().to_string(),
            stderr: stderr.trim().to_string(),
            timed_out,
            duration: started.elapsed(),
            command: command_line,
        })
    }
}

#[cfg(test)]
mod tests {
    #[cfg(unix)]
    use super::Runner;
    use super::{CommandSpec, SecretInput, SudoMode};
    #[cfg(unix)]
    use std::time::Duration;

    #[test]
    fn describe_oculta_secretos_de_sudo() {
        let spec = CommandSpec::new("id")
            .arg("-u")
            .sudo(SudoMode::WithPassword(SecretInput::new(
                "clave-supersecreta",
            )));
        let described = spec.describe();
        assert!(described.starts_with("sudo id -u"));
        assert!(!described.contains("clave-supersecreta"));
    }

    #[test]
    fn describe_escapa_argumentos_con_espacios() {
        let spec = CommandSpec::new("echo").arg("hola mundo");
        assert_eq!(spec.describe(), "echo 'hola mundo'");
    }

    #[cfg(unix)]
    #[test]
    fn ejecuta_sin_shell_y_redacta() {
        let runner = Runner::new().with_secret("valor-secreto");
        let out = runner
            .run(&CommandSpec::new("printf").arg("%s").arg("valor-secreto"))
            .expect("ejecución");
        assert!(out.is_success());
        assert_eq!(out.stdout, "***");
    }

    #[cfg(unix)]
    #[test]
    fn respeta_el_timeout() {
        let runner = Runner::new();
        let spec = CommandSpec::new("sleep")
            .arg("5")
            .timeout(Duration::from_millis(150));
        let out = runner.run(&spec).expect("ejecución");
        assert!(out.timed_out);
        assert!(!out.is_success());
    }
}
