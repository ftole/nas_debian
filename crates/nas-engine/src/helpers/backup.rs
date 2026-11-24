//! Helpers para instalación, control y diagnóstico de tareas de respaldo.

mod conn;
mod control;
mod install;
mod permanent;
mod restore;

#[cfg(test)]
mod tests;

pub use conn::{test_cifs_conn, test_ssh_conn};
pub use control::{backup_logs, delete_backup_task, run_backup_task, stop_backup_task};
pub use install::{
    delete_cred, delete_cron, install_backup_spec, install_backup_task, write_cred, write_cron,
};
pub use permanent::{
    delete_permanent_backup, freeze_backup_task, list_permanent_backups, PermanentBackupInfo,
};
pub use restore::restore_backup;
