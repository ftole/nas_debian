//! Parsers de `/proc` y `cgroup v2` para detección de recursos.

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
