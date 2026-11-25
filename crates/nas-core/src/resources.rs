//! Detección inteligente de recursos del sistema y presupuesto dinámico de concurrencia.
//!
//! Inspecciona CPU lógica, cuotas de cgroups v1/v2, memoria disponible en
//! `/proc/meminfo`, carga promedio (`/proc/loadavg`) e indicadores PSI
//! (`/proc/pressure/*`) para dimensionar trabajadores, semáforos y buffers sin
//! saturar el servidor.

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};

/// Nivel de presión de memoria observado en el host o cgroup.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum MemoryPressure {
    /// Memoria disponible holgada (>= 25% y PSI bajo).
    Normal,
    /// Memoria moderadamente ajustada (12%-25% o PSI moderado).
    Elevated,
    /// Memoria crítica (< 12%, < 192 MiB disponibles o PSI alto).
    Critical,
}

impl MemoryPressure {
    /// Etiqueta legible para consola y JSON.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Normal => "NORMAL",
            Self::Elevated => "ELEVATED",
            Self::Critical => "CRITICAL",
        }
    }
}

/// Instantánea de recursos detectados en el sistema operativo y cgroups.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SystemResources {
    /// CPUs lógicas visibles por el kernel.
    pub host_cpus: usize,
    /// CPUs efectivas considerando cuotas de cgroup (si existen).
    pub effective_cpus: usize,
    /// Memoria total efectiva en MiB.
    pub total_mem_mb: u64,
    /// Memoria disponible efectiva en MiB.
    pub available_mem_mb: u64,
    /// Carga promedio de 1 minuto (`/proc/loadavg`).
    pub load_1m: f64,
    /// Relación carga 1m / CPUs efectivas.
    pub load_ratio: f64,
    /// PSI `some avg10` de CPU (0.0 si no está disponible).
    pub cpu_psi_avg10: f64,
    /// PSI `some avg10` de memoria (0.0 si no está disponible).
    pub mem_psi_avg10: f64,
    /// PSI `some avg10` de E/S (0.0 si no está disponible).
    pub io_psi_avg10: f64,
    /// Nivel de presión de memoria calculado.
    pub memory_pressure: MemoryPressure,
    /// Indica si existe una cuota activa de cgroup sobre CPU o memoria.
    pub cgroup_limited: bool,
}

impl SystemResources {
    /// Detecta los recursos actuales del entorno Linux (o valores seguros en otros SO).
    pub fn detect() -> Self {
        let host_cpus = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(1)
            .max(1);

        if !cfg!(target_os = "linux") {
            return Self::from_snapshot(host_cpus, None, 4096, 2048, None, 0.25, 0.0, 0.0, 0.0);
        }

        let cgroup_rel = std::fs::read_to_string("/proc/self/cgroup")
            .ok()
            .and_then(|raw| parse_cgroup_v2_rel_path(&raw));

        let cgroup_cpus = detect_cgroup_cpu_limit(cgroup_rel.as_deref());
        let (host_total_kb, host_avail_kb) = std::fs::read_to_string("/proc/meminfo")
            .ok()
            .map(|raw| parse_meminfo_kb(&raw))
            .unwrap_or((2 * 1024 * 1024, 1024 * 1024));

        let host_total_mb = (host_total_kb / 1024).max(1);
        let host_avail_mb = host_avail_kb / 1024;
        let cgroup_mem_limit_mb = detect_cgroup_mem_limit_mb(cgroup_rel.as_deref());

        let load_1m = std::fs::read_to_string("/proc/loadavg")
            .ok()
            .and_then(|raw| parse_loadavg_1m(&raw))
            .unwrap_or(0.0);

        let cpu_psi_avg10 = std::fs::read_to_string("/proc/pressure/cpu")
            .ok()
            .and_then(|raw| parse_psi_avg10(&raw))
            .unwrap_or(0.0);
        let mem_psi_avg10 = std::fs::read_to_string("/proc/pressure/memory")
            .ok()
            .and_then(|raw| parse_psi_avg10(&raw))
            .unwrap_or(0.0);
        let io_psi_avg10 = std::fs::read_to_string("/proc/pressure/io")
            .ok()
            .and_then(|raw| parse_psi_avg10(&raw))
            .unwrap_or(0.0);

        Self::from_snapshot(
            host_cpus,
            cgroup_cpus,
            host_total_mb,
            host_avail_mb,
            cgroup_mem_limit_mb,
            load_1m,
            cpu_psi_avg10,
            mem_psi_avg10,
            io_psi_avg10,
        )
    }

