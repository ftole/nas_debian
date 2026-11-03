//! Explorador de archivos confinado al directorio de datos.
//!
//! Todas las rutas son relativas a la raíz (`/srv/nas`). Se rechazan rutas
//! absolutas, componentes `..` y enlaces simbólicos que escapen de la raíz.

use std::path::{Component, Path, PathBuf};
use std::pin::Pin;
use std::task::{Context, Poll};

use futures_core::Stream;
use nas_core::{NasError, NasResult};
use serde::Serialize;
use tokio::io::{AsyncRead, ReadBuf};

/// Stream asíncrono sobre un lector con buffer constante O(1) de 64 KiB.
pub struct ReaderStream<R> {
    reader: R,
    buf: Box<[u8]>,
    done: bool,
}

impl<R> std::fmt::Debug for ReaderStream<R> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ReaderStream")
            .field("buf_len", &self.buf.len())
            .field("done", &self.done)
            .finish()
    }
}

impl<R> ReaderStream<R> {
    /// Crea un nuevo stream con tamaño de búfer de 64 KiB.
    pub fn new(reader: R) -> Self {
        Self::with_capacity(reader, 64 * 1024)
    }

    /// Crea un nuevo stream especificando el tamaño del búfer.
    pub fn with_capacity(reader: R, capacity: usize) -> Self {
        Self {
            reader,
            buf: vec![0u8; capacity].into_boxed_slice(),
            done: false,
        }
    }
}

impl<R: AsyncRead + Unpin> Stream for ReaderStream<R> {
    type Item = std::io::Result<axum::body::Bytes>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = &mut *self;
        if this.done {
            return Poll::Ready(None);
        }
        let mut read_buf = ReadBuf::new(&mut this.buf);
        match Pin::new(&mut this.reader).poll_read(cx, &mut read_buf) {
            Poll::Ready(Ok(())) => {
                let filled = read_buf.filled();
                if filled.is_empty() {
                    this.done = true;
                    Poll::Ready(None)
                } else {
                    Poll::Ready(Some(Ok(axum::body::Bytes::copy_from_slice(filled))))
                }
            }
            Poll::Ready(Err(e)) => {
                this.done = true;
                Poll::Ready(Some(Err(e)))
            }
            Poll::Pending => Poll::Pending,
        }
    }
}

/// Archivo temporal que se elimina automáticamente al destruirse (`Drop`).
#[derive(Debug)]
pub struct TempFile {
    path: PathBuf,
}

impl TempFile {
    /// Envuelve una ruta temporal para borrado garantizado.
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    /// Ruta del archivo temporal.
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

/// Stream que sirve un archivo temporal en bloques constantes O(1) y lo elimina
/// de disco automáticamente al completarse o abortarse la conexión.
pub struct CleaningReaderStream {
    stream: ReaderStream<tokio::fs::File>,
    _guard: TempFile,
}

impl std::fmt::Debug for CleaningReaderStream {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CleaningReaderStream")
            .field("path", &self._guard.path())
            .finish()
    }
}

impl CleaningReaderStream {
    /// Crea el stream envolviendo el descriptor abierto y la guarda temporal.
    pub fn new(file: tokio::fs::File, guard: TempFile) -> Self {
        Self {
            stream: ReaderStream::new(file),
            _guard: guard,
        }
    }
}

impl Stream for CleaningReaderStream {
    type Item = std::io::Result<axum::body::Bytes>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        Pin::new(&mut self.stream).poll_next(cx)
    }
}

/// Rango de bytes solicitado según HTTP Range (RFC 9110).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ByteRange {
    /// Desplazamiento inicial inclusivo (0-indexed).
    pub start: u64,
    /// Desplazamiento final inclusivo (0-indexed).
    pub end: u64,
    /// Tamaño total del archivo en bytes.
    pub total: u64,
}

impl ByteRange {
    /// Longitud en bytes del rango solicitado.
    pub fn length(&self) -> u64 {
        self.end - self.start + 1
    }

