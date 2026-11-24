//! Gestión de respaldos permanentes inmutables (Golden Snapshots).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use nas_core::{NasError, NasResult};
use nas_exec::{CommandSpec, Runner};

use crate::backup_control::valid_id;

/// Metadatos e información de un respaldo permanente congelado (Golden Snapshot).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct PermanentBackupInfo {
    /// Identificador de la tarea.
    pub task: String,
    /// Nombre del directorio (ej. `FULL_2026-10-30_190000_cierre`).
    pub name: String,
    /// Ruta absoluta en disco.
    pub path: String,
    /// Fecha de congelación formateada.
    pub date: String,
    /// Etiqueta descriptiva opcional.
    pub tag: Option<String>,
    /// Tamaño aproximado en bytes.
    pub size_bytes: u64,
    /// Número de archivos contenidos.
    pub files_count: u64,
}

/// Tope estricto de inodos distintos en memoria al calcular estadísticas de un respaldo permanente.
const DIR_STATS_MAX_INODES: usize = 250_000;

fn permanent_stats_cache_path() -> Option<PathBuf> {
    if cfg!(test) {
        return None;
    }
    let state_dir = std::env::var("NAS_STATE_DIR")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "/var/lib/nas".to_string());
    Some(PathBuf::from(state_dir).join("permanent_backup_stats.json"))
}

fn load_permanent_stats_from_file(path: &Path) -> HashMap<String, (u64, u64)> {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|raw| serde_json::from_str::<HashMap<String, (u64, u64)>>(&raw).ok())
        .unwrap_or_default()
}

fn persist_permanent_stats_to_file(path: &Path, map: &HashMap<String, (u64, u64)>) {
    let Some(parent) = path.parent() else {
        return;
    };
    let _ = std::fs::create_dir_all(parent);
    let Ok(json) = serde_json::to_string(map) else {
        return;
    };
    let tmp = parent.join(format!(
        ".permanent_backup_stats.tmp.{}",
        std::process::id()
    ));
    if std::fs::write(&tmp, json).is_ok() {
        let _ = std::fs::rename(&tmp, path);
    }
}

fn permanent_stats_cache() -> &'static Mutex<HashMap<String, (u64, u64)>> {
    static CACHE: OnceLock<Mutex<HashMap<String, (u64, u64)>>> = OnceLock::new();
    CACHE.get_or_init(|| {
        let initial = permanent_stats_cache_path()
            .map(|p| load_permanent_stats_from_file(&p))
            .unwrap_or_default();
        Mutex::new(initial)
    })
}

fn cached_dir_stats(path: &Path) -> (u64, u64) {
    let key = path.to_string_lossy().to_string();
    if let Ok(cache) = permanent_stats_cache().lock() {
        if let Some(&stats) = cache.get(&key) {
            return stats;
        }
    }
    let stats = dir_stats(path);
    let snapshot = if let Ok(mut cache) = permanent_stats_cache().lock() {
        cache.insert(key, stats);
        Some(cache.clone())
    } else {
        None
    };
    if let (Some(cache_path), Some(snap)) = (permanent_stats_cache_path(), snapshot) {
        persist_permanent_stats_to_file(&cache_path, &snap);
    }
    stats
}

fn dir_stats(path: &Path) -> (u64, u64) {
    let mut files = 0u64;
    let mut bytes = 0u64;
    #[cfg(unix)]
    let mut seen = std::collections::HashSet::<(u64, u64)>::new();
    let mut stack = vec![path.to_path_buf()];
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
            if meta.is_file() {
                files += 1;
                #[cfg(unix)]
                {
                    use std::os::unix::fs::MetadataExt;
                    let count_file = if seen.len() < DIR_STATS_MAX_INODES {
                        seen.insert((meta.dev(), meta.ino()))
                    } else {
                        true
                    };
                    if count_file {
                        bytes += meta.len();
                    }
                }
                #[cfg(not(unix))]
                {
                    bytes += meta.len();
                }
            } else if meta.is_dir() {
                stack.push(entry.path());
            }
        }
    }
    (files, bytes)
}

