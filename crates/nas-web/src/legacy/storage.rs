//! Cálculo de métricas y tamaños de almacenamiento para tareas de respaldo con caché TTL.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use nas_exec::Runner;

use super::human_bytes;

/// Métricas de almacenamiento de la partición de destino de respaldos.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct BackupDiskMetrics {
    /// Capacidad total en bytes del disco/volumen de destino.
    pub total_bytes: u64,
    /// Espacio usado en bytes en el disco/volumen de destino.
    pub used_bytes: u64,
    /// Espacio disponible en bytes en el disco/volumen de destino.
    pub available_bytes: u64,
    /// Porcentaje de ocupación del disco/volumen de destino (0.0 a 100.0).
    pub used_percent: f64,
    /// Capacidad total formateada (ej. `232.9 GB`).
    pub total_formatted: String,
    /// Espacio usado formateado (ej. `192.5 GB`).
    pub used_formatted: String,
    /// Espacio disponible formateado (ej. `40.4 GB`).
    pub available_formatted: String,
    /// Resumen legible consolidado (ej. `Disco: 192.5 GB / 232.9 GB (84%)`).
    pub summary: String,
}

/// Resuelve una ruta existente subiendo por los ancestros si el directorio no existe aún.
fn resolve_existing_path(dest_root: &str) -> &Path {
    let mut path = Path::new(dest_root);
    while !path.exists() {
        if let Some(parent) = path.parent() {
            if parent.as_os_str().is_empty() {
                return Path::new("/");
            }
            path = parent;
        } else {
            return Path::new("/");
        }
    }
    path
}

/// Consulta el uso de disco del punto de montaje donde se aloja el respaldo mediante `df -Pk`.
pub fn calculate_destination_disk(dest_root: &str) -> BackupDiskMetrics {
    let check_path = resolve_existing_path(dest_root);
    let check_path_str = check_path.to_string_lossy();
    let runner = Runner::new();
    let spec = nas_exec::CommandSpec::new("df")
        .args(["-Pk", check_path_str.as_ref()])
        .timeout(Duration::from_millis(1500));

    if let Ok(out) = runner.run(&spec) {
        if out.is_success() {
            if let Some(df_full) = nas_engine::storage::parse_df_full(&out.stdout) {
                let total_bytes = df_full.total_kb.saturating_mul(1024);
                let used_bytes = df_full.used_kb.saturating_mul(1024);
                let available_bytes = df_full.available_kb.saturating_mul(1024);
                let used_percent = df_full.used_percent;

                let total_formatted = human_bytes(total_bytes);
                let used_formatted = human_bytes(used_bytes);
                let available_formatted = human_bytes(available_bytes);
                let summary =
                    format!("Disco: {used_formatted} / {total_formatted} ({used_percent:.0}%)");

                return BackupDiskMetrics {
                    total_bytes,
                    used_bytes,
                    available_bytes,
                    used_percent,
                    total_formatted,
                    used_formatted,
                    available_formatted,
                    summary,
                };
            }
        }
    }

    BackupDiskMetrics::default()
}

/// Resumen de almacenamiento de una tarea de respaldo.
#[derive(Clone, Copy, Debug)]
pub struct TaskStorageSummary {
    /// Capacidad real que ocupa el respaldo completo (el último snapshot o full completado).
    pub full_size_bytes: u64,
    /// Espacio físico consolidado en disco (históricos + permanentes deduplicados).
    pub disk_usage_bytes: u64,
    /// `true` si `du` terminó limpio; `false` si se usó un fallback aproximado.
    pub complete: bool,
    /// Marca de tiempo UNIX en la que se calculó el resumen.
    pub computed_at_epoch: u64,
}

/// Tiempo de vida en segundos de la caché de tamaño de almacenamiento.
pub const SIZE_CACHE_TTL_SECS: u64 = 300;

const INITIAL_DU_TIMEOUT_SECS: u64 = 15;
const BACKGROUND_DU_TIMEOUT_SECS: u64 = 180;
const FALLBACK_MAX_ENTRIES: usize = 20_000;

fn storage_cache() -> &'static Mutex<HashMap<String, TaskStorageSummary>> {
    static CACHE: OnceLock<Mutex<HashMap<String, TaskStorageSummary>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn in_flight_refreshes() -> &'static Mutex<HashSet<String>> {
    static IN_FLIGHT: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    IN_FLIGHT.get_or_init(|| Mutex::new(HashSet::new()))
}

