//! Cálculo de métricas y tamaños de almacenamiento para tareas de respaldo con caché TTL.

use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

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
fn resolve_existing_path(dest_root: &str) -> &std::path::Path {
    let mut path = std::path::Path::new(dest_root);
    while !path.exists() {
        if let Some(parent) = path.parent() {
            if parent.as_os_str().is_empty() {
                return std::path::Path::new("/");
            }
            path = parent;
        } else {
            return std::path::Path::new("/");
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
        .timeout(std::time::Duration::from_millis(1500));

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
pub const SIZE_CACHE_TTL_SECS: u64 = 60;

fn storage_cache() -> &'static Mutex<std::collections::HashMap<String, TaskStorageSummary>> {
    static CACHE: OnceLock<Mutex<std::collections::HashMap<String, TaskStorageSummary>>> =
        OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(std::collections::HashMap::new()))
}

fn now_epoch() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn cached_task_storage(task_id: &str) -> Option<TaskStorageSummary> {
    let cache = storage_cache().lock().ok()?;
    let entry = cache.get(task_id)?;
    let fresh = now_epoch().saturating_sub(entry.computed_at_epoch) < SIZE_CACHE_TTL_SECS;
    fresh.then_some(*entry)
}

fn store_task_storage(task_id: &str, summary: TaskStorageSummary) {
    if let Ok(mut cache) = storage_cache().lock() {
        cache.insert(task_id.to_string(), summary);
    }
}

/// Calcula el tamaño de un directorio individual en bytes mediante `du -sb`.
pub fn calculate_single_dir_size_ex(dir: &std::path::Path) -> (u64, bool) {
    if !dir.is_dir() {
        return (0, true);
    }

    let runner = Runner::new();
    let spec = nas_exec::CommandSpec::new("du")
        .args(["-sb", &dir.to_string_lossy()])
        .timeout(std::time::Duration::from_secs(15));

    if let Ok(out) = runner.run(&spec) {
        if let Some(first_line) = out.stdout.lines().next() {
            if let Some(bytes_str) = first_line.split_whitespace().next() {
                if let Ok(bytes) = bytes_str.parse::<u64>() {
                    return (bytes, out.is_success());
                }
            }
        }
    }

    let approx = crate::files::dir_size_recursive_bounded(dir, 5000);
    (approx, false)
}

/// Calcula el espacio físico consolidado ocupado en disco por múltiples directorios mediante `du -scb`.
pub fn calculate_dirs_combined_size_ex(dirs: &[std::path::PathBuf]) -> (u64, bool) {
    if dirs.is_empty() {
        return (0, true);
    }
    if dirs.len() == 1 {
        return calculate_single_dir_size_ex(&dirs[0]);
    }

    let runner = Runner::new();
    let mut args = vec!["-scb".to_string()];
    for d in dirs {
        args.push(d.to_string_lossy().to_string());
    }

    let spec = nas_exec::CommandSpec::new("du")
        .args(args)
        .timeout(std::time::Duration::from_secs(20));

    if let Ok(out) = runner.run(&spec) {
        if let Some(last_line) = out.stdout.lines().rev().find(|l| !l.trim().is_empty()) {
            if let Some(bytes_str) = last_line.split_whitespace().next() {
                if let Ok(bytes) = bytes_str.parse::<u64>() {
                    return (bytes, out.is_success());
                }
            }
        }
    }

    let mut sum = 0u64;
    let mut all_ok = true;
    for d in dirs {
        let (b, ok) = calculate_single_dir_size_ex(d);
        sum = sum.saturating_add(b);
        if !ok {
            all_ok = false;
        }
    }
    (sum, all_ok)
}

/// Calcula las métricas de almacenamiento de una tarea considerando tanto la capacidad
/// del último respaldo completo (snapshot o full) como el espacio físico consolidado en disco.
pub fn calculate_task_storage(task: &nas_engine::tasks::BackupTask) -> TaskStorageSummary {
    let hist_dir = task.task_dir();
    let s = hist_dir.to_string_lossy();
    let perm_dir = if s.contains("BACKUPS_HISTORICOS") {
        std::path::PathBuf::from(s.replace("BACKUPS_HISTORICOS", "BACKUPS_PERMANENTES"))
    } else {
        std::path::PathBuf::from(format!("/srv/nas/BACKUPS_PERMANENTES/{}", task.id))
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

    // 2. Determinar la capacidad del respaldo completo más reciente
    let (full_size_bytes, full_complete) = if !candidates.is_empty() {
        candidates.sort_by(|a, b| b.file_name().cmp(&a.file_name()));
        calculate_single_dir_size_ex(&candidates[0])
    } else if hist_dir.is_dir() {
        calculate_single_dir_size_ex(&hist_dir)
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
        calculate_dirs_combined_size_ex(&existing_dirs)
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

/// Obtiene el resumen de almacenamiento de una tarea respetando la caché en memoria.
pub fn task_storage_with_cache(task: &nas_engine::tasks::BackupTask) -> TaskStorageSummary {
    if let Some(cached) = cached_task_storage(&task.id) {
        let task_clone = task.clone();
        std::thread::spawn(move || {
            let fresh = calculate_task_storage(&task_clone);
            store_task_storage(&task_clone.id, fresh);
        });
        return cached;
    }

    let computed = calculate_task_storage(task);
    store_task_storage(&task.id, computed);
    computed
}

/// Compatibilidad: calcula el espacio en bytes consumido por el directorio de la tarea de respaldo.
pub fn calculate_task_size_ex(task_dir: &std::path::Path) -> (u64, bool) {
    calculate_single_dir_size_ex(task_dir)
}

/// Compatibilidad: devuelve solo los bytes.
#[allow(dead_code)]
pub fn calculate_task_size(task_dir: &std::path::Path) -> u64 {
    calculate_task_size_ex(task_dir).0
}