    /// Construye la instantánea a partir de mediciones ya extraídas (determinista para pruebas).
    #[allow(clippy::too_many_arguments)]
    pub fn from_snapshot(
        host_cpus: usize,
        cgroup_cpus: Option<usize>,
        host_total_mb: u64,
        host_avail_mb: u64,
        cgroup_mem_limit_mb: Option<(u64, u64)>,
        load_1m: f64,
        cpu_psi_avg10: f64,
        mem_psi_avg10: f64,
        io_psi_avg10: f64,
    ) -> Self {
        let host_cpus = host_cpus.max(1);
        let effective_cpus = cgroup_cpus
            .map(|limit| host_cpus.min(limit.max(1)))
            .unwrap_or(host_cpus)
            .max(1);

        let (total_mem_mb, available_mem_mb, mem_limited) = match cgroup_mem_limit_mb {
            Some((limit_mb, current_mb)) if limit_mb > 0 && limit_mb < host_total_mb => {
                let cgroup_avail = limit_mb.saturating_sub(current_mb);
                (limit_mb, host_avail_mb.min(cgroup_avail), true)
            }
            _ => (host_total_mb, host_avail_mb.min(host_total_mb), false),
        };

        let cgroup_limited = mem_limited || cgroup_cpus.is_some_and(|c| c < host_cpus);
        let load_ratio = round2((load_1m.max(0.0)) / (effective_cpus as f64));
        let avail_pct = if total_mem_mb > 0 {
            (available_mem_mb as f64 / total_mem_mb as f64) * 100.0
        } else {
            100.0
        };

        let memory_pressure = if (total_mem_mb > 0 && (avail_pct < 12.0 || available_mem_mb < 192))
            || mem_psi_avg10 >= 30.0
        {
            MemoryPressure::Critical
        } else if (total_mem_mb > 0 && avail_pct < 25.0) || mem_psi_avg10 >= 10.0 {
            MemoryPressure::Elevated
        } else {
            MemoryPressure::Normal
        };

        Self {
            host_cpus,
            effective_cpus,
            total_mem_mb,
            available_mem_mb,
            load_1m: round2(load_1m.max(0.0)),
            load_ratio,
            cpu_psi_avg10: round2(cpu_psi_avg10.max(0.0)),
            mem_psi_avg10: round2(mem_psi_avg10.max(0.0)),
            io_psi_avg10: round2(io_psi_avg10.max(0.0)),
            memory_pressure,
            cgroup_limited,
        }
    }

    /// Indica si la presión de E/S (`/proc/pressure/io` `some avg10`) es elevada (>= 25%).
    pub fn is_io_pressured(&self) -> bool {
        self.io_psi_avg10 >= 25.0
    }

    /// Indica si la presión de E/S (`/proc/pressure/io` `some avg10`) es crítica (>= 50%).
    pub fn is_io_critical(&self) -> bool {
        self.io_psi_avg10 >= 50.0
    }
}

const BLOCKING_POOL_DB_AUTH_HEADROOM: usize = 4;

/// Presupuesto operativo derivado de [`SystemResources`] con soporte de overrides por entorno.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResourceBudget {
    /// Recursos base detectados en el servidor.
    pub resources: SystemResources,
    /// Hilos de trabajo del reactor Tokio en `nas-web`.
    pub tokio_worker_threads: usize,
    /// Tope de hilos bloqueantes del pool `spawn_blocking` en `nas-web`.
    pub tokio_blocking_threads: usize,
    /// Concurrencia máxima de operaciones del sistema (`systemctl`, `lsblk`, `df`, etc.).
    pub max_concurrent_system_ops: usize,
    /// Concurrencia máxima de operaciones pesadas (`zip`, `audit_verify`, `du`, discos).
    pub max_concurrent_heavy_ops: usize,
    /// Concurrencia máxima de verificaciones criptográficas de inicio de sesión (PBKDF2).
    pub max_concurrent_auth_ops: usize,
    /// Concurrencia máxima de hosts remotos simultáneos en `nasctl`.
    pub max_concurrent_remote_hosts: usize,
    /// Trabajos paralelos recomendados para compilación `cargo build`.
    pub cargo_build_jobs: usize,
    /// Límite de captura en memoria por flujo (`stdout`/`stderr`) en `nas-exec`.
    pub exec_output_limit_bytes: usize,
    /// Tope de entradas en memoria para cachés de deduplicación.
    pub dedup_max_entries_in_memory: usize,
    /// Concurrencia máxima de refrescos en segundo plano de tamaños de respaldo (`du`).
    pub max_concurrent_background_refreshes: usize,
    /// Concurrencia máxima de subidas de archivos simultáneas en `nas-web`.
    pub max_concurrent_upload_ops: usize,
}

impl ResourceBudget {
    /// Detecta recursos del sistema y calcula el presupuesto aplicando variables `NAS_*`.
    pub fn detect() -> Self {
        let resources = SystemResources::detect();
        Self::from_resources(resources).with_env_overrides()
    }

    fn min_blocking_threads_for(system_ops: usize, heavy_ops: usize, auth_ops: usize) -> usize {
        system_ops
            .saturating_add(heavy_ops)
            .saturating_add(auth_ops)
            .saturating_add(BLOCKING_POOL_DB_AUTH_HEADROOM)
    }