fn now_epoch() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn get_cached_entry(task_id: &str) -> Option<(TaskStorageSummary, bool)> {
    let cache = storage_cache().lock().ok()?;
    let entry = *cache.get(task_id)?;
    let fresh = entry.computed_at_epoch > 0
        && now_epoch().saturating_sub(entry.computed_at_epoch) < SIZE_CACHE_TTL_SECS;
    Some((entry, fresh))
}

fn store_task_storage(task_id: &str, summary: TaskStorageSummary) {
    if let Ok(mut cache) = storage_cache().lock() {
        cache.insert(task_id.to_string(), summary);
    }
}

/// Invalida la caché de almacenamiento de una tarea tras congelar o eliminar respaldos.
pub fn invalidate_task_storage_cache(task_id: &str) {
    if let Ok(mut cache) = storage_cache().lock() {
        cache.remove(task_id);
    }
}

const MAX_CONCURRENT_BACKGROUND_REFRESHES: usize = 2;

fn spawn_background_refresh(task: nas_engine::tasks::BackupTask) {
    let id = task.id.clone();
    {
        let Ok(mut set) = in_flight_refreshes().lock() else {
            return;
        };
        if set.len() >= MAX_CONCURRENT_BACKGROUND_REFRESHES || !set.insert(id.clone()) {
            return;
        }
    }

    let work = move || {
        let fresh = calculate_task_storage_with_timeout(&task, BACKGROUND_DU_TIMEOUT_SECS);
        store_task_storage(&task.id, fresh);
        if let Ok(mut set) = in_flight_refreshes().lock() {
            set.remove(&id);
        }
    };

    if let Ok(handle) = tokio::runtime::Handle::try_current() {
        drop(handle.spawn_blocking(work));
    } else if std::thread::Builder::new()
        .name(format!("storage-refresh-{}", task.id))
        .spawn(work)
        .is_err()
    {
        if let Ok(mut set) = in_flight_refreshes().lock() {
            set.remove(&task.id);
        }
    }
}

/// Recorre múltiples directorios deduplicando por `(dev, ino)` con un límite de entradas.
fn walk_size_dedup_bounded(dirs: &[PathBuf], physical_blocks: bool, max_entries: usize) -> u64 {
    let mut total = 0u64;
    let mut count = 0usize;
    #[cfg(unix)]
    let mut seen: HashSet<(u64, u64)> = HashSet::new();
    let mut stack: Vec<PathBuf> = dirs.iter().filter(|d| d.is_dir()).cloned().collect();

    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let Ok(meta) = std::fs::symlink_metadata(entry.path()) else {
                continue;
            };
            if meta.file_type().is_symlink() {
                continue;
            }
            count += 1;
            if meta.is_dir() {
                stack.push(entry.path());
            } else if meta.is_file() {
                #[cfg(unix)]
                {
                    use std::os::unix::fs::MetadataExt;
                    if seen.insert((meta.dev(), meta.ino())) {
                        let sz = if physical_blocks {
                            meta.blocks().saturating_mul(512)
                        } else {
                            meta.len()
                        };
                        total = total.saturating_add(sz);
                    }
                }
                #[cfg(not(unix))]
                {
                    let _ = physical_blocks;
                    total = total.saturating_add(meta.len());
                }
            }
            if count >= max_entries {
                return total;
            }
        }
    }
    total
}

fn parse_du_total_bytes(stdout: &str) -> Option<u64> {
    let last_line = stdout.lines().rev().find(|l| !l.trim().is_empty())?;
    let bytes_str = last_line.split_whitespace().next()?;
    bytes_str.parse::<u64>().ok()
}

fn calculate_single_dir_size_with_timeout(dir: &Path, timeout_secs: u64) -> (u64, bool) {
    if !dir.is_dir() {
        return (0, true);
    }

    let runner = Runner::new();
    let spec = nas_exec::CommandSpec::new("du")
        .args(["-sb", &dir.to_string_lossy()])
        .timeout(Duration::from_secs(timeout_secs));

    if let Ok(out) = runner.run(&spec) {
        if let Some(bytes) = parse_du_total_bytes(&out.stdout) {
            return (bytes, out.is_success());
        }
    }

    let approx = walk_size_dedup_bounded(&[dir.to_path_buf()], false, FALLBACK_MAX_ENTRIES);
    (approx, false)
}

