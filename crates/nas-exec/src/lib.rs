//! Ejecución segura de comandos externos.
//!
//! Reglas invariables:
//! - Nunca se invoca un intérprete de shell (`sh -c`, `bash -c`) con datos dinámicos.
//! - Los argumentos viajan como vectores, jamás como una cadena interpolada.
//! - Los secretos se escriben por `stdin`, nunca en `argv`.
//! - Toda salida capturada pasa por el redactor antes de registrarse o mostrarse.

#![forbid(unsafe_code)]
#![deny(missing_docs)]

/// Ejecución de comandos con timeout y sudo.
pub mod command;
/// Redacción de secretos en salidas.
pub mod redact;

pub use command::{CommandOutput, CommandSpec, Runner, SecretInput, SudoMode};
pub use redact::Redactor;