fn clean_stale_freezing_dirs(perm_task_dir: &Path) {
    let Ok(entries) = std::fs::read_dir(perm_task_dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir()
            && path
                .file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with(".freezing_"))
        {
            let _ = std::fs::remove_dir_all(&path);
        }
    }
}

#[derive(Default)]
pub(super) struct TaskScriptInfo {
    pub(super) dest_root: String,
    pub(super) source_type: String,
    pub(super) src_ip: Option<String>,
    pub(super) src_share: Option<String>,
    pub(super) src_path: Option<String>,
    pub(super) src_user: Option<String>,
    pub(super) src_port: u16,
    pub(super) cred_file: Option<String>,
    pub(super) key_file: Option<String>,
}

pub(super) fn read_task_info(id: &str) -> TaskScriptInfo {
    let runner_path = Path::new("/usr/local/bin").join(format!("backup_{id}.sh"));
    let content = std::fs::read_to_string(&runner_path).unwrap_or_default();
    let mut info = TaskScriptInfo {
        dest_root: "/srv/nas/BACKUPS_HISTORICOS".to_string(),
        source_type: "local".to_string(),
        src_port: 22,
        ..Default::default()
    };

    for line in content.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("DEST_ROOT=") {
            let val = rest.trim_matches(|c| c == '\'' || c == '"' || c == ' ');
            if val.starts_with("/srv/nas") {
                info.dest_root = val.to_string();
            }
        } else if let Some(rest) = line.strip_prefix("SOURCE_TYPE=") {
            info.source_type = rest
                .trim_matches(|c| c == '\'' || c == '"' || c == ' ')
                .to_ascii_lowercase();
        } else if let Some(rest) = line.strip_prefix("SRC_IP=") {
            info.src_ip = Some(
                rest.trim_matches(|c| c == '\'' || c == '"' || c == ' ')
                    .to_string(),
            );
        } else if let Some(rest) = line.strip_prefix("SRC_SHARE=") {
            info.src_share = Some(
                rest.trim_matches(|c| c == '\'' || c == '"' || c == ' ')
                    .to_string(),
            );
        } else if let Some(rest) = line.strip_prefix("SRC_PATH=") {
            info.src_path = Some(
                rest.trim_matches(|c| c == '\'' || c == '"' || c == ' ')
                    .to_string(),
            );
        } else if let Some(rest) = line.strip_prefix("SRC_USER=") {
            info.src_user = Some(
                rest.trim_matches(|c| c == '\'' || c == '"' || c == ' ')
                    .to_string(),
            );
        } else if let Some(rest) = line.strip_prefix("SRC_PORT=") {
            if let Ok(p) = rest
                .trim_matches(|c| c == '\'' || c == '"' || c == ' ')
                .parse::<u16>()
            {
                info.src_port = p;
            }
        } else if let Some(rest) = line.strip_prefix("CRED_FILE=") {
            info.cred_file = Some(
                rest.trim_matches(|c| c == '\'' || c == '"' || c == ' ')
                    .to_string(),
            );
        } else if let Some(rest) = line.strip_prefix("KEY_FILE=") {
            let val = rest.trim_matches(|c| c == '\'' || c == '"' || c == ' ');
            if !val.is_empty() {
                info.key_file = Some(val.to_string());
            }
        }
    }
    info
}

