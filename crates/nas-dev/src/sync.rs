//! Planificación y ejecución de la sincronización hacia un entorno de desarrollo.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use nas_core::{ApiResponse, NasError, NasResult};
use nas_inventory::ResolvedHost;
use nas_ssh::{with_sudo, OpenSsh, RemoteExecutor, SshTarget};

use crate::preflight::{create_archive, sha256_file};
use crate::remote::{
    current_actor, ensure_development, ensure_known_hosts_dir, load_inventory, local_audit_path,
    target_for,
};

/// Paso remoto del plan de sincronización.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RemoteStep {
    pub(crate) label: String,
    pub(crate) argv: Vec<String>,
    pub(crate) ignore_failure: bool,
    pub(crate) healthcheck: bool,
    pub(crate) sudo: bool,
}

impl RemoteStep {
    pub(crate) fn new(label: &str, argv: Vec<String>) -> Self {
        Self {
            label: label.to_string(),
            argv,
            ignore_failure: false,
            healthcheck: false,
            sudo: false,
        }
    }

    pub(crate) fn optional(mut self) -> Self {
        self.ignore_failure = true;
        self
    }

    pub(crate) fn healthcheck(mut self) -> Self {
        self.healthcheck = true;
        self
    }

    pub(crate) fn with_sudo(mut self) -> Self {
        self.sudo = true;
        self
    }
}

struct TempWorkdirGuard(PathBuf);

impl Drop for TempWorkdirGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn create_unique_workdir() -> NasResult<TempWorkdirGuard> {
    let base = std::env::temp_dir();
    let pid = std::process::id();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    for attempt in 0..32u32 {
        let candidate = base.join(format!("nas-dev-{pid}-{nanos}-{attempt}"));
        #[cfg(unix)]
        let res = {
            use std::os::unix::fs::DirBuilderExt;
            let mut b = std::fs::DirBuilder::new();
            b.mode(0o700);
            b.create(&candidate)
        };
        #[cfg(not(unix))]
        let res = std::fs::DirBuilder::new().create(&candidate);
        match res {
            Ok(()) => return Ok(TempWorkdirGuard(candidate)),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(NasError::io(&candidate, e)),
        }
    }
    Err(NasError::Denied(
        "no se pudo crear directorio temporal único para nas-dev".into(),
    ))
}

fn validate_remote_path(label: &str, path: &str) -> NasResult<()> {
    let trimmed = path.trim();
    if trimmed.is_empty()
        || !trimmed.starts_with('/')
        || trimmed.contains("..")
        || trimmed.chars().any(|c| c.is_whitespace() || c.is_control())
    {
        return Err(NasError::Invalid(format!(
            "ruta remota {label} inválida: '{path}'"
        )));
    }
    Ok(())
}

/// Plan completo de sincronización hacia un entorno de desarrollo.
#[derive(Debug, Clone)]
pub(crate) struct SyncPlan {
    pub(crate) incoming: String,
    pub(crate) stage: String,
    pub(crate) snapshot: String,
    pub(crate) steps: Vec<RemoteStep>,
    pub(crate) rollback: RemoteStep,
}

/// Opciones de la sincronización.
#[derive(Debug, Default)]
pub(crate) struct SyncOptions {
    pub(crate) dry_run: bool,
    pub(crate) confirmed: bool,
    pub(crate) sudo: bool,
    pub(crate) user: Option<String>,
    pub(crate) identity: Option<PathBuf>,
    pub(crate) known_hosts: Option<PathBuf>,
}

