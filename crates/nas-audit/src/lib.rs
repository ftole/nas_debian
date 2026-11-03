//! Bitácora de auditoría append-only con encadenamiento de hashes.
//!
//! Cada entrada incluye el hash de la anterior, de modo que cualquier alteración
//! del histórico se detecta al verificar la cadena.

#![forbid(unsafe_code)]
#![deny(missing_docs)]

use std::path::{Path, PathBuf};

use chrono::Local;
use nas_core::{NasError, NasResult};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Entrada de auditoría.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuditEntry {
    /// Marca de tiempo local en formato ISO-8601.
    pub ts: String,
    /// Usuario o proceso que ejecutó la acción.
    pub actor: String,
    /// Host destino (o `local`).
    pub host: String,
    /// Comando lógico ejecutado.
    pub action: String,
    /// Objeto afectado.
    pub target: String,
    /// Resultado (`SUCCESS`, `FAILED`, `DRY-RUN`).
    pub result: String,
    /// Detalle libre, ya redactado.
    pub detail: String,
    /// Hash de la entrada anterior.
    pub prev_hash: String,
    /// Hash de esta entrada.
    pub hash: String,
}

impl AuditEntry {
    /// Crea una entrada sin sellar.
    pub fn new(
        actor: impl Into<String>,
        host: impl Into<String>,
        action: impl Into<String>,
        target: impl Into<String>,
        result: impl Into<String>,
        detail: impl Into<String>,
    ) -> Self {
        Self {
            ts: Local::now().to_rfc3339(),
            actor: actor.into(),
            host: host.into(),
            action: action.into(),
            target: target.into(),
            result: result.into(),
            detail: detail.into(),
            prev_hash: String::new(),
            hash: String::new(),
        }
    }

    /// Calcula y asigna el hash encadenado.
    pub fn seal(mut self, prev_hash: &str) -> Self {
        self.prev_hash = prev_hash.to_string();
        let canonical = format!(
            "{}|{}|{}|{}|{}|{}|{}|{}",
            self.ts,
            self.actor,
            self.host,
            self.action,
            self.target,
            self.result,
            self.detail,
            self.prev_hash
        );
        let mut hasher = Sha256::new();
        hasher.update(canonical.as_bytes());
        self.hash = hex::encode(hasher.finalize());
        self
    }
}

/// Bitácora persistida en disco en formato JSON Lines.
#[derive(Debug, Clone)]
pub struct AuditLog {
    path: PathBuf,
}

impl AuditLog {
    /// Crea una bitácora en la ruta indicada.
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    /// Ruta del archivo de auditoría.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Hash de la última entrada registrada en tiempo constante O(1).
    pub fn last_hash(&self) -> NasResult<String> {
        use std::io::{Read, Seek, SeekFrom};

        let mut file = match std::fs::File::open(&self.path) {
            Ok(file) => file,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(String::from("0")),
            Err(e) => return Err(NasError::io(&self.path, e)),
        };

        let file_len = file
            .metadata()
            .map_err(|e| NasError::io(&self.path, e))?
            .len();
        if file_len == 0 {
            return Ok(String::from("0"));
        }

        const CHUNK_SIZE: u64 = 8192;
        let mut read_len = CHUNK_SIZE.min(file_len);

        loop {
            let seek_offset = file_len - read_len;
            file.seek(SeekFrom::Start(seek_offset))
                .map_err(|e| NasError::io(&self.path, e))?;

            let mut buffer = vec![0u8; read_len as usize];
            file.read_exact(&mut buffer)
                .map_err(|e| NasError::io(&self.path, e))?;

            let text = String::from_utf8_lossy(&buffer);
            let lines: Vec<&str> = text.lines().collect();
            let valid_lines = if seek_offset > 0 && lines.len() > 1 {
                &lines[1..]
            } else {
                &lines[..]
            };

            for line in valid_lines.iter().rev() {
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    continue;
                }
                if let Ok(entry) = serde_json::from_str::<AuditEntry>(trimmed) {
                    return Ok(entry.hash);
                }
            }

            if seek_offset == 0 {
                return Ok(String::from("0"));
            }

            read_len = (read_len * 2).min(file_len);
        }
    }

    /// Añade una entrada sellada y devuelve la versión persistida.
    pub fn append(&self, entry: AuditEntry) -> NasResult<AuditEntry> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| NasError::io(parent, e))?;
        }

        let _guard = AuditLockGuard::acquire(&self.path)?;

        let previous = self.last_hash()?;
        let sealed = entry.seal(&previous);
        let line = serde_json::to_string(&sealed)?;

        use std::io::Write;
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .map_err(|e| NasError::io(&self.path, e))?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = file.set_permissions(std::fs::Permissions::from_mode(0o640));
        }

        writeln!(file, "{line}").map_err(|e| NasError::io(&self.path, e))?;
        file.flush().map_err(|e| NasError::io(&self.path, e))?;

        Ok(sealed)
    }

    /// Verifica la integridad completa de la cadena.
    pub fn verify(&self) -> NasResult<usize> {
        let content =
            std::fs::read_to_string(&self.path).map_err(|e| NasError::io(&self.path, e))?;

        let mut expected_prev = String::from("0");
        let mut count = 0;

        for (index, line) in content.lines().enumerate() {
            if line.trim().is_empty() {
                continue;
            }
            let entry: AuditEntry = serde_json::from_str(line)?;
            if entry.prev_hash != expected_prev {
                return Err(NasError::Invalid(format!(
                    "cadena de auditoría rota en la línea {}",
                    index + 1
                )));
            }

            let recomputed = entry.clone().seal(&expected_prev);
            if recomputed.hash != entry.hash {
                return Err(NasError::Invalid(format!(
                    "hash alterado en la línea {}",
                    index + 1
                )));
            }

            expected_prev = entry.hash;
            count += 1;
        }

        Ok(count)
    }
}

