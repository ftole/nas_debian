//! Ejecución de comandos con argumentos tipados, timeout y redacción.

use std::io::{Read, Write};
use std::process::{Command, Stdio};
use std::sync::OnceLock;
use std::thread;
use std::time::{Duration, Instant};

use nas_core::{NasError, NasResult, ResourceBudget};

use crate::redact::Redactor;

fn default_max_output_bytes() -> usize {
    static LIMIT: OnceLock<usize> = OnceLock::new();
    *LIMIT.get_or_init(|| ResourceBudget::detect().exec_output_limit_bytes.max(65_536))
}

fn read_bounded<R: Read>(mut reader: R, limit: usize) -> (Vec<u8>, bool) {
    let mut out = Vec::new();
    let mut chunk = [0u8; 16 * 1024];
    let mut truncated = false;
    loop {
        match reader.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => {
                if out.len() < limit {
                    let take = n.min(limit - out.len());
                    out.extend_from_slice(&chunk[..take]);
                    if take < n {
                        truncated = true;
                    }
                } else {
                    truncated = true;
                }
            }
            Err(err) if err.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(_) => break,
        }
    }
    (out, truncated)
}

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
    /// Conserva espacios iniciales/finales en `stdout` sin recortar.
    pub preserve_whitespace: bool,
    /// Límite máximo de bytes capturados por flujo (`stdout` y `stderr`).
    pub max_output_bytes: usize,
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
            preserve_whitespace: false,
            max_output_bytes: default_max_output_bytes(),
        }
    }

    /// Ajusta el límite máximo de bytes capturados por flujo (`stdout` / `stderr`).
    pub fn max_output_bytes(mut self, bytes: usize) -> Self {
        self.max_output_bytes = bytes.max(1024);
        self
    }

    /// Conserva los espacios en blanco de `stdout` tal cual los emite el proceso.
    pub fn preserve_whitespace(mut self) -> Self {
        self.preserve_whitespace = true;
        self
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

    /// Define datos de entrada por `stdin` (garantiza un único salto de línea final si falta).
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
                if !data.ends_with(b"\n") {
                    data.push(b'\n');
                }
                stdin
                    .write_all(&data)
                    .map_err(|e| NasError::io(&spec.program, e))?;
            }
        }

        let stdout_pipe = child
            .stdout
            .take()
            .ok_or_else(|| NasError::Invalid("stdout no disponible".into()))?;
        let stderr_pipe = child
            .stderr
            .take()
            .ok_or_else(|| NasError::Invalid("stderr no disponible".into()))?;

        let max_bytes = spec.max_output_bytes.max(1024);
        let stdout_handle = thread::spawn(move || read_bounded(stdout_pipe, max_bytes));
        let stderr_handle = thread::spawn(move || read_bounded(stderr_pipe, max_bytes));

        let mut timed_out = false;
        let code;
        let mut poll_ms = 2u64;

        loop {
            match child.try_wait() {
                Ok(Some(status)) => {
                    code = status.code().unwrap_or(-1);
                    break;
                }
                Ok(None) => {}
                Err(e) => return Err(NasError::io(&spec.program, e)),
            }

            let elapsed = started.elapsed();
            if elapsed >= spec.timeout {
                let _ = child.kill();
                let _ = child.wait();
                timed_out = true;
                code = -1;
                break;
            }

            let remaining = spec.timeout.saturating_sub(elapsed);
            let sleep_for = Duration::from_millis(poll_ms).min(remaining);
            thread::sleep(sleep_for);
            poll_ms = (poll_ms.saturating_mul(2)).min(25);
        }

        let (stdout_raw, stdout_truncated) = stdout_handle.join().unwrap_or_default();
        let (stderr_raw, stderr_truncated) = stderr_handle.join().unwrap_or_default();
        let mut stdout = self.redactor.redact(&String::from_utf8_lossy(&stdout_raw));
        let mut stderr = self.redactor.redact(&String::from_utf8_lossy(&stderr_raw));
        if stdout_truncated {
            stdout.push_str("\n[salida truncada al superar límite de memoria]");
        }
        if stderr_truncated {
            stderr.push_str("\n[error truncado al superar límite de memoria]");
        }

        let stdout_final = if spec.preserve_whitespace {
            stdout
        } else {
            stdout.trim().to_string()
        };

        Ok(CommandOutput {
            code,
            stdout: stdout_final,
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

    #[cfg(unix)]
    #[test]
    fn stdin_secret_no_duplica_salto_de_linea_final() {
        let runner = Runner::new();
        let con_salto = runner
            .run(
                &CommandSpec::new("cat")
                    .preserve_whitespace()
                    .stdin_secret(SecretInput::new("nas:Clave123\n")),
            )
            .expect("ejecución con salto");
        assert!(con_salto.is_success());
        assert_eq!(con_salto.stdout, "nas:Clave123\n");

        let doble_linea = runner
            .run(
                &CommandSpec::new("cat")
                    .preserve_whitespace()
                    .stdin_secret(SecretInput::new("Clave123\nClave123\n")),
            )
            .expect("ejecución multilínea");
        assert!(doble_linea.is_success());
        assert_eq!(doble_linea.stdout, "Clave123\nClave123\n");

        let sin_salto = runner
            .run(
                &CommandSpec::new("cat")
                    .preserve_whitespace()
                    .stdin_secret(SecretInput::new("nas:Clave123")),
            )
            .expect("ejecución sin salto");
        assert!(sin_salto.is_success());
        assert_eq!(sin_salto.stdout, "nas:Clave123\n");
    }

    #[cfg(unix)]
    #[test]
    fn acota_memoria_de_salida_sin_bloquear_proceso() {
        let runner = Runner::new();
        let payload = "A".repeat(4096);
        let out = runner
            .run(
                &CommandSpec::new("printf")
                    .arg("%s")
                    .arg(&payload)
                    .max_output_bytes(1024),
            )
            .expect("ejecución acotada");
        assert!(out.is_success());
        assert!(out
            .stdout
            .contains("[salida truncada al superar límite de memoria]"));
        assert!(out.stdout.len() < 1200);
    }
}
