//! Bloqueo exclusivo por tarea.
//!
//! Evita ejecuciones simultáneas de la misma tarea de respaldo sin depender de
//! `flock`: se usa creación atómica de un archivo con el PID del proceso y
//! detección de bloqueos huérfanos (proceso ya inexistente).

use std::io::Write;
use std::path::{Path, PathBuf};

use nas_core::{NasError, NasResult};

/// Bloqueo exclusivo liberado automáticamente al salir del ámbito.
#[derive(Debug)]
pub struct TaskLock {
    path: PathBuf,
}

impl TaskLock {
    /// Intenta adquirir el bloqueo; falla si otra ejecución lo mantiene activo.
    pub fn acquire(path: impl Into<PathBuf>) -> NasResult<Self> {
        let path = path.into();

        if let Some(dir) = path.parent() {
            if !dir.as_os_str().is_empty() {
                std::fs::create_dir_all(dir).map_err(|e| NasError::io(dir, e))?;
            }
        }

        match Self::create(&path) {
            Ok(()) => Ok(Self { path }),
            Err(NasError::Io { source, .. })
                if source.kind() == std::io::ErrorKind::AlreadyExists =>
            {
                if Self::is_stale(&path) {
                    let _ = std::fs::remove_file(&path);
                    Self::create(&path)?;
                    Ok(Self { path })
                } else {
                    Err(NasError::Denied(format!(
                        "ya hay una ejecución en curso para este recurso (bloqueo {})",
                        path.display()
                    )))
                }
            }
            Err(error) => Err(error),
        }
    }

    /// Ruta del archivo de bloqueo.
    pub fn path(&self) -> &Path {
        &self.path
    }

    fn create(path: &Path) -> NasResult<()> {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .map_err(|e| NasError::io(path, e))?;

        writeln!(file, "{}", std::process::id()).map_err(|e| NasError::io(path, e))
    }

    fn is_stale(path: &Path) -> bool {
        let Ok(raw) = std::fs::read_to_string(path) else {
            return false;
        };
        let Ok(pid) = raw.trim().parse::<u32>() else {
            return false;
        };

        #[cfg(target_os = "linux")]
        {
            !Path::new(&format!("/proc/{pid}")).exists()
        }

        #[cfg(not(target_os = "linux"))]
        {
            let _ = pid;
            false
        }
    }
}

impl Drop for TaskLock {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

#[cfg(test)]
mod tests {
    use super::TaskLock;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn unique_path(tag: &str) -> std::path::PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        std::env::temp_dir().join(format!(
            "nas-lock-{tag}-{}-{nanos}.lock",
            std::process::id()
        ))
    }

    #[test]
    fn impide_dos_ejecuciones_simultaneas() {
        let path = unique_path("exclusivo");
        let first = TaskLock::acquire(&path).expect("primer bloqueo");
        assert!(TaskLock::acquire(&path).is_err());
        drop(first);
        assert!(TaskLock::acquire(&path).is_ok());
    }

    #[test]
    fn libera_el_archivo_al_salir_del_ambito() {
        let path = unique_path("drop");
        {
            let _lock = TaskLock::acquire(&path).expect("bloqueo");
            assert!(path.exists());
        }
        assert!(!path.exists());
    }

    #[test]
    fn recupera_bloqueos_huerfanos() {
        let path = unique_path("huerfano");
        std::fs::write(&path, "999999\n").expect("escritura");

        #[cfg(target_os = "linux")]
        assert!(TaskLock::acquire(&path).is_ok());

        #[cfg(not(target_os = "linux"))]
        assert!(TaskLock::acquire(&path).is_err());

        let _ = std::fs::remove_file(&path);
    }
}
