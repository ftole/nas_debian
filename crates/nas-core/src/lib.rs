//! Núcleo del servidor NAS Debian 13.
//!
//! Concentra lo que el CLI, el panel web y las herramientas de desarrollo deben
//! compartir sin duplicar lógica: errores, contratos de salida, configuración y
//! rutas del sistema.

#![forbid(unsafe_code)]
#![deny(missing_docs)]

/// Configuración, rol del servidor y rutas.
pub mod config;
/// Contratos de salida compartidos.
pub mod contract;
/// Errores del dominio.
pub mod error;

pub use config::{NasPaths, ServerRole};
pub use contract::ApiResponse;
pub use error::{NasError, NasResult};