struct AuditLockGuard {
    lock_path: PathBuf,
}

impl AuditLockGuard {
    fn acquire(log_path: &Path) -> NasResult<Self> {
        let mut os = log_path.as_os_str().to_os_string();
        os.push(".lock");
        let lock_path = PathBuf::from(os);

        for attempt in 0..400 {
            match std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&lock_path)
            {
                Ok(_) => return Ok(Self { lock_path }),
                Err(err)
                    if err.kind() == std::io::ErrorKind::AlreadyExists
                        || err.kind() == std::io::ErrorKind::PermissionDenied =>
                {
                    if attempt == 200 {
                        if let Ok(meta) = std::fs::metadata(&lock_path) {
                            if let Ok(modified) = meta.modified() {
                                if modified
                                    .elapsed()
                                    .map(|age| age.as_secs() > 30)
                                    .unwrap_or(false)
                                {
                                    let _ = std::fs::remove_file(&lock_path);
                                    continue;
                                }
                            }
                        }
                    }
                    std::thread::sleep(std::time::Duration::from_millis(5));
                }
                Err(err) => return Err(NasError::io(&lock_path, err)),
            }
        }
        Err(NasError::Denied(format!(
            "bitácora de auditoría bloqueada: {}",
            lock_path.display()
        )))
    }
}

impl Drop for AuditLockGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.lock_path);
    }
}

#[cfg(test)]
mod tests {
    use super::{AuditEntry, AuditLog};

    fn temp_log(name: &str) -> AuditLog {
        let mut path = std::env::temp_dir();
        path.push(format!("nas-audit-{name}-{}.jsonl", std::process::id()));
        let _ = std::fs::remove_file(&path);
        AuditLog::new(path)
    }

    #[test]
    fn encadena_y_verifica_entradas() {
        let log = temp_log("chain");
        log.append(AuditEntry::new(
            "dev",
            "lab",
            "dev-sync",
            "web",
            "SUCCESS",
            "sin cambios",
        ))
        .expect("append 1");
        log.append(AuditEntry::new(
            "dev",
            "lab",
            "dev-sync",
            "web",
            "SUCCESS",
            "2 archivos",
        ))
        .expect("append 2");

        assert_eq!(log.verify().expect("cadena válida"), 2);
        let _ = std::fs::remove_file(log.path());
    }

    #[test]
    fn detecta_manipulacion_del_archivo() {
        let log = temp_log("tamper");
        log.append(AuditEntry::new(
            "dev", "lab", "update", "srv", "SUCCESS", "ok",
        ))
        .expect("append");

        let content = std::fs::read_to_string(log.path()).expect("lectura");
        let tampered = content.replace("\"SUCCESS\"", "\"FAILED\"");
        std::fs::write(log.path(), tampered).expect("escritura");

        assert!(log.verify().is_err());
        let _ = std::fs::remove_file(log.path());
    }

    #[test]
    fn primera_entrada_encadena_al_origen() {
        let log = temp_log("first");
        let entry = log
            .append(AuditEntry::new(
                "dev", "lab", "status", "local", "SUCCESS", "",
            ))
            .expect("append");
        assert_eq!(entry.prev_hash, "0");
        assert_eq!(entry.hash.len(), 64);
        let _ = std::fs::remove_file(log.path());
    }

    #[test]
    fn concurrencia_preserva_cadena_integra() {
        let log = std::sync::Arc::new(temp_log("concurrent"));
        let mut handles = Vec::new();
        for t in 0..8 {
            let log = std::sync::Arc::clone(&log);
            handles.push(std::thread::spawn(move || {
                for i in 0..5 {
                    log.append(AuditEntry::new(
                        format!("actor-{t}"),
                        "local",
                        "test",
                        format!("item-{i}"),
                        "SUCCESS",
                        "ok",
                    ))
                    .expect("append concurrent");
                }
            }));
        }
        for handle in handles {
            handle.join().expect("thread");
        }
        assert_eq!(log.verify().expect("cadena íntegra"), 40);
        let _ = std::fs::remove_file(log.path());
    }

    #[test]
    fn last_hash_archivo_inexistente_o_vacio_devuelve_cero() {
        let log = temp_log("vacio");
        assert_eq!(log.last_hash().expect("hash"), "0");

        // Archivo creado pero de 0 bytes
        std::fs::write(log.path(), b"").expect("write vacio");
        assert_eq!(log.last_hash().expect("hash"), "0");

        // Archivo con solo saltos de línea
        std::fs::write(log.path(), b"\n\n\n").expect("write newlines");
        assert_eq!(log.last_hash().expect("hash"), "0");
        let _ = std::fs::remove_file(log.path());
    }

    #[test]
    fn last_hash_obtiene_ultimo_hash_con_multiples_entradas() {
        let log = temp_log("multiples");
        let e1 = log
            .append(AuditEntry::new(
                "dev", "lab", "test1", "srv", "SUCCESS", "d1",
            ))
            .expect("append 1");
        assert_eq!(log.last_hash().expect("hash 1"), e1.hash);

        let e2 = log
            .append(AuditEntry::new(
                "dev", "lab", "test2", "srv", "SUCCESS", "d2",
            ))
            .expect("append 2");
        assert_eq!(log.last_hash().expect("hash 2"), e2.hash);

        let e3 = log
            .append(AuditEntry::new(
                "dev", "lab", "test3", "srv", "SUCCESS", "d3",
            ))
            .expect("append 3");
        assert_eq!(log.last_hash().expect("hash 3"), e3.hash);

        let _ = std::fs::remove_file(log.path());
    }
}
