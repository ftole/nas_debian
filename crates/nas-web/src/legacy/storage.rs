//! Cálculo de métricas y tamaños de almacenamiento para tareas de respaldo con caché TTL.

use std::collections::{HashMap, HashSet, VecDeque};
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
#[derive(Clone, Copy, Debug, serde::Serialize, serde::Deserialize)]
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

fn cache_file_path() -> Option<PathBuf> {
    if cfg!(test) {
        return None;
    }
    let state_dir = std::env::var("NAS_STATE_DIR")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "/var/lib/nas".to_string());
    Some(PathBuf::from(state_dir).join("backup_size_cache.json"))
}

fn load_persisted_cache_from_file(path: &Path) -> HashMap<String, TaskStorageSummary> {
    let Ok(raw) = std::fs::read_to_string(path) else {
        return HashMap::new();
    };
    let Ok(mut map) = serde_json::from_str::<HashMap<String, TaskStorageSummary>>(&raw) else {
        return HashMap::new();
    };
    // Marcar como obsoleto (epoch=1) para mostrar el último valor tras reinicio y refrescar en segundo plano.
    for summary in map.values_mut() {
        summary.computed_at_epoch = 1;
    }
    map
}

fn persist_cache_to_file(path: &Path, map: &HashMap<String, TaskStorageSummary>) {
    let Some(parent) = path.parent() else {
        return;
    };
    let _ = std::fs::create_dir_all(parent);
    let Ok(json) = serde_json::to_string(map) else {
        return;
    };
    let tmp = parent.join(format!(".backup_size_cache.tmp.{}", std::process::id()));
    if std::fs::write(&tmp, json).is_ok() {
        let _ = std::fs::rename(&tmp, path);
    }
}

fn storage_cache() -> &'static Mutex<HashMap<String, TaskStorageSummary>> {
    static CACHE: OnceLock<Mutex<HashMap<String, TaskStorageSummary>>> = OnceLock::new();
    CACHE.get_or_init(|| {
        let initial = cache_file_path()
            .map(|p| load_persisted_cache_from_file(&p))
            .unwrap_or_default();
        Mutex::new(initial)
    })
}

#[derive(Default)]
struct BackgroundRefreshQueue {
    pending: VecDeque<nas_engine::tasks::BackupTask>,
    queued_ids: HashSet<String>,
    worker_running: bool,
}

fn background_queue() -> &'static Mutex<BackgroundRefreshQueue> {
    static QUEUE: OnceLock<Mutex<BackgroundRefreshQueue>> = OnceLock::new();
    QUEUE.get_or_init(|| Mutex::new(BackgroundRefreshQueue::default()))
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
    let snapshot = if let Ok(mut cache) = storage_cache().lock() {
        cache.insert(task_id.to_string(), summary);
        cache.clone()
    } else {
        return;
    };
    if let Some(path) = cache_file_path() {
        persist_cache_to_file(&path, &snapshot);
    }
}

/// Invalida la caché de almacenamiento de una tarea tras congelar o eliminar respaldos.
pub fn invalidate_task_storage_cache(task_id: &str) {
    let snapshot = if let Ok(mut cache) = storage_cache().lock() {
        cache.remove(task_id);
        Some(cache.clone())
    } else {
        None
    };
    if let (Some(path), Some(snap)) = (cache_file_path(), snapshot) {
        persist_cache_to_file(&path, &snap);
    }
}

fn run_background_worker_loop() {
    loop {
        let next_task = {
            let Ok(mut q) = background_queue().lock() else {
                return;
            };
            match q.pending.pop_front() {
                Some(task) => Some(task),
                None => {
                    q.worker_running = false;
                    return;
                }
            }
        };

        if let Some(task) = next_task {
            let fresh = calculate_task_storage_with_timeout(&task, BACKGROUND_DU_TIMEOUT_SECS);
            store_task_storage(&task.id, fresh);
            if let Ok(mut q) = background_queue().lock() {
                q.queued_ids.remove(&task.id);
            }
        }
    }
}