    /// Cabecera `Content-Range: bytes start-end/total`.
    pub fn content_range_header(&self) -> String {
        format!("bytes {}-{}/{}", self.start, self.end, self.total)
    }
}

/// Error al interpretar la cabecera HTTP Range.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RangeError {
    /// Sintaxis no reconocida o malformada (debe ignorarse según RFC 9110 y responder 200).
    Malformed,
    /// Rango insatisfactorio (debe responder 416 Range Not Satisfiable).
    Unsatisfiable,
}

/// Analiza la cabecera HTTP `Range: bytes=...` contra el tamaño total del archivo.
pub fn parse_range(header_val: &str, total_size: u64) -> Result<Option<ByteRange>, RangeError> {
    let trimmed = header_val.trim();
    let Some(range_spec) = trimmed.strip_prefix("bytes=") else {
        return Ok(None);
    };

    let first_range = range_spec.split(',').next().unwrap_or("").trim();
    if first_range.is_empty() {
        return Err(RangeError::Malformed);
    }

    if total_size == 0 {
        return Err(RangeError::Unsatisfiable);
    }

    if let Some((start_str, end_str)) = first_range.split_once('-') {
        let start_str = start_str.trim();
        let end_str = end_str.trim();

        if start_str.is_empty() && end_str.is_empty() {
            return Err(RangeError::Malformed);
        }

        if start_str.is_empty() {
            // Caso sufijo: bytes=-500 (los últimos 500 bytes)
            let suffix_len: u64 = end_str.parse().map_err(|_| RangeError::Malformed)?;
            if suffix_len == 0 {
                return Err(RangeError::Unsatisfiable);
            }
            let length = suffix_len.min(total_size);
            let start = total_size - length;
            let end = total_size - 1;
            return Ok(Some(ByteRange {
                start,
                end,
                total: total_size,
            }));
        }

        let start: u64 = start_str.parse().map_err(|_| RangeError::Malformed)?;
        if start >= total_size {
            return Err(RangeError::Unsatisfiable);
        }

        let end = if end_str.is_empty() {
            // Caso abierto: bytes=500- (desde 500 hasta el final)
            total_size - 1
        } else {
            // Caso cerrado: bytes=500-999
            let explicit_end: u64 = end_str.parse().map_err(|_| RangeError::Malformed)?;
            if explicit_end < start {
                return Err(RangeError::Malformed);
            }
            explicit_end.min(total_size - 1)
        };

        Ok(Some(ByteRange {
            start,
            end,
            total: total_size,
        }))
    } else {
        Err(RangeError::Malformed)
    }
}

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

    /// Crea un directorio (y sus padres) con permisos de grupo Samba (`2770`).
    pub fn mkdir(&self, relative: &str) -> NasResult<PathBuf> {
        let path = self.resolve(relative)?;
        std::fs::create_dir_all(&path).map_err(|e| NasError::io(&path, e))?;
        set_dir_permissions(&path);
        Ok(path)
    }

    /// Renombra o mueve un elemento dentro de la raíz.
    pub fn rename(&self, from: &str, to: &str) -> NasResult<PathBuf> {
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
        std::fs::rename(&source, &target).map_err(|e| NasError::io(&target, e))?;
        Ok(target)
    }

    /// Elimina definitivamente un archivo o carpeta sin pasar por la papelera.
    pub fn delete_permanent(&self, relative: &str) -> NasResult<()> {
        let target = self.resolve(relative)?;
        if target == self.root {
            return Err(NasError::Denied("no se puede eliminar la raíz".into()));
        }
        if !target.exists() {
            return Err(NasError::NotFound(format!("no existe: {relative}")));
        }
        if target.is_dir() {
            std::fs::remove_dir_all(&target).map_err(|e| NasError::io(&target, e))
        } else {
            std::fs::remove_file(&target).map_err(|e| NasError::io(&target, e))
        }
    }

    /// Mueve un elemento a la papelera interna (`.trash`).
    pub fn trash(&self, relative: &str) -> NasResult<PathBuf> {
        self.trash_with_actor(relative, "sistemas")
    }

    /// Mueve un elemento a la papelera interna registrando el usuario que lo eliminó.
    pub fn trash_with_actor(&self, relative: &str, actor: &str) -> NasResult<PathBuf> {
        let source = self.resolve(relative)?;
        if source == self.root {
            return Err(NasError::Denied("no se puede eliminar la raíz".into()));
        }
        if !source.exists() {
            return Err(NasError::NotFound(format!("no existe: {relative}")));
        }

        let trash = self.root.join(".trash");
        std::fs::create_dir_all(&trash).map_err(|e| NasError::io(&trash, e))?;
        set_dir_permissions(&trash);

        let stamp_ms = next_unique_trash_ms(&trash);
        let name = source
            .file_name()
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_else(|| "elemento".to_string());
        let id = format!("{stamp_ms}_{name}");
        let target = trash.join(&id);
        let deleted_at = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();

        std::fs::rename(&source, &target).map_err(|e| NasError::io(&target, e))?;
        if let Err(error) = self.write_trash_meta(&id, relative, &name, actor, &deleted_at) {
            let _ = std::fs::rename(&target, &source);
            return Err(error);
        }
        Ok(target)
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

/// Tamaño máximo agregado al comprimir una carpeta en ZIP (256 MiB).
pub const ZIP_MAX_BYTES: u64 = 256 * 1024 * 1024;

fn clean_stale_staging_files(staging: &Path) {
    let Ok(entries) = std::fs::read_dir(staging) else {
        return;
    };
    let now = std::time::SystemTime::now();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) == Some("tmp") {
            if let Ok(metadata) = entry.metadata() {
                if let Ok(modified) = metadata.modified() {
                    if let Ok(age) = now.duration_since(modified) {
                        if age.as_secs() > 3600 {
                            let _ = std::fs::remove_file(&path);
                        }
                    }
                }
            }
        }
    }
}

