//! Explorador de archivos confinado al directorio de datos.
//!
//! Todas las rutas son relativas a la raíz (`/srv/nas`). Se rechazan rutas
//! absolutas, componentes `..` y enlaces simbólicos que escapen de la raíz.

use std::path::{Component, Path, PathBuf};

use nas_core::{NasError, NasResult};
use serde::Serialize;

/// Entrada de un directorio.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Entry {
    /// Nombre del archivo o carpeta.
    pub name: String,
    /// Ruta relativa a la raíz.
    pub path: String,
    /// Indica si es un directorio.
    pub is_dir: bool,
    /// Tamaño en bytes (0 para directorios).
    pub size: u64,
    /// Fecha de modificación (segundos UNIX).
    pub modified: i64,
}

/// Raíz confinada del explorador.
#[derive(Debug, Clone)]
pub struct FileRoot {
    root: PathBuf,
}

impl FileRoot {
    /// Crea la raíz y la canonicaliza.
    pub fn new(root: impl Into<PathBuf>) -> NasResult<Self> {
        let root = root.into();
        std::fs::create_dir_all(&root).map_err(|e| NasError::io(&root, e))?;
        let canonical = std::fs::canonicalize(&root).map_err(|e| NasError::io(&root, e))?;
        Ok(Self { root: canonical })
    }

    /// Raíz canónica.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Resuelve una ruta relativa sin permitir escapes.
    pub fn resolve(&self, relative: &str) -> NasResult<PathBuf> {
        let relative = relative.trim();
        if relative.starts_with('/') {
            return Err(NasError::Denied(
                "la ruta debe ser relativa al almacenamiento".to_string(),
            ));
        }
        let candidate = Path::new(relative);

        for component in candidate.components() {
            match component {
                Component::Normal(_) | Component::CurDir => {}
                Component::ParentDir => {
                    return Err(NasError::Denied(
                        "la ruta no puede contener '..'".to_string(),
                    ))
                }
                Component::RootDir | Component::Prefix(_) => {
                    return Err(NasError::Denied(
                        "la ruta debe ser relativa al almacenamiento".to_string(),
                    ))
                }
            }
        }

        let joined = if relative.is_empty() {
            self.root.clone()
        } else {
            self.root.join(relative)
        };

        // Canonicaliza el ancestro existente más cercano y recompone el resto,
        // de modo que se puedan crear rutas anidadas sin escapar de la raíz.
        let mut existing = joined.clone();
        let mut remaining: Vec<std::ffi::OsString> = Vec::new();

        while !existing.exists() {
            let Some(name) = existing.file_name().map(|name| name.to_os_string()) else {
                break;
            };
            remaining.push(name);
            let Some(parent) = existing.parent() else {
                break;
            };
            existing = parent.to_path_buf();
        }

        let base = if existing.exists() {
            std::fs::canonicalize(&existing).map_err(|e| NasError::io(&existing, e))?
        } else {
            self.root.clone()
        };

        let mut resolved = base;
        for name in remaining.iter().rev() {
            resolved.push(name);
        }

        if resolved.starts_with(&self.root) {
            Ok(resolved)
        } else {
            Err(NasError::Denied(
                "la ruta escapa del almacenamiento confinado".to_string(),
            ))
        }
    }

    /// Lista el contenido de un directorio (ordenado: carpetas primero).
    pub fn list(&self, relative: &str) -> NasResult<Vec<Entry>> {
        let dir = self.resolve(relative)?;
        if !dir.is_dir() {
            return Err(NasError::NotFound(format!(
                "no es un directorio: {relative}"
            )));
        }

        let mut entries = Vec::new();
        for item in std::fs::read_dir(&dir).map_err(|e| NasError::io(&dir, e))? {
            let item = item.map_err(|e| NasError::io(&dir, e))?;
            let name = item.file_name().to_string_lossy().to_string();
            if name == ".trash" {
                continue;
            }
            let metadata = entry_metadata(&item);
            let rel = if relative.is_empty() {
                name.clone()
            } else {
                format!("{relative}/{name}")
            };
            entries.push(Entry {
                name,
                path: rel,
                is_dir: metadata.0,
                size: metadata.1,
                modified: metadata.2,
            });
        }

        entries.sort_by(|a, b| b.is_dir.cmp(&a.is_dir).then(a.name.cmp(&b.name)));
        Ok(entries)
    }