/// Congela el snapshot más reciente de una tarea como respaldo permanente inmutable (a costo 0).
pub fn freeze_backup_task(
    runner: &Runner,
    id: &str,
    tag: Option<&str>,
) -> NasResult<PermanentBackupInfo> {
    if !valid_id(id) {
        return Err(NasError::Invalid(format!(
            "identificador de tarea inválido: '{id}'"
        )));
    }

    let task_info = read_task_info(id);
    let task_dir = Path::new(&task_info.dest_root).join(id);

    // Buscar snapshots existentes
    let entries = std::fs::read_dir(&task_dir).map_err(|_| {
        NasError::NotFound(format!(
            "no se encontró el directorio de la tarea {}",
            task_dir.display()
        ))
    })?;
    let mut snapshots: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("snapshot_"))
        })
        .collect();
    snapshots.sort();

    let Some(latest) = snapshots.pop() else {
        return Err(NasError::NotFound(format!(
            "no hay snapshots disponibles para congelar en '{}'",
            task_dir.display()
        )));
    };

    let perm_root = task_info
        .dest_root
        .replace("BACKUPS_HISTORICOS", "BACKUPS_PERMANENTES");
    let perm_task_dir = PathBuf::from(&perm_root).join(id);
    std::fs::create_dir_all(&perm_task_dir).map_err(|e| NasError::Denied(e.to_string()))?;
    let _ = runner
        .run(&CommandSpec::new("chown").args(["root:grp_samba", &perm_task_dir.to_string_lossy()]));
    let _ = runner.run(&CommandSpec::new("chmod").args(["2770", &perm_task_dir.to_string_lossy()]));
    let _ = runner.run(&CommandSpec::new("chattr").args(["-i", &perm_task_dir.to_string_lossy()]));
    clean_stale_freezing_dirs(&perm_task_dir);

    let now = chrono::Local::now();
    let date_str = now.format("%Y-%m-%d_%H%M%S").to_string();
    let clean_tag = tag
        .map(|t| {
            t.trim()
                .chars()
                .filter(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-')
                .take(32)
                .collect::<String>()
        })
        .filter(|t| !t.is_empty());

    let folder_name = match &clean_tag {
        Some(t) => format!("FULL_{date_str}_{t}"),
        None => format!("FULL_{date_str}"),
    };

    let stage_path = perm_task_dir.join(format!(".freezing_{folder_name}"));
    let target_path = perm_task_dir.join(&folder_name);

    // Levantar temporalmente inmutabilidad en el snapshot origen para permitir crear enlaces duros
    let _ = runner.run(&CommandSpec::new("chattr").args(["-i", &latest.to_string_lossy()]));

    // Clonación en directorio temporal `.freezing_*` para evitar `FULL_*` huérfanos ante un corte eléctrico.
    let latest_s = latest.to_string_lossy().to_string();
    let stage_s = stage_path.to_string_lossy().to_string();
    let target_s = target_path.to_string_lossy().to_string();
    let cp_res = runner.run(
        &CommandSpec::new("cp")
            .args(["-al", &latest_s, &stage_s])
            .timeout(Duration::from_secs(120)),
    )?;
    let cp_ok = if cp_res.is_success() {
        Ok(())
    } else {
        let _ = std::fs::remove_dir_all(&stage_path);
        let reflink_res = runner.run(
            &CommandSpec::new("cp")
                .args(["-a", "--reflink=always", &latest_s, &stage_s])
                .timeout(Duration::from_secs(120)),
        )?;
        if reflink_res.is_success() {
            Ok(())
        } else {
            let _ = std::fs::remove_dir_all(&stage_path);
            Err(if cp_res.stderr.is_empty() {
                reflink_res.stderr
            } else {
                cp_res.stderr
            })
        }
    };

    // Restaurar inmutabilidad en el contenedor origen (sin -R ni chmod -R a-w para permitir hardlinks)
    let _ = runner.run(&CommandSpec::new("chattr").args(["+i", &latest_s]));

    if let Err(err) = cp_ok {
        return Err(NasError::Invalid(format!(
            "error al congelar snapshot vía hardlinks/reflink: {err}"
        )));
    }

    let (files_count, size_bytes) = dir_stats(&stage_path);

    let info = PermanentBackupInfo {
        task: id.to_string(),
        name: folder_name.clone(),
        path: target_s.clone(),
        date: date_str,
        tag: clean_tag,
        size_bytes,
        files_count,
    };

    // Guardar metadatos y propiedad del contenedor antes de publicar y fijar `+i`
    let meta_file = stage_path.join(".freeze_meta.json");
    if let Ok(meta_json) = serde_json::to_string_pretty(&info) {
        let _ = std::fs::write(&meta_file, meta_json);
    }
    let _ = runner.run(&CommandSpec::new("chown").args(["root:grp_samba", &stage_s]));
    if let Err(e) = std::fs::rename(&stage_path, &target_path) {
        let _ = std::fs::remove_dir_all(&stage_path);
        return Err(NasError::Denied(format!(
            "no se pudo publicar respaldo permanente {target_s}: {e}"
        )));
    }

    // Inmutabilidad solo en el directorio contenedor (sin -R ni chmod -R a-w para preservar hardlinks en snapshot origen)
    let _ = runner.run(&CommandSpec::new("chattr").args(["+i", &target_s]));

    // Registrar en la bitácora de la tarea
    let log_file = format!("/srv/nas/LOGS_BACKUP/backup_{id}.log");
    let log_line = format!(
        "[{}] [{}] [FREEZE] Respaldo permanente congelado: {} (desde {})\n",
        now.format("%F %T"),
        id,
        target_path.display(),
        latest.display()
    );
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_file)
    {
        use std::io::Write;
        let _ = f.write_all(log_line.as_bytes());
    }

    Ok(info)
}

