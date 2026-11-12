//! Utilidades de permisos POSIX (`grp_samba`), saneamiento de nombres y cálculo recursivo de tamaño.

use std::path::Path;

use nas_core::{NasError, NasResult};

use super::FileRoot;

impl FileRoot {
    /// Escribe un archivo de texto dentro de la raíz con permisos `0660`.
    pub fn write(&self, relative: &str, contents: &str) -> NasResult<u64> {
        if let Some(desc) = self.is_protected_path(relative) {
            return Err(NasError::Denied(format!(
                "operación denegada: {desc} están protegidos contra escritura"
            )));
        }
        let path = self.resolve(relative)?;
        if path == self.root {
            return Err(NasError::Denied("no se puede sobrescribir la raíz".into()));
        }
        let parent = path
            .parent()
            .ok_or_else(|| NasError::Invalid("ruta sin padre".into()))?;
        std::fs::create_dir_all(parent).map_err(|e| NasError::io(parent, e))?;
        set_dir_permissions(parent);
        std::fs::write(&path, contents).map_err(|e| NasError::io(&path, e))?;
        set_file_permissions(&path);
        Ok(contents.len() as u64)
    }
}

/// Valida el nombre de un archivo subido (sin separadores ni rutas).
pub fn sanitize_filename(name: &str) -> NasResult<String> {
    let name = name.trim();
    if name.is_empty() || name == "." || name == ".." {
        return Err(NasError::Invalid("nombre de archivo inválido".into()));
    }
    if name.contains('/') || name.contains('\\') || name.contains('\0') {
        return Err(NasError::Denied(
            "el nombre de archivo no puede contener separadores".into(),
        ));
    }
    Ok(name.to_string())
}

pub(super) fn entry_metadata(entry: &std::fs::DirEntry) -> (bool, u64, i64) {
    let Ok(metadata) = entry.metadata() else {
        return (false, 0, 0);
    };
    let is_dir = metadata.is_dir();
    let size = if is_dir {
        dir_size_recursive_bounded(&entry.path(), 10_000)
    } else {
        metadata.len()
    };
    let modified = metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |duration| duration.as_secs() as i64);
    (is_dir, size, modified)
}

#[cfg(unix)]
fn resolve_grp_samba_gid() -> Option<u32> {
    let content = std::fs::read_to_string("/etc/group").ok()?;
    for line in content.lines() {
        let mut parts = line.split(':');
        if parts.next() == Some("grp_samba") {
            let _ = parts.next();
            return parts.next().and_then(|gid| gid.trim().parse::<u32>().ok());
        }
    }
    None
}

/// Aplica permisos `0660` y pertenencia a `grp_samba` sobre un archivo recién creado o subido (Unix).
#[cfg(unix)]
pub fn set_file_permissions(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o660));
    if let Some(gid) = resolve_grp_samba_gid() {
        let _ = std::os::unix::fs::chown(path, None, Some(gid));
    }
}

/// No-op fuera de Unix.
#[cfg(not(unix))]
pub fn set_file_permissions(_path: &Path) {}

#[cfg(unix)]
pub(super) fn set_dir_permissions(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o2770));
    if let Some(gid) = resolve_grp_samba_gid() {
        let _ = std::os::unix::fs::chown(path, None, Some(gid));
    }
}

#[cfg(not(unix))]
pub(super) fn set_dir_permissions(_path: &Path) {}

/// Calcula el tamaño acumulado de un directorio en bytes, con un límite de elementos para evitar bloqueos.
pub fn dir_size_recursive_bounded(root: &Path, max_entries: usize) -> u64 {
    let mut total = 0u64;
    let mut count = 0usize;
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let Ok(meta) = entry.metadata() else {
                continue;
            };
            if meta.file_type().is_symlink() {
                continue;
            }
            count += 1;
            if meta.is_dir() {
                stack.push(entry.path());
            } else {
                total = total.saturating_add(meta.len());
            }
            if count >= max_entries {
                return total;
            }
        }
    }
    total
}

/// Calcula el tamaño acumulado completo de un directorio en bytes.
pub fn dir_size_recursive(root: &Path) -> u64 {
    dir_size_recursive_bounded(root, usize::MAX)
}

pub(super) fn split_trash_name(id: &str) -> (&str, &str) {
    // Formato legado: YYYYMMDD_HHMMSS_nombre
    if id.len() > 16 && id.as_bytes().get(8) == Some(&b'_') && id.as_bytes().get(15) == Some(&b'_')
    {
        return (&id[..15], &id[16..]);
    }
    // Formato milisegundos únicos: <epoch_ms>_<nombre>
    if let Some((prefix, rest)) = id.split_once('_') {
        if !prefix.is_empty() && prefix.bytes().all(|b| b.is_ascii_digit()) && !rest.is_empty() {
            return (prefix, rest);
        }
    }
    ("", id)
}

pub(super) fn format_trash_stamp(stamp: &str) -> String {
    if let Ok(dt) = chrono::NaiveDateTime::parse_from_str(stamp, "%Y%m%d_%H%M%S") {
        return dt.format("%Y-%m-%d %H:%M:%S").to_string();
    }
    if let Ok(ms) = stamp.parse::<i64>() {
        if let Some(dt) = chrono::DateTime::from_timestamp_millis(ms) {
            return dt
                .with_timezone(&chrono::Local)
                .format("%Y-%m-%d %H:%M:%S")
                .to_string();
        }
    }
    stamp.to_string()
}

pub(super) fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["B", "KiB", "MiB", "GiB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}