    /// Crea un directorio (y sus padres).
    pub fn mkdir(&self, relative: &str) -> NasResult<PathBuf> {
        let path = self.resolve(relative)?;
        std::fs::create_dir_all(&path).map_err(|e| NasError::io(&path, e))?;
        Ok(path)
    }

    /// Renombra o mueve un elemento dentro de la raíz.
    pub fn rename(&self, from: &str, to: &str) -> NasResult<PathBuf> {
        let source = self.resolve(from)?;
        let target = self.resolve(to)?;
        if !source.exists() {
            return Err(NasError::NotFound(format!("no existe: {from}")));
        }
        std::fs::rename(&source, &target).map_err(|e| NasError::io(&target, e))?;
        Ok(target)
    }

    /// Mueve un elemento a la papelera interna (`.trash`).
    pub fn trash(&self, relative: &str) -> NasResult<PathBuf> {
        let source = self.resolve(relative)?;
        if source == self.root {
            return Err(NasError::Denied("no se puede eliminar la raíz".into()));
        }
        if !source.exists() {
            return Err(NasError::NotFound(format!("no existe: {relative}")));
        }

        let trash = self.root.join(".trash");
        std::fs::create_dir_all(&trash).map_err(|e| NasError::io(&trash, e))?;

        let stamp = chrono::Utc::now().format("%Y%m%d_%H%M%S");
        let name = source
            .file_name()
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_else(|| "elemento".to_string());
        let id = format!("{stamp}_{name}");
        let target = trash.join(&id);

        std::fs::rename(&source, &target).map_err(|e| NasError::io(&target, e))?;
        // Guarda la ruta relativa original para restaurar a su ubicación exacta.
        if let Err(error) = self.write_trash_meta(&id, relative) {
            let _ = std::fs::rename(&target, &source);
            return Err(error);
        }
        Ok(target)
    }

    /// Lee un archivo con límite de tamaño.
    pub fn read(&self, relative: &str, max_bytes: u64) -> NasResult<Vec<u8>> {
        let path = self.resolve(relative)?;
        let metadata = std::fs::metadata(&path).map_err(|e| NasError::io(&path, e))?;
        if !metadata.is_file() {
            return Err(NasError::Invalid(format!("no es un archivo: {relative}")));
        }
        if metadata.len() > max_bytes {
            return Err(NasError::Denied(format!(
                "el archivo supera el límite de {max_bytes} bytes"
            )));
        }
        std::fs::read(&path).map_err(|e| NasError::io(&path, e))
    }

    /// Directorio validado para recibir una subida.
    pub fn resolve_dir(&self, relative: &str) -> NasResult<PathBuf> {
        let dir = self.resolve(relative)?;
        if !dir.is_dir() {
            return Err(NasError::NotFound(format!(
                "no es un directorio: {relative}"
            )));
        }
        Ok(dir)
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

fn entry_metadata(entry: &std::fs::DirEntry) -> (bool, u64, i64) {
    let Ok(metadata) = entry.metadata() else {
        return (false, 0, 0);
    };
    let is_dir = metadata.is_dir();
    let size = if is_dir { 0 } else { metadata.len() };
    let modified = metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |duration| duration.as_secs() as i64);
    (is_dir, size, modified)
}

#[cfg(test)]
mod tests {
    use super::{sanitize_filename, FileRoot};