/// Lista los respaldos permanentes inmutables existentes.
pub fn list_permanent_backups(task_filter: Option<&str>) -> NasResult<Vec<PermanentBackupInfo>> {
    let mut roots = vec![PathBuf::from("/srv/nas/BACKUPS_PERMANENTES")];

    if let Ok(units) = std::fs::read_dir("/srv/nas/unidades") {
        for unit in units.flatten() {
            let perm = unit.path().join("BACKUPS_PERMANENTES");
            if perm.is_dir() {
                roots.push(perm);
            }
        }
    }

    let mut result = Vec::new();

    for root in roots {
        let Ok(tasks) = std::fs::read_dir(&root) else {
            continue;
        };
        for task_entry in tasks.flatten() {
            let task_path = task_entry.path();
            if !task_path.is_dir() {
                continue;
            }
            let task_name = task_entry.file_name().to_string_lossy().to_string();
            if let Some(filter) = task_filter {
                if !filter.trim().is_empty() && filter != task_name {
                    continue;
                }
            }

            let Ok(backups) = std::fs::read_dir(&task_path) else {
                continue;
            };
            for bkp_entry in backups.flatten() {
                let bkp_path = bkp_entry.path();
                if !bkp_path.is_dir() {
                    continue;
                }
                let folder_name = bkp_entry.file_name().to_string_lossy().to_string();
                if !folder_name.starts_with("FULL_") {
                    continue;
                }

                let meta_path = bkp_path.join(".freeze_meta.json");
                if let Ok(content) = std::fs::read_to_string(&meta_path) {
                    if let Ok(info) = serde_json::from_str::<PermanentBackupInfo>(&content) {
                        result.push(info);
                        continue;
                    }
                }

                let remainder = folder_name.trim_start_matches("FULL_");
                let parts: Vec<&str> = remainder.split('_').collect();
                let (date_str, tag) = if parts.len() >= 3 {
                    (
                        format!("{}_{}", parts[0], parts[1]),
                        Some(parts[2..].join("_")),
                    )
                } else if parts.len() == 2 {
                    if parts[1].len() == 6 && parts[1].chars().all(|c| c.is_ascii_digit()) {
                        (format!("{}_{}", parts[0], parts[1]), None)
                    } else {
                        (parts[0].to_string(), Some(parts[1].to_string()))
                    }
                } else {
                    (parts.first().unwrap_or(&"desconocida").to_string(), None)
                };

                let (files_count, size_bytes) = cached_dir_stats(&bkp_path);
                let info = PermanentBackupInfo {
                    task: task_name.clone(),
                    name: folder_name,
                    path: bkp_path.to_string_lossy().to_string(),
                    date: date_str,
                    tag,
                    size_bytes,
                    files_count,
                };
                if let Ok(meta_json) = serde_json::to_string_pretty(&info) {
                    let _ = std::fs::write(&meta_path, meta_json);
                }
                result.push(info);
            }
        }
    }

    result.sort_by(|a, b| b.name.cmp(&a.name));
    Ok(result)
}

