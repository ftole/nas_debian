//! Planificación de actualizaciones atómicas A/B sobre enlaces simbólicos.

use super::ReleaseManifest;

/// Plan de actualización o regresión A/B sobre enlaces simbólicos.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdatePlan {
    /// Versión objetivo.
    pub version: String,
    /// Directorio de la release.
    pub release_dir: String,
    /// Pasos ordenados de la actualización.
    pub steps: Vec<Vec<String>>,
    /// Pasos de reversión al estado anterior.
    pub rollback: Vec<Vec<String>>,
}

pub(super) fn service_reload_steps() -> Vec<Vec<String>> {
    vec![
        vec![
            "systemctl".to_string(),
            "reload".to_string(),
            "nginx".to_string(),
        ],
        vec![
            "systemctl".to_string(),
            "restart".to_string(),
            "nas-web".to_string(),
        ],
    ]
}

pub(super) fn healthcheck_step() -> Vec<String> {
    vec![
        "curl".to_string(),
        "-k".to_string(),
        "-s".to_string(),
        "-o".to_string(),
        "/dev/null".to_string(),
        "-w".to_string(),
        "%{http_code}".to_string(),
        "https://127.0.0.1/login".to_string(),
    ]
}

/// Construye el plan A/B para una release.
pub fn build_plan(
    manifest: &ReleaseManifest,
    root: &str,
    artifact: &str,
    previous_version: Option<&str>,
) -> UpdatePlan {
    let root = root.trim_end_matches('/');
    let releases_root = format!("{root}/releases");
    let release_dir = format!("{releases_root}/{}", manifest.version);
    let link = format!("{root}/current");
    let link_next = format!("{root}/current.new");

    let reloads = service_reload_steps();

    let mut steps = vec![
        vec!["mkdir".to_string(), "-p".to_string(), release_dir.clone()],
        vec![
            "tar".to_string(),
            "-xzf".to_string(),
            artifact.to_string(),
            "-C".to_string(),
            release_dir.clone(),
            "--strip-components=1".to_string(),
        ],
        // Garantiza que `nas-web` (sin privilegios) pueda listar releases y leer `version.json`.
        vec!["chmod".to_string(), "0755".to_string(), releases_root],
        vec!["chmod".to_string(), "0755".to_string(), release_dir.clone()],
        vec![
            "test".to_string(),
            "-f".to_string(),
            format!("{release_dir}/Cargo.toml"),
        ],
        vec![
            "ln".to_string(),
            "-sfn".to_string(),
            release_dir.clone(),
            link_next.clone(),
        ],
        vec![
            "mv".to_string(),
            "-T".to_string(),
            link_next.clone(),
            link.clone(),
        ],
    ];

    for bin in ["nas", "nas-web", "nas-tui", "nasctl", "nas-dev"] {
        steps.push(vec![
            "install".to_string(),
            "-m".to_string(),
            "0755".to_string(),
            format!("{link}/bin/{bin}"),
            format!("/usr/local/bin/{bin}"),
        ]);
    }
    for helper in crate::deploy::helpers::ROOT_HELPERS {
        steps.push(vec![
            "install".to_string(),
            "-m".to_string(),
            "0755".to_string(),
            "-o".to_string(),
            "root".to_string(),
            "-g".to_string(),
            "root".to_string(),
            format!("{link}/packaging/{helper}"),
            format!("/usr/local/sbin/{helper}"),
        ]);
    }
    steps.push(vec![
        "install".to_string(),
        "-m".to_string(),
        "0644".to_string(),
        format!("{link}/version.json"),
        "/etc/nas/version.json".to_string(),
    ]);
    steps.push(vec![
        "install".to_string(),
        "-m".to_string(),
        "0644".to_string(),
        format!("{link}/version.json"),
        format!("{root}/version.json"),
    ]);

    steps.push(vec!["nginx".to_string(), "-t".to_string()]);
    steps.extend(reloads.clone());
    steps.push(healthcheck_step());

    let mut rollback = Vec::new();
    if let Some(previous) = previous_version {
        let previous_dir = format!("{root}/releases/{previous}");
        rollback.push(vec![
            "ln".to_string(),
            "-sfn".to_string(),
            previous_dir,
            link_next.clone(),
        ]);
        rollback.push(vec![
            "mv".to_string(),
            "-T".to_string(),
            link_next,
            link.clone(),
        ]);
        for bin in ["nas", "nas-web", "nas-tui", "nasctl", "nas-dev"] {
            rollback.push(vec![
                "install".to_string(),
                "-m".to_string(),
                "0755".to_string(),
                format!("{link}/bin/{bin}"),
                format!("/usr/local/bin/{bin}"),
            ]);
        }
        for helper in crate::deploy::helpers::ROOT_HELPERS {
            rollback.push(vec![
                "install".to_string(),
                "-m".to_string(),
                "0755".to_string(),
                "-o".to_string(),
                "root".to_string(),
                "-g".to_string(),
                "root".to_string(),
                format!("{link}/packaging/{helper}"),
                format!("/usr/local/sbin/{helper}"),
            ]);
        }
        rollback.push(vec![
            "install".to_string(),
            "-m".to_string(),
            "0644".to_string(),
            format!("{link}/version.json"),
            "/etc/nas/version.json".to_string(),
        ]);
        rollback.push(vec![
            "install".to_string(),
            "-m".to_string(),
            "0644".to_string(),
            format!("{link}/version.json"),
            format!("{root}/version.json"),
        ]);
        rollback.extend(reloads);
    }

    UpdatePlan {
        version: manifest.version.clone(),
        release_dir,
        steps,
        rollback,
    }
}
