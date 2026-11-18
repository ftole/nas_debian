//! Reparación de runners y deduplicación segura (hardlinks / Btrfs reflink) de respaldos.
//!
//! Corrige el fallo histórico provocado por `chmod -R a-w` sobre los snapshots
//! completados (que impedía a `rsync --link-dest` enlazar archivos sin cambios)
//! y recupera el espacio duplicado en producción de forma idempotente y atómica,
//! sin alterar el contenido ni los metadatos de los respaldos existentes.

use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use nas_core::{MemoryPressure, NasError, NasResult, ResourceBudget};
use nas_exec::{CommandSpec, Runner};
use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::lock::TaskLock;

/// Nombre del archivo marcador de migración completada (una sola vez).
pub const DEDUP_MIGRATION_MARKER: &str = "migrations/backup_dedup_v1.done";

const PARTIAL_SAMPLE_BYTES: usize = 4096;
const THROTTLE_CHECK_OPS: usize = 512;
const THROTTLE_CHECK_INTERVAL: Duration = Duration::from_millis(250);
const THROTTLE_SHA_CHUNK_OPS: usize = 256;
const THROTTLE_PAUSE_OVERLOADED: Duration = Duration::from_millis(25);
const THROTTLE_PAUSE_CRITICAL: Duration = Duration::from_millis(50);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PressureAction {
    Continue,
    PauseOverloaded,
    PauseCritical,
}

struct DedupThrottle<F>
where
    F: FnMut() -> (MemoryPressure, bool),
{
    ops_since_check: usize,
    last_check: Instant,
    check_every_ops: usize,
    check_interval: Duration,
    probe: F,
}

impl<F> DedupThrottle<F>
where
    F: FnMut() -> (MemoryPressure, bool),
{
    fn new(check_every_ops: usize, check_interval: Duration, probe: F) -> Self {
        Self {
            ops_since_check: 0,
            last_check: Instant::now(),
            check_every_ops: check_every_ops.max(1),
            check_interval,
            probe,
        }
    }

    fn tick_with_sleep(&mut self, units: usize, sleep_fn: impl FnOnce(Duration)) -> PressureAction {
        self.ops_since_check = self.ops_since_check.saturating_add(units);
        if self.ops_since_check < self.check_every_ops
            && self.last_check.elapsed() < self.check_interval
        {
            return PressureAction::Continue;
        }
        self.ops_since_check = 0;
        self.last_check = Instant::now();

        let (pressure, overloaded) = (self.probe)();
        if pressure == MemoryPressure::Critical {
            sleep_fn(THROTTLE_PAUSE_CRITICAL);
            PressureAction::PauseCritical
        } else if overloaded {
            sleep_fn(THROTTLE_PAUSE_OVERLOADED);
            PressureAction::PauseOverloaded
        } else {
            PressureAction::Continue
        }
    }

    fn tick(&mut self, units: usize) -> PressureAction {
        self.tick_with_sleep(units, std::thread::sleep)
    }
}

fn live_pressure_probe() -> (MemoryPressure, bool) {
    let budget = ResourceBudget::detect();
    (budget.resources.memory_pressure, budget.is_overloaded())
}

/// Par de directorios de respaldos (históricos y permanentes) en una misma unidad.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BackupRootPair {
    /// Directorio raíz de snapshots rotativos (`BACKUPS_HISTORICOS`).
    pub hist_root: PathBuf,
    /// Directorio raíz de respaldos permanentes (`BACKUPS_PERMANENTES`).
    pub perm_root: PathBuf,
}

/// Resultado de la reparación de scripts `/usr/local/bin/backup_*.sh`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct RunnerRepairReport {
    /// Runners inspeccionados.
    pub inspected: usize,
    /// Runners actualizados con éxito.
    pub updated: Vec<String>,
    /// Runners que ya estaban al día.
    pub already_ok: usize,
    /// Errores por runner (no fatales; el runner original permanece intacto).
    pub errors: Vec<String>,
}

/// Opciones para la deduplicación de respaldos existentes.
#[derive(Debug, Clone)]
pub struct DeduplicateOptions {
    /// Pares de raíces `(BACKUPS_HISTORICOS, BACKUPS_PERMANENTES)` a procesar.
    /// Si está vacío, descubre `/srv/nas` y `/srv/nas/unidades/*`.
    pub roots: Vec<BackupRootPair>,
    /// Directorio donde residen `backup_<id>.sh`.
    pub runner_dir: PathBuf,
    /// Directorio de bloqueos `flock` por tarea.
    pub lock_dir: PathBuf,
    /// Directorio de bitácoras de respaldo.
    pub log_dir: PathBuf,
    /// Filtrar por una tarea específica.
    pub task_filter: Option<String>,
    /// Simula sin modificar permisos ni crear hardlinks/reflinks.
    pub dry_run: bool,
    /// Adquiere `backup_<id>.lock` antes de tocar una tarea.
    pub use_flock: bool,
}

impl Default for DeduplicateOptions {
    fn default() -> Self {
        Self {
            roots: Vec::new(),
            runner_dir: PathBuf::from("/usr/local/bin"),
            lock_dir: PathBuf::from("/var/lock"),
            log_dir: PathBuf::from("/srv/nas/LOGS_BACKUP"),
            task_filter: None,
            dry_run: false,
            use_flock: true,
        }
    }
}

/// Métricas de deduplicación para una tarea individual.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct TaskDedupSummary {
    /// Identificador de la tarea.
    pub task: String,
    /// `true` si se omitió porque había una copia activa ejecutándose.
    pub skipped_locked: bool,
    /// Número de snapshots y respaldos `FULL_*` examinados.
    pub snapshots_scanned: usize,
    /// Archivos regulares analizados.
    pub files_scanned: u64,
    /// Permisos de escritura restaurados en archivos/directorios afectados por `a-w`.
    pub permissions_repaired: u64,
    /// Archivos reemplazados atómicamente mediante hardlink.
    pub hardlinks_created: u64,
    /// Archivos deduplicados mediante Btrfs `--reflink=always` (cruce de subvolumen).
    pub reflinks_created: u64,
    /// Bytes físicos recuperados estimados.
    pub bytes_reclaimed: u64,
    /// Errores no fatales registrados durante la tarea.
    pub errors: Vec<String>,
}

/// Resumen global de deduplicación.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct DeduplicateReport {
    /// Modo simulación (`--dry-run`).
    pub dry_run: bool,
    /// Detalle por tarea procesada.
    pub tasks: Vec<TaskDedupSummary>,
    /// Total de tareas omitidas por bloqueo activo.
    pub tasks_skipped: usize,
    /// Total de hardlinks creados (o detectados en dry-run).
    pub total_hardlinks: u64,
    /// Total de reflinks Btrfs creados.
    pub total_reflinks: u64,
    /// Total de permisos reparados.
    pub total_permissions_repaired: u64,
    /// Total de bytes recuperados (o recuperables en dry-run).
    pub total_bytes_reclaimed: u64,
    /// Errores acumulados.
    pub errors: Vec<String>,
}

/// Opciones de migración única de despliegue/actualización.
#[derive(Debug, Clone)]
pub struct MigrationOptions {
    /// Directorio de estado persistente (`/var/lib/nas`).
    pub state_dir: PathBuf,
    /// Opciones de deduplicación.
    pub dedup: DeduplicateOptions,
    /// Fuerza la re-ejecución aunque ya exista el marcador.
    pub force: bool,
}

impl Default for MigrationOptions {
    fn default() -> Self {
        Self {
            state_dir: PathBuf::from("/var/lib/nas"),
            dedup: DeduplicateOptions::default(),
            force: false,
        }
    }
}

/// Resultado de la migración automática al desplegar o actualizar.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct MigrationReport {
    /// `true` si la migración ya se había completado previamente y se omitió.
    pub already_completed: bool,
    /// Resultado de la reparación de runners.
    pub runners: RunnerRepairReport,
    /// Resultado de la deduplicación (presente si se ejecutó).
    pub dedup: Option<DeduplicateReport>,
    /// Ruta del marcador persistido al completar sin errores ni tareas omitidas.
    pub marker_path: Option<String>,
}