fn spawn_background_refresh(task: nas_engine::tasks::BackupTask) {
    let should_spawn = {
        let Ok(mut q) = background_queue().lock() else {
            return;
        };
        if !q.queued_ids.insert(task.id.clone()) {
            return;
        }
        q.pending.push_back(task);
        if q.worker_running {
            false
        } else {
            q.worker_running = true;
            true
        }
    };

    if !should_spawn {
        return;
    }

    if let Ok(handle) = tokio::runtime::Handle::try_current() {
        drop(handle.spawn_blocking(run_background_worker_loop));
    } else if std::thread::Builder::new()
        .name("storage-refresh-worker".to_string())
        .spawn(run_background_worker_loop)
        .is_err()
    {
        if let Ok(mut q) = background_queue().lock() {
            q.worker_running = false;
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

fn run_du_low_prio(
    runner: &Runner,
    du_args: &[String],
    timeout_secs: u64,
) -> Option<nas_exec::CommandOutput> {
    let mut nice_args = vec![
        "-n".to_string(),
        "19".to_string(),
        "ionice".to_string(),
        "-c".to_string(),
        "3".to_string(),
        "du".to_string(),
    ];
    nice_args.extend(du_args.iter().cloned());
    if let Ok(out) = runner.run(
        &nas_exec::CommandSpec::new("nice")
            .args(nice_args)
            .timeout(Duration::from_secs(timeout_secs)),
    ) {
        if out.is_success() || !out.stdout.trim().is_empty() {
            return Some(out);
        }
    }
    runner
        .run(
            &nas_exec::CommandSpec::new("du")
                .args(du_args.iter().cloned())
                .timeout(Duration::from_secs(timeout_secs)),
        )
        .ok()
}

fn calculate_single_dir_size_with_timeout(dir: &Path, timeout_secs: u64) -> (u64, bool) {
    if !dir.is_dir() {
        return (0, true);
    }

    let runner = Runner::new();
    let args = vec!["-sb".to_string(), dir.to_string_lossy().to_string()];
    if let Some(out) = run_du_low_prio(&runner, &args, timeout_secs) {
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

    if let Some(out) = run_du_low_prio(&runner, &args, timeout_secs) {
        if let Some(bytes) = parse_du_total_bytes(&out.stdout) {
            return (bytes, out.is_success());
        }
        if !out.is_success() {
            // Fallback a `-scb` solo si `-B1` no fuera soportado (p. ej. du no GNU).
            let mut compat_args = vec!["-scb".to_string()];
            for d in dirs {
                compat_args.push(d.to_string_lossy().to_string());
            }
            if let Some(compat_out) = run_du_low_prio(&runner, &compat_args, timeout_secs) {
                if let Some(bytes) = parse_du_total_bytes(&compat_out.stdout) {
                    return (bytes, compat_out.is_success());
                }
            }
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
/// (stale-while-revalidate no bloqueante con cola serializada en segundo plano).
pub fn task_storage_with_cache(task: &nas_engine::tasks::BackupTask) -> TaskStorageSummary {
    if let Some((cached, fresh)) = get_cached_entry(&task.id) {
        if !fresh {
            spawn_background_refresh(task.clone());
        }
        return cached;
    }

    let hist_dir = task.task_dir();
    let s = hist_dir.to_string_lossy();
    let perm_dir = if s.contains("BACKUPS_HISTORICOS") {
        PathBuf::from(s.replace("BACKUPS_HISTORICOS", "BACKUPS_PERMANENTES"))
    } else {
        PathBuf::from(format!("/srv/nas/BACKUPS_PERMANENTES/{}", task.id))
    };

    if !hist_dir.is_dir() && !perm_dir.is_dir() {
        let empty = TaskStorageSummary {
            full_size_bytes: 0,
            disk_usage_bytes: 0,
            complete: true,
            computed_at_epoch: now_epoch(),
        };
        store_task_storage(&task.id, empty);
        return empty;
    }

    let placeholder = TaskStorageSummary {
        full_size_bytes: 0,
        disk_usage_bytes: 0,
        complete: false,
        computed_at_epoch: 0,
    };
    if let Ok(mut cache) = storage_cache().lock() {
        cache.insert(task.id.clone(), placeholder);
    }
    spawn_background_refresh(task.clone());
    placeholder
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

    #[test]
    fn cache_persistida_marca_obsoleta_y_no_bloquea_en_arranque_frio() {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let root =
            std::env::temp_dir().join(format!("nas-stg-cold-{}-{nanos}", std::process::id()));
        let task_dir = root.join("cold_task");
        fs::create_dir_all(&task_dir).unwrap();
        fs::write(task_dir.join("data.bin"), vec![1u8; 4096]).unwrap();

        let cache_path = root.join("cache.json");
        let mut map = HashMap::new();
        map.insert(
            "cold_task".to_string(),
            TaskStorageSummary {
                full_size_bytes: 4096,
                disk_usage_bytes: 4096,
                complete: true,
                computed_at_epoch: 9_999_999,
            },
        );
        persist_cache_to_file(&cache_path, &map);
        let loaded = load_persisted_cache_from_file(&cache_path);
        let entry = loaded.get("cold_task").expect("loaded entry");
        assert_eq!(entry.full_size_bytes, 4096);
        assert_eq!(entry.computed_at_epoch, 1);

        let task = nas_engine::tasks::BackupTask {
            id: format!("cold_task_{nanos}"),
            kind: nas_engine::tasks::SourceKind::Local,
            source_ip: None,
            share: None,
            path: Some("/tmp".into()),
            user: None,
            port: None,
            retention: 3,
            destination_root: root.to_string_lossy().to_string(),
            log_dir: root.to_string_lossy().to_string(),
            credentials_file: None,
            key_file: None,
            auto_permanent: false,
        };
        fs::create_dir_all(task.task_dir()).unwrap();
        let initial = task_storage_with_cache(&task);
        assert!(!initial.complete);
        assert_eq!(initial.full_size_bytes, 0);
        invalidate_task_storage_cache(&task.id);

        let _ = fs::remove_dir_all(&root);
    }
}
