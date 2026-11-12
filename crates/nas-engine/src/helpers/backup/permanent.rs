//! Gestión de respaldos permanentes inmutables (Golden Snapshots).

use std::path::{Path, PathBuf};
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

fn dir_stats(path: &Path) -> (u64, u64) {
    let mut files = 0u64;
    let mut bytes = 0u64;
    if let Ok(entries) = std::fs::read_dir(path) {
        for entry in entries.flatten() {
            if let Ok(meta) = entry.metadata() {
                if meta.is_file() {
                    files += 1;
                    bytes += meta.len();
                } else if meta.is_dir() {
                    let (sub_files, sub_bytes) = dir_stats(&entry.path());
                    files += sub_files;
                    bytes += sub_bytes;
                }
            }
        }
    }
    (files, bytes)
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
    let _ = runner.run(&CommandSpec::new("chown").args([
        "-R",
        "root:grp_samba",
        &perm_task_dir.to_string_lossy(),
    ]));
    let _ = runner.run(&CommandSpec::new("chmod").args(["2770", &perm_task_dir.to_string_lossy()]));
    let _ = runner.run(&CommandSpec::new("chattr").args(["-i", &perm_task_dir.to_string_lossy()]));

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

    let target_path = perm_task_dir.join(&folder_name);

    // Levantar temporalmente inmutabilidad en el snapshot origen para permitir crear enlaces duros
    let _ = runner.run(&CommandSpec::new("chattr").args(["-i", &latest.to_string_lossy()]));

    // Clonación instantánea mediante Hardlinks (costo 0 bytes extras)
    let cp_res = runner.run(
        &CommandSpec::new("cp")
            .args([
                "-al",
                &latest.to_string_lossy(),
                &target_path.to_string_lossy(),
            ])
            .timeout(Duration::from_secs(120)),
    )?;

    // Restaurar inmutabilidad en el contenedor origen (sin -R para permitir deduplicación)
    let _ = runner.run(&CommandSpec::new("chattr").args(["+i", &latest.to_string_lossy()]));

    if !cp_res.is_success() {
        return Err(NasError::Invalid(format!(
            "error al congelar snapshot vía hardlinks: {}",
            cp_res.stderr
        )));
    }

    let (files_count, size_bytes) = dir_stats(&target_path);

    let info = PermanentBackupInfo {
        task: id.to_string(),
        name: folder_name.clone(),
        path: target_path.to_string_lossy().to_string(),
        date: date_str,
        tag: clean_tag,
        size_bytes,
        files_count,
    };

    // Guardar metadatos en el snapshot antes de bloquear escritura
    let meta_file = target_path.join(".freeze_meta.json");
    if let Ok(meta_json) = serde_json::to_string_pretty(&info) {
        let _ = std::fs::write(&meta_file, meta_json);
    }

    // Inmutabilidad: remover permisos de escritura y asegurar con chattr solo el contenedor (sin -R)
    let _ =
        runner.run(&CommandSpec::new("chmod").args(["-R", "a-w", &target_path.to_string_lossy()]));
    let _ = runner.run(&CommandSpec::new("chattr").args(["+i", &target_path.to_string_lossy()]));

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

                let (files_count, size_bytes) = dir_stats(&bkp_path);
                result.push(PermanentBackupInfo {
                    task: task_name.clone(),
                    name: folder_name,
                    path: bkp_path.to_string_lossy().to_string(),
                    date: date_str,
                    tag,
                    size_bytes,
                    files_count,
                });
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

    let _ = runner.run(&CommandSpec::new("chattr").args(["-R", "-i", &path.to_string_lossy()]));
    let _ = runner.run(&CommandSpec::new("chmod").args(["-R", "u+w", &path.to_string_lossy()]));
    let mut rm_success = false;
    if let Ok(res) = runner.run(&CommandSpec::new("rm").args(["-rf", &path.to_string_lossy()])) {
        if res.is_success() {
            rm_success = true;
        }
    }
    if !rm_success {
        let _ = std::fs::remove_dir_all(&path);
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
}