/// Descubre automáticamente las raíces de respaldo en `/srv/nas` y `/srv/nas/unidades/*`.
pub fn discover_backup_roots() -> Vec<BackupRootPair> {
    let mut pairs = Vec::new();
    let main_hist = PathBuf::from("/srv/nas/BACKUPS_HISTORICOS");
    let main_perm = PathBuf::from("/srv/nas/BACKUPS_PERMANENTES");
    if main_hist.is_dir() || main_perm.is_dir() {
        pairs.push(BackupRootPair {
            hist_root: main_hist,
            perm_root: main_perm,
        });
    }

    if let Ok(entries) = fs::read_dir("/srv/nas/unidades") {
        let mut units: Vec<PathBuf> = entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.is_dir())
            .collect();
        units.sort();
        for unit in units {
            let hist = unit.join("BACKUPS_HISTORICOS");
            let perm = unit.join("BACKUPS_PERMANENTES");
            if hist.is_dir() || perm.is_dir() {
                pairs.push(BackupRootPair {
                    hist_root: hist,
                    perm_root: perm,
                });
            }
        }
    }
    pairs
}

/// Parchea de forma idempotente el contenido de un script runner `backup_<id>.sh`.
pub fn patch_runner_content(content: &str) -> String {
    let mut out = Vec::new();
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed == "chmod -R a-w \"$FINAL\" 2>/dev/null || true"
            || trimmed == "chmod -R a-w \"$PERM_DEST\" 2>/dev/null || true"
            || trimmed == "chmod -R a-w \"$FINAL\""
            || trimmed == "chmod -R a-w \"$PERM_DEST\""
        {
            continue;
        }

        let mut updated = line.to_string();
        if updated.contains("chattr -R +i \"$FINAL\"") {
            updated = updated.replace("chattr -R +i \"$FINAL\"", "chattr +i \"$FINAL\"");
        }
        if updated.contains("chattr -R +i \"$PERM_DEST\"") {
            updated = updated.replace("chattr -R +i \"$PERM_DEST\"", "chattr +i \"$PERM_DEST\"");
        }
        if updated.contains("chattr -R +i \"$PERM_DIR\"") {
            updated = updated.replace(
                "chattr -R +i \"$PERM_DIR\"",
                "find \"$PERM_DIR\" -mindepth 1 -maxdepth 1 -type d -name 'FULL_*' -exec chattr +i {} +",
            );
        }
        if updated.contains("chattr -R +i {}") {
            updated = updated.replace("chattr -R +i {}", "chattr +i {}");
        }
        if trimmed == "chmod -R u+w \"$OLD\" 2>/dev/null || true"
            || trimmed == "chmod -R u+w \"$OLD\""
        {
            let indent: String = line.chars().take_while(|c| c.is_whitespace()).collect();
            updated =
                format!("{indent}find \"$OLD\" -type d -exec chmod u+w {{}} + 2>/dev/null || true");
        }
        if updated.contains("if cp -al \"$FINAL\" \"$PERM_DEST\" 2>/dev/null; then") {
            updated = updated.replace(
                "if cp -al \"$FINAL\" \"$PERM_DEST\" 2>/dev/null; then",
                "if cp -al \"$FINAL\" \"$PERM_DEST\" 2>/dev/null || cp -a --reflink=always \"$FINAL\" \"$PERM_DEST\" 2>/dev/null; then",
            );
        }
        out.push(updated);
    }

    let mut result = out.join("\n");
    if content.ends_with('\n') && !result.ends_with('\n') {
        result.push('\n');
    }
    result
}

/// Actualiza todos los scripts `backup_*.sh` en `bin_dir` validando sintaxis con `bash -n`.
pub fn repair_runners(runner: &Runner, bin_dir: &Path) -> NasResult<RunnerRepairReport> {
    let mut report = RunnerRepairReport::default();
    if !bin_dir.is_dir() {
        return Ok(report);
    }

    let mut entries: Vec<PathBuf> = fs::read_dir(bin_dir)
        .map_err(|e| NasError::io(bin_dir, e))?
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.is_file()
                && p.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.starts_with("backup_") && n.ends_with(".sh"))
        })
        .collect();
    entries.sort();

    for path in entries {
        report.inspected += 1;
        let original = match fs::read_to_string(&path) {
            Ok(txt) => txt,
            Err(e) => {
                report
                    .errors
                    .push(format!("{}: no se pudo leer: {e}", path.display()));
                continue;
            }
        };

        let patched = patch_runner_content(&original);
        if patched == original {
            report.already_ok += 1;
            continue;
        }

        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        let tmp = bin_dir.join(format!(".{name}.repair.{}", std::process::id()));
        if let Err(e) = fs::write(&tmp, patched.as_bytes()) {
            report.errors.push(format!(
                "{}: no se pudo escribir temporal: {e}",
                path.display()
            ));
            continue;
        }

        let check = runner.run(
            &CommandSpec::new("bash")
                .args(["-n", &tmp.to_string_lossy()])
                .timeout(Duration::from_secs(15)),
        );
        match check {
            Ok(out) if out.is_success() => {}
            Ok(out) => {
                let _ = fs::remove_file(&tmp);
                report.errors.push(format!(
                    "{}: sintaxis inválida tras parchear: {}",
                    path.display(),
                    out.stderr.trim()
                ));
                continue;
            }
            Err(e) => {
                let _ = fs::remove_file(&tmp);
                report.errors.push(format!(
                    "{}: fallo al validar sintaxis: {e}",
                    path.display()
                ));
                continue;
            }
        }

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(&tmp, fs::Permissions::from_mode(0o755));
        }

        if let Err(e) = fs::rename(&tmp, &path) {
            let _ = fs::remove_file(&tmp);
            report.errors.push(format!(
                "{}: fallo al reemplazar atómicamente: {e}",
                path.display()
            ));
            continue;
        }

        report.updated.push(path.display().to_string());
    }

    Ok(report)
}

#[derive(Debug, Clone)]
struct FileMetaRecord {
    container_idx: usize,
    path: PathBuf,
    dev: u64,
    ino: u64,
    size: u64,
    mtime_sec: i64,
    mtime_nsec: i64,
    uid: u32,
    gid: u32,
    mode: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ContainerFsInfo {
    fsid: String,
    fstype: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct MetaKey {
    uid: u32,
    gid: u32,
    mode: u32,
    mtime_sec: i64,
    mtime_nsec: i64,
}

impl FileMetaRecord {
    fn meta_key(&self) -> MetaKey {
        MetaKey {
            uid: self.uid,
            gid: self.gid,
            mode: self.mode,
            mtime_sec: self.mtime_sec,
            mtime_nsec: self.mtime_nsec,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct PartialSig {
    head: Vec<u8>,
    tail: Vec<u8>,
}

fn task_is_cifs(runner_dir: &Path, task_id: &str) -> bool {
    let runner_path = runner_dir.join(format!("backup_{task_id}.sh"));
    let Ok(content) = fs::read_to_string(&runner_path) else {
        return false;
    };
    content.lines().any(|line| {
        let trimmed = line.trim();
        trimmed == "SOURCE_TYPE='cifs'"
            || trimmed == "SOURCE_TYPE=\"cifs\""
            || trimmed == "SOURCE_TYPE=cifs"
    })
}

fn collect_task_dirs(pair: &BackupRootPair, filter: Option<&str>) -> Vec<String> {
    let mut ids = HashSet::new();
    for root in [&pair.hist_root, &pair.perm_root] {
        let Ok(entries) = fs::read_dir(root) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                if name.starts_with('.') {
                    continue;
                }
                if let Some(f) = filter {
                    if !f.trim().is_empty() && name != f.trim() {
                        continue;
                    }
                }
                ids.insert(name.to_string());
            }
        }
    }
    let mut sorted: Vec<String> = ids.into_iter().collect();
    sorted.sort();
    sorted
}

/// Lista los directorios completados de una tarea en orden de prioridad canónica:
/// primero `snapshot_*` (más reciente primero), luego `FULL_*` (más reciente primero).
/// Ignora `.inprogress_*` y cualquier directorio temporal.
fn list_completed_containers(hist_task_dir: &Path, perm_task_dir: &Path) -> Vec<PathBuf> {
    let mut snaps = Vec::new();
    if let Ok(entries) = fs::read_dir(hist_task_dir) {
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_dir()
                && p.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.starts_with("snapshot_"))
            {
                snaps.push(p);
            }
        }
    }
    snaps.sort_by(|a, b| b.file_name().cmp(&a.file_name()));

    let mut perms = Vec::new();
    if let Ok(entries) = fs::read_dir(perm_task_dir) {
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_dir()
                && p.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.starts_with("FULL_"))
            {
                perms.push(p);
            }
        }
    }
    perms.sort_by(|a, b| b.file_name().cmp(&a.file_name()));

    snaps.extend(perms);
    snaps
}