/// Calcula el tamaño de un directorio individual en bytes mediante `du -sb`.
pub fn calculate_single_dir_size_ex(dir: &Path) -> (u64, bool) {
    calculate_single_dir_size_with_timeout(dir, INITIAL_DU_TIMEOUT_SECS)
}

fn calculate_dirs_combined_size_with_timeout(dirs: &[PathBuf], timeout_secs: u64) -> (u64, bool) {
    if dirs.is_empty() {
        return (0, true);
    }

    let runner = Runner::new();
    // Usar `-sc -B1` (bloques físicos reales en bytes, deduplicados por inodo,
    // reflejando compresión Btrfs zstd:3 y archivos dispersos).
    let mut args = vec!["-sc".to_string(), "-B1".to_string()];
    for d in dirs {
        args.push(d.to_string_lossy().to_string());
    }

    let spec = nas_exec::CommandSpec::new("du")
        .args(args)
        .timeout(Duration::from_secs(timeout_secs));

    match runner.run(&spec) {
        Ok(out) => {
            if let Some(bytes) = parse_du_total_bytes(&out.stdout) {
                return (bytes, out.is_success());
            }
            if !out.is_success() {
                // Fallback a `-scb` solo si `-B1` no fuera soportado (p. ej. du no GNU).
                let mut compat_args = vec!["-scb".to_string()];
                for d in dirs {
                    compat_args.push(d.to_string_lossy().to_string());
                }
                if let Ok(compat_out) = runner.run(
                    &nas_exec::CommandSpec::new("du")
                        .args(compat_args)
                        .timeout(Duration::from_secs(timeout_secs)),
                ) {
                    if let Some(bytes) = parse_du_total_bytes(&compat_out.stdout) {
                        return (bytes, compat_out.is_success());
                    }
                }
            }
        }
        Err(_) => {
            // Timeout o fallo de ejecución: no duplicar la espera con un segundo `du`.
        }
    }

    let approx = walk_size_dedup_bounded(dirs, true, FALLBACK_MAX_ENTRIES);
    (approx, false)
}

/// Calcula el espacio físico consolidado ocupado en disco por múltiples directorios mediante `du -sc -B1`.
pub fn calculate_dirs_combined_size_ex(dirs: &[PathBuf]) -> (u64, bool) {
    calculate_dirs_combined_size_with_timeout(dirs, 20)
}

fn calculate_task_storage_with_timeout(
    task: &nas_engine::tasks::BackupTask,
    timeout_secs: u64,
) -> TaskStorageSummary {
    let hist_dir = task.task_dir();
    let s = hist_dir.to_string_lossy();
    let perm_dir = if s.contains("BACKUPS_HISTORICOS") {
        PathBuf::from(s.replace("BACKUPS_HISTORICOS", "BACKUPS_PERMANENTES"))
    } else {
        PathBuf::from(format!("/srv/nas/BACKUPS_PERMANENTES/{}", task.id))
    };

    // 1. Encontrar todos los candidatos a respaldo completo (snapshots e inmutables FULL)
    let mut candidates = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&hist_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                    if name.starts_with("snapshot_") {
                        candidates.push(path);
                    }
                }
            }
        }
    }
    if let Ok(entries) = std::fs::read_dir(&perm_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                    if name.starts_with("FULL_") {
                        candidates.push(path);
                    }
                }
            }
        }
    }

    // 2. Determinar la capacidad lógica del respaldo completo más reciente
    let (full_size_bytes, full_complete) = if !candidates.is_empty() {
        candidates.sort_by(|a, b| b.file_name().cmp(&a.file_name()));
        calculate_single_dir_size_with_timeout(&candidates[0], timeout_secs)
    } else if hist_dir.is_dir() {
        calculate_single_dir_size_with_timeout(&hist_dir, timeout_secs)
    } else {
        (0, true)
    };

    // 3. Determinar el espacio físico consolidado ocupado en disco por la tarea
    let mut existing_dirs = Vec::new();
    if hist_dir.is_dir() {
        existing_dirs.push(hist_dir);
    }
    if perm_dir.is_dir() {
        existing_dirs.push(perm_dir);
    }

    let (disk_usage_bytes, disk_complete) = if !existing_dirs.is_empty() {
        calculate_dirs_combined_size_with_timeout(&existing_dirs, timeout_secs)
    } else {
        (0, true)
    };

    let final_full = if full_size_bytes == 0 && disk_usage_bytes > 0 {
        disk_usage_bytes
    } else {
        full_size_bytes
    };
    let final_disk = if disk_usage_bytes == 0 && final_full > 0 {
        final_full
    } else {
        disk_usage_bytes
    };

    TaskStorageSummary {
        full_size_bytes: final_full,
        disk_usage_bytes: final_disk,
        complete: full_complete && disk_complete,
        computed_at_epoch: now_epoch(),
    }
}

