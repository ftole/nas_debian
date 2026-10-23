//! Motor de reglas del servidor NAS.
//!
//! Toda la lógica que antes estaba duplicada en Bash, PHP y Python vive aquí:
//! parsers de salidas del sistema, agregación de estado, visión de red y
//! políticas de respaldo (snapshots, retención y control de espacio).

#![forbid(unsafe_code)]
#![deny(missing_docs)]

/// Políticas de respaldo: snapshots, retención y espacio.
pub mod backup;
/// Bloqueo exclusivo por tarea.
pub mod lock;
/// Inventario agregado de red.
pub mod network;
/// Parsers de salidas del sistema operativo.
pub mod parsers;
/// Métricas de estado del sistema.
pub mod status;
/// Tareas de respaldo y sus planes de ejecución.
pub mod tasks;

pub use backup::{
    evaluate_space, parse_df, retention_victims, snapshot_dir_name, staging_dir_name, DfUsage,
    SpaceDecision,
};
pub use lock::TaskLock;
pub use network::{collect as collect_network, NetworkOverview};
pub use status::{collect as collect_status, SystemMetrics};
pub use tasks::{BackupTask, SourceKind};