    /// Devuelve el mínimo de hilos bloqueantes requerido para garantizar holgura a DB/auth
    /// y refrescos en segundo plano aun con los semáforos de sistema/pesados saturados.
    pub fn min_blocking_threads_for_headroom(&self) -> usize {
        Self::min_blocking_threads_for(
            self.max_concurrent_system_ops,
            self.max_concurrent_heavy_ops,
            self.max_concurrent_auth_ops,
        )
    }

    /// Calcula el presupuesto únicamente a partir de una instantánea de recursos.
    pub fn from_resources(resources: SystemResources) -> Self {
        let cpus = resources.effective_cpus.max(1);
        let avail_mb = resources.available_mem_mb;
        let high_cpu_pressure = resources.load_ratio >= 0.85 || resources.cpu_psi_avg10 >= 25.0;

        let tokio_worker_threads = if cpus <= 1 {
            1
        } else if cpus <= 4 {
            2
        } else {
            (cpus / 2).clamp(2, 4)
        };

        let mut max_concurrent_system_ops = cpus.clamp(2, 8);
        if resources.memory_pressure != MemoryPressure::Normal || high_cpu_pressure {
            max_concurrent_system_ops = (max_concurrent_system_ops / 2).max(1);
        }

        let max_concurrent_heavy_ops = if cpus <= 2
            || resources.memory_pressure == MemoryPressure::Critical
            || avail_mb < 1024
        {
            1
        } else {
            (cpus / 4).clamp(1, 3)
        };

        let max_concurrent_auth_ops = if cpus <= 2
            || resources.memory_pressure == MemoryPressure::Critical
            || high_cpu_pressure
        {
            1
        } else {
            (cpus / 2).clamp(1, 4)
        };

        let min_blocking = Self::min_blocking_threads_for(
            max_concurrent_system_ops,
            max_concurrent_heavy_ops,
            max_concurrent_auth_ops,
        );
        let mut tokio_blocking_threads = (cpus * 2).clamp(2, 24);
        if resources.memory_pressure == MemoryPressure::Critical || avail_mb < 512 {
            tokio_blocking_threads = (tokio_blocking_threads / 2).max(2);
        }
        tokio_blocking_threads = tokio_blocking_threads.clamp(min_blocking, 32);

        let mut max_concurrent_remote_hosts = (cpus * 2).clamp(2, 8);
        if resources.memory_pressure == MemoryPressure::Critical || high_cpu_pressure {
            max_concurrent_remote_hosts = 2;
        }

        let max_jobs_by_cpu = if cpus > 2 { cpus - 1 } else { 1 };
        let max_jobs_by_mem = usize::try_from((avail_mb / 1536).max(1)).unwrap_or(1);
        let cargo_build_jobs = max_jobs_by_cpu.min(max_jobs_by_mem).clamp(1, 16);

        let exec_output_limit_bytes = match resources.memory_pressure {
            MemoryPressure::Critical => 2 * 1024 * 1024,
            MemoryPressure::Elevated => 4 * 1024 * 1024,
            MemoryPressure::Normal => 8 * 1024 * 1024,
        };

        let dedup_max_entries_in_memory =
            usize::try_from(avail_mb.saturating_mul(256).clamp(10_000, 500_000)).unwrap_or(50_000);

        let max_concurrent_background_refreshes = if cpus <= 2
            || resources.memory_pressure != MemoryPressure::Normal
            || resources.is_io_pressured()
        {
            1
        } else {
            (cpus / 2).clamp(2, 4)
        };

        let max_concurrent_upload_ops = if cpus <= 2
            || resources.memory_pressure != MemoryPressure::Normal
            || resources.is_io_pressured()
            || avail_mb < 1024
        {
            1
        } else {
            (cpus / 2).clamp(2, 3)
        };

        Self {
            resources,
            tokio_worker_threads,
            tokio_blocking_threads,
            max_concurrent_system_ops,
            max_concurrent_heavy_ops,
            max_concurrent_auth_ops,
            max_concurrent_remote_hosts,
            cargo_build_jobs,
            exec_output_limit_bytes,
            dedup_max_entries_in_memory,
            max_concurrent_background_refreshes,
            max_concurrent_upload_ops,
        }
    }

    /// Aplica sobrescrituras desde variables de entorno del proceso.
    pub fn with_env_overrides(self) -> Self {
        self.with_env_lookup(|key| std::env::var(key).ok())
    }

