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

/// Limpia archivos temporales huérfanos (`zip_*.tmp` y `.upload-*`) en `staging`
/// cuya antigüedad supere `max_age_secs` (`0` elimina cualquiera existente).
pub fn clean_stale_staging_files_in(staging: &Path, max_age_secs: u64) {
    let Ok(entries) = std::fs::read_dir(staging) else {
        return;
    };
    let now = std::time::SystemTime::now();
    for entry in entries.flatten() {
        let path = entry.path();
        let is_temp = path
            .file_name()
            .and_then(|s| s.to_str())
            .is_some_and(|name| {
                (name.starts_with("zip_") && name.ends_with(".tmp")) || name.starts_with(".upload-")
            });
        if is_temp {
            if max_age_secs == 0 {
                let _ = std::fs::remove_file(&path);
                continue;
            }
            if let Ok(metadata) = entry.metadata() {
                if let Ok(modified) = metadata.modified() {
                    if let Ok(age) = now.duration_since(modified) {
                        if age.as_secs() > max_age_secs {
                            let _ = std::fs::remove_file(&path);
                        }
                    }
                }
            }
        }
    }
}

fn clean_stale_staging_files(staging: &Path) {
    clean_stale_staging_files_in(staging, 3600);
}

fn ensure_writable_dir(dir: &Path) -> bool {
    if std::fs::create_dir_all(dir).is_err() || !dir.is_dir() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if dir
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n == "staging" || n == ".nas_staging")
        {
            let _ = std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700));
        }
    }
    let mut rand_bytes = [0u8; 4];
    let _ = getrandom::getrandom(&mut rand_bytes);
    let probe = dir.join(format!(
        ".nas_probe_{}_{}",
        std::process::id(),
        hex::encode(rand_bytes)
    ));
    if create_secure_temp_file(&probe).is_ok() {
        let _ = std::fs::remove_file(&probe);
        true
    } else {
        false
    }
}

fn select_staging_dir_from_candidates(
    candidates: &[PathBuf],
    required_free: u64,
    prefer_headroom_check: bool,
) -> Option<(PathBuf, Option<u64>)> {
    let mut writable: Vec<(PathBuf, Option<u64>)> = Vec::new();
    for dir in candidates {
        if ensure_writable_dir(dir) {
            let free = free_bytes_at(dir, false);
            writable.push((dir.clone(), free));
        }
    }
    if writable.is_empty() {
        return None;
    }

    if prefer_headroom_check {
        let min_headroom = required_free.max(1024 * 1024 * 1024).saturating_mul(2);
        if let Some((first_dir, Some(free))) = writable.first() {
            if *free >= min_headroom && *free >= 2 * 1024 * 1024 * 1024 {
                return Some((first_dir.clone(), Some(*free)));
            }
        }
    }

    writable
        .into_iter()
        .max_by_key(|(_, free)| free.unwrap_or(0))
}

fn build_temp_zip_path_in(base_dir: &Path) -> PathBuf {
    clean_stale_staging_files(base_dir);
    let mut random_bytes = [0u8; 8];
    let _ = getrandom::getrandom(&mut random_bytes);
    let rand_hex = hex::encode(random_bytes);
    let stamp = chrono::Local::now().format("%Y%m%d%H%M%S");
    base_dir.join(format!("zip_{stamp}_{}_{rand_hex}.tmp", std::process::id()))
}