/// Calcula las métricas de almacenamiento de una tarea considerando tanto la capacidad
/// del último respaldo completo (snapshot o full) como el espacio físico consolidado en disco.
pub fn calculate_task_storage(task: &nas_engine::tasks::BackupTask) -> TaskStorageSummary {
    calculate_task_storage_with_timeout(task, INITIAL_DU_TIMEOUT_SECS)
}

/// Obtiene el resumen de almacenamiento de una tarea respetando la caché en memoria
/// (stale-while-revalidate con exclusión de refrescos duplicados).
pub fn task_storage_with_cache(task: &nas_engine::tasks::BackupTask) -> TaskStorageSummary {
    if let Some((cached, fresh)) = get_cached_entry(&task.id) {
        if !fresh {
            spawn_background_refresh(task.clone());
        }
        return cached;
    }

    let mut computed = calculate_task_storage_with_timeout(task, INITIAL_DU_TIMEOUT_SECS);
    if !computed.complete {
        // Marcar como obsoleto para que el sondeo en segundo plano lo complete sin truncar.
        computed.computed_at_epoch = 0;
        store_task_storage(&task.id, computed);
        spawn_background_refresh(task.clone());
    } else {
        store_task_storage(&task.id, computed);
    }
    computed
}

/// Compatibilidad: calcula el espacio en bytes consumido por el directorio de la tarea de respaldo.
pub fn calculate_task_size_ex(task_dir: &Path) -> (u64, bool) {
    calculate_single_dir_size_ex(task_dir)
}

/// Compatibilidad: devuelve solo los bytes.
#[allow(dead_code)]
pub fn calculate_task_size(task_dir: &Path) -> u64 {
    calculate_task_size_ex(task_dir).0
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn combined_size_deduplica_hardlinks_entre_directorios() {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let root =
            std::env::temp_dir().join(format!("nas-stg-test-{}-{nanos}", std::process::id()));
        let d1 = root.join("hist");
        let d2 = root.join("perm");
        fs::create_dir_all(&d1).unwrap();
        fs::create_dir_all(&d2).unwrap();

        let payload = vec![0x55u8; 512 * 1024];
        fs::write(d1.join("file.bin"), &payload).unwrap();
        fs::hard_link(d1.join("file.bin"), d2.join("file.bin")).unwrap();

        let (single, ok1) = calculate_single_dir_size_ex(&d1);
        let (combined, ok2) = calculate_dirs_combined_size_ex(&[d1.clone(), d2.clone()]);
        assert!(ok1 && ok2);
        assert_eq!(single, 512 * 1024);
        // En disco no debe duplicarse (approx 512 KiB + overhead mínimo de directorio).
        assert!(
            combined <= 540 * 1024,
            "hardlinks no deduplicados: single={single}, combined={combined}"
        );

        let fallback = walk_size_dedup_bounded(&[d1, d2], false, 1000);
        assert_eq!(fallback, 512 * 1024);

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn cache_de_almacenamiento_se_invalida_explicitamente() {
        let task = nas_engine::tasks::BackupTask {
            id: "cache_test_task".into(),
            kind: nas_engine::tasks::SourceKind::Local,
            source_ip: None,
            share: None,
            path: Some("/nonexistent".into()),
            user: None,
            port: None,
            retention: 7,
            destination_root: "/tmp/nonexistent-nas-hist".into(),
            log_dir: "/tmp/nonexistent-nas-logs".into(),
            credentials_file: None,
            key_file: None,
            auto_permanent: true,
        };
        let summary = task_storage_with_cache(&task);
        assert_eq!(summary.full_size_bytes, 0);
        assert!(get_cached_entry(&task.id).is_some());
        invalidate_task_storage_cache(&task.id);
        assert!(get_cached_entry(&task.id).is_none());
    }
}