    /// Aplica sobrescrituras consultando una fuente de entorno inyectable (útil en pruebas).
    pub fn with_env_lookup<F>(mut self, lookup: F) -> Self
    where
        F: Fn(&str) -> Option<String>,
    {
        let parse_usize = |key: &str, min: usize, max: usize| -> Option<usize> {
            lookup(key)
                .and_then(|val| val.trim().parse::<usize>().ok())
                .map(|n| n.clamp(min, max))
        };

        if let Some(v) = parse_usize("NAS_WEB_WORKERS", 1, 32) {
            self.tokio_worker_threads = v;
        }
        let blocking_override = parse_usize("NAS_WEB_BLOCKING_THREADS", 1, 64);
        if let Some(v) = blocking_override {
            self.tokio_blocking_threads = v;
        }
        if let Some(v) = parse_usize("NAS_MAX_SYSTEM_OPS", 1, 32) {
            self.max_concurrent_system_ops = v;
        }
        if let Some(v) = parse_usize("NAS_MAX_HEAVY_OPS", 1, 16) {
            self.max_concurrent_heavy_ops = v;
        }
        if let Some(v) = parse_usize("NAS_MAX_AUTH_OPS", 1, 16) {
            self.max_concurrent_auth_ops = v;
        }
        if let Some(v) = parse_usize("NAS_REMOTE_CONCURRENCY", 1, 32) {
            self.max_concurrent_remote_hosts = v;
        }
        if let Some(v) = parse_usize("NAS_CARGO_BUILD_JOBS", 1, 64) {
            self.cargo_build_jobs = v;
        }
        if let Some(v) = parse_usize("NAS_EXEC_MAX_OUTPUT_BYTES", 65_536, 134_217_728) {
            self.exec_output_limit_bytes = v;
        }
        if let Some(v) = parse_usize("NAS_MAX_BACKGROUND_REFRESHES", 1, 8) {
            self.max_concurrent_background_refreshes = v;
        }
        if let Some(v) = parse_usize("NAS_MAX_UPLOAD_OPS", 1, 8) {
            self.max_concurrent_upload_ops = v;
        }
        if blocking_override.is_none() {
            let min_blocking = self.min_blocking_threads_for_headroom().min(64);
            self.tokio_blocking_threads = self.tokio_blocking_threads.clamp(min_blocking, 64);
        }
        self
    }

    /// Indica si el servidor está bajo presión severa de CPU, memoria o E/S.
    pub fn is_overloaded(&self) -> bool {
        self.resources.memory_pressure == MemoryPressure::Critical
            || self.resources.load_ratio >= 1.2
            || self.resources.is_io_pressured()
    }

    /// Calcula cuántos permisos del semáforo `heavy_ops` adquirir y el backoff a aplicar
    /// según la presión viva de memoria/CPU/E/S en `live`.
    pub fn heavy_ops_throttle_plan(&self, live: &SystemResources) -> (u32, Duration) {
        let configured = self.max_concurrent_heavy_ops.max(1);
        let (effective_cap, backoff) =
            if live.memory_pressure == MemoryPressure::Critical || live.is_io_critical() {
                (1usize, Duration::from_millis(150))
            } else if live.memory_pressure == MemoryPressure::Elevated
                || live.is_io_pressured()
                || live.load_ratio >= 1.2
            {
                ((configured / 2).max(1), Duration::from_millis(60))
            } else {
                (configured, Duration::ZERO)
            };

        let permits = if effective_cap >= configured {
            1
        } else {
            configured.div_ceil(effective_cap).clamp(1, configured)
        };
        (u32::try_from(permits).unwrap_or(1), backoff)
    }

    /// Cuotas recomendadas de `systemd` para `nas-web`:
    /// `(MemoryHigh MiB, MemoryMax MiB, CPUQuota %, TasksMax)`.
    pub fn systemd_resource_limits(&self) -> (u64, u64, usize, usize) {
        let total_mem_mb = self.resources.total_mem_mb.max(256);
        let mem_high_mb = (total_mem_mb / 4)
            .clamp(384, 4096)
            .min((total_mem_mb * 60 / 100).max(192));
        let mem_max_cap = (total_mem_mb * 80 / 100).max(256).min(total_mem_mb);
        let mem_max_mb = (mem_high_mb.saturating_mul(2))
            .clamp(512, 8192)
            .min(mem_max_cap)
            .max(mem_high_mb.saturating_add(64).min(total_mem_mb));

        let cpus = self.resources.effective_cpus.max(1);
        let cpu_quota_pct = cpus.saturating_mul(75).clamp(100, 800);
        let tasks_max = cpus.saturating_mul(64).clamp(256, 1024);
        (mem_high_mb, mem_max_mb, cpu_quota_pct, tasks_max)
    }

    /// Cuotas recomendadas de `systemd` para la migración en segundo plano
    /// (`nas-backup-dedup-migration`): `(MemoryHigh MiB, MemoryMax MiB, IOWeight)`.
    pub fn dedup_migration_systemd_limits(&self) -> (u64, u64, usize) {
        let total_mem_mb = self.resources.total_mem_mb.max(256);
        let mem_high_mb = (total_mem_mb / 4)
            .clamp(256, 2048)
            .min((total_mem_mb / 2).max(192));
        let mem_max_cap = (total_mem_mb * 75 / 100).max(256).min(total_mem_mb);
        let mem_max_mb = (total_mem_mb * 40 / 100)
            .clamp(384, 4096)
            .min(mem_max_cap)
            .max(mem_high_mb.saturating_add(64).min(total_mem_mb));
        (mem_high_mb, mem_max_mb, 10)
    }