/// Elimina un respaldo permanente inmutable.
pub fn delete_permanent_backup(runner: &Runner, id: &str, name: &str) -> NasResult<String> {
    if !valid_id(id) {
        return Err(NasError::Invalid(format!(
            "identificador de tarea inválido: '{id}'"
        )));
    }
    let name = name.trim();
    if !name.starts_with("FULL_")
        || name.contains('/')
        || name.contains('\\')
        || name.contains("..")
    {
        return Err(NasError::Invalid(format!(
            "nombre de respaldo permanente inválido: '{name}'"
        )));
    }

    let mut found_path = None;
    let candidates = [PathBuf::from("/srv/nas/BACKUPS_PERMANENTES")
        .join(id)
        .join(name)];
    for cand in &candidates {
        if cand.is_dir() {
            found_path = Some(cand.clone());
            break;
        }
    }

    if found_path.is_none() {
        if let Ok(units) = std::fs::read_dir("/srv/nas/unidades") {
            for unit in units.flatten() {
                let cand = unit.path().join("BACKUPS_PERMANENTES").join(id).join(name);
                if cand.is_dir() {
                    found_path = Some(cand);
                    break;
                }
            }
        }
    }

    let Some(path) = found_path else {
        return Err(NasError::NotFound(format!(
            "respaldo permanente '{name}' no encontrado"
        )));
    };

    let path_s = path.to_string_lossy().to_string();
    // Levantar primero +i solo en el contenedor raíz para evitar tormentas recursivas de metadatos;
    // únicamente si rm -rf falla (respaldo legado con +i/permisos anidados), ejecutar saneamiento recursivo.
    let _ = runner.run(&CommandSpec::new("chattr").args(["-i", &path_s]));
    let mut rm_success = runner
        .run(&CommandSpec::new("rm").args(["-rf", &path_s]))
        .map(|r| r.is_success())
        .unwrap_or(false);
    if !rm_success && std::fs::remove_dir_all(&path).is_ok() {
        rm_success = true;
    }
    if !rm_success && path.exists() {
        let _ = runner.run(
            &CommandSpec::new("chattr")
                .args(["-R", "-i", &path_s])
                .timeout(Duration::from_secs(600)),
        );
        let _ = runner.run(
            &CommandSpec::new("find")
                .args([&path_s, "-type", "d", "-exec", "chmod", "u+w", "{}", "+"])
                .timeout(Duration::from_secs(600)),
        );
        if let Ok(res) = runner.run(&CommandSpec::new("rm").args(["-rf", &path_s])) {
            if res.is_success() {
                rm_success = true;
            }
        }
        if !rm_success {
            let _ = std::fs::remove_dir_all(&path);
        }
    }

    if let Some(perm_parent) = path.parent() {
        if let Ok(entries) = std::fs::read_dir(perm_parent) {
            for entry in entries.flatten() {
                let p = entry.path();
                if p.is_dir()
                    && p.file_name()
                        .and_then(|n| n.to_str())
                        .is_some_and(|n| n.starts_with("FULL_"))
                {
                    let _ =
                        runner.run(&CommandSpec::new("chattr").args(["+i", &p.to_string_lossy()]));
                }
            }
        }
    }

    if path.exists() {
        return Err(NasError::Invalid(format!(
            "fallo al eliminar el respaldo permanente '{name}'"
        )));
    }

    Ok(format!(
        "Respaldo permanente '{name}' eliminado exitosamente"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delete_permanent_rechaza_entradas_invalidas() {
        let runner = Runner::new();
        assert!(
            delete_permanent_backup(&runner, "invalido con espacios", "FULL_2026-10-30").is_err()
        );
        assert!(delete_permanent_backup(&runner, "tarea1", "../escape").is_err());
        assert!(delete_permanent_backup(&runner, "tarea1", "NOTFULL_2026").is_err());
    }

    #[test]
    fn list_permanent_backups_deduce_tag_y_fecha_correctamente() {
        let f1 = "FULL_2026-11-04_144640_INICIAL".trim_start_matches("FULL_");
        let p1: Vec<&str> = f1.split('_').collect();
        assert_eq!(p1.len(), 3);
        let (d1, t1) = if p1.len() >= 3 {
            (format!("{}_{}", p1[0], p1[1]), Some(p1[2..].join("_")))
        } else {
            (p1[0].to_string(), None)
        };
        assert_eq!(d1, "2026-11-04_144640");
        assert_eq!(t1.as_deref(), Some("INICIAL"));

        let f2 = "FULL_2026-12-01_020000_MENSUAL".trim_start_matches("FULL_");
        let p2: Vec<&str> = f2.split('_').collect();
        let (d2, t2) = if p2.len() >= 3 {
            (format!("{}_{}", p2[0], p2[1]), Some(p2[2..].join("_")))
        } else {
            (p2[0].to_string(), None)
        };
        assert_eq!(d2, "2026-12-01_020000");
        assert_eq!(t2.as_deref(), Some("MENSUAL"));

        let f3 = "FULL_2026-10-30_120000".trim_start_matches("FULL_");
        let p3: Vec<&str> = f3.split('_').collect();
        let (d3, t3) = if p3.len() >= 3 {
            (format!("{}_{}", p3[0], p3[1]), Some(p3[2..].join("_")))
        } else if p3.len() == 2 && p3[1].len() == 6 && p3[1].chars().all(|c| c.is_ascii_digit()) {
            (format!("{}_{}", p3[0], p3[1]), None)
        } else {
            (p3[0].to_string(), None)
        };
        assert_eq!(d3, "2026-10-30_120000");
        assert_eq!(t3, None);
    }

    #[test]
    fn limpia_directorios_freezing_interrumpidos_sin_tocar_full() {
        let dir = std::env::temp_dir().join(format!(
            "nas_perm_clean_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        let freezing = dir.join(".freezing_FULL_2026-11-20_100000_INICIAL");
        let full = dir.join("FULL_2026-11-19_100000_INICIAL");
        std::fs::create_dir_all(&freezing).expect("freezing");
        std::fs::create_dir_all(&full).expect("full");
        std::fs::write(freezing.join("a"), b"123").expect("write");

        clean_stale_freezing_dirs(&dir);
        assert!(!freezing.exists());
        assert!(full.is_dir());

        let (files, bytes) = dir_stats(&dir);
        assert_eq!(files, 0);
        assert_eq!(bytes, 0);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn cache_de_estadisticas_permanentes_persiste_y_reutiliza_resultado() {
        let dir = std::env::temp_dir().join(format!(
            "nas_perm_cache_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        let full = dir.join("FULL_2026-11-04_230001_INICIAL");
        std::fs::create_dir_all(&full).unwrap();
        std::fs::write(full.join("doc.bin"), vec![0xAAu8; 2048]).unwrap();

        let (files, bytes) = cached_dir_stats(&full);
        assert_eq!(files, 1);
        assert_eq!(bytes, 2048);

        let cache_file = dir.join("permanent_backup_stats.json");
        let mut map = HashMap::new();
        map.insert(full.to_string_lossy().to_string(), (files, bytes));
        persist_permanent_stats_to_file(&cache_file, &map);
        let reloaded = load_permanent_stats_from_file(&cache_file);
        assert_eq!(
            reloaded.get(&full.to_string_lossy().to_string()),
            Some(&(1, 2048))
        );

        let _ = std::fs::remove_dir_all(&dir);
    }
}