fn secure_temp_zip_path() -> PathBuf {
    let staging = Path::new("/var/lib/nas/staging");
    let base_dir = if std::fs::create_dir_all(staging).is_ok() && staging.is_dir() {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(staging, std::fs::Permissions::from_mode(0o700));
        }
        clean_stale_staging_files(staging);
        staging.to_path_buf()
    } else {
        let tmp = std::env::temp_dir();
        clean_stale_staging_files(&tmp);
        tmp
    };
    let mut random_bytes = [0u8; 8];
    let _ = getrandom::getrandom(&mut random_bytes);
    let rand_hex = hex::encode(random_bytes);
    let stamp = chrono::Local::now().format("%Y%m%d%H%M%S");
    base_dir.join(format!("zip_{stamp}_{}_{rand_hex}.tmp", std::process::id()))
}

fn create_secure_temp_file(path: &Path) -> NasResult<std::fs::File> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        std::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .read(true)
            .mode(0o600)
            .open(path)
            .map_err(|e| NasError::io(path, e))
    }
    #[cfg(not(unix))]
    {
        std::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .read(true)
            .open(path)
            .map_err(|e| NasError::io(path, e))
    }
}

impl FileRoot {
    /// Comprime una carpeta (o archivo) hacia un archivo temporal seguro en disco
    /// con cuota máxima de 256 MiB, retornando la guarda para borrado automático y el tamaño.
    pub fn zip_temp(&self, relative: &str) -> NasResult<(TempFile, u64)> {
        use std::io::BufWriter;
        use zip::write::SimpleFileOptions;

        let source = self.resolve(relative)?;
        if !source.exists() {
            return Err(NasError::NotFound(format!("no existe: {relative}")));
        }

        let temp_path = secure_temp_zip_path();
        let file = create_secure_temp_file(&temp_path)?;
        let temp_guard = TempFile::new(temp_path);

        let mut buffered = BufWriter::new(file);
        let mut writer = zip::ZipWriter::new(&mut buffered);
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
            let mut total_written: u64 = 0;
            for (absolute, relative_name) in entries {
                add_to_zip(
                    &mut writer,
                    &options,
                    &absolute,
                    &relative_name,
                    &mut total_written,
                )?;
            }
        }