    /// Cuotas recomendadas de `systemd` para tareas pesadas en segundo plano
    /// (`nas-backup-*`, `scrub`, aprovisionamiento): `(MemoryHigh MiB, MemoryMax MiB, CPUQuota %, TasksMax)`.
    pub fn background_task_systemd_limits(&self) -> (u64, u64, usize, usize) {
        let total_mem_mb = self.resources.total_mem_mb.max(256);
        let mem_high_mb = (total_mem_mb / 3)
            .clamp(256, 2048)
            .min((total_mem_mb / 2).max(192));
        let mem_max_cap = (total_mem_mb * 75 / 100).max(256).min(total_mem_mb);
        let mem_max_mb = (total_mem_mb / 2)
            .clamp(384, 4096)
            .min(mem_max_cap)
            .max(mem_high_mb.saturating_add(64).min(total_mem_mb));

        let cpus = self.resources.effective_cpus.max(1);
        let cpu_quota_pct = if cpus <= 1 {
            65
        } else {
            cpus.saturating_mul(65).clamp(120, 600)
        };
        let tasks_max = cpus.saturating_mul(32).clamp(96, 256);
        (mem_high_mb, mem_max_mb, cpu_quota_pct, tasks_max)
    }

    /// Límites proporcionales de caché sucia del kernel:
    /// `(vm.dirty_background_bytes, vm.dirty_bytes)`.
    pub fn sysctl_dirty_bytes(&self) -> (u64, u64) {
        let dirty_mb = (self.resources.total_mem_mb / 16).clamp(32, 512);
        let bg_mb = (dirty_mb / 4).clamp(8, 128);
        (bg_mb * 1024 * 1024, dirty_mb * 1024 * 1024)
    }

    /// Trabajadores recomendados para hashing en `backup_dedup`:
    /// estrictamente `1` en almacenamiento rotacional o bajo presión.
    pub fn dedup_parallel_workers(&self, rotational: bool) -> usize {
        if rotational
            || self.resources.effective_cpus < 4
            || self.resources.memory_pressure != MemoryPressure::Normal
            || self.resources.is_io_pressured()
            || self.resources.load_ratio >= 1.0
        {
            1
        } else {
            (self.resources.effective_cpus / 4).clamp(2, 4)
        }
    }
}

/// Parsea `/proc/meminfo` devolviendo `(MemTotal_kB, MemAvailable_kB)`.
pub fn parse_meminfo_kb(raw: &str) -> (u64, u64) {
    let find_kb = |prefix: &str| -> Option<u64> {
        raw.lines()
            .find(|line| line.starts_with(prefix))
            .and_then(|line| line.split_whitespace().nth(1))
            .and_then(|val| val.parse::<u64>().ok())
    };

    let total = find_kb("MemTotal:").unwrap_or(0);
    let available = find_kb("MemAvailable:").unwrap_or_else(|| {
        let free = find_kb("MemFree:").unwrap_or(0);
        let buffers = find_kb("Buffers:").unwrap_or(0);
        let cached = find_kb("Cached:").unwrap_or(0);
        free.saturating_add(buffers).saturating_add(cached)
    });
    (total, available)
}

/// Parsea la carga promedio de 1 minuto desde `/proc/loadavg`.
pub fn parse_loadavg_1m(raw: &str) -> Option<f64> {
    raw.split_whitespace()
        .next()?
        .parse::<f64>()
        .ok()
        .filter(|v| v.is_finite() && *v >= 0.0)
}

/// Parsea `some avg10=X.XX` de `/proc/pressure/*`.
pub fn parse_psi_avg10(raw: &str) -> Option<f64> {
    for line in raw.lines() {
        let trimmed = line.trim();
        if !trimmed.starts_with("some ") {
            continue;
        }
        for token in trimmed.split_whitespace() {
            if let Some(value) = token.strip_prefix("avg10=") {
                if let Ok(parsed) = value.parse::<f64>() {
                    if parsed.is_finite() && parsed >= 0.0 {
                        return Some(parsed);
                    }
                }
            }
        }
    }
    None
}

/// Extrae la ruta relativa del cgroup v2 (`0::/ruta`) desde `/proc/self/cgroup`.
pub fn parse_cgroup_v2_rel_path(raw: &str) -> Option<String> {
    raw.lines().find_map(|line| {
        let path = line.trim().strip_prefix("0::")?.trim();
        if path.is_empty() || path == "/" {
            None
        } else {
            Some(path.to_string())
        }
    })
}

/// Parsea el archivo `cpu.max` de cgroups v2 devolviendo el límite entero de CPUs.
pub fn parse_cgroup_v2_cpu_max(raw: &str) -> Option<usize> {
    let mut parts = raw.split_whitespace();
    let quota_raw = parts.next()?;
    if quota_raw == "max" {
        return None;
    }
    let quota_us = quota_raw.parse::<u64>().ok()?;
    let period_us = parts.next()?.parse::<u64>().ok()?;
    if quota_us == 0 || period_us == 0 {
        return None;
    }
    let cpus = (quota_us as f64 / period_us as f64).ceil();
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    Some((cpus as usize).max(1))
}

