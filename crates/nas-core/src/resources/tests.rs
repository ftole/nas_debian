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
    let big = SystemResources::from_snapshot(16, None, 16_384, 12_288, None, 0.5, 0.0, 0.0, 0.0);
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