#[cfg(unix)]
fn repair_container_permissions<F>(
    root: &Path,
    is_cifs: bool,
    dry_run: bool,
    errors: &mut Vec<String>,
    throttle: &mut DedupThrottle<F>,
) -> u64
where
    F: FnMut() -> (MemoryPressure, bool),
{
    use std::os::unix::fs::PermissionsExt;

    let mut repaired = 0u64;
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            throttle.tick(1);
            let path = entry.path();
            let Ok(meta) = fs::symlink_metadata(&path) else {
                continue;
            };
            let ft = meta.file_type();
            if ft.is_symlink() || (!ft.is_file() && !ft.is_dir()) {
                continue;
            }

            let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if !dry_run && ft.is_file() && file_name.starts_with(".nas_dedup_") {
                let _ = fs::remove_file(&path);
                continue;
            }

            let is_freeze_meta = ft.is_file() && file_name == ".freeze_meta.json";
            let mode = meta.permissions().mode() & 0o7777;
            if !is_freeze_meta && (mode & 0o200) == 0 {
                let new_mode = if is_cifs { mode | 0o222 } else { mode | 0o200 };
                if !dry_run {
                    if let Err(e) = fs::set_permissions(&path, fs::Permissions::from_mode(new_mode))
                    {
                        errors.push(format!("{}: chmod fallo: {e}", path.display()));
                    } else {
                        repaired += 1;
                    }
                } else {
                    repaired += 1;
                }
            }

            if ft.is_dir() {
                stack.push(path);
            }
        }
    }

    if let Ok(meta) = fs::symlink_metadata(root) {
        let mode = meta.permissions().mode() & 0o7777;
        if (mode & 0o200) == 0 {
            let new_mode = if is_cifs { mode | 0o222 } else { mode | 0o200 };
            if !dry_run {
                if let Err(e) = fs::set_permissions(root, fs::Permissions::from_mode(new_mode)) {
                    errors.push(format!("{}: chmod raíz fallo: {e}", root.display()));
                } else {
                    repaired += 1;
                }
            } else {
                repaired += 1;
            }
        }
    }

    repaired
}

#[cfg(not(unix))]
fn repair_container_permissions<F>(
    _root: &Path,
    _is_cifs: bool,
    _dry_run: bool,
    _errors: &mut Vec<String>,
    _throttle: &mut DedupThrottle<F>,
) -> u64
where
    F: FnMut() -> (MemoryPressure, bool),
{
    0
}

#[cfg(unix)]
fn collect_files_in_container<F>(
    container_idx: usize,
    container: &Path,
    out: &mut Vec<FileMetaRecord>,
    throttle: &mut DedupThrottle<F>,
) where
    F: FnMut() -> (MemoryPressure, bool),
{
    use std::os::unix::fs::MetadataExt;

    let mut stack = vec![container.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        let mut children: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
        children.sort();
        for path in children {
            throttle.tick(1);
            let Ok(meta) = fs::symlink_metadata(&path) else {
                continue;
            };
            let ft = meta.file_type();
            if ft.is_symlink() {
                continue;
            }
            if ft.is_dir() {
                stack.push(path);
            } else if ft.is_file() {
                let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                if name == ".freeze_meta.json" || name.starts_with(".nas_dedup_") {
                    continue;
                }
                out.push(FileMetaRecord {
                    container_idx,
                    path,
                    dev: meta.dev(),
                    ino: meta.ino(),
                    size: meta.size(),
                    mtime_sec: meta.mtime(),
                    mtime_nsec: meta.mtime_nsec(),
                    uid: meta.uid(),
                    gid: meta.gid(),
                    mode: meta.mode() & 0o7777,
                });
            }
        }
    }
}

#[cfg(not(unix))]
fn collect_files_in_container<F>(
    _container_idx: usize,
    _container: &Path,
    _out: &mut Vec<FileMetaRecord>,
    _throttle: &mut DedupThrottle<F>,
) where
    F: FnMut() -> (MemoryPressure, bool),
{
}

fn query_container_fs(runner: &Runner, dir: &Path) -> Option<ContainerFsInfo> {
    let out = runner
        .run(
            &CommandSpec::new("stat")
                .args(["-f", "-c", "%i %T", &dir.to_string_lossy()])
                .timeout(Duration::from_secs(10)),
        )
        .ok()?;
    if !out.is_success() {
        return None;
    }
    let mut parts = out.stdout.split_whitespace();
    let fsid = parts.next()?.to_string();
    let fstype = parts.next()?.to_string();
    Some(ContainerFsInfo { fsid, fstype })
}

fn is_btrfs_ro_subvolume(runner: &Runner, dir: &Path) -> bool {
    let Ok(out) = runner.run(
        &CommandSpec::new("btrfs")
            .args(["property", "get", &dir.to_string_lossy(), "ro"])
            .timeout(Duration::from_secs(10)),
    ) else {
        return false;
    };
    out.is_success() && out.stdout.trim() == "ro=true"
}

fn parse_filefrag_extents(stdout: &str) -> Option<Vec<(String, String, String, bool)>> {
    let mut extents = Vec::new();
    for line in stdout.lines() {
        let trimmed = line.trim();
        let parts: Vec<&str> = trimmed.split(':').collect();
        if parts.len() < 5 {
            continue;
        }
        if parts[0].trim().parse::<u64>().is_err() {
            continue;
        }
        let logical = parts[1].trim().to_string();
        let physical = parts[2].trim().to_string();
        let length = parts[3].trim().to_string();
        let flags = parts[4..].join(":");
        let is_inline = flags.contains("inline");
        let is_shared = flags.contains("shared");
        if is_inline {
            return None;
        }
        extents.push((logical, physical, length, is_shared));
    }
    if extents.is_empty() {
        None
    } else {
        Some(extents)
    }
}

fn filefrag_extents(runner: &Runner, path: &Path) -> Option<Vec<(String, String, String, bool)>> {
    let bin = if Path::new("/usr/sbin/filefrag").is_file() {
        "/usr/sbin/filefrag"
    } else if Path::new("/sbin/filefrag").is_file() {
        "/sbin/filefrag"
    } else {
        "filefrag"
    };
    let out = runner
        .run(
            &CommandSpec::new(bin)
                .args(["-e", "-k", &path.to_string_lossy()])
                .timeout(Duration::from_secs(15)),
        )
        .ok()?;
    if !out.is_success() {
        return None;
    }
    parse_filefrag_extents(&out.stdout)
}

fn files_share_physical_extents(runner: &Runner, canon: &Path, dup: &Path) -> bool {
    let Some(ext_a) = filefrag_extents(runner, canon) else {
        return false;
    };
    let Some(ext_b) = filefrag_extents(runner, dup) else {
        return false;
    };
    if ext_a.len() != ext_b.len() {
        return false;
    }
    ext_a
        .iter()
        .zip(ext_b.iter())
        .all(|(a, b)| a.0 == b.0 && a.1 == b.1 && a.2 == b.2 && a.1 != "0..0" && (a.3 || b.3))
}

fn read_partial_sig(path: &Path, size: u64) -> std::io::Result<PartialSig> {
    let mut file = File::open(path)?;
    let head_len = (size as usize).min(PARTIAL_SAMPLE_BYTES);
    let mut head = vec![0u8; head_len];
    file.read_exact(&mut head)?;

    let tail = if size > PARTIAL_SAMPLE_BYTES as u64 {
        let tail_len = ((size - PARTIAL_SAMPLE_BYTES as u64) as usize).min(PARTIAL_SAMPLE_BYTES);
        let mut buf = vec![0u8; tail_len];
        file.seek(SeekFrom::Start(size - tail_len as u64))?;
        file.read_exact(&mut buf)?;
        buf
    } else {
        Vec::new()
    };

    Ok(PartialSig { head, tail })
}