        writer
            .finish()
            .map_err(|error| NasError::Invalid(format!("zip: {error}")))?;

        use std::io::Write;
        buffered
            .flush()
            .map_err(|e| NasError::io(temp_guard.path(), e))?;

        let size = std::fs::metadata(temp_guard.path())
            .map_err(|e| NasError::io(temp_guard.path(), e))?
            .len();

        Ok((temp_guard, size))
    }

    /// Comprime una carpeta (o archivo) en un ZIP con límite de tamaño (compatibilidad).
    pub fn zip(&self, relative: &str) -> NasResult<Vec<u8>> {
        let (temp_guard, _size) = self.zip_temp(relative)?;
        std::fs::read(temp_guard.path()).map_err(|e| NasError::io(temp_guard.path(), e))
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
    use std::io::Read;

    let metadata = std::fs::metadata(path).map_err(|e| NasError::io(path, e))?;
    if total.saturating_add(metadata.len()) > ZIP_MAX_BYTES {
        return Err(NasError::Denied(
            "el contenido supera el límite de 256 MiB para descarga ZIP".into(),
        ));
    }

    writer
        .start_file(name.to_string(), *options)
        .map_err(|error| NasError::Invalid(format!("zip: {error}")))?;

    let mut file = std::fs::File::open(path).map_err(|e| NasError::io(path, e))?;
    let mut limited = (&mut file).take(ZIP_MAX_BYTES.saturating_sub(*total) + 1);
    let copied = std::io::copy(&mut limited, writer).map_err(|e| NasError::io(path, e))?;
    *total = total.saturating_add(copied);
    if *total > ZIP_MAX_BYTES {
        return Err(NasError::Denied(
            "el contenido supera el límite de 256 MiB para descarga ZIP".into(),
        ));
    }

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

    #[test]
    fn zip_temp_crea_y_limpia_archivo_temporal() {
        let root = root("zip-temp");
        root.mkdir("docs").expect("mkdir");
        std::fs::write(root.root().join("docs/nota.txt"), b"datos de prueba").expect("write");

        let (temp, size) = root.zip_temp("docs").expect("zip temp");
        let path = temp.path().to_path_buf();
        assert!(path.exists());
        assert_eq!(size, std::fs::metadata(&path).expect("meta").len());
        assert!(size > 0);

        drop(temp);
        assert!(
            !path.exists(),
            "el archivo temporal debe eliminarse al hacer drop"
        );
        let _ = std::fs::remove_dir_all(root.root());
    }

    #[test]
    fn zip_temp_no_duplica_conteo_de_tamano_en_carpetas() {
        let root = root("no-duplica");
        root.mkdir("datos").expect("mkdir");

        // Creamos dos archivos de 70 MiB cada uno (total carpeta: 140 MiB).
        // 140 MiB < 256 MiB (ZIP_MAX_BYTES).
        // En la versión con el bug de doble conteo: 140 MiB + 140 MiB = 280 MiB > 256 MiB, fallando erróneamente.
        // Con la corrección de total_written independiente, se comprime exitosamente.
        let f1 = std::fs::File::create(root.root().join("datos/f1.bin")).expect("create f1");
        f1.set_len(70 * 1024 * 1024).expect("set_len f1");
        drop(f1);

        let f2 = std::fs::File::create(root.root().join("datos/f2.bin")).expect("create f2");
        f2.set_len(70 * 1024 * 1024).expect("set_len f2");
        drop(f2);

        let (temp, size) = root.zip_temp("datos").expect("zip temp exitoso");
        assert!(size > 0);
        let path = temp.path().to_path_buf();
        assert!(path.exists());
        drop(temp);
        assert!(!path.exists());
        let _ = std::fs::remove_dir_all(root.root());
    }
}

