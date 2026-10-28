//! Motor de reglas del servidor NAS.
//!
//! Toda la lógica que antes estaba duplicada en Bash, PHP y Python vive aquí:
//! parsers de salidas del sistema, agregación de estado, visión de red y
//! políticas de respaldo (snapshots, retención y control de espacio).

#![forbid(unsafe_code)]
#![deny(missing_docs)]

/// Políticas de respaldo: snapshots, retención y espacio.
pub mod backup;
/// Especificación y generación de tareas de respaldo.
pub mod backup_control;
pub mod backup_spec;
/// Planificación del despliegue del servidor.
pub mod deploy;
/// Diagnóstico integral del servidor.
pub mod diagnostics;
/// Integración con Active Directory.
pub mod domain;
/// Módulos helpers privilegiados ejecutados con permisos acotados de root.
pub mod helpers;
/// Bloqueo exclusivo por tarea.
pub mod lock;
/// Inventario agregado de red.
pub mod network;
/// Parsers de salidas del sistema operativo.
pub mod parsers;
/// Edición de smb.conf y ACLs de recursos.
pub mod samba;
/// Estado y gestión de demonios.
pub mod services;
/// Métricas de estado del sistema.
pub mod status;
/// Inventario de almacenamiento.
pub mod storage;
/// Operaciones privilegiadas de discos (formateo, LVM, subvolúmenes).
pub mod storage_ops;
/// Tareas de respaldo y sus planes de ejecución.
pub mod tasks;
/// Terminal web sobre sesiones tmux.
pub mod terminal;
/// Verificación y planificación de actualizaciones firmadas.
pub mod uninstall;
pub mod update;
/// Inventario de usuarios y grupos locales.
pub mod users;

pub use backup::{
    evaluate_space, parse_df, retention_victims, snapshot_dir_name, staging_dir_name, DfUsage,
    SpaceDecision,
};
pub use deploy::{DeployParams, FileSystem};
pub use diagnostics::Check;
pub use domain::DomainStatus;
pub use lock::TaskLock;
pub use network::{collect as collect_network, NetworkOverview};
pub use services::ServiceStatus;
pub use status::{collect as collect_status, SystemMetrics};
pub use storage::{Disk, StorageOverview};
pub use tasks::{last_snapshot, run as run_backup, BackupTask, RunOptions, RunReport, SourceKind};
pub use terminal::TerminalSession;
pub use update::{build_plan as build_update_plan, ReleaseManifest, UpdatePlan};
pub use users::{GroupEntry, UserEntry};