/// Parsea el archivo `memory.max` de cgroups v2 en MiB.
pub fn parse_cgroup_v2_memory_max_mb(raw: &str) -> Option<u64> {
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed == "max" {
        return None;
    }
    let bytes = trimmed.parse::<u64>().ok()?;
    Some((bytes / (1024 * 1024)).max(1))
}

fn cgroup_candidate_dirs(rel_path: Option<&str>) -> Vec<PathBuf> {
    let root = Path::new("/sys/fs/cgroup");
    let mut dirs = Vec::new();
    if let Some(rel) = rel_path {
        let trimmed = rel.trim_start_matches('/');
        if !trimmed.is_empty() {
            let mut current = root.join(trimmed);
            loop {
                dirs.push(current.clone());
                if current == root {
                    break;
                }
                if !current.pop() {
                    break;
                }
            }
        }
    }
    if !dirs.iter().any(|p| p == root) {
        dirs.push(root.to_path_buf());
    }
    dirs
}

fn detect_cgroup_cpu_limit(rel_path: Option<&str>) -> Option<usize> {
    let mut min_cpus: Option<usize> = None;
    for dir in cgroup_candidate_dirs(rel_path) {
        if let Ok(raw) = std::fs::read_to_string(dir.join("cpu.max")) {
            if let Some(cpus) = parse_cgroup_v2_cpu_max(&raw) {
                min_cpus = Some(min_cpus.map_or(cpus, |prev| prev.min(cpus)));
            }
        }
    }
    if min_cpus.is_some() {
        return min_cpus;
    }

    // Fallback cgroups v1
    let quota = std::fs::read_to_string("/sys/fs/cgroup/cpu/cpu.cfs_quota_us")
        .ok()
        .and_then(|s| s.trim().parse::<i64>().ok())
        .filter(|&q| q > 0)?;
    let period = std::fs::read_to_string("/sys/fs/cgroup/cpu/cpu.cfs_period_us")
        .ok()
        .and_then(|s| s.trim().parse::<i64>().ok())
        .filter(|&p| p > 0)?;
    let cpus = (quota as f64 / period as f64).ceil();
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    Some((cpus as usize).max(1))
}

fn detect_cgroup_mem_limit_mb(rel_path: Option<&str>) -> Option<(u64, u64)> {
    let mut tightest: Option<(u64, u64)> = None;
    for dir in cgroup_candidate_dirs(rel_path) {
        if let Ok(raw) = std::fs::read_to_string(dir.join("memory.max")) {
            if let Some(limit_mb) = parse_cgroup_v2_memory_max_mb(&raw) {
                let current_mb = std::fs::read_to_string(dir.join("memory.current"))
                    .ok()
                    .and_then(|s| s.trim().parse::<u64>().ok())
                    .map(|b| b / (1024 * 1024))
                    .unwrap_or(0);
                let replace = match tightest {
                    None => true,
                    Some((prev_lim, _)) => limit_mb < prev_lim,
                };
                if replace {
                    tightest = Some((limit_mb, current_mb));
                }
            }
        }
    }
    if tightest.is_some() {
        return tightest;
    }

    // Fallback cgroups v1
    let limit_bytes = std::fs::read_to_string("/sys/fs/cgroup/memory/memory.limit_in_bytes")
        .ok()
        .and_then(|s| s.trim().parse::<u64>().ok())
        .filter(|&b| b > 0 && b < (1u64 << 60))?;
    let usage_bytes = std::fs::read_to_string("/sys/fs/cgroup/memory/memory.usage_in_bytes")
        .ok()
        .and_then(|s| s.trim().parse::<u64>().ok())
        .unwrap_or(0);
    Some((
        (limit_bytes / (1024 * 1024)).max(1),
        usage_bytes / (1024 * 1024),
    ))
}

