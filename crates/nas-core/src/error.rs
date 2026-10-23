//! Errores unificados del dominio NAS.

use std::path::PathBuf;

/// Error del dominio NAS.
#[derive(Debug, thiserror::Error)]
pub enum NasError {
    /// Fallo de entrada/salida local.
    #[error("error de E/S en {path}: {source}")]
    Io {
        /// Ruta afectada.
        path: PathBuf,
        /// Causa original.
        #[source]
        source: std::io::Error,
    },

    /// Error de serialización o deserialización JSON.
    #[error("error de JSON: {0}")]
    Json(#[from] serde_json::Error),

    /// Un comando externo terminó con código distinto de cero.
    #[error("el comando '{command}' terminó con código {code}: {stderr}")]
    Command {
        /// Línea de comandos lógica (sin secretos).
        command: String,
        /// Código de salida.
        code: i32,
        /// Salida de error (ya redactada).
        stderr: String,
    },

    /// Un comando externo superó el tiempo máximo permitido.
    #[error("el comando '{command}' excedió el tiempo límite de {timeout_secs}s")]
    Timeout {
        /// Línea de comandos lógica.
        command: String,
        /// Límite en segundos.
        timeout_secs: u64,
    },

    /// Entrada inválida proporcionada por el usuario o por un archivo.
    #[error("valor inválido: {0}")]
    Invalid(String),

    /// Recurso no encontrado.
    #[error("no encontrado: {0}")]
    NotFound(String),

    /// Operación denegada por política de seguridad.
    #[error("operación denegada: {0}")]
    Denied(String),

    /// Dependencia externa ausente en el sistema.
    #[error("dependencia ausente: {0}")]
    MissingDependency(String),
}

impl NasError {
    /// Construye un error de E/S conservando la ruta afectada.
    pub fn io(path: impl Into<PathBuf>, source: std::io::Error) -> Self {
        Self::Io {
            path: path.into(),
            source,
        }
    }

    /// Código de salida sugerido para el proceso.
    pub fn exit_code(&self) -> i32 {
        match self {
            Self::Invalid(_) => 2,
            Self::Denied(_) => 3,
            Self::NotFound(_) | Self::MissingDependency(_) => 4,
            Self::Timeout { .. } => 5,
            Self::Command { code, .. } => {
                if *code == 0 {
                    1
                } else {
                    *code
                }
            }
            Self::Io { .. } | Self::Json(_) => 1,
        }
    }
}

/// Alias de resultado del dominio NAS.
pub type NasResult<T> = Result<T, NasError>;