pub(crate) fn sync(
    source: &Path,
    alias: &str,
    inventory_path: Option<PathBuf>,
    options: SyncOptions,
    json: bool,
) -> NasResult<ExitCode> {
    let inventory = load_inventory(inventory_path)?;
    let host = inventory.resolve(alias)?;
    ensure_development(&host)?;
    validate_remote_path("web", &host.web)?;
    validate_remote_path("state", &host.state)?;

    let target = target_for(
        &host,
        options.user.clone(),
        options.identity.clone(),
        options.known_hosts.clone(),
    );

    if options.dry_run {
        return render_dry_run(&host, &target, options.sudo, json);
    }

    if !options.confirmed {
        return Err(NasError::Denied(
            "la sincronización modifica el servidor: repita con --yes para confirmar".into(),
        ));
    }

    ensure_known_hosts_dir(&target)?;

    if json {
        let (_, failed) = crate::preflight::run_preflight_checks(source);
        if failed > 0 {
            return Err(NasError::Invalid(
                "el preflight falló antes de sincronizar; ejecuta `nas-dev preflight` para ver detalles".into(),
            ));
        }
    } else {
        let preflight_code = crate::preflight::preflight(source, false)?;
        if preflight_code != ExitCode::SUCCESS {
            return Ok(preflight_code);
        }
    }

    let workdir_guard = create_unique_workdir()?;
    let workdir = &workdir_guard.0;

    let bundle = workdir.join("bundle.tar.gz");
    create_archive(source, &bundle)?;
    let sha = sha256_file(&bundle)?;

    let archive = workdir.join(format!("{sha}.tar.gz"));
    std::fs::rename(&bundle, &archive).map_err(|e| NasError::io(&archive, e))?;

    let timestamp = chrono::Local::now().format("%Y%m%d_%H%M%S").to_string();
    let plan = build_plan(&host, &sha, &timestamp);

    let executor = OpenSsh::new();
    let audit = nas_audit::AuditLog::new(local_audit_path());

    let _ = audit.append(nas_audit::AuditEntry::new(
        current_actor(),
        &host.alias,
        "dev-sync",
        &host.web,
        "START",
        format!("sha={sha} archive={}", archive.display()),
    ));

    let result = execute(&plan, &executor, &target, &archive, options.sudo);

    let (status, detail) = match &result {
        Ok(()) => ("SUCCESS", format!("sha={sha}")),
        Err(error) => ("FAILED", error.to_string()),
    };
    let _ = audit.append(nas_audit::AuditEntry::new(
        current_actor(),
        &host.alias,
        "dev-sync",
        &host.web,
        status,
        &detail,
    ));

    result?;

    let _ = std::fs::remove_file(&archive);

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&ApiResponse::ok(
                serde_json::json!({
                    "host": host.alias,
                    "sha256": sha,
                    "stage": plan.stage,
                    "snapshot": plan.snapshot,
                }),
                "Sincronización completada"
            ))?
        );
    } else {
        println!();
        println!("Sincronización completada en {}.", host.alias);
        println!("sha256 : {sha}");
        println!("stage  : {}", plan.stage);
        println!("backup : {}", plan.snapshot);
    }

    Ok(ExitCode::SUCCESS)
}

fn execute(
    plan: &SyncPlan,
    executor: &dyn RemoteExecutor,
    target: &SshTarget,
    archive: &Path,
    sudo: bool,
) -> NasResult<()> {
    // Crear directorios de recepción ANTES de subir el paquete por scp.
    if let Some(mkdir_step) = plan.steps.first() {
        let output = executor.run(target, &mkdir_step.argv)?;
        if !output.is_success() {
            return Err(NasError::Command {
                command: format!("{}: {}", mkdir_step.label, output.command),
                code: output.code,
                stderr: output.stderr,
            });
        }
    }

    executor.upload(target, archive, &plan.incoming)?;

    for step in plan.steps.iter().skip(1) {
        let argv = if sudo && step.sudo {
            with_sudo(&step.argv)
        } else {
            step.argv.clone()
        };

        let output = executor.run(target, &argv)?;
        let success = output.is_success();

        if step.healthcheck {
            if success && health_ok(&output.stdout) {
                continue;
            }
            let _ = executor.run(
                target,
                &if sudo && plan.rollback.sudo {
                    with_sudo(&plan.rollback.argv)
                } else {
                    plan.rollback.argv.clone()
                },
            );
            return Err(NasError::Denied(format!(
                "healthcheck falló (salida: '{}'); se restauró el snapshot {}",
                output.stdout, plan.snapshot
            )));
        }

        if !success && !step.ignore_failure {
            return Err(NasError::Command {
                command: format!("{}: {}", step.label, output.command),
                code: output.code,
                stderr: output.stderr,
            });
        }
    }

    Ok(())
}