fn round2(value: f64) -> f64 {
    (value * 100.0).round() / 100.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn parsea_meminfo_loadavg_psi_y_cgroups() {
        let meminfo = "\
MemTotal:       16384000 kB
MemFree:         2048000 kB
MemAvailable:    8192000 kB
";
        assert_eq!(parse_meminfo_kb(meminfo), (16_384_000, 8_192_000));

        assert_eq!(parse_loadavg_1m("1.42 1.15 0.98 2/312 4512\n"), Some(1.42));
        assert_eq!(
            parse_psi_avg10("some avg10=12.50 avg60=5.10 avg300=1.00 total=100\n"),
            Some(12.5)
        );
        assert_eq!(
            parse_cgroup_v2_rel_path("0::/system.slice/nas-web.service\n"),
            Some("/system.slice/nas-web.service".to_string())
        );
        assert_eq!(parse_cgroup_v2_rel_path("0::/\n"), None);
        assert_eq!(parse_cgroup_v2_cpu_max("max 100000\n"), None);
        assert_eq!(parse_cgroup_v2_cpu_max("200000 100000\n"), Some(2));
        assert_eq!(parse_cgroup_v2_cpu_max("150000 100000\n"), Some(2));
        assert_eq!(parse_cgroup_v2_memory_max_mb("max\n"), None);
        assert_eq!(parse_cgroup_v2_memory_max_mb("1073741824\n"), Some(1024));
    }

    #[test]
    fn calcula_presupuesto_conservador_en_servidor_pequeno_y_bajo_presion() {
        let small = SystemResources::from_snapshot(2, None, 1024, 128, None, 2.5, 30.0, 35.0, 10.0);
        assert_eq!(small.memory_pressure, MemoryPressure::Critical);
        assert_eq!(small.effective_cpus, 2);

        let budget = ResourceBudget::from_resources(small);
        assert_eq!(budget.tokio_worker_threads, 2);
        assert_eq!(budget.max_concurrent_system_ops, 1);
        assert_eq!(budget.max_concurrent_heavy_ops, 1);
        assert_eq!(budget.max_concurrent_remote_hosts, 2);
        assert_eq!(budget.cargo_build_jobs, 1);
        assert!(budget.is_overloaded());
    }

    #[test]
    fn calcula_presupuesto_escalado_en_servidor_grande_con_cuota_cgroup() {
        let big = SystemResources::from_snapshot(
            16,
            Some(4),
            65_536,
            49_152,
            Some((2048, 512)),
            0.8,
            1.0,
            0.0,
            0.5,
        );
        assert!(big.cgroup_limited);
        assert_eq!(big.effective_cpus, 4);
        assert_eq!(big.total_mem_mb, 2048);
        assert_eq!(big.available_mem_mb, 1536);
        assert_eq!(big.memory_pressure, MemoryPressure::Normal);

        let budget = ResourceBudget::from_resources(big);
        assert_eq!(budget.tokio_worker_threads, 2);
        assert_eq!(budget.max_concurrent_system_ops, 4);
        assert_eq!(budget.max_concurrent_heavy_ops, 1);
        assert_eq!(budget.cargo_build_jobs, 1);
        assert!(!budget.is_overloaded());
    }

    #[test]
    fn aplica_overrides_de_entorno_con_limites_seguros() {
        let res = SystemResources::from_snapshot(8, None, 16384, 12288, None, 0.5, 0.0, 0.0, 0.0);
        let base = ResourceBudget::from_resources(res);

        let mut map = HashMap::new();
        map.insert("NAS_WEB_WORKERS", "6");
        map.insert("NAS_WEB_BLOCKING_THREADS", "12");
        map.insert("NAS_MAX_SYSTEM_OPS", "5");
        map.insert("NAS_MAX_HEAVY_OPS", "3");
        map.insert("NAS_REMOTE_CONCURRENCY", "99"); // clamped to 32
        map.insert("NAS_CARGO_BUILD_JOBS", "4");
        map.insert("NAS_EXEC_MAX_OUTPUT_BYTES", "1048576");

        let overridden = base.with_env_lookup(|k| map.get(k).map(|v| (*v).to_string()));
        assert_eq!(overridden.tokio_worker_threads, 6);
        assert_eq!(overridden.tokio_blocking_threads, 12);
        assert_eq!(overridden.max_concurrent_system_ops, 5);
        assert_eq!(overridden.max_concurrent_heavy_ops, 3);
        assert_eq!(overridden.max_concurrent_remote_hosts, 32);
        assert_eq!(overridden.cargo_build_jobs, 4);
        assert_eq!(overridden.exec_output_limit_bytes, 1_048_576);
    }

    #[test]
    fn pool_bloqueante_garantiza_holgura_para_db_y_auth_en_equipos_pequenos() {
        let scenarios = [
            // 1 CPU holgado
            SystemResources::from_snapshot(1, None, 2048, 1536, None, 0.2, 0.0, 0.0, 0.0),
            // 1 CPU presión crítica
            SystemResources::from_snapshot(1, None, 1024, 96, None, 2.0, 35.0, 40.0, 10.0),
            // 2 CPUs holgado
            SystemResources::from_snapshot(2, None, 4096, 3072, None, 0.4, 0.0, 0.0, 0.0),
            // 2 CPUs memoria ajustada / elevada
            SystemResources::from_snapshot(2, None, 2048, 400, None, 1.8, 10.0, 15.0, 5.0),
            // 2 CPUs presión crítica
            SystemResources::from_snapshot(2, None, 1024, 100, None, 3.0, 30.0, 35.0, 20.0),
            // 4, 8 y 16 CPUs
            SystemResources::from_snapshot(4, None, 8192, 6144, None, 1.0, 0.0, 0.0, 0.0),
            SystemResources::from_snapshot(8, None, 16384, 12288, None, 1.0, 0.0, 0.0, 0.0),
            SystemResources::from_snapshot(16, None, 32768, 24576, None, 1.0, 0.0, 0.0, 0.0),
        ];

        for res in scenarios {
            let cpus = res.effective_cpus;
            let pressure = res.memory_pressure;
            let budget = ResourceBudget::from_resources(res);
            let reserved_ops = budget.max_concurrent_system_ops
                + budget.max_concurrent_heavy_ops
                + budget.max_concurrent_auth_ops;
            assert!(
                budget.tokio_blocking_threads >= reserved_ops + BLOCKING_POOL_DB_AUTH_HEADROOM,
                "holgura insuficiente ({cpus} CPU, {pressure:?}): blk={}, sys={}, heavy={}, auth={}",
                budget.tokio_blocking_threads,
                budget.max_concurrent_system_ops,
                budget.max_concurrent_heavy_ops,
                budget.max_concurrent_auth_ops,
            );
            assert!(
                budget.tokio_blocking_threads >= budget.min_blocking_threads_for_headroom(),
                "no cumple min_blocking_threads_for_headroom en {cpus} CPU"
            );
        }

        // Override parcial de concurrencia sin tocar NAS_WEB_BLOCKING_THREADS mantiene holgura.
        let small = SystemResources::from_snapshot(1, None, 2048, 1536, None, 0.1, 0.0, 0.0, 0.0);
        let base = ResourceBudget::from_resources(small);
        let mut map = HashMap::new();
        map.insert("NAS_MAX_SYSTEM_OPS", "8");
        map.insert("NAS_MAX_HEAVY_OPS", "4");
        map.insert("NAS_MAX_AUTH_OPS", "2");
        let adjusted = base.with_env_lookup(|k| map.get(k).map(|v| (*v).to_string()));
        assert!(adjusted.tokio_blocking_threads >= 8 + 4 + 2 + BLOCKING_POOL_DB_AUTH_HEADROOM);
    }

    #[test]
    fn calcula_plan_reactivo_heavy_ops_y_limites_sistema() {
        let big =
            SystemResources::from_snapshot(16, None, 16_384, 12_288, None, 0.5, 0.0, 0.0, 0.0);
        let budget = ResourceBudget::from_resources(big.clone());
        assert_eq!(budget.max_concurrent_heavy_ops, 3);
        assert_eq!(budget.max_concurrent_background_refreshes, 4);
        assert_eq!(budget.max_concurrent_upload_ops, 3);
        assert_eq!(budget.heavy_ops_throttle_plan(&big), (1, Duration::ZERO));
        assert_eq!(budget.dedup_parallel_workers(true), 1);
        assert_eq!(budget.dedup_parallel_workers(false), 4);
        assert_eq!(budget.systemd_resource_limits(), (4096, 8192, 800, 1024));
        assert_eq!(budget.dedup_migration_systemd_limits(), (2048, 4096, 10));
        assert_eq!(
            budget.background_task_systemd_limits(),
            (2048, 4096, 600, 256)
        );
        assert_eq!(
            budget.sysctl_dirty_bytes(),
            (128 * 1024 * 1024, 512 * 1024 * 1024)
        );

        let io_pressured =
            SystemResources::from_snapshot(16, None, 16_384, 12_288, None, 0.5, 0.0, 0.0, 30.0);
        assert!(io_pressured.is_io_pressured());
        assert!(!io_pressured.is_io_critical());
        assert!(ResourceBudget::from_resources(io_pressured.clone()).is_overloaded());
        assert_eq!(
            budget.heavy_ops_throttle_plan(&io_pressured),
            (3, Duration::from_millis(60))
        );
        assert_eq!(
            ResourceBudget::from_resources(io_pressured).dedup_parallel_workers(false),
            1
        );

        let critical =
            SystemResources::from_snapshot(16, None, 16_384, 12_288, None, 0.5, 0.0, 0.0, 55.0);
        assert!(critical.is_io_critical());
        assert_eq!(
            budget.heavy_ops_throttle_plan(&critical),
            (3, Duration::from_millis(150))
        );

        let small = SystemResources::from_snapshot(1, None, 1024, 768, None, 0.2, 0.0, 0.0, 0.0);
        let small_budget = ResourceBudget::from_resources(small);
        assert_eq!(small_budget.max_concurrent_background_refreshes, 1);
        assert_eq!(small_budget.max_concurrent_upload_ops, 1);
        assert_eq!(small_budget.systemd_resource_limits(), (384, 768, 100, 256));
        assert_eq!(
            small_budget.dedup_migration_systemd_limits(),
            (256, 409, 10)
        );
        assert_eq!(
            small_budget.background_task_systemd_limits(),
            (341, 512, 65, 96)
        );
        assert_eq!(
            small_budget.sysctl_dirty_bytes(),
            (16 * 1024 * 1024, 64 * 1024 * 1024)
        );
    }
}