    fn unique_root(tag: &str) -> FileRoot {
        let dir = std::env::temp_dir().join(format!(
            "nas-files-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        FileRoot::new(dir).expect("root")
    }

    #[test]
    fn rechaza_rutas_con_parent_dir() {
        let root = unique_root("traversal");
        assert!(root.resolve("../etc/passwd").is_err());
        assert!(root.resolve("docs/../../etc").is_err());
        assert!(root.resolve("/etc/passwd").is_err());
        assert!(root.resolve("docs/../ok").is_err());
        assert!(root.resolve("").is_ok());
        let _ = std::fs::remove_dir_all(root.root());
    }

    #[test]
    fn confina_al_directorio_raiz() {
        let root = unique_root("confinado");
        root.mkdir("SISTEMAS/2026").expect("mkdir");
        let resolved = root.resolve("SISTEMAS/2026").expect("ruta");
        assert!(resolved.starts_with(root.root()));
        let _ = std::fs::remove_dir_all(root.root());
    }

    #[test]
    fn lista_ordenando_carpetas_primero() {
        let root = unique_root("listado");
        root.mkdir("carpeta").expect("mkdir");
        std::fs::write(root.root().join("b.txt"), b"hola").expect("write");
        std::fs::write(root.root().join("a.txt"), b"mundo").expect("write");

        let entries = root.list("").expect("list");
        assert_eq!(entries[0].name, "carpeta");
        assert!(entries[0].is_dir);
        assert_eq!(entries[1].name, "a.txt");
        assert_eq!(entries[1].size, 5);
        assert_eq!(entries[2].name, "b.txt");
        let _ = std::fs::remove_dir_all(root.root());
    }

    #[test]
    fn renombra_y_envia_a_papelera() {
        let root = unique_root("operaciones");
        std::fs::write(root.root().join("original.txt"), b"x").expect("write");

        let moved = root
            .rename("original.txt", "renombrado.txt")
            .expect("rename");
        assert!(moved.exists());

        let trashed = root.trash("renombrado.txt").expect("trash");
        assert!(trashed.exists());
        assert!(trashed.starts_with(root.root().join(".trash")));
        assert!(!root.root().join("renombrado.txt").exists());
        assert!(root.list("").expect("list").is_empty());

        let _ = std::fs::remove_dir_all(root.root());
    }

    #[test]
    fn rechaza_eliminar_la_raiz() {
        let root = unique_root("raiz");
        assert!(root.trash("").is_err());
        let _ = std::fs::remove_dir_all(root.root());
    }

    #[test]
    fn lee_con_limite_de_tamano() {
        let root = unique_root("lectura");
        std::fs::write(root.root().join("grande.bin"), vec![0u8; 2048]).expect("write");

        assert!(root.read("grande.bin", 1024).is_err());
        assert_eq!(root.read("grande.bin", 4096).expect("lectura").len(), 2048);
        let _ = std::fs::remove_dir_all(root.root());
    }

    #[test]
    fn sanea_nombres_de_subida() {
        assert!(sanitize_filename("informe.pdf").is_ok());
        assert!(sanitize_filename("../../etc/passwd").is_err());
        assert!(sanitize_filename("carpeta/archivo.txt").is_err());
        assert!(sanitize_filename("..").is_err());
        assert!(sanitize_filename("").is_err());
    }
}

/// Tamaño máximo agregado al comprimir una carpeta en ZIP (256 MiB).
pub const ZIP_MAX_BYTES: u64 = 256 * 1024 * 1024;

impl FileRoot {
    /// Comprime una carpeta (o archivo) en un ZIP en memoria, con límite de tamaño.
    pub fn zip(&self, relative: &str) -> NasResult<Vec<u8>> {
        use std::io::Cursor;
        use zip::write::SimpleFileOptions;

        let source = self.resolve(relative)?;
        if !source.exists() {
            return Err(NasError::NotFound(format!("no existe: {relative}")));
        }

        let mut buffer = Cursor::new(Vec::new());
        let mut writer = zip::ZipWriter::new(&mut buffer);
        let options =
            SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        let mut total: u64 = 0;

        if source.is_file() {
            let name = source
                .file_name()
                .map(|name| name.to_string_lossy().to_string())
                .unwrap_or_else(|| "archivo".to_string());
            add_to_zip(&mut writer, &options, &source, &name, &mut total)?;
        } else {
            let entries = collect_files(&source, &mut total)?;
            for (absolute, relative_name) in entries {
                add_to_zip(&mut writer, &options, &absolute, &relative_name, &mut total)?;
            }
        }

        writer
            .finish()
            .map_err(|error| NasError::Invalid(format!("zip: {error}")))?;

        Ok(buffer.into_inner())
    }
}

fn collect_files(root: &Path, total: &mut u64) -> NasResult<Vec<(PathBuf, String)>> {
    let mut files = Vec::new();
    let mut stack = vec![root.to_path_buf()];

    while let Some(dir) = stack.pop() {
        let entries = std::fs::read_dir(&dir).map_err(|e| NasError::io(&dir, e))?;
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(metadata) = entry.metadata() else {
                continue;
            };
            if metadata.file_type().is_symlink() {
                continue;
            }
            if metadata.is_dir() {
                stack.push(path);
                continue;
            }
            *total = total.saturating_add(metadata.len());
            if *total > ZIP_MAX_BYTES {
                return Err(NasError::Denied(
                    "la carpeta supera el límite de 256 MiB para descarga ZIP".into(),
                ));
            }
            let name = path
                .strip_prefix(root)
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('\\', "/");
            files.push((path, name));
        }
    }

    Ok(files)
}

fn add_to_zip<W: std::io::Write + std::io::Seek>(
    writer: &mut zip::ZipWriter<W>,
    options: &zip::write::SimpleFileOptions,
    path: &Path,
    name: &str,
    total: &mut u64,
) -> NasResult<()> {
    use std::io::Write;

    let metadata = std::fs::metadata(path).map_err(|e| NasError::io(path, e))?;
    *total = total.saturating_add(metadata.len());
    if *total > ZIP_MAX_BYTES {
        return Err(NasError::Denied(
            "el contenido supera el límite de 256 MiB para descarga ZIP".into(),
        ));
    }

    writer
        .start_file(name.to_string(), *options)
        .map_err(|error| NasError::Invalid(format!("zip: {error}")))?;

    let data = std::fs::read(path).map_err(|e| NasError::io(path, e))?;
    writer
        .write_all(&data)
        .map_err(|error| NasError::Invalid(format!("zip: {error}")))?;

    Ok(())
}

#[cfg(test)]
mod ziptests {
    use super::FileRoot;