fn render_dry_run(
    host: &ResolvedHost,
    target: &SshTarget,
    sudo: bool,
    json: bool,
) -> NasResult<ExitCode> {
    let plan = build_plan(host, "<sha256>", "<timestamp>");
    let prefix = if sudo { "sudo -n -- " } else { "" };

    if json {
        let steps: Vec<_> = plan
            .steps
            .iter()
            .map(|step| {
                let step_prefix = if sudo && step.sudo { "sudo -n -- " } else { "" };
                serde_json::json!({
                    "label": step.label,
                    "command": format!("{step_prefix}{}", step.argv.join(" ")),
                    "ignore_failure": step.ignore_failure,
                    "healthcheck": step.healthcheck,
                })
            })
            .collect();
        let rb_prefix = if sudo && plan.rollback.sudo {
            "sudo -n -- "
        } else {
            ""
        };
        println!(
            "{}",
            serde_json::to_string_pretty(&ApiResponse::ok(
                serde_json::json!({
                    "host": host.alias,
                    "target": target.destination(),
                    "upload": plan.incoming,
                    "steps": steps,
                    "rollback": format!("{rb_prefix}{}", plan.rollback.argv.join(" ")),
                }),
                "Simulación de sincronización"
            ))?
        );
        return Ok(ExitCode::SUCCESS);
    }

    println!("SIMULACIÓN DE SINCRONIZACIÓN (dry-run)");
    println!("Host    : {} ({})", host.alias, host.env);
    println!("Destino : {}", target.destination());
    println!("Docroot : {}", host.web);
    println!();
    println!("[upload] paquete -> {}", plan.incoming);
    for (index, step) in plan.steps.iter().enumerate() {
        let step_prefix = if sudo && step.sudo { "sudo -n -- " } else { "" };
        println!(
            "{:>2}. {:<26} {step_prefix}{}",
            index + 1,
            step.label,
            step.argv.join(" ")
        );
    }
    let rb_prefix = if sudo && plan.rollback.sudo {
        "sudo -n -- "
    } else {
        ""
    };
    println!(
        "RB. {:<26} {rb_prefix}{}",
        plan.rollback.label,
        plan.rollback.argv.join(" ")
    );
    let _ = prefix;

    Ok(ExitCode::SUCCESS)
}

pub(crate) fn build_plan(host: &ResolvedHost, sha: &str, timestamp: &str) -> SyncPlan {
    let base = format!("{}/dev-sync", host.state);
    let incoming = format!("{base}/incoming/{sha}.tar.gz");
    let stage = format!("{base}/stage/{sha}");
    let snapshot = format!("{base}/backups/{timestamp}.tar.gz");

    let (web_parent, web_name) = split_path(&host.web);

    let steps = vec![
        RemoteStep::new(
            "crear directorios",
            vec![
                "mkdir".into(),
                "-p".into(),
                format!("{base}/incoming"),
                format!("{base}/stage"),
                format!("{base}/backups"),
            ],
        ),
        RemoteStep::new(
            "limpiar staging",
            vec!["rm".into(), "-rf".into(), stage.clone()],
        ),
        RemoteStep::new(
            "preparar staging",
            vec!["mkdir".into(), "-p".into(), stage.clone()],
        ),
        RemoteStep::new(
            "extraer paquete",
            vec![
                "tar".into(),
                "-xzf".into(),
                incoming.clone(),
                "-C".into(),
                stage.clone(),
                "--strip-components=1".into(),
            ],
        ),
        RemoteStep::new(
            "verificar árbol",
            vec![
                "test".into(),
                "-d".into(),
                format!("{stage}/crates/nas-web"),
            ],
        ),
        RemoteStep::new(
            "snapshot del docroot",
            vec![
                "tar".into(),
                "-czf".into(),
                snapshot.clone(),
                "-C".into(),
                web_parent.clone(),
                web_name.clone(),
            ],
        )
        .optional(),
        RemoteStep::new(
            "promover cambios",
            vec![
                "rsync".into(),
                "-a".into(),
                "--delete".into(),
                "--exclude=data/".into(),
                format!("{stage}/crates/nas-web/"),
                format!("{}/", host.web),
            ],
        ),
        RemoteStep::new(
            "recargar nginx",
            vec!["systemctl".into(), "reload".into(), "nginx".into()],
        )
        .with_sudo()
        .optional(),
        RemoteStep::new(
            "reiniciar nas-web",
            vec!["systemctl".into(), "restart".into(), "nas-web".into()],
        )
        .with_sudo()
        .optional(),
        RemoteStep::new(
            "healthcheck",
            vec![
                "curl".into(),
                "-k".into(),
                "-s".into(),
                "-o".into(),
                "/dev/null".into(),
                "-w".into(),
                "%{http_code}".into(),
                "https://127.0.0.1/login".into(),
            ],
        )
        .healthcheck(),
    ];

    let rollback = RemoteStep::new(
        "restaurar snapshot",
        vec![
            "tar".into(),
            "-xzf".into(),
            snapshot.clone(),
            "-C".into(),
            web_parent,
        ],
    );

    SyncPlan {
        incoming,
        stage,
        snapshot,
        steps,
        rollback,
    }
}

pub(crate) fn split_path(path: &str) -> (String, String) {
    match path.rsplit_once('/') {
        Some(("", name)) => ("/".to_string(), name.to_string()),
        Some((parent, name)) if !name.is_empty() => (parent.to_string(), name.to_string()),
        _ => ("/".to_string(), path.to_string()),
    }
}

pub(crate) fn health_ok(body: &str) -> bool {
    let code = body.trim();
    code == "200" || code == "302"
}
