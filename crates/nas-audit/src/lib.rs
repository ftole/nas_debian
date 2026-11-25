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
            let at_start = read_len >= file_len;
            file.seek(SeekFrom::End(-(read_len as i64)))
                .map_err(|e| NasError::io(&self.path, e))?;

            let mut buffer = vec![0u8; read_len as usize];
            file.read_exact(&mut buffer)
                .map_err(|e| NasError::io(&self.path, e))?;

            let text = String::from_utf8_lossy(&buffer);
            let lines: Vec<&str> = text.lines().collect();
            let valid_lines = if !at_start && lines.len() > 1 {
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

            if at_start {
                return Ok(String::from("0"));
            }

            read_len = (read_len * 2).min(file_len);
        }
    }

    fn trim_truncated_tail_unlocked(&self) -> NasResult<bool> {
        use std::io::{Read, Seek, SeekFrom};

        if !self.path.exists() {
            return Ok(false);
        }
        let mut file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&self.path)
            .map_err(|e| NasError::io(&self.path, e))?;
        let file_len = file
            .metadata()
            .map_err(|e| NasError::io(&self.path, e))?
            .len();
        if file_len == 0 {
            return Ok(false);
        }

        // Lee solo la cola del archivo (O(1) en RAM) aumentando la ventana únicamente si la última línea excede 64 KiB.
        let mut window: u64 = 64 * 1024;
        loop {
            let read_len = window.min(file_len);
            let start = file_len - read_len;
            file.seek(SeekFrom::Start(start))
                .map_err(|e| NasError::io(&self.path, e))?;
            let mut buf = vec![0u8; read_len as usize];
            file.read_exact(&mut buf)
                .map_err(|e| NasError::io(&self.path, e))?;

            let valid_slice = if start > 0 {
                if let Some(first_nl) = buf.iter().position(|&b| b == b'\n') {
                    (start + first_nl as u64 + 1, &buf[first_nl + 1..])
                } else if read_len < file_len {
                    window = (window * 2).min(file_len);
                    continue;
                } else {
                    (0, buf.as_slice())
                }
            } else {
                (0, buf.as_slice())
            };

            let (base_offset, tail_bytes) = valid_slice;
            let trimmed_end = tail_bytes
                .iter()
                .rposition(|&b| b != b'\n' && b != b'\r')
                .map(|idx| idx + 1)
                .unwrap_or(0);
            if trimmed_end == 0 {
                return Ok(false);
            }
            let content_no_trailing_nl = &tail_bytes[..trimmed_end];
            let (line_start, last_line) =
                match content_no_trailing_nl.iter().rposition(|&b| b == b'\n') {
                    Some(nl) => (nl + 1, &content_no_trailing_nl[nl + 1..]),
                    None => (0, content_no_trailing_nl),
                };

            let is_valid_json = std::str::from_utf8(last_line)
                .ok()
                .is_some_and(|s| serde_json::from_str::<AuditEntry>(s.trim()).is_ok());
            if is_valid_json {
                return Ok(false);
            }

            let new_len = base_offset + line_start as u64;
            file.set_len(new_len)
                .map_err(|e| NasError::io(&self.path, e))?;
            file.sync_all().map_err(|e| NasError::io(&self.path, e))?;
            return Ok(true);
        }
    }

    /// Recupera la bitácora recortando únicamente una última línea truncada por corte eléctrico.
    pub fn recover_truncated_tail(&self) -> NasResult<bool> {
        let _guard = AuditLockGuard::acquire(&self.path)?;
        self.trim_truncated_tail_unlocked()
    }

    /// Añade una entrada sellada y devuelve la versión persistida.
    pub fn append(&self, entry: AuditEntry) -> NasResult<AuditEntry> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| NasError::io(parent, e))?;
        }

        let _guard = AuditLockGuard::acquire(&self.path)?;
        let _ = self.trim_truncated_tail_unlocked()?;

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
        file.sync_data().map_err(|e| NasError::io(&self.path, e))?;

        Ok(sealed)
    }

    /// Verifica la integridad completa de la cadena.
    pub fn verify(&self) -> NasResult<usize> {
        let content =
            std::fs::read_to_string(&self.path).map_err(|e| NasError::io(&self.path, e))?;

        let non_empty: Vec<(usize, &str)> = content
            .lines()
            .enumerate()
            .filter(|(_, line)| !line.trim().is_empty())
            .collect();
        let total = non_empty.len();

        let mut expected_prev = String::from("0");
        let mut count = 0;

        for (pos, (index, line)) in non_empty.iter().enumerate() {
            let entry: AuditEntry = match serde_json::from_str(line) {
                Ok(entry) => entry,
                Err(err) => {
                    if pos + 1 == total {
                        return Err(NasError::Invalid(format!(
                            "última entrada truncada en la línea {} (posible corte eléctrico; {} entradas previas íntegras): {err}",
                            index + 1,
                            count
                        )));
                    }
                    return Err(err.into());
                }
            };
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
                Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => {
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

    #[test]
    fn last_hash_log_mayor_a_8k_recupera_ultimo_hash_correctamente() {
        let log = temp_log("log_8k");
        let mut last_expected = String::new();

        // Creamos más de 60 entradas con datos detallados para superar con creces los 8 KiB
        for i in 0..60 {
            let details = format!("detalle de evento de auditoria largo numero {i} con relleno de texto para exceder el tamano de bloque");
            let entry = log
                .append(AuditEntry::new(
                    "operador",
                    "panel",
                    "backup_run",
                    "10.10.1.50",
                    "SUCCESS",
                    &details,
                ))
                .expect("append");
            last_expected = entry.hash;
        }

        let file_len = std::fs::metadata(log.path()).expect("metadata").len();
        assert!(
            file_len > 8192,
            "el log debe superar 8192 bytes para probar lectura parcial desde el final (tam: {file_len})"
        );

        assert_eq!(log.last_hash().expect("last hash"), last_expected);
        let _ = std::fs::remove_file(log.path());
    }

    #[test]
    fn soporta_cola_truncada_por_apagon_y_recupera_encadenamiento() {
        let log = temp_log("truncated_tail");
        let e1 = log
            .append(AuditEntry::new(
                "admin", "panel", "login", "web", "SUCCESS", "ok",
            ))
            .expect("append 1");
        let e2 = log
            .append(AuditEntry::new(
                "admin", "panel", "share", "data", "SUCCESS", "ok2",
            ))
            .expect("append 2");

        // Simular corte eléctrico a mitad de escritura de la 3ra línea (sin `\n`).
        use std::io::Write;
        {
            let mut f = std::fs::OpenOptions::new()
                .append(true)
                .open(log.path())
                .expect("open append");
            write!(f, "{{\"ts\":123,\"user\":\"admin\",\"truncated").expect("partial write");
        }

        let err = log
            .verify()
            .expect_err("debe detectar línea final truncada");
        assert!(err.to_string().contains("última entrada truncada"));

        // Una nueva escritura sana limpia únicamente la cola truncada y preserva e1->e2->e3.
        let e3 = log
            .append(AuditEntry::new(
                "admin", "panel", "backup", "t1", "SUCCESS", "ok3",
            ))
            .expect("append 3");
        assert_eq!(e2.prev_hash, e1.hash);
        assert_eq!(e3.prev_hash, e2.hash);
        assert_eq!(log.verify().expect("cadena recuperada"), 3);

        let _ = std::fs::remove_file(log.path());
    }

    #[test]
    fn recorta_unica_linea_corrupta_terminada_en_salto() {
        let log = temp_log("single-corrupt-nl");
        std::fs::write(log.path(), "{\"ts\":123,\"corrupt\n").expect("write");
        assert!(log.recover_truncated_tail().expect("recover"));
        let entry = log
            .append(AuditEntry::new(
                "admin", "panel", "login", "web", "SUCCESS", "ok",
            ))
            .expect("append");
        assert_eq!(entry.prev_hash, "0");
        assert_eq!(log.verify().expect("verify"), 1);
        let _ = std::fs::remove_file(log.path());
    }
}