fn sha256_file<F>(path: &Path, throttle: &mut DedupThrottle<F>) -> std::io::Result<([u8; 32], bool)>
where
    F: FnMut() -> (MemoryPressure, bool),
{
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 64 * 1024];
    let mut chunks = 0usize;
    let mut saw_critical = false;
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
        chunks += 1;
        if chunks >= THROTTLE_SHA_CHUNK_OPS {
            chunks = 0;
            if throttle.tick(1) == PressureAction::PauseCritical {
                saw_critical = true;
            }
        }
    }
    Ok((hasher.finalize().into(), saw_critical))
}

fn temp_link_path(target_file: &Path, seq: u64) -> Option<PathBuf> {
    let parent = target_file.parent()?;
    Some(parent.join(format!(".nas_dedup_{}_{}", std::process::id(), seq)))
}

enum LinkOutcome {
    Hardlinked,
    Reflinked,
}

fn replace_with_link_or_reflink(
    runner: &Runner,
    canon: &Path,
    dup: &Path,
    seq: u64,
) -> Result<LinkOutcome, String> {
    if !canon.exists() || !dup.exists() {
        return Err("origen o destino desapareció antes de enlazar".into());
    }
    let Some(tmp) = temp_link_path(dup, seq) else {
        return Err(format!("{}: sin directorio padre", dup.display()));
    };
    let _ = fs::remove_file(&tmp);

    match fs::hard_link(canon, &tmp) {
        Ok(()) => {
            if let Err(e) = fs::rename(&tmp, dup) {
                let _ = fs::remove_file(&tmp);
                return Err(format!("{}: rename falló: {e}", dup.display()));
            }
            Ok(LinkOutcome::Hardlinked)
        }
        Err(err) => {
            let is_exdev = err.raw_os_error() == Some(18);
            if !is_exdev {
                return Err(format!("{}: hard_link falló: {err}", dup.display()));
            }

            // Cruce de subvolumen Btrfs: intentar clonación CoW sin gasto de espacio.
            let cp = runner
                .run(
                    &CommandSpec::new("cp")
                        .args([
                            "-a",
                            "--reflink=always",
                            &canon.to_string_lossy(),
                            &tmp.to_string_lossy(),
                        ])
                        .timeout(Duration::from_secs(120)),
                )
                .map_err(|e| format!("{}: fallo cp --reflink=always: {e}", dup.display()))?;
            if !cp.is_success() {
                let _ = fs::remove_file(&tmp);
                return Err(format!(
                    "{}: cp --reflink=always falló: {}",
                    dup.display(),
                    cp.stderr.trim()
                ));
            }
            if let Err(e) = fs::rename(&tmp, dup) {
                let _ = fs::remove_file(&tmp);
                return Err(format!("{}: rename reflink falló: {e}", dup.display()));
            }
            Ok(LinkOutcome::Reflinked)
        }
    }
}

fn deduplicate_task(
    runner: &Runner,
    pair: &BackupRootPair,
    task_id: &str,
    options: &DeduplicateOptions,
) -> TaskDedupSummary {
    let mut throttle = DedupThrottle::new(
        THROTTLE_CHECK_OPS,
        THROTTLE_CHECK_INTERVAL,
        live_pressure_probe,
    );
    deduplicate_task_with_throttle(runner, pair, task_id, options, &mut throttle)
}