fn effective_zip_ceiling(free_bytes: Option<u64>) -> u64 {
    match free_bytes {
        Some(free) => ZIP_MAX_BYTES.min(free.saturating_mul(80) / 100).max(1),
        None => ZIP_MAX_BYTES,
    }
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
    /// seleccionando inteligentemente el volumen de staging con mayor holgura y
    /// ajustando el techo dinámico al espacio libre disponible.
    pub fn zip_temp(&self, relative: &str) -> NasResult<(TempFile, u64)> {
        let paths = nas_core::NasPaths::from_env();
        self.zip_temp_in(relative, Some(&paths.state_dir), Some(&paths.data_root))
    }

    /// Variante explícita de [`Self::zip_temp`] con rutas de estado y datos configurables.
    pub fn zip_temp_in(
        &self,
        relative: &str,
        state_dir: Option<&Path>,
        data_root: Option<&Path>,
    ) -> NasResult<(TempFile, u64)> {
        use std::io::BufWriter;
        use zip::write::SimpleFileOptions;

        let source = self.resolve(relative)?;
        if !source.exists() {
            return Err(NasError::NotFound(format!("no existe: {relative}")));
        }

        let needed = source_available_estimate(&source);
        let required_free = needed.saturating_mul(11) / 10;

        let mut candidates: Vec<PathBuf> = Vec::new();
        if let Ok(custom) = std::env::var("NAS_ZIP_STAGING_DIR") {
            let trimmed = custom.trim();
            if !trimmed.is_empty() {
                candidates.push(PathBuf::from(trimmed));
            }
        }
        if self.is_protected_path("").is_none() {
            candidates.push(self.root().join(".nas_staging"));
        }
        if let Some(dr) = data_root {
            let data_staging = dr.join(".nas_staging");
            if !candidates.contains(&data_staging) {
                candidates.push(data_staging);
            }
        }
        let state_staging = state_dir
            .map(|p| p.join("staging"))
            .unwrap_or_else(|| PathBuf::from("/var/lib/nas/staging"));
        if !candidates.contains(&state_staging) {
            candidates.push(state_staging);
        }
        candidates.push(std::env::temp_dir());

        let (staging_dir, free_opt) =
            select_staging_dir_from_candidates(&candidates, required_free, true).ok_or_else(
                || NasError::Denied("no se encontró un directorio de staging escribible".into()),
            )?;

        if let Some(free) = free_opt {
            if free < required_free || free < 1024 * 1024 * 1024 {
                return Err(NasError::Denied(format!(
                    "espacio insuficiente para comprimir en disco: requeridos al menos {}, disponibles {}. Libera espacio en el volumen o intenta una descarga más pequeña.",
                    human_bytes_lossy(required_free.max(1024 * 1024 * 1024)),
                    human_bytes_lossy(free)
                )));
            }
        }

        let max_zip_bytes = effective_zip_ceiling(free_opt);
        let temp_path = build_temp_zip_path_in(&staging_dir);
        let file = create_secure_temp_file(&temp_path)?;
        let temp_guard = TempFile::new(temp_path);

        let mut buffered = BufWriter::new(file);
        let mut writer = zip::ZipWriter::new(&mut buffered);
        let options = SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated)
            .large_file(true);
        let mut total: u64 = 0;

        let mut total_written: u64 = 0;
        let mut last_space_check_bytes: u64 = 0;

        if source.is_file() {
            let name = source
                .file_name()
                .map(|name| name.to_string_lossy().to_string())
                .unwrap_or_else(|| "archivo".to_string());
            let mut progress = ZipWriteProgress {
                total: &mut total_written,
                max_bytes: max_zip_bytes,
                staging_path: temp_guard.path(),
                last_space_check: &mut last_space_check_bytes,
            };
            add_to_zip(&mut writer, &options, &source, &name, &mut progress)?;
        } else {
            let entries = collect_files(&source, &mut total, max_zip_bytes)?;
            let mut progress = ZipWriteProgress {
                total: &mut total_written,
                max_bytes: max_zip_bytes,
                staging_path: temp_guard.path(),
                last_space_check: &mut last_space_check_bytes,
            };
            for (absolute, relative_name) in entries {
                add_to_zip(
                    &mut writer,
                    &options,
                    &absolute,
                    &relative_name,
                    &mut progress,
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

/// Devuelve `(bytes_libres, bytes_totales)` del sistema de archivos que aloja `path`.
pub fn free_and_total_bytes_at(path: &std::path::Path) -> Option<(u64, u64)> {
    let mut current: Option<std::path::PathBuf> = Some(path.to_path_buf());
    while let Some(p) = current {
        if p.exists() {
            if let Ok(canon) = std::fs::canonicalize(&p) {
                let canon_str = canon.to_string_lossy().to_string();
                let output = Runner::new()
                    .run(
                        &nas_exec::CommandSpec::new("df")
                            .args(["-Pk", canon_str.as_str()])
                            .timeout(std::time::Duration::from_secs(3)),
                    )
                    .ok()?;
                if output.is_success() {
                    if let Some(line) = output.stdout.lines().nth(1) {
                        let fields: Vec<&str> = line.split_whitespace().collect();
                        if fields.len() >= 4 {
                            if let (Ok(total_kb), Ok(avail_kb)) =
                                (fields[1].parse::<u64>(), fields[3].parse::<u64>())
                            {
                                return Some((
                                    avail_kb.saturating_mul(1024),
                                    total_kb.saturating_mul(1024),
                                ));
                            }
                        }
                    }
                }
            }
        }
        current = p.parent().map(|parent| parent.to_path_buf());
    }
    None
}

/// Espacio libre en bytes del filesystem que aloja `path`.
/// Si la consulta falla, devuelve `None` (se omite el guard).
fn free_bytes_at(path: &std::path::Path, _strict: bool) -> Option<u64> {
    free_and_total_bytes_at(path).map(|(avail, _)| avail)
}

fn collect_files(
    root: &Path,
    total: &mut u64,
    max_zip_bytes: u64,
) -> NasResult<Vec<(PathBuf, String)>> {
    let mut files = Vec::new();
    let mut stack = vec![root.to_path_buf()];

    while let Some(dir) = stack.pop() {
        let entries = std::fs::read_dir(&dir).map_err(|e| NasError::io(&dir, e))?;
        for entry in entries.flatten() {
            let path = entry.path();
            let entry_name = entry.file_name();
            let entry_name_str = entry_name.to_string_lossy();
            if entry_name_str == ".trash"
                || entry_name_str == ".nas_staging"
                || entry_name_str.starts_with(".upload-")
                || (entry_name_str.starts_with("zip_") && entry_name_str.ends_with(".tmp"))
            {
                continue;
            }
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
            if *total > max_zip_bytes {
                return Err(NasError::Denied(format!(
                    "la carpeta supera el límite de descarga ZIP ({}); contenido actual: {}. Si necesita una descarga mayor, haga un respaldo permanente o descargue los snapshots por partes.",
                    human_bytes_lossy(max_zip_bytes),
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

struct ZipWriteProgress<'a> {
    total: &'a mut u64,
    max_bytes: u64,
    staging_path: &'a Path,
    last_space_check: &'a mut u64,
}

fn add_to_zip<W: std::io::Write + std::io::Seek>(
    writer: &mut zip::ZipWriter<W>,
    options: &zip::write::SimpleFileOptions,
    path: &Path,
    name: &str,
    progress: &mut ZipWriteProgress<'_>,
) -> NasResult<()> {
    use std::io::{Read, Write};

    let metadata = std::fs::metadata(path).map_err(|e| NasError::io(path, e))?;
    if progress.total.saturating_add(metadata.len()) > progress.max_bytes {
        return Err(NasError::Denied(format!(
            "el contenido supera el límite de descarga ZIP ({}); al menos {} en la carpeta actual.",
            human_bytes_lossy(progress.max_bytes),
            human_bytes_lossy(metadata.len())
        )));
    }

    writer
        .start_file(name.to_string(), *options)
        .map_err(|error| NasError::Invalid(format!("zip: {error}")))?;

    let mut file = std::fs::File::open(path).map_err(|e| NasError::io(path, e))?;
    let mut buf = vec![0u8; 1024 * 1024];
    loop {
        if progress.total.saturating_sub(*progress.last_space_check) >= 128 * 1024 * 1024 {
            *progress.last_space_check = *progress.total;
            if let Some(free_now) = free_bytes_at(progress.staging_path, false) {
                if free_now < 512 * 1024 * 1024 {
                    return Err(NasError::Denied(format!(
                        "espacio libre agotándose durante la compresión ({} disponibles); operación abortada para proteger el volumen",
                        human_bytes_lossy(free_now)
                    )));
                }
            }
        }
        let read_n = file.read(&mut buf).map_err(|e| NasError::io(path, e))?;
        if read_n == 0 {
            break;
        }
        *progress.total = progress.total.saturating_add(read_n as u64);
        if *progress.total > progress.max_bytes {
            return Err(NasError::Denied(format!(
                "el contenido supera el límite de descarga ZIP ({})",
                human_bytes_lossy(progress.max_bytes)
            )));
        }
        writer
            .write_all(&buf[..read_n])
            .map_err(|e| NasError::io(path, e))?;
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

    #[test]
    fn techo_zip_se_ajusta_al_espacio_libre_del_volumen() {
        assert_eq!(super::effective_zip_ceiling(None), super::ZIP_MAX_BYTES);
        assert_eq!(
            super::effective_zip_ceiling(Some(10 * 1024 * 1024 * 1024)),
            8 * 1024 * 1024 * 1024
        );
        assert_eq!(
            super::effective_zip_ceiling(Some(200 * 1024 * 1024 * 1024)),
            super::ZIP_MAX_BYTES
        );
    }

    #[test]
    fn limpia_temporales_huerfanos_de_upload_y_zip() {
        let root = root("clean-orphans");
        let dir = root.root();
        std::fs::write(dir.join(".upload-abc"), b"1").unwrap();
        std::fs::write(dir.join("zip_123.tmp"), b"2").unwrap();
        std::fs::write(dir.join("archivo.txt"), b"3").unwrap();

        super::clean_stale_staging_files_in(dir, 0);
        assert!(!dir.join(".upload-abc").exists());
        assert!(!dir.join("zip_123.tmp").exists());
        assert!(dir.join("archivo.txt").exists());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn zip_excluye_staging_y_temporales_del_archivo_generado() {
        let root = root("exclude-staging");
        let base = root.root();
        let target = base.join("carpeta");
        std::fs::create_dir_all(target.join(".nas_staging")).unwrap();
        std::fs::write(target.join(".nas_staging/zip_activo.tmp"), b"secreto").unwrap();
        std::fs::write(target.join(".upload-xyz"), b"parcial").unwrap();
        std::fs::write(target.join("zip_viejo.tmp"), b"viejo").unwrap();
        std::fs::write(target.join("valido.txt"), b"ok").unwrap();

        let staging_state = base.join("_state");
        let (temp_zip, _) = root
            .zip_temp_in("carpeta", Some(&staging_state), Some(base))
            .unwrap();
        let file = std::fs::File::open(temp_zip.path()).unwrap();
        let mut archive = zip::ZipArchive::new(file).unwrap();
        let names: Vec<String> = (0..archive.len())
            .map(|i| archive.by_index(i).unwrap().name().to_string())
            .collect();
        assert_eq!(names, vec!["valido.txt".to_string()]);
        drop(temp_zip);
        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn staging_prefiere_volumen_de_datos_sobre_estado_del_so() {
        let root = root("staging-order");
        let base = root.root();
        let target = base.join("carpeta");
        let state_staging = base.join("_os_state/staging");
        let data_staging = base.join(".nas_staging");
        std::fs::create_dir_all(&target).unwrap();
        std::fs::write(target.join("archivo.txt"), b"contenido").unwrap();

        let (selected, _) = super::select_staging_dir_from_candidates(
            &[data_staging.clone(), state_staging.clone()],
            1024,
            true,
        )
        .expect("staging seleccionable");
        assert_eq!(selected, data_staging);
        let _ = std::fs::remove_dir_all(base);
    }
}
