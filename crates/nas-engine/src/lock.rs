//! Bloqueo exclusivo por tarea basado en `flock(2)`.
//!
//! Es interoperable con los runners Bash (`flock -n 9`): ambos usan el mismo
//! archivo de bloqueo y el kernel libera el cerrojo automáticamente si el
//! proceso muere, evitando bloqueos huérfanos permanentes.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use fs2::FileExt;
use nas_core::{NasError, NasResult};

/// Bloqueo exclusivo liberado automáticamente al salir del ámbito.
#[derive(Debug)]
pub struct TaskLock {
    file: File,
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

        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&path)
            .map_err(|e| NasError::io(&path, e))?;

        FileExt::try_lock_exclusive(&file).map_err(|_| {
            NasError::Denied(format!(
                "ya hay una ejecución en curso para este recurso (bloqueo {})",
                path.display()
            ))
        })?;

        file.set_len(0).map_err(|e| NasError::io(&path, e))?;
        writeln!(file, "{}", std::process::id()).map_err(|e| NasError::io(&path, e))?;

        Ok(Self { file, path })
    }

    /// Intenta adquirir el bloqueo con espera acotada hasta `timeout`.
    pub fn acquire_with_timeout(path: impl Into<PathBuf>, timeout: Duration) -> NasResult<Self> {
        let path = path.into();
        let deadline = Instant::now() + timeout;
        loop {
            match Self::acquire(&path) {
                Ok(guard) => return Ok(guard),
                Err(err) => {
                    if Instant::now() >= deadline {
                        return Err(err);
                    }
                    std::thread::sleep(Duration::from_millis(250));
                }
            }
        }
    }

    /// Adquiere un slot de concurrencia acotada por volumen/pool de destino.
    pub fn acquire_pool_slot(
        lock_dir: &Path,
        pool_key: &str,
        task_id: &str,
        max_slots: usize,
        wait_timeout: Duration,
    ) -> NasResult<Self> {
        let slots = max_slots.max(1);
        let safe_key: String = pool_key
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        for slot in 1..=slots {
            let slot_path = lock_dir.join(format!("nas-pool-{safe_key}_slot_{slot}.lock"));
            if let Ok(guard) = Self::acquire(&slot_path) {
                return Ok(guard);
            }
        }
        let hash: usize = task_id.bytes().fold(0usize, |acc, b| {
            acc.wrapping_mul(31).wrapping_add(b as usize)
        });
        let wait_slot = (hash % slots) + 1;
        let wait_path = lock_dir.join(format!("nas-pool-{safe_key}_slot_{wait_slot}.lock"));
        Self::acquire_with_timeout(wait_path, wait_timeout)
    }

    /// Ruta del archivo de bloqueo.
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TaskLock {
    fn drop(&mut self) {
        let _ = FileExt::unlock(&self.file);
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
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn libera_el_bloqueo_al_salir_del_ambito() {
        let path = unique_path("drop");
        {
            let _lock = TaskLock::acquire(&path).expect("bloqueo");
        }
        assert!(TaskLock::acquire(&path).is_ok());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn recupera_bloqueos_huerfanos_sin_proceso() {
        let path = unique_path("huerfano");
        std::fs::write(&path, "999999\n").expect("escritura");
        // Sin proceso vivo no hay flock activo: el bloqueo debe adquirirse.
        assert!(TaskLock::acquire(&path).is_ok());
        let _ = std::fs::remove_file(&path);
    }
}