fn deduplicate_task_with_throttle<F>(
    runner: &Runner,
    pair: &BackupRootPair,
    task_id: &str,
    options: &DeduplicateOptions,
    throttle: &mut DedupThrottle<F>,
) -> TaskDedupSummary
where
    F: FnMut() -> (MemoryPressure, bool),
{
    let mut summary = TaskDedupSummary {
        task: task_id.to_string(),
        ..Default::default()
    };

    let _lock_guard = if options.use_flock {
        let lock_path = options.lock_dir.join(format!("backup_{task_id}.lock"));
        match TaskLock::acquire(&lock_path) {
            Ok(guard) => Some(guard),
            Err(_) => {
                summary.skipped_locked = true;
                return summary;
            }
        }
    } else {
        None
    };

    let hist_task_dir = pair.hist_root.join(task_id);
    let perm_task_dir = pair.perm_root.join(task_id);
    let containers = list_completed_containers(&hist_task_dir, &perm_task_dir);
    summary.snapshots_scanned = containers.len();
    if containers.is_empty() {
        return summary;
    }

    let is_cifs = task_is_cifs(&options.runner_dir, task_id);

    // 1. Levantar temporalmente inmutabilidad (y propiedad Btrfs ro si aplica) y reparar permisos `a-w`.
    let mut ro_subvols: Vec<PathBuf> = Vec::new();
    let mut container_writable = vec![true; containers.len()];
    for (idx, container) in containers.iter().enumerate() {
        if !options.dry_run {
            if is_btrfs_ro_subvolume(runner, container) {
                match runner.run(
                    &CommandSpec::new("btrfs")
                        .args([
                            "property",
                            "set",
                            &container.to_string_lossy(),
                            "ro",
                            "false",
                        ])
                        .timeout(Duration::from_secs(30)),
                ) {
                    Ok(out) if out.is_success() => {
                        ro_subvols.push(container.clone());
                    }
                    Ok(out) => {
                        container_writable[idx] = false;
                        summary.errors.push(format!(
                            "{}: no se pudo desactivar ro en subvolumen Btrfs: {}",
                            container.display(),
                            out.stderr.trim()
                        ));
                        continue;
                    }
                    Err(e) => {
                        container_writable[idx] = false;
                        summary.errors.push(format!(
                            "{}: fallo al ejecutar btrfs property set ro false: {e}",
                            container.display()
                        ));
                        continue;
                    }
                }
            }

            // Nota: `chattr -R -i` devuelve código 1 si encuentra enlaces simbólicos dentro del snapshot,
            // por lo que únicamente un fallo de ejecución o timeout es error real.
            if let Err(e) = runner.run(
                &CommandSpec::new("chattr")
                    .args(["-R", "-i", &container.to_string_lossy()])
                    .timeout(Duration::from_secs(3600)),
            ) {
                container_writable[idx] = false;
                summary.errors.push(format!(
                    "{}: chattr -R -i falló o excedió el tiempo: {e}",
                    container.display()
                ));
                continue;
            }
            let _ = runner.run(
                &CommandSpec::new("chattr")
                    .args(["-i", &container.to_string_lossy()])
                    .timeout(Duration::from_secs(30)),
            );
        }
        summary.permissions_repaired += repair_container_permissions(
            container,
            is_cifs,
            options.dry_run,
            &mut summary.errors,
            throttle,
        );
    }

    let container_fs: Vec<Option<ContainerFsInfo>> = containers
        .iter()
        .map(|c| query_container_fs(runner, c))
        .collect();

    // 2. Recolectar archivos regulares en orden canónico (newest snapshot_* primero).
    let mut all_files = Vec::new();
    for (c_idx, container) in containers.iter().enumerate() {
        if container_writable[c_idx] {
            collect_files_in_container(c_idx, container, &mut all_files, throttle);
        }
    }
    summary.files_scanned = all_files.len() as u64;

    // 3. Agrupar primero por (tamaño > 0, MetaKey exacto) en memoria para evitar leer/hashear
    // archivos cuyos metadatos nunca permitirían hardlink.
    let mut by_size_meta: HashMap<(u64, MetaKey), Vec<usize>> = HashMap::new();
    for (idx, rec) in all_files.iter().enumerate() {
        if rec.size > 0 {
            by_size_meta
                .entry((rec.size, rec.meta_key()))
                .or_default()
                .push(idx);
        }
    }

    let max_cache_entries = ResourceBudget::detect()
        .dedup_max_entries_in_memory
        .max(1024);
    let mut partial_cache: HashMap<(u64, u64), Option<PartialSig>> = HashMap::new();
    let mut sha_cache: HashMap<(u64, u64), Option<[u8; 32]>> = HashMap::new();
    let mut reclaimed_inodes: HashSet<(u64, u64)> = HashSet::new();
    let mut seq = 0u64;

    for ((size, _mk), indices) in by_size_meta {
        if throttle.tick(1) == PressureAction::PauseCritical {
            partial_cache.clear();
            sha_cache.clear();
        }
        let unique_inodes: HashSet<(u64, u64)> = indices
            .iter()
            .map(|&i| (all_files[i].dev, all_files[i].ino))
            .collect();
        if unique_inodes.len() < 2 {
            continue;
        }

        // Sub-agrupar por firma rápida (4 KiB inicio + 4 KiB final).
        let mut by_partial: HashMap<PartialSig, Vec<usize>> = HashMap::new();
        for &idx in &indices {
            let rec = &all_files[idx];
            let key = (rec.dev, rec.ino);
            let sig = if let Some(cached) = partial_cache.get(&key) {
                cached.clone()
            } else {
                let computed = read_partial_sig(&rec.path, size).ok();
                if partial_cache.len() < max_cache_entries {
                    partial_cache.insert(key, computed.clone());
                }
                computed
            };
            if throttle.tick(1) == PressureAction::PauseCritical {
                partial_cache.clear();
                sha_cache.clear();
            }
            if let Some(s) = sig {
                by_partial.entry(s).or_default().push(idx);
            }
        }

        for (_psig, p_indices) in by_partial {
            let unique_p: HashSet<(u64, u64)> = p_indices
                .iter()
                .map(|&i| (all_files[i].dev, all_files[i].ino))
                .collect();
            if unique_p.len() < 2 {
                continue;
            }

            // Verificación criptográfica completa SHA-256.
            let mut by_sha: HashMap<[u8; 32], Vec<usize>> = HashMap::new();
            for &idx in &p_indices {
                let rec = &all_files[idx];
                let key = (rec.dev, rec.ino);
                let digest = if let Some(cached) = sha_cache.get(&key) {
                    *cached
                } else {
                    let (computed, saw_critical) = match sha256_file(&rec.path, throttle) {
                        Ok((digest, saw_critical)) => (Some(digest), saw_critical),
                        Err(_) => (None, false),
                    };
                    if saw_critical {
                        partial_cache.clear();
                        sha_cache.clear();
                    } else if sha_cache.len() < max_cache_entries {
                        sha_cache.insert(key, computed);
                    }
                    computed
                };
                if throttle.tick(1) == PressureAction::PauseCritical {
                    partial_cache.clear();
                    sha_cache.clear();
                }
                if let Some(d) = digest {
                    by_sha.entry(d).or_default().push(idx);
                }
            }

            for (_sha, meta_indices) in by_sha {
                if meta_indices.len() < 2 {
                    continue;
                }
                let canon_idx = meta_indices[0];
                let canon_container_idx = all_files[canon_idx].container_idx;
                let canon_dev = all_files[canon_idx].dev;
                let canon_ino = all_files[canon_idx].ino;
                let canon_path = all_files[canon_idx].path.clone();

                for &dup_idx in &meta_indices[1..] {
                    if throttle.tick(1) == PressureAction::PauseCritical {
                        partial_cache.clear();
                        sha_cache.clear();
                    }
                    let dup = &all_files[dup_idx];
                    if (dup.dev, dup.ino) == (canon_dev, canon_ino) {
                        continue;
                    }
                    let cross_dev = dup.dev != canon_dev;
                    if cross_dev {
                        let same_btrfs_fs = match (
                            &container_fs[canon_container_idx],
                            &container_fs[dup.container_idx],
                        ) {
                            (Some(cfs), Some(dfs)) => cfs.fsid == dfs.fsid && cfs.fstype == "btrfs",
                            _ => false,
                        };
                        if !same_btrfs_fs
                            || files_share_physical_extents(runner, &canon_path, &dup.path)
                        {
                            continue;
                        }
                    }

                    if options.dry_run {
                        if cross_dev {
                            summary.reflinks_created += 1;
                        } else {
                            summary.hardlinks_created += 1;
                        }
                        if reclaimed_inodes.insert((dup.dev, dup.ino)) {
                            summary.bytes_reclaimed =
                                summary.bytes_reclaimed.saturating_add(dup.size);
                        }
                    } else {
                        seq += 1;
                        match replace_with_link_or_reflink(runner, &canon_path, &dup.path, seq) {
                            Ok(LinkOutcome::Hardlinked) => {
                                summary.hardlinks_created += 1;
                                if reclaimed_inodes.insert((dup.dev, dup.ino)) {
                                    summary.bytes_reclaimed =
                                        summary.bytes_reclaimed.saturating_add(dup.size);
                                }
                            }
                            Ok(LinkOutcome::Reflinked) => {
                                summary.reflinks_created += 1;
                                if reclaimed_inodes.insert((dup.dev, dup.ino)) {
                                    summary.bytes_reclaimed =
                                        summary.bytes_reclaimed.saturating_add(dup.size);
                                }
                            }
                            Err(err) => summary.errors.push(err),
                        }
                    }
                }
            }
        }

        // Liberar cachés por cubo `(size, MetaKey)` para mantener la RAM acotada.
        partial_cache.clear();
        sha_cache.clear();
    }

    // 4. Restaurar inmutabilidad únicamente en los contenedores raíz (sin -R ni chmod -R a-w),
    // y restaurar propiedad `ro=true` en subvolúmenes Btrfs que originalmente eran read-only.
    if !options.dry_run {
        for ro_sub in &ro_subvols {
            let _ = runner.run(
                &CommandSpec::new("btrfs")
                    .args(["property", "set", &ro_sub.to_string_lossy(), "ro", "true"])
                    .timeout(Duration::from_secs(30)),
            );
        }
        for container in &containers {
            let _ = runner.run(
                &CommandSpec::new("chattr")
                    .args(["+i", &container.to_string_lossy()])
                    .timeout(Duration::from_secs(15)),
            );
        }

        if summary.hardlinks_created > 0
            || summary.reflinks_created > 0
            || summary.permissions_repaired > 0
        {
            let log_path = options.log_dir.join(format!("backup_{task_id}.log"));
            if let Some(parent) = log_path.parent() {
                let _ = fs::create_dir_all(parent);
            }
            if let Ok(mut f) = fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&log_path)
            {
                use std::io::Write;
                let now = chrono::Local::now().format("%F %T");
                let msg = format!(
                    "[{now}] [{task_id}] [DEDUP] Recuperación completada: {} permisos reparados, {} hardlinks, {} reflinks, {} bytes recuperados\n",
                    summary.permissions_repaired,
                    summary.hardlinks_created,
                    summary.reflinks_created,
                    summary.bytes_reclaimed
                );
                let _ = f.write_all(msg.as_bytes());
            }
        }
    }

    summary
}

/// Ejecuta la reparación y deduplicación segura sobre todas las tareas encontradas.
pub fn deduplicate_backups(
    runner: &Runner,
    options: &DeduplicateOptions,
) -> NasResult<DeduplicateReport> {
    let roots = if options.roots.is_empty() {
        discover_backup_roots()
    } else {
        options.roots.clone()
    };

    let mut report = DeduplicateReport {
        dry_run: options.dry_run,
        ..Default::default()
    };

    for pair in &roots {
        let task_ids = collect_task_dirs(pair, options.task_filter.as_deref());
        for task_id in task_ids {
            let (pressure, overloaded) = live_pressure_probe();
            if pressure == MemoryPressure::Critical {
                std::thread::sleep(THROTTLE_PAUSE_CRITICAL);
            } else if overloaded {
                std::thread::sleep(THROTTLE_PAUSE_OVERLOADED);
            }
            let summary = deduplicate_task(runner, pair, &task_id, options);
            if summary.skipped_locked {
                report.tasks_skipped += 1;
            }
            report.total_hardlinks = report
                .total_hardlinks
                .saturating_add(summary.hardlinks_created);
            report.total_reflinks = report
                .total_reflinks
                .saturating_add(summary.reflinks_created);
            report.total_permissions_repaired = report
                .total_permissions_repaired
                .saturating_add(summary.permissions_repaired);
            report.total_bytes_reclaimed = report
                .total_bytes_reclaimed
                .saturating_add(summary.bytes_reclaimed);
            report.errors.extend(summary.errors.iter().cloned());
            report.tasks.push(summary);
        }
    }

    Ok(report)
}