    fn root(tag: &str) -> FileRoot {
        let dir = std::env::temp_dir().join(format!(
            "nas-zip-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        FileRoot::new(dir).expect("root")
    }

    #[test]
    fn comprime_carpeta_completa() {
        let root = root("dir");
        root.mkdir("SISTEMAS/2026").expect("mkdir");
        std::fs::write(root.root().join("SISTEMAS/2026/a.txt"), b"hola").expect("a");
        std::fs::write(root.root().join("SISTEMAS/b.txt"), b"mundo").expect("b");

        let bytes = root.zip("SISTEMAS").expect("zip");
        assert!(bytes.starts_with(b"PK"));

        let archive = zip::ZipArchive::new(std::io::Cursor::new(bytes)).expect("zip válido");
        assert_eq!(archive.len(), 2);
        let _ = std::fs::remove_dir_all(root.root());
    }

    #[test]
    fn comprime_un_archivo() {
        let root = root("file");
        std::fs::write(root.root().join("informe.txt"), b"x").expect("write");
        assert!(root.zip("informe.txt").expect("zip").starts_with(b"PK"));
        let _ = std::fs::remove_dir_all(root.root());
    }

    #[test]
    fn rechaza_escapes() {
        let root = root("escape");
        assert!(root.zip("../etc").is_err());
        assert!(root.zip("/etc").is_err());
        let _ = std::fs::remove_dir_all(root.root());
    }
}

/// Entrada de la papelera interna.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct TrashEntry {
    /// Identificador (nombre del archivo en `.trash`).
    pub id: String,
    /// Nombre original del elemento.
    pub name: String,
    /// Tamaño en bytes.
    pub size: u64,
    /// Tamaño legible.
    pub size_formatted: String,
    /// Marca de borrado (`YYYY-MM-DD HH:MM:SS`).
    pub deleted_at: String,
}

impl FileRoot {
    /// Escribe un archivo de texto dentro de la raíz.
    pub fn write(&self, relative: &str, contents: &str) -> NasResult<u64> {
        let path = self.resolve(relative)?;
        if path == self.root {
            return Err(NasError::Denied("no se puede sobrescribir la raíz".into()));
        }
        let parent = path
            .parent()
            .ok_or_else(|| NasError::Invalid("ruta sin padre".into()))?;
        std::fs::create_dir_all(parent).map_err(|e| NasError::io(parent, e))?;
        std::fs::write(&path, contents).map_err(|e| NasError::io(&path, e))?;
        Ok(contents.len() as u64)
    }