/// Entrada de la papelera interna.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct TrashEntry {
    /// Identificador único en `.trash` (`<epoch_ms>_<nombre>`).
    pub id: String,
    /// Nombre original del elemento.
    pub name: String,
    /// Alias de nombre consumido por el frontend (`app.js`).
    pub filename: String,
    /// Ruta relativa original antes del envío a la papelera.
    pub original_path: String,
    /// Usuario que envió el elemento a la papelera.
    pub deleted_by: String,
    /// Indica si el elemento es un directorio.
    pub is_dir: bool,
    /// Tamaño real en bytes (recursivo para carpetas).
    pub size: u64,
    /// Tamaño legible.
    pub size_formatted: String,
    /// Marca de borrado (`YYYY-MM-DD HH:MM:SS`).
    pub deleted_at: String,
}

#[derive(Debug, Default, serde::Serialize, serde::Deserialize)]
struct TrashMetaRecord {
    #[serde(default)]
    original_path: String,
    #[serde(default)]
    filename: String,
    #[serde(default)]
    deleted_by: String,
    #[serde(default)]
    deleted_at: String,
}

impl FileRoot {
    /// Escribe un archivo de texto dentro de la raíz con permisos `0660`.
    pub fn write(&self, relative: &str, contents: &str) -> NasResult<u64> {
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

    /// Lista la papelera interna con metadatos completos.
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
            let entry_path = item.path();
            let metadata = item.metadata().ok();
            let is_dir = metadata.as_ref().is_some_and(|m| m.is_dir());
            let size = if is_dir {
                dir_size_recursive(&entry_path)
            } else {
                metadata.as_ref().map_or(0, |m| m.len())
            };

            let (stamp, raw_name) = split_trash_name(&id);
            let meta = self.read_trash_meta_record(&id);
            let name = meta
                .as_ref()
                .map(|m| m.filename.as_str())
                .filter(|s| !s.is_empty())
                .unwrap_or(raw_name)
                .to_string();
            let original_path = meta
                .as_ref()
                .map(|m| m.original_path.as_str())
                .filter(|s| !s.is_empty())
                .unwrap_or(name.as_str())
                .to_string();
            let deleted_by = meta
                .as_ref()
                .map(|m| m.deleted_by.as_str())
                .filter(|s| !s.is_empty())
                .unwrap_or("sistemas")
                .to_string();
            let deleted_at = meta
                .as_ref()
                .map(|m| m.deleted_at.as_str())
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .unwrap_or_else(|| format_trash_stamp(stamp));

            entries.push(TrashEntry {
                id,
                filename: name.clone(),
                name,
                original_path,
                deleted_by,
                is_dir,
                size,
                size_formatted: human_size(size),
                deleted_at,
            });
        }
        entries.sort_by(|a, b| {
            b.deleted_at
                .cmp(&a.deleted_at)
                .then_with(|| b.id.cmp(&a.id))
        });
        Ok(entries)
    }

    /// Restaura un elemento de la papelera a su ruta original.
    pub fn restore_trash(&self, id: &str) -> NasResult<String> {
        let (real_id, source) = self.resolve_trash_entry(id)?;
        let (_, fallback_name) = split_trash_name(&real_id);
        let meta = self.read_trash_meta_record(&real_id);
        let name = meta
            .as_ref()
            .map(|m| m.filename.as_str())
            .filter(|s| !s.is_empty())
            .unwrap_or(fallback_name);
        let relative = meta
            .as_ref()
            .map(|m| m.original_path.clone())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| name.to_string());

        let mut target = self.resolve(&relative)?;
        if target == self.root || target.parent().is_none() {
            target = self.root.join(name);
        }
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).map_err(|e| NasError::io(parent, e))?;
            set_dir_permissions(parent);
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
        let _ = self.remove_trash_meta(&real_id);
        Ok(target
            .strip_prefix(&self.root)
            .map(|path| path.to_string_lossy().replace('\\', "/"))
            .unwrap_or_else(|_| name.to_string()))
    }

    /// Elimina definitivamente un elemento de la papelera.
    pub fn delete_trash(&self, id: &str) -> NasResult<()> {
        let (real_id, path) = self.resolve_trash_entry(id)?;
        let result = if path.is_dir() {
            std::fs::remove_dir_all(&path).map_err(|e| NasError::io(&path, e))
        } else {
            std::fs::remove_file(&path).map_err(|e| NasError::io(&path, e))
        };
        let _ = self.remove_trash_meta(&real_id);
        result
    }

    fn trash_meta_dir(&self) -> PathBuf {
        self.root.join(".trash").join(".meta")
    }

    fn write_trash_meta(
        &self,
        id: &str,
        relative: &str,
        filename: &str,
        actor: &str,
        deleted_at: &str,
    ) -> NasResult<()> {
        let dir = self.trash_meta_dir();
        std::fs::create_dir_all(&dir).map_err(|e| NasError::io(&dir, e))?;
        let record = TrashMetaRecord {
            original_path: relative.to_string(),
            filename: filename.to_string(),
            deleted_by: if actor.trim().is_empty() {
                "sistemas".to_string()
            } else {
                actor.trim().to_string()
            },
            deleted_at: deleted_at.to_string(),
        };
        let json = serde_json::to_string(&record)
            .map_err(|e| NasError::Invalid(format!("metadatos de papelera: {e}")))?;
        let path = dir.join(id);
        std::fs::write(&path, json).map_err(|e| NasError::io(&path, e))
    }

    fn read_trash_meta_record(&self, id: &str) -> Option<TrashMetaRecord> {
        let path = self.trash_meta_dir().join(id);
        let value = std::fs::read_to_string(path).ok()?;
        let trimmed = value.trim();
        if trimmed.is_empty() {
            return None;
        }
        if trimmed.starts_with('{') {
            if let Ok(record) = serde_json::from_str::<TrashMetaRecord>(trimmed) {
                return Some(record);
            }
        }
        Some(TrashMetaRecord {
            original_path: trimmed.to_string(),
            ..TrashMetaRecord::default()
        })
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

    fn resolve_trash_entry(&self, id: &str) -> NasResult<(String, PathBuf)> {
        let id = id.trim();
        if id.is_empty()
            || id.starts_with('.')
            || id.contains('/')
            || id.contains('\\')
            || id.contains("..")
            || id.contains('\0')
        {
            return Err(NasError::Denied(
                "identificador de papelera inválido".into(),
            ));
        }
        let trash = self.root.join(".trash");
        let exact = trash.join(id);
        if exact.exists() {
            return Ok((id.to_string(), exact));
        }
        if id.bytes().all(|b| b.is_ascii_digit()) && trash.is_dir() {
            let prefix = format!("{id}_");
            if let Ok(entries) = std::fs::read_dir(&trash) {
                let mut matches: Vec<(String, PathBuf)> = entries
                    .flatten()
                    .filter_map(|entry| {
                        let name = entry.file_name().to_string_lossy().to_string();
                        name.starts_with(&prefix).then(|| (name, entry.path()))
                    })
                    .collect();
                if matches.len() == 1 {
                    return Ok(matches.remove(0));
                }
            }
        }
        Err(NasError::NotFound(format!("no existe en papelera: {id}")))
    }
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
fn set_dir_permissions(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o2770));
    if let Some(gid) = resolve_grp_samba_gid() {
        let _ = std::os::unix::fs::chown(path, None, Some(gid));
    }
}