/// Ejecuta la migración de reparación + deduplicación una sola vez de forma idempotente.
pub fn run_dedup_migration_if_needed(
    runner: &Runner,
    options: &MigrationOptions,
) -> NasResult<MigrationReport> {
    let marker = options.state_dir.join(DEDUP_MIGRATION_MARKER);
    let runners = repair_runners(runner, &options.dedup.runner_dir)?;

    if !options.force && marker.is_file() {
        return Ok(MigrationReport {
            already_completed: true,
            runners,
            dedup: None,
            marker_path: Some(marker.display().to_string()),
        });
    }

    let _migration_lock = if options.dedup.use_flock {
        let lock_path = options.dedup.lock_dir.join("nas-backup-dedup.lock");
        Some(TaskLock::acquire(&lock_path)?)
    } else {
        None
    };

    // Re-comprobar tras adquirir el bloqueo por si otra instancia acaba de completarla.
    if !options.force && marker.is_file() {
        return Ok(MigrationReport {
            already_completed: true,
            runners,
            dedup: None,
            marker_path: Some(marker.display().to_string()),
        });
    }

    let dedup = deduplicate_backups(runner, &options.dedup)?;
    let mut marker_written = None;

    if !options.dedup.dry_run
        && dedup.tasks_skipped == 0
        && dedup.errors.is_empty()
        && runners.errors.is_empty()
    {
        if let Some(parent) = marker.parent() {
            let _ = fs::create_dir_all(parent);
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = fs::set_permissions(parent, fs::Permissions::from_mode(0o755));
            }
        }
        let completed_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let payload = serde_json::json!({
            "completed_at_epoch": completed_at,
            "runners_updated": runners.updated,
            "total_hardlinks": dedup.total_hardlinks,
            "total_reflinks": dedup.total_reflinks,
            "total_permissions_repaired": dedup.total_permissions_repaired,
            "total_bytes_reclaimed": dedup.total_bytes_reclaimed,
        });
        if let Ok(json_txt) = serde_json::to_string_pretty(&payload) {
            if fs::write(&marker, format!("{json_txt}\n")).is_ok() {
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    let _ = fs::set_permissions(&marker, fs::Permissions::from_mode(0o644));
                }
                marker_written = Some(marker.display().to_string());
            }
        }
    }

    Ok(MigrationReport {
        already_completed: false,
        runners,
        dedup: Some(dedup),
        marker_path: marker_written,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_root(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let dir =
            std::env::temp_dir().join(format!("nas-dedup-{name}-{}-{nanos}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn patch_runner_content_elimina_chmod_aw_y_conserva_chattr_contenedor() {
        let old = r#"#!/bin/bash
if btrfs subvolume show "$FINAL" >/dev/null 2>&1; then btrfs property set "$FINAL" ro true 2>/dev/null || true; fi
chmod -R a-w "$FINAL" 2>/dev/null || true
if cp -al "$FINAL" "$PERM_DEST" 2>/dev/null; then
    chmod -R a-w "$PERM_DEST" 2>/dev/null || true
    chattr -R +i "$PERM_DEST" 2>/dev/null || true
fi
chattr -R +i "$FINAL" 2>/dev/null || true
"#;
        let patched = patch_runner_content(old);
        assert!(!patched.contains("chmod -R a-w"));
        assert!(!patched.contains("chattr -R +i"));
        assert!(patched.contains("chattr +i \"$FINAL\""));
        assert!(patched.contains("chattr +i \"$PERM_DEST\""));
        assert!(patched.contains("--reflink=always"));
        // Idempotente en segunda pasada.
        assert_eq!(patch_runner_content(&patched), patched);
    }

    #[test]
    fn repair_runners_actualiza_solo_runners_obsoletos_y_valida_sintaxis() {
        let dir = temp_root("runners");
        let old_runner = dir.join("backup_cifs1.sh");
        let new_runner = dir.join("backup_ok.sh");
        fs::write(
            &old_runner,
            "#!/bin/bash\nchmod -R a-w \"$FINAL\" 2>/dev/null || true\nchattr +i \"$FINAL\" 2>/dev/null || true\n",
        )
        .unwrap();
        fs::write(
            &new_runner,
            "#!/bin/bash\nchattr +i \"$FINAL\" 2>/dev/null || true\n",
        )
        .unwrap();

        let runner = Runner::new();
        let rep = repair_runners(&runner, &dir).unwrap();
        assert_eq!(rep.inspected, 2);
        assert_eq!(rep.updated.len(), 1);
        assert_eq!(rep.already_ok, 1);
        assert!(rep.errors.is_empty());

        let rep2 = repair_runners(&runner, &dir).unwrap();
        assert_eq!(rep2.updated.len(), 0);
        assert_eq!(rep2.already_ok, 2);

        let _ = fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn deduplica_snapshots_y_full_preservando_contenido_y_permisos() {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};

        let root = temp_root("dedup");
        let hist = root.join("BACKUPS_HISTORICOS");
        let perm = root.join("BACKUPS_PERMANENTES");
        let runner_dir = root.join("bin");
        let lock_dir = root.join("lock");
        let log_dir = root.join("logs");
        fs::create_dir_all(&runner_dir).unwrap();
        fs::create_dir_all(&lock_dir).unwrap();
        fs::create_dir_all(&log_dir).unwrap();

        fs::write(
            runner_dir.join("backup_t1.sh"),
            "#!/bin/bash\nSOURCE_TYPE='cifs'\n",
        )
        .unwrap();

        let s1 = hist.join("t1/snapshot_2026-11-10_020000");
        let s2 = hist.join("t1/snapshot_2026-11-11_020000");
        let full = perm.join("t1/FULL_2026-11-01_020000_MENSUAL");
        let inprog = hist.join("t1/.inprogress_2026-11-12_020000");
        for d in [&s1, &s2, &full, &inprog] {
            fs::create_dir_all(d.join("sub")).unwrap();
        }

        let payload_same = vec![b'A'; 32 * 1024];
        let payload_diff = vec![b'B'; 32 * 1024];
        for p in [
            s1.join("sub/data.bin"),
            s2.join("sub/data.bin"),
            full.join("sub/data.bin"),
            inprog.join("sub/data.bin"),
        ] {
            fs::write(&p, &payload_same).unwrap();
        }
        fs::write(s2.join("sub/only_s2.bin"), &payload_diff).unwrap();

        // Igualar mtime en s1, s2, full (simulando copias idénticas separadas por culpa de chmod a-w).
        let fixed_time = SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000);
        for p in [
            s1.join("sub/data.bin"),
            s2.join("sub/data.bin"),
            full.join("sub/data.bin"),
        ] {
            let f = File::options().write(true).open(&p).unwrap();
            f.set_modified(fixed_time).unwrap();
            fs::set_permissions(&p, fs::Permissions::from_mode(0o555)).unwrap();
        }
        // También marcar directorios como 0555 (resultado de chmod -R a-w).
        fs::set_permissions(s1.join("sub"), fs::Permissions::from_mode(0o555)).unwrap();
        fs::set_permissions(s2.join("sub"), fs::Permissions::from_mode(0o555)).unwrap();
        fs::set_permissions(full.join("sub"), fs::Permissions::from_mode(0o555)).unwrap();

        let opts = DeduplicateOptions {
            roots: vec![BackupRootPair {
                hist_root: hist.clone(),
                perm_root: perm.clone(),
            }],
            runner_dir: runner_dir.clone(),
            lock_dir: lock_dir.clone(),
            log_dir: log_dir.clone(),
            task_filter: None,
            dry_run: true,
            use_flock: true,
        };
        let dry = deduplicate_backups(&Runner::new(), &opts).unwrap();
        assert_eq!(dry.total_hardlinks, 2);
        assert_eq!(dry.total_bytes_reclaimed, 64 * 1024);

        // Verificar que dry-run no modificó inodos ni permisos.
        assert_ne!(
            fs::metadata(s1.join("sub/data.bin")).unwrap().ino(),
            fs::metadata(s2.join("sub/data.bin")).unwrap().ino()
        );
        assert_eq!(
            fs::metadata(s2.join("sub/data.bin"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o555
        );

        let real_opts = DeduplicateOptions {
            dry_run: false,
            ..opts
        };
        let res = deduplicate_backups(&Runner::new(), &real_opts).unwrap();
        assert!(
            res.errors.is_empty(),
            "errores inesperados: {:?}",
            res.errors
        );
        assert_eq!(res.total_hardlinks, 2);
        assert_eq!(res.total_bytes_reclaimed, 64 * 1024);

        // Todos apuntan al mismo inodo canónico (s2, el snapshot más reciente) y conservan contenido y 0777.
        let ino_s2 = fs::metadata(s2.join("sub/data.bin")).unwrap().ino();
        let ino_s1 = fs::metadata(s1.join("sub/data.bin")).unwrap().ino();
        let ino_full = fs::metadata(full.join("sub/data.bin")).unwrap().ino();
        let ino_inprog = fs::metadata(inprog.join("sub/data.bin")).unwrap().ino();
        assert_eq!(ino_s1, ino_s2);
        assert_eq!(ino_full, ino_s2);
        assert_ne!(ino_inprog, ino_s2, ".inprogress_* no debe tocarse");
        assert_eq!(
            fs::metadata(s2.join("sub/data.bin"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o777
        );
        assert_eq!(fs::read(full.join("sub/data.bin")).unwrap(), payload_same);
        assert_eq!(fs::read(s2.join("sub/only_s2.bin")).unwrap(), payload_diff);

        // Segunda ejecución es idempotente (0 nuevos hardlinks).
        let res2 = deduplicate_backups(&Runner::new(), &real_opts).unwrap();
        assert_eq!(res2.total_hardlinks, 0);
        assert_eq!(res2.total_bytes_reclaimed, 0);

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn migracion_unica_crea_marcador_y_no_se_repite() {
        let root = temp_root("migration");
        let state_dir = root.join("state");
        let runner_dir = root.join("bin");
        let lock_dir = root.join("lock");
        let hist = root.join("BACKUPS_HISTORICOS");
        let perm = root.join("BACKUPS_PERMANENTES");
        fs::create_dir_all(&runner_dir).unwrap();
        fs::create_dir_all(&lock_dir).unwrap();
        fs::create_dir_all(hist.join("t1/snapshot_2026-11-10_020000")).unwrap();

        let opts = MigrationOptions {
            state_dir: state_dir.clone(),
            dedup: DeduplicateOptions {
                roots: vec![BackupRootPair {
                    hist_root: hist,
                    perm_root: perm,
                }],
                runner_dir,
                lock_dir,
                log_dir: root.join("logs"),
                task_filter: None,
                dry_run: false,
                use_flock: true,
            },
            force: false,
        };

        let first = run_dedup_migration_if_needed(&Runner::new(), &opts).unwrap();
        assert!(!first.already_completed);
        assert!(first.marker_path.is_some());
        assert!(state_dir.join(DEDUP_MIGRATION_MARKER).is_file());

        let second = run_dedup_migration_if_needed(&Runner::new(), &opts).unwrap();
        assert!(second.already_completed);
        assert!(second.dedup.is_none());

        let _ = fs::remove_dir_all(&root);
    }

    #[cfg(unix)]
    #[test]
    fn ssh_restaura_solo_uw_y_omite_tarea_con_lock_activo() {
        use std::os::unix::fs::PermissionsExt;

        let root = temp_root("ssh-lock");
        let hist = root.join("BACKUPS_HISTORICOS");
        let perm = root.join("BACKUPS_PERMANENTES");
        let runner_dir = root.join("bin");
        let lock_dir = root.join("lock");
        let state_dir = root.join("state");
        fs::create_dir_all(&runner_dir).unwrap();
        fs::create_dir_all(&lock_dir).unwrap();

        fs::write(
            runner_dir.join("backup_srv_ssh.sh"),
            "#!/bin/bash\nSOURCE_TYPE='ssh'\n",
        )
        .unwrap();
        fs::write(
            runner_dir.join("backup_srv_busy.sh"),
            "#!/bin/bash\nSOURCE_TYPE='local'\n",
        )
        .unwrap();

        let ssh_snap = hist.join("srv_ssh/snapshot_2026-11-10_020000");
        fs::create_dir_all(&ssh_snap).unwrap();
        let ssh_file = ssh_snap.join("config.yml");
        fs::write(&ssh_file, b"key: val\n").unwrap();
        fs::set_permissions(&ssh_file, fs::Permissions::from_mode(0o444)).unwrap();

        let busy_snap = hist.join("srv_busy/snapshot_2026-11-10_020000");
        fs::create_dir_all(&busy_snap).unwrap();
        fs::write(busy_snap.join("a.txt"), b"busy").unwrap();

        // Simular respaldo en curso en srv_busy.
        let held_lock = TaskLock::acquire(lock_dir.join("backup_srv_busy.lock")).unwrap();

        let opts = MigrationOptions {
            state_dir: state_dir.clone(),
            dedup: DeduplicateOptions {
                roots: vec![BackupRootPair {
                    hist_root: hist,
                    perm_root: perm,
                }],
                runner_dir,
                lock_dir,
                log_dir: root.join("logs"),
                task_filter: None,
                dry_run: false,
                use_flock: true,
            },
            force: false,
        };

        let rep = run_dedup_migration_if_needed(&Runner::new(), &opts).unwrap();
        assert!(
            rep.marker_path.is_none(),
            "no debe sellar migración mientras haya una tarea omitida por lock"
        );
        let dedup = rep.dedup.unwrap();
        assert_eq!(dedup.tasks_skipped, 1);

        // SSH debe restaurar u+w (0444 -> 0644), nunca 0666/0777.
        let mode = fs::metadata(&ssh_file).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o644);

        drop(held_lock);

        // Al liberar el lock, la siguiente pasada completa todo y escribe el marcador.
        let rep2 = run_dedup_migration_if_needed(&Runner::new(), &opts).unwrap();
        assert!(rep2.marker_path.is_some());
        assert_eq!(rep2.dedup.unwrap().tasks_skipped, 0);

        let _ = fs::remove_dir_all(&root);
    }

    #[cfg(unix)]
    #[test]
    fn rsync_link_dest_vuelve_a_deduplicar_tras_reparar_snapshot_aw() {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};

        let root = temp_root("rsync-verify");
        let src = root.join("src");
        let hist = root.join("BACKUPS_HISTORICOS");
        let perm = root.join("BACKUPS_PERMANENTES");
        let runner_dir = root.join("bin");
        let lock_dir = root.join("lock");
        fs::create_dir_all(&src).unwrap();
        fs::create_dir_all(hist.join("t1")).unwrap();
        fs::create_dir_all(&perm).unwrap();
        fs::create_dir_all(&runner_dir).unwrap();
        fs::create_dir_all(&lock_dir).unwrap();

        fs::write(
            runner_dir.join("backup_t1.sh"),
            "#!/bin/bash\nSOURCE_TYPE='local'\n",
        )
        .unwrap();
        let src_file = src.join("grande.dat");
        fs::write(&src_file, vec![0x7Au8; 16 * 1024]).unwrap();
        fs::set_permissions(&src_file, fs::Permissions::from_mode(0o644)).unwrap();

        let snap1 = hist.join("t1/snapshot_2026-11-10_020000");
        let snap2 = hist.join("t1/snapshot_2026-11-11_020000");

        let runner = Runner::new();
        let r1 = runner
            .run(&CommandSpec::new("rsync").args([
                "-aAXH",
                "--numeric-ids",
                &format!("{}/", src.display()),
                &format!("{}/", snap1.display()),
            ]))
            .unwrap();
        assert!(r1.is_success(), "rsync snap1 falló: {}", r1.stderr);

        // Simular bug antiguo: chmod a-w en snap1.
        fs::set_permissions(snap1.join("grande.dat"), fs::Permissions::from_mode(0o444)).unwrap();

        // Reparar y deduplicar.
        let opts = DeduplicateOptions {
            roots: vec![BackupRootPair {
                hist_root: hist,
                perm_root: perm,
            }],
            runner_dir,
            lock_dir,
            log_dir: root.join("logs"),
            task_filter: None,
            dry_run: false,
            use_flock: true,
        };
        let rep = deduplicate_backups(&runner, &opts).unwrap();
        assert!(rep.errors.is_empty());

        // Nuevo rsync --link-dest contra snap1 reparado debe crear hardlink (mismo inodo).
        let r2 = runner
            .run(&CommandSpec::new("rsync").args([
                "-aAXH",
                "--numeric-ids",
                "--link-dest",
                &snap1.to_string_lossy(),
                &format!("{}/", src.display()),
                &format!("{}/", snap2.display()),
            ]))
            .unwrap();
        assert!(r2.is_success(), "rsync snap2 falló: {}", r2.stderr);
        assert_eq!(
            fs::metadata(snap1.join("grande.dat")).unwrap().ino(),
            fs::metadata(snap2.join("grande.dat")).unwrap().ino(),
            "rsync --link-dest debe enlazar inodos tras la reparación"
        );

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn patch_runner_content_protege_hardlinks_en_retencion() {
        let old = r#"#!/bin/bash
    for ((i = 0; i < TO_REMOVE; i++)); do
        OLD="${ALL_SNAPS[$i]}"
        chattr -R -i "$OLD" 2>/dev/null || true
        chmod -R u+w "$OLD" 2>/dev/null || true
        if rm -rf -- "$OLD"; then
            log "retención: eliminado $(basename "$OLD")"
        fi
    done
"#;
        let patched = patch_runner_content(old);
        assert!(!patched.contains("chmod -R u+w \"$OLD\""));
        assert!(patched.contains("find \"$OLD\" -type d -exec chmod u+w {} + 2>/dev/null || true"));
        assert_eq!(patch_runner_content(&patched), patched);
    }

    #[test]
    fn parsea_extensiones_filefrag_compartidas() {
        let sample = "\
Filesystem type is: 9123683e
File size of /srv/nas/a.bin is 65536 (64 blocks of 1024 bytes)
 ext:     logical_offset:        physical_offset: length:   expected: flags:
   0:        0..      63:   12345678..  12345741:     64:             last,shared,eof
/srv/nas/a.bin: 1 extent found
";
        let exts = parse_filefrag_extents(sample).expect("extents");
        assert_eq!(exts.len(), 1);
        assert_eq!(exts[0].0, "0..      63");
        assert_eq!(exts[0].1, "12345678..  12345741");
        assert_eq!(exts[0].2, "64");
        assert!(exts[0].3);

        let inline = "\
Filesystem type is: 9123683e
 ext:     logical_offset:        physical_offset: length:   expected: flags:
   0:        0..       0:          0..         0:      1:             last,inline,eof
";
        assert!(parse_filefrag_extents(inline).is_none());
    }

    #[cfg(unix)]
    #[test]
    fn marcador_de_migracion_tiene_permisos_legibles() {
        use std::os::unix::fs::PermissionsExt;

        let root = temp_root("marker-perms");
        let state_dir = root.join("state");
        let runner_dir = root.join("bin");
        let lock_dir = root.join("lock");
        fs::create_dir_all(&runner_dir).unwrap();
        fs::create_dir_all(&lock_dir).unwrap();

        let opts = MigrationOptions {
            state_dir: state_dir.clone(),
            dedup: DeduplicateOptions {
                roots: Vec::new(),
                runner_dir,
                lock_dir,
                log_dir: root.join("logs"),
                task_filter: None,
                dry_run: false,
                use_flock: true,
            },
            force: false,
        };
        let rep = run_dedup_migration_if_needed(&Runner::new(), &opts).unwrap();
        let marker_path = PathBuf::from(rep.marker_path.expect("marcador"));
        let parent_mode = fs::metadata(marker_path.parent().unwrap())
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        let file_mode = fs::metadata(&marker_path).unwrap().permissions().mode() & 0o777;
        assert_eq!(parent_mode, 0o755);
        assert_eq!(file_mode, 0o644);

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn dedup_throttle_pausa_segun_presion_e_intervalo() {
        use std::sync::atomic::{AtomicU8, AtomicUsize, Ordering};
        use std::sync::Arc;

        let mode = Arc::new(AtomicU8::new(0));
        let probes = Arc::new(AtomicUsize::new(0));
        let mode_clone = Arc::clone(&mode);
        let probes_clone = Arc::clone(&probes);

        let mut throttle = DedupThrottle::new(4, Duration::from_secs(60), move || {
            probes_clone.fetch_add(1, Ordering::Relaxed);
            match mode_clone.load(Ordering::Relaxed) {
                1 => (MemoryPressure::Elevated, true),
                2 => (MemoryPressure::Critical, true),
                _ => (MemoryPressure::Normal, false),
            }
        });

        let mut slept = Vec::new();
        for _ in 0..3 {
            assert_eq!(
                throttle.tick_with_sleep(1, |d| slept.push(d)),
                PressureAction::Continue
            );
        }
        assert_eq!(probes.load(Ordering::Relaxed), 0);

        // 4a operación dispara verificación con presión Normal -> sin pausa.
        assert_eq!(
            throttle.tick_with_sleep(1, |d| slept.push(d)),
            PressureAction::Continue
        );
        assert_eq!(probes.load(Ordering::Relaxed), 1);
        assert!(slept.is_empty());

        // Sobrecarga -> pausa corta.
        mode.store(1, Ordering::Relaxed);
        assert_eq!(
            throttle.tick_with_sleep(4, |d| slept.push(d)),
            PressureAction::PauseOverloaded
        );
        assert_eq!(slept, vec![THROTTLE_PAUSE_OVERLOADED]);

        // Presión crítica -> pausa mayor.
        mode.store(2, Ordering::Relaxed);
        assert_eq!(
            throttle.tick_with_sleep(4, |d| slept.push(d)),
            PressureAction::PauseCritical
        );
        assert_eq!(
            slept,
            vec![THROTTLE_PAUSE_OVERLOADED, THROTTLE_PAUSE_CRITICAL]
        );

        // Intervalo vencido también dispara aunque ops < check_every_ops.
        let mut time_throttle =
            DedupThrottle::new(10_000, Duration::ZERO, || (MemoryPressure::Critical, true));
        let mut time_slept = Vec::new();
        assert_eq!(
            time_throttle.tick_with_sleep(1, |d| time_slept.push(d)),
            PressureAction::PauseCritical
        );
        assert_eq!(time_slept, vec![THROTTLE_PAUSE_CRITICAL]);
    }

    #[cfg(unix)]
    #[test]
    fn regulacion_intra_tarea_ejecuta_sondeos_periodicos_dentro_de_una_sola_tarea() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::Arc;

        let root = temp_root("intra-throttle");
        let hist = root.join("BACKUPS_HISTORICOS");
        let perm = root.join("BACKUPS_PERMANENTES");
        let runner_dir = root.join("bin");
        let lock_dir = root.join("lock");
        let log_dir = root.join("logs");
        fs::create_dir_all(&runner_dir).unwrap();
        fs::create_dir_all(&lock_dir).unwrap();
        fs::create_dir_all(&log_dir).unwrap();

        let s1 = hist.join("t1/snapshot_2026-11-10_020000");
        let s2 = hist.join("t1/snapshot_2026-11-11_020000");
        fs::create_dir_all(&s1).unwrap();
        fs::create_dir_all(&s2).unwrap();

        let fixed_time = SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000);
        for i in 0..300 {
            let content = format!("archivo-duplicado-{i:04}-datos-para-hashing");
            for dir in [&s1, &s2] {
                let p = dir.join(format!("f_{i:04}.txt"));
                fs::write(&p, content.as_bytes()).unwrap();
                let f = File::options().write(true).open(&p).unwrap();
                f.set_modified(fixed_time).unwrap();
            }
        }

        let pair = BackupRootPair {
            hist_root: hist,
            perm_root: perm,
        };
        let opts = DeduplicateOptions {
            roots: vec![pair.clone()],
            runner_dir,
            lock_dir,
            log_dir,
            task_filter: None,
            dry_run: false,
            use_flock: true,
        };

        let probes = Arc::new(AtomicUsize::new(0));
        let probes_clone = Arc::clone(&probes);
        let mut throttle =
            DedupThrottle::new(THROTTLE_CHECK_OPS, THROTTLE_CHECK_INTERVAL, move || {
                probes_clone.fetch_add(1, Ordering::Relaxed);
                (MemoryPressure::Critical, true)
            });

        let summary =
            deduplicate_task_with_throttle(&Runner::new(), &pair, "t1", &opts, &mut throttle);
        assert!(summary.errors.is_empty(), "errores: {:?}", summary.errors);
        assert_eq!(summary.files_scanned, 600);
        assert_eq!(summary.hardlinks_created, 300);
        assert!(
            probes.load(Ordering::Relaxed) >= 3,
            "se esperaban múltiples sondeos intra-tarea, hubo {}",
            probes.load(Ordering::Relaxed)
        );

        let _ = fs::remove_dir_all(&root);
    }
}
