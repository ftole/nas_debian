//! Módulos helpers privilegiados ejecutados con permisos acotados de root.
//!
//! Sustituyen a los 7 scripts bash de `packaging/nas-*` por implementaciones
//! fuertemente tipadas en Rust, con validación estricta de argumentos,
//! control de discos del sistema operativo y manejo seguro de rutas en `/srv/nas`.

/// Gestión de usuarios, grupos y credenciales del sistema y Samba.
pub mod accounts;
/// Control, instalación, programación y pruebas de tareas de respaldo.
pub mod backup;
/// Aplicación atómica y validada de configuraciones de Samba.
pub mod samba;
/// Creación de directorios compartidos y aplicación de ACLs POSIX.
pub mod share;
/// Operaciones de almacenamiento (formateo, LVM, subvolúmenes y montajes).
pub mod storage;
/// Sesiones de terminal interactivas PTY sobre tmux aisladas por usuario.
pub mod terminal;
