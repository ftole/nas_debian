//! Detección de límites de CPU y memoria en cgroups v1 y v2.

use std::path::{Path, PathBuf};

use super::parsers::{parse_cgroup_v2_cpu_max, parse_cgroup_v2_memory_max_mb};

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

pub(super) fn detect_cgroup_cpu_limit(rel_path: Option<&str>) -> Option<usize> {
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

pub(super) fn detect_cgroup_mem_limit_mb(rel_path: Option<&str>) -> Option<(u64, u64)> {
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

pub(super) fn round2(value: f64) -> f64 {
    (value * 100.0).round() / 100.0
}