#[cfg(not(unix))]
fn set_dir_permissions(_path: &Path) {}

fn next_unique_trash_ms(trash: &Path) -> i64 {
    let mut candidate = chrono::Utc::now().timestamp_millis().max(1);
    let existing: Vec<String> = std::fs::read_dir(trash)
        .ok()
        .map(|entries| {
            entries
                .flatten()
                .map(|entry| entry.file_name().to_string_lossy().to_string())
                .collect()
        })
        .unwrap_or_default();

    while existing
        .iter()
        .any(|name| name.starts_with(&format!("{candidate}_")))
    {
        candidate += 1;
    }
    candidate
}

fn dir_size_recursive(root: &Path) -> u64 {
    let mut total = 0u64;
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
            if meta.is_dir() {
                stack.push(entry.path());
            } else {
                total = total.saturating_add(meta.len());
            }
        }
    }
    total
}

fn split_trash_name(id: &str) -> (&str, &str) {
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

fn format_trash_stamp(stamp: &str) -> String {
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

    #[test]
    fn soporta_id_numerico_parseint_tamano_recursivo_y_borrado_permanente() {
        let root = root("parseint");
        root.write("CARPETA/a.txt", "12345").expect("a");
        root.write("CARPETA/b.txt", "6789").expect("b");
        root.trash_with_actor("CARPETA", "admin").expect("trash");

        let entries = root.trash_entries().expect("lista");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].filename, "CARPETA");
        assert_eq!(entries[0].original_path, "CARPETA");
        assert_eq!(entries[0].deleted_by, "admin");
        assert!(entries[0].is_dir);
        assert_eq!(entries[0].size, 9);

        let numeric_prefix = entries[0].id.split('_').next().expect("prefijo");
        assert!(numeric_prefix.bytes().all(|b| b.is_ascii_digit()));
        let restored = root.restore_trash(numeric_prefix).expect("restore numeric");
        assert_eq!(restored, "CARPETA");
        root.delete_permanent("CARPETA")
            .expect("borrado permanente");
        assert!(!root.root().join("CARPETA").exists());
        let _ = std::fs::remove_dir_all(root.root());
    }

    #[test]
    fn analiza_cabeceras_http_range() {
        use super::{parse_range, ByteRange, RangeError};

        // Rango cerrado normal
        let r = parse_range("bytes=0-499", 1000).expect("ok").expect("some");
        assert_eq!(
            r,
            ByteRange {
                start: 0,
                end: 499,
                total: 1000
            }
        );
        assert_eq!(r.length(), 500);
        assert_eq!(r.content_range_header(), "bytes 0-499/1000");

        // Rango abierto hasta el final
        let r = parse_range("bytes=500-", 1000).expect("ok").expect("some");
        assert_eq!(
            r,
            ByteRange {
                start: 500,
                end: 999,
                total: 1000
            }
        );
        assert_eq!(r.length(), 500);

        // Sufijo (últimos 200 bytes)
        let r = parse_range("bytes=-200", 1000).expect("ok").expect("some");
        assert_eq!(
            r,
            ByteRange {
                start: 800,
                end: 999,
                total: 1000
            }
        );
        assert_eq!(r.length(), 200);

        // Sufijo mayor que el tamaño total
        let r = parse_range("bytes=-1500", 1000).expect("ok").expect("some");
        assert_eq!(
            r,
            ByteRange {
                start: 0,
                end: 999,
                total: 1000
            }
        );

        // Sin prefijo bytes= se ignora (None -> 200)
        assert_eq!(parse_range("none", 1000), Ok(None));

        // Rango insatisfactorio (start >= total)
        assert_eq!(
            parse_range("bytes=1000-", 1000),
            Err(RangeError::Unsatisfiable)
        );
        assert_eq!(
            parse_range("bytes=1500-2000", 1000),
            Err(RangeError::Unsatisfiable)
        );

        // Archivo vacío es insatisfactorio
        assert_eq!(parse_range("bytes=0-10", 0), Err(RangeError::Unsatisfiable));

        // Rango invertido es malformado
        assert_eq!(
            parse_range("bytes=500-200", 1000),
            Err(RangeError::Malformed)
        );
        assert_eq!(
            parse_range("bytes=abc-def", 1000),
            Err(RangeError::Malformed)
        );
    }
}