    /// Lista la papelera interna con metadatos.
    pub fn trash_entries(&self) -> NasResult<Vec<TrashEntry>> {
        let trash = self.root.join(".trash");
        if !trash.is_dir() {
            return Ok(Vec::new());
        }

        let mut entries = Vec::new();
        for item in std::fs::read_dir(&trash).map_err(|e| NasError::io(&trash, e))? {
            let item = item.map_err(|e| NasError::io(&trash, e))?;
            let id = item.file_name().to_string_lossy().to_string();
            if id.starts_with('.') {
                continue;
            }
            let metadata = item.metadata().ok();
            let size = metadata.as_ref().map(|m| m.len()).unwrap_or(0);

            let (stamp, name) = split_trash_name(&id);
            let name = name.to_string();
            let deleted_at = chrono::NaiveDateTime::parse_from_str(stamp, "%Y%m%d_%H%M%S")
                .map(|dt| dt.format("%Y-%m-%d %H:%M:%S").to_string())
                .unwrap_or_else(|_| stamp.to_string());

            entries.push(TrashEntry {
                id,
                name,
                size,
                size_formatted: human_size(size),
                deleted_at,
            });
        }
        entries.sort_by(|a, b| b.deleted_at.cmp(&a.deleted_at));
        Ok(entries)
    }

    /// Restaura un elemento de la papelera a su ruta original.
    pub fn restore_trash(&self, id: &str) -> NasResult<String> {
        let source = self.trash_path(id)?;
        let relative = self
            .read_trash_meta(id)
            .unwrap_or_else(|| split_trash_name(id).1.to_string());
        let (_, name) = split_trash_name(id);

        let mut target = self.resolve(&relative)?;
        if target == self.root || target.parent().is_none() {
            target = self.root.join(name);
        }
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).map_err(|e| NasError::io(parent, e))?;
        }

        let mut counter = 1;
        while target.exists() {
            let candidate = format!("{name}.{counter}");
            target = match target.parent() {
                Some(parent) => parent.join(&candidate),
                None => self.root.join(&candidate),
            };
            counter += 1;
        }

        std::fs::rename(&source, &target).map_err(|e| NasError::io(&target, e))?;
        let _ = self.remove_trash_meta(id);
        Ok(target
            .strip_prefix(&self.root)
            .map(|path| path.to_string_lossy().replace('\\', "/"))
            .unwrap_or_else(|_| name.to_string()))
    }

    /// Elimina definitivamente un elemento de la papelera.
    pub fn delete_trash(&self, id: &str) -> NasResult<()> {
        let path = self.trash_path(id)?;
        let result = if path.is_dir() {
            std::fs::remove_dir_all(&path).map_err(|e| NasError::io(&path, e))
        } else {
            std::fs::remove_file(&path).map_err(|e| NasError::io(&path, e))
        };
        let _ = self.remove_trash_meta(id);
        result
    }

    fn trash_meta_dir(&self) -> PathBuf {
        self.root.join(".trash").join(".meta")
    }

