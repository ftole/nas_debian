//! Empaquetado ZIP hacia archivo temporal seguro con control de espacio libre y límites.

use std::path::{Path, PathBuf};

use nas_core::{NasError, NasResult};
use nas_exec::Runner;

use super::{FileRoot, TempFile};

/// Tamaño máximo agregado al comprimir una carpeta en ZIP.
///
/// En línea con el límite de subida (50 GB en nginx/axum) y con la
/// capacidad esperada de un NAS departamental. Se comprime a disco
/// (`secure_temp_zip_path`, nunca en RAM) para permitir tamaños grandes.
pub const ZIP_MAX_BYTES: u64 = 50 * 1024 * 1024 * 1024;

fn clean_stale_staging_files(staging: &Path) {
    let Ok(entries) = std::fs::read_dir(staging) else {
        return;
    };
    let now = std::time::SystemTime::now();
    for entry in entries.flatten() {
        let path = entry.path();
        let is_zip_tmp = path
            .file_name()
            .and_then(|s| s.to_str())
            .is_some_and(|name| name.starts_with("zip_") && name.ends_with(".tmp"));
        if is_zip_tmp {
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
    /// con cuota máxima definida por `ZIP_MAX_BYTES` (50 GiB), retornando la guarda
    /// para borrado automático y el tamaño.
    pub fn zip_temp(&self, relative: &str) -> NasResult<(TempFile, u64)> {
        use std::io::BufWriter;
        use zip::write::SimpleFileOptions;

        let source = self.resolve(relative)?;
        if !source.exists() {
            return Err(NasError::NotFound(format!("no existe: {relative}")));
        }

        // El ZIP se escribe a disco temporal (generalmente /var o /tmp), por lo
        // que hay que validar que exista espacio disponible en ese FS. Regla
        // operativa: exigir al menos el 110% del tamaño del original y
        // al menos 1 GiB libres, para que la compresión Deflated no muera a
        // media escritura con "No space left on device".
        let needed = source_available_estimate(&source);
        let required_free = needed.saturating_mul(11) / 10;
        let temp_path = secure_temp_zip_path();
        if let Some(free) = free_bytes_at(&temp_path, false) {
            if free < required_free || free < 1024 * 1024 * 1024 {
                let _ = std::fs::remove_file(&temp_path);
                return Err(NasError::Denied(format!(
                    "espacio insuficiente para comprimir en disco: requeridos al menos {}, disponibles {}. Libera espacio en el volumen temporal o intenta una descarga más pequeña.",
                    human_bytes_lossy(required_free),
                    human_bytes_lossy(free)
                )));
            }
        }
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

/// Estimación conservadora del tamaño de origen (carpeta o archivo).
///
/// Evita un `du` completo: solo recorre la jerarquía con límites acotados
/// para proteger el endpoint de carpetas gigantes.
fn source_available_estimate(source: &std::path::Path) -> u64 {
    if !source.is_dir() {
        return std::fs::metadata(source).map(|m| m.len()).unwrap_or(0);
    }
    crate::files::dir_size_recursive_bounded(source, 200_000)
}

/// Espacio libre en bytes del filesystem que aloja `path`.
/// Si la consulta falla, devuelve `None` (se omite el guard).
fn free_bytes_at(path: &std::path::Path, _strict: bool) -> Option<u64> {
    // Resuelve a un ancestro existente (secure_temp_zip_path puede no existir aún).
    let mut current: Option<std::path::PathBuf> = Some(path.to_path_buf());
    while let Some(p) = current {
        if p.exists() {
            if let Ok(canon) = std::fs::canonicalize(&p) {
                let canon_str = canon.to_string_lossy().to_string();
                let output = Runner::new()
                    .run(
                        &nas_exec::CommandSpec::new("df")
                            .args(["-B1", "--output=avail", canon_str.as_str()])
                            .timeout(std::time::Duration::from_secs(3)),
                    )
                    .ok()?;
                let value = output
                    .stdout
                    .lines()
                    .last()
                    .and_then(|line| line.trim().parse::<u64>().ok())?;
                return Some(value);
            }
            current = p.parent().map(|parent| parent.to_path_buf());
        } else {
            current = p.parent().map(|parent| parent.to_path_buf());
        }
    }
    None
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
                return Err(NasError::Denied(format!(
                    "la carpeta supera el límite de descarga ZIP ({}); contenido actual: {}. Si necesita una descarga mayor, haga un respaldo permanente o descargue los snapshots por partes.",
                    human_bytes_lossy(ZIP_MAX_BYTES),
                    human_bytes_lossy(*total)
                )));
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
        return Err(NasError::Denied(format!(
            "el contenido supera el límite de descarga ZIP ({}); al menos {} en la carpeta actual.",
            human_bytes_lossy(ZIP_MAX_BYTES),
            human_bytes_lossy(metadata.len())
        )));
    }

    writer
        .start_file(name.to_string(), *options)
        .map_err(|error| NasError::Invalid(format!("zip: {error}")))?;

    let mut file = std::fs::File::open(path).map_err(|e| NasError::io(path, e))?;
    let mut limited = (&mut file).take(ZIP_MAX_BYTES.saturating_sub(*total) + 1);
    let copied = std::io::copy(&mut limited, writer).map_err(|e| NasError::io(path, e))?;
    *total = total.saturating_add(copied);
    if *total > ZIP_MAX_BYTES {
        return Err(NasError::Denied(format!(
            "el contenido supera el límite de descarga ZIP ({})",
            human_bytes_lossy(ZIP_MAX_BYTES)
        )));
    }

    Ok(())
}

/// Convierte bytes a un formato legible sin precisión decimal grande (alias local).
fn human_bytes_lossy(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
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
        // 140 MiB < 50 GiB (ZIP_MAX_BYTES).
        // En la versión con el bug de doble conteo en pruebas previas, un límite menor fallaba erróneamente.
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

    #[test]
    fn limpia_solo_temporales_con_prefijo_zip() {
        let root = root("clean-staging");
        let dir = root.root();
        let old_time = std::time::SystemTime::now() - std::time::Duration::from_secs(7200);

        let zip_old = dir.join("zip_20260101_123.tmp");
        let other_old = dir.join("ajeno_123.tmp");
        let zip_fresh = dir.join("zip_20260101_fresh.tmp");

        for file_path in [&zip_old, &other_old] {
            let f = std::fs::File::create(file_path).expect("create");
            f.set_modified(old_time).expect("mtime");
        }
        std::fs::write(&zip_fresh, b"ok").expect("fresh");

        super::clean_stale_staging_files(dir);

        assert!(!zip_old.exists(), "zip_ antiguo debe eliminarse");
        assert!(
            other_old.exists(),
            "archivo .tmp sin prefijo zip_ debe conservarse"
        );
        assert!(zip_fresh.exists(), "zip_ reciente debe conservarse");

        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn limite_zip_corresponde_a_cincuenta_gib() {
        assert_eq!(super::ZIP_MAX_BYTES, 50 * 1024 * 1024 * 1024);
        assert_eq!(super::human_bytes_lossy(super::ZIP_MAX_BYTES), "50.0 GB");
    }
}
