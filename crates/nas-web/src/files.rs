//! Explorador de archivos confinado al directorio de datos.
//!
//! Todas las rutas son relativas a la raíz (`/srv/nas`). Se rechazan rutas
//! absolutas, componentes `..` y enlaces simbólicos que escapen de la raíz.

mod stream;
mod trash;
mod util;
mod zip;

use std::path::{Component, Path, PathBuf};

use nas_core::{NasError, NasResult};
use serde::Serialize;

pub use stream::{
    parse_range, ByteRange, CleaningReaderStream, RangeError, ReaderStream, TempFile,
};
pub use trash::TrashEntry;
pub use util::{
    dir_size_recursive, dir_size_recursive_bounded, sanitize_filename, set_file_permissions,
};
use util::{entry_metadata, set_dir_permissions};
pub use zip::ZIP_MAX_BYTES;

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

    /// Determina si una ruta dentro de la raíz pertenece a un subsistema protegido de solo lectura
    /// (como respaldos históricos inmutables contra ransomware, registros o papelera interna).
    pub fn is_protected_path(&self, relative: &str) -> Option<&'static str> {
        let root_name = self.root.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if root_name == "BACKUPS_HISTORICOS" || root_name == "BACKUPS_PERMANENTES" {
            return Some("los respaldos (inmutables contra ransomware)");
        }
        if root_name == "LOGS_BACKUP" {
            return Some("los registros de respaldo del sistema");
        }
        if root_name == ".trash" {
            return Some("la papelera de reciclaje interna");
        }

        let clean = relative
            .trim_matches(['/', '\\'].as_ref())
            .replace('\\', "/");
        let first = clean.split('/').next().unwrap_or("");
        match first {
            "BACKUPS_HISTORICOS" => Some("los respaldos históricos (inmutables contra ransomware)"),
            "BACKUPS_PERMANENTES" => {
                Some("los respaldos permanentes (inmutables contra ransomware)")
            }
            "LOGS_BACKUP" => Some("los registros de respaldo del sistema (LOGS_BACKUP)"),
            ".trash" => Some("la papelera de reciclaje interna (.trash)"),
            _ => None,
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

    /// Crea un directorio (y sus padres) con permisos de grupo Samba (`2770`).
    pub fn mkdir(&self, relative: &str) -> NasResult<PathBuf> {
        if let Some(desc) = self.is_protected_path(relative) {
            return Err(NasError::Denied(format!(
                "operación denegada: {desc} están protegidos contra escritura"
            )));
        }
        let path = self.resolve(relative)?;
        std::fs::create_dir_all(&path).map_err(|e| NasError::io(&path, e))?;
        set_dir_permissions(&path);
        Ok(path)
    }

    /// Renombra o mueve un elemento dentro de la raíz.
    pub fn rename(&self, from: &str, to: &str) -> NasResult<PathBuf> {
        if let Some(desc) = self.is_protected_path(from) {
            return Err(NasError::Denied(format!(
                "operación denegada: {desc} están protegidos contra escritura e inmutables (chattr +i); no se pueden renombrar ni mover"
            )));
        }
        if let Some(desc) = self.is_protected_path(to) {
            return Err(NasError::Denied(format!(
                "operación denegada: {desc} están protegidos contra escritura; no admiten modificaciones"
            )));
        }
        let source = self.resolve(from)?;
        let target = self.resolve(to)?;
        if source == self.root || target == self.root {
            return Err(NasError::Denied("no se puede mover la raíz".into()));
        }
        if !source.exists() {
            return Err(NasError::NotFound(format!("no existe: {from}")));
        }
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).map_err(|e| NasError::io(parent, e))?;
        }
        std::fs::rename(&source, &target).map_err(|e| {
            if e.raw_os_error() == Some(1) {
                NasError::Denied(format!(
                    "operación no permitida (EPERM): el elemento '{from}' o su contenedor es inmutable o de solo lectura"
                ))
            } else if e.kind() == std::io::ErrorKind::PermissionDenied || e.raw_os_error() == Some(13) {
                NasError::Denied(format!(
                    "permiso denegado: el servicio web no tiene permisos suficientes para renombrar o mover '{from}'"
                ))
            } else {
                NasError::io(&source, e)
            }
        })?;
        Ok(target)
    }

    /// Elimina definitivamente un archivo o carpeta sin pasar por la papelera.
    pub fn delete_permanent(&self, relative: &str) -> NasResult<()> {
        if let Some(desc) = self.is_protected_path(relative) {
            return Err(NasError::Denied(format!(
                "operación denegada: {desc} están protegidos contra escritura e inmutables (chattr +i); no se pueden eliminar definitivamente desde el explorador. Para gestionar o depurar copias de seguridad, utilice el módulo Respaldos"
            )));
        }
        let target = self.resolve(relative)?;
        if target == self.root {
            return Err(NasError::Denied("no se puede eliminar la raíz".into()));
        }
        if !target.exists() {
            return Err(NasError::NotFound(format!("no existe: {relative}")));
        }
        if target.is_dir() {
            std::fs::remove_dir_all(&target).map_err(|e| {
                if e.raw_os_error() == Some(1) {
                    NasError::Denied(format!(
                        "operación no permitida (EPERM): el directorio '{relative}' es inmutable o de solo lectura"
                    ))
                } else if e.kind() == std::io::ErrorKind::PermissionDenied || e.raw_os_error() == Some(13) {
                    NasError::Denied(format!(
                        "permiso denegado: el servicio web no tiene permisos suficientes para eliminar '{relative}'"
                    ))
                } else {
                    NasError::io(&target, e)
                }
            })
        } else {
            std::fs::remove_file(&target).map_err(|e| {
                if e.raw_os_error() == Some(1) {
                    NasError::Denied(format!(
                        "operación no permitida (EPERM): el archivo '{relative}' es inmutable o de solo lectura"
                    ))
                } else if e.kind() == std::io::ErrorKind::PermissionDenied || e.raw_os_error() == Some(13) {
                    NasError::Denied(format!(
                        "permiso denegado: el servicio web no tiene permisos suficientes para eliminar '{relative}'"
                    ))
                } else {
                    NasError::io(&target, e)
                }
            })
        }
    }

    /// Resuelve y valida que la ruta corresponda a un archivo regular existente confinado.
    pub fn resolve_file(&self, relative: &str) -> NasResult<(PathBuf, u64)> {
        let path = self.resolve(relative)?;
        let metadata = std::fs::metadata(&path).map_err(|e| NasError::io(&path, e))?;
        if !metadata.is_file() {
            return Err(NasError::Invalid(format!("no es un archivo: {relative}")));
        }
        Ok((path, metadata.len()))
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
        std::fs::write(root.root().join("carpeta/archivo.txt"), b"123456").expect("write");
        std::fs::write(root.root().join("b.txt"), b"hola").expect("write");
        std::fs::write(root.root().join("a.txt"), b"mundo").expect("write");

        let entries = root.list("").expect("list");
        assert_eq!(entries[0].name, "carpeta");
        assert!(entries[0].is_dir);
        assert_eq!(entries[0].size, 6);
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

    #[tokio::test]
    async fn resuelve_archivo_y_hace_streaming() {
        use http_body_util::BodyExt;

        let root = unique_root("stream");
        std::fs::write(
            root.root().join("video.mp4"),
            b"contenido de video de prueba",
        )
        .expect("write");

        let (path, size) = root.resolve_file("video.mp4").expect("resolve_file");
        assert_eq!(size, 28);
        assert!(root.resolve_file("inexistente.bin").is_err());
        assert!(root.resolve_file("").is_err());

        let file = tokio::fs::File::open(&path).await.expect("open");
        let stream = super::ReaderStream::with_capacity(file, 8);
        let body = axum::body::Body::from_stream(stream);
        let collected = body.collect().await.expect("collect").to_bytes();
        assert_eq!(&collected[..], b"contenido de video de prueba");

        let _ = std::fs::remove_dir_all(root.root());
    }
}