    fn write_trash_meta(&self, id: &str, relative: &str) -> NasResult<()> {
        let dir = self.trash_meta_dir();
        std::fs::create_dir_all(&dir).map_err(|e| NasError::io(&dir, e))?;
        let path = dir.join(id);
        std::fs::write(&path, relative).map_err(|e| NasError::io(&path, e))
    }

    fn read_trash_meta(&self, id: &str) -> Option<String> {
        let path = self.trash_meta_dir().join(id);
        let value = std::fs::read_to_string(path).ok()?;
        let value = value.trim().to_string();
        if value.is_empty() {
            None
        } else {
            Some(value)
        }
    }

    fn remove_trash_meta(&self, id: &str) -> std::io::Result<()> {
        let path = self.trash_meta_dir().join(id);
        match std::fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error),
        }
    }

    /// Vacía la papelera completa.
    pub fn empty_trash(&self) -> NasResult<u32> {
        let entries = self.trash_entries()?;
        let mut removed = 0;
        for entry in entries {
            if self.delete_trash(&entry.id).is_ok() {
                removed += 1;
            }
        }
        Ok(removed)
    }

    fn trash_path(&self, id: &str) -> NasResult<PathBuf> {
        if id.is_empty() || id.contains('/') || id.contains('\\') || id.contains("..") {
            return Err(NasError::Denied(
                "identificador de papelera inválido".into(),
            ));
        }
        let path = self.root.join(".trash").join(id);
        if !path.exists() {
            return Err(NasError::NotFound(format!("no existe en papelera: {id}")));
        }
        Ok(path)
    }
}

fn split_trash_name(id: &str) -> (&str, &str) {
    // Formato: YYYYMMDD_HHMMSS_nombre
    if id.len() > 16 && id.as_bytes().get(15) == Some(&b'_') {
        (&id[..15], &id[16..])
    } else {
        ("", id)
    }
}

fn human_size(bytes: u64) -> String {
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

#[cfg(test)]
mod trash_tests {
    use super::FileRoot;

    fn root(tag: &str) -> FileRoot {
        let dir = std::env::temp_dir().join(format!(
            "nas-trash-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        FileRoot::new(dir).expect("root")
    }

    #[test]
    fn escribe_lista_restaura_y_vacia() {
        let root = root("flujo");
        root.write("SISTEMAS/nota.txt", "hola").expect("write");
        assert_eq!(root.read("SISTEMAS/nota.txt", 1024).expect("read"), b"hola");

        root.trash("SISTEMAS/nota.txt").expect("trash");
        let entries = root.trash_entries().expect("lista");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "nota.txt");

        let restored = root.restore_trash(&entries[0].id).expect("restore");
        assert_eq!(restored, "SISTEMAS/nota.txt");
        assert!(root.read("SISTEMAS/nota.txt", 1024).is_ok());

        root.trash("SISTEMAS/nota.txt").expect("trash 2");
        assert_eq!(root.empty_trash().expect("vaciar"), 1);
        assert!(root.trash_entries().expect("lista").is_empty());
        let _ = std::fs::remove_dir_all(root.root());
    }

    #[test]
    fn restaura_a_la_ruta_original_con_subdirectorios() {
        let root = root("anidado");
        root.write("A/B/nota.txt", "datos").expect("write");
        root.trash("A/B/nota.txt").expect("trash");

        let entries = root.trash_entries().expect("lista");
        assert_eq!(entries[0].name, "nota.txt");

        let restored = root.restore_trash(&entries[0].id).expect("restore");
        assert_eq!(restored, "A/B/nota.txt");
        assert_eq!(root.read("A/B/nota.txt", 64).expect("contenido"), b"datos");
        assert!(root.trash_entries().expect("lista").is_empty());
        let _ = std::fs::remove_dir_all(root.root());
    }

    #[test]
    fn rechaza_ids_peligrosos() {
        let root = root("seguridad");
        assert!(root.restore_trash("../etc/passwd").is_err());
        assert!(root.delete_trash("a/b").is_err());
        let _ = std::fs::remove_dir_all(root.root());
    }
}
