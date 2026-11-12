//! `nas-dev`: herramienta de desarrollo.
//!
//! Despliega cambios locales en un entorno de desarrollo **sin publicar** en el
//! repositorio remoto. Nunca toca hosts de producción ni ejecuta el actualizador
//! firmado; esas capacidades pertenecen a `nasctl`.

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use nas_core::{ApiResponse, NasError, NasResult};

mod preflight;
mod remote;
mod sync;

use preflight::{pack, plan, preflight};
use remote::{doctor, onboard};
use sync::{sync, SyncOptions};

#[derive(Debug, Parser)]
#[command(
    name = "nas-dev",
    version,
    about = "Herramienta de desarrollo del servidor NAS (sin push)",
    long_about = "Valida, empaqueta y sincroniza los cambios locales hacia un entorno de \
                  desarrollo aislado. Rechaza por diseño cualquier host de producción."
)]
struct Cli {
    /// Directorio fuente del proyecto.
    #[arg(long, global = true, value_name = "DIR", default_value = ".")]
    source: PathBuf,

    /// Emite la respuesta en JSON.
    #[arg(long, global = true)]
    json: bool,

    /// Ruta del inventario YAML.
    #[arg(long, global = true, value_name = "PATH")]
    inventory: Option<PathBuf>,

    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Describe el plan de sincronización paso a paso (sin ejecutar nada).
    Plan,
    /// Ejecuta las validaciones previas (git, JS, Bash).
    Preflight,
    /// Empaqueta el árbol de trabajo en un tar.gz reproducible.
    Pack {
        /// Archivo de salida.
        #[arg(long, value_name = "FILE")]
        out: PathBuf,
    },
    /// Sincroniza el árbol local con un host de desarrollo del inventario.
    Sync {
        /// Alias del host en el inventario.
        #[arg(long)]
        host: String,

        /// Simula la sincronización sin modificar el servidor.
        #[arg(long)]
        dry_run: bool,

        /// Confirma la ejecución real.
        #[arg(long)]
        yes: bool,

        /// No elevar privilegios con sudo en el servidor remoto.
        #[arg(long)]
        no_sudo: bool,

        /// Usuario SSH (sobrescribe el inventario).
        #[arg(long)]
        user: Option<String>,

        /// Clave privada SSH.
        #[arg(long)]
        identity: Option<PathBuf>,

        /// Archivo known_hosts dedicado.
        #[arg(long)]
        known_hosts: Option<PathBuf>,
    },
    /// Verifica los requisitos del host de desarrollo antes de sincronizar.
    Doctor {
        /// Alias del host en el inventario.
        #[arg(long)]
        host: String,

        /// Usuario SSH (sobrescribe el inventario).
        #[arg(long)]
        user: Option<String>,

        /// Clave privada SSH.
        #[arg(long)]
        identity: Option<PathBuf>,

        /// Archivo known_hosts dedicado.
        #[arg(long)]
        known_hosts: Option<PathBuf>,
    },
    /// Imprime el procedimiento de alta del host de desarrollo (no ejecuta nada).
    Onboard {
        /// Alias del host en el inventario.
        #[arg(long)]
        host: String,
    },
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(cli) {
        Ok(code) => code,
        Err(error) => {
            let payload = ApiResponse::<serde_json::Value>::error(error.to_string());
            eprintln!(
                "{}",
                serde_json::to_string_pretty(&payload).unwrap_or_else(|_| error.to_string())
            );
            ExitCode::from(u8::try_from(error.exit_code()).unwrap_or(1))
        }
    }
}

fn run(cli: Cli) -> NasResult<ExitCode> {
    let source = cli.source;
    if !source.is_dir() {
        return Err(NasError::NotFound(format!(
            "directorio fuente inexistente: {}",
            source.display()
        )));
    }

    match cli.command {
        Command::Plan => plan(&source, cli.json),
        Command::Preflight => preflight(&source, cli.json),
        Command::Pack { out } => pack(&source, &out, cli.json),
        Command::Sync {
            host,
            dry_run,
            yes,
            no_sudo,
            user,
            identity,
            known_hosts,
        } => sync(
            &source,
            &host,
            cli.inventory,
            SyncOptions {
                dry_run,
                confirmed: yes,
                sudo: !no_sudo,
                user,
                identity,
                known_hosts,
            },
            cli.json,
        ),
        Command::Doctor {
            host,
            user,
            identity,
            known_hosts,
        } => doctor(&host, cli.inventory, user, identity, known_hosts, cli.json),
        Command::Onboard { host } => onboard(&host, cli.inventory, cli.json),
    }
}

#[cfg(test)]
mod tests {
    use crate::preflight::{collect_by_extension, Check};
    use crate::remote::{build_doctor_checks, ensure_development, onboard_script};
    use crate::sync::{build_plan, health_ok, split_path};
    use nas_inventory::ResolvedHost;
    use std::path::PathBuf;

    fn host(web: &str) -> ResolvedHost {
        ResolvedHost {
            alias: "lab-dev".to_string(),
            address: "10.10.1.222".to_string(),
            group: "dev".to_string(),
            env: "dev".to_string(),
            role: "ARCHIVOS_BACKUP".to_string(),
            port: 22,
            user: "nasdev".to_string(),
            repo: "/opt/nas_debian-dev".to_string(),
            web: web.to_string(),
            state: "/var/lib/nas-dev".to_string(),
            timeout_secs: 30,
        }
    }

    #[test]
    fn check_falla_solo_con_fail() {
        assert!(Check::new("a", "FAIL", "x").failed());
        assert!(!Check::new("a", "WARN", "x").failed());
        assert!(!Check::new("a", "PASS", "x").failed());
        assert!(!Check::new("a", "SKIP", "x").failed());
    }

    #[test]
    fn recorre_extensiones_ignorando_target() {
        let root = std::env::temp_dir().join(format!("nas-dev-{}", std::process::id()));
        let nested = root.join("sub");
        std::fs::create_dir_all(&nested).expect("mkdir");
        std::fs::create_dir_all(root.join("target")).expect("mkdir target");
        std::fs::write(nested.join("a.js"), "const a = 1;").expect("write");
        std::fs::write(root.join("target").join("b.js"), "const b = 2;").expect("write");

        let mut files = Vec::new();
        collect_by_extension(&root, "js", &mut files);

        assert_eq!(files.len(), 1);
        assert!(files[0].ends_with(PathBuf::from("sub/a.js")));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn plan_usa_rutas_del_host_y_snapshot_con_marca() {
        let plan = build_plan(&host("/var/www/nas-web-dev"), "abc123", "20261023_160000");
        assert_eq!(
            plan.incoming,
            "/var/lib/nas-dev/dev-sync/incoming/abc123.tar.gz"
        );
        assert_eq!(plan.stage, "/var/lib/nas-dev/dev-sync/stage/abc123");
        assert_eq!(
            plan.snapshot,
            "/var/lib/nas-dev/dev-sync/backups/20261023_160000.tar.gz"
        );
        assert!(plan
            .steps
            .iter()
            .any(|step| step.argv.iter().any(|arg| arg == "--delete")));
        assert!(plan.steps.iter().any(|step| step.healthcheck));
        assert!(plan.rollback.argv.contains(&"tar".to_string()));
    }

    #[test]
    fn plan_snapshotea_el_docroot_correcto() {
        let plan = build_plan(&host("/srv/dev-web"), "sha", "ts");
        let snapshot = plan
            .steps
            .iter()
            .find(|step| step.label == "snapshot del docroot")
            .expect("paso de snapshot");
        assert!(snapshot.argv.contains(&"/srv".to_string()));
        assert!(snapshot.argv.contains(&"dev-web".to_string()));
        assert!(snapshot.ignore_failure);
    }

    #[test]
    fn separa_rutas_del_docroot() {
        assert_eq!(
            split_path("/var/www/nas-web-dev"),
            ("/var/www".to_string(), "nas-web-dev".to_string())
        );
        assert_eq!(split_path("/web"), ("/".to_string(), "web".to_string()));
    }

    #[test]
    fn guarda_de_entorno_rechaza_produccion() {
        let mut prod = host("/var/www/nas-web-dev");
        prod.env = "prod".to_string();
        assert!(ensure_development(&prod).is_err());
        assert!(ensure_development(&host("/var/www/nas-web-dev")).is_ok());
    }

    #[test]
    fn doctor_incluye_comprobaciones_criticas() {
        let checks = build_doctor_checks(&host("/var/www/nas-web-dev"));
        let names: Vec<&str> = checks.iter().map(|check| check.name.as_str()).collect();

        assert!(names.contains(&"sudo-sin-password"));
        assert!(names.contains(&"docroot-escribible"));
        assert!(names.contains(&"nginx-config"));
        let docroot = checks
            .iter()
            .find(|check| check.name == "docroot")
            .expect("comprobación de docroot");
        assert!(docroot.argv.contains(&"/var/www/nas-web-dev".to_string()));
        assert!(docroot.sudo);
    }

    #[test]
    fn onboard_genera_sudoers_acotado_y_rutas_propias() {
        let script = onboard_script(&host("/var/www/nas-web-dev"));
        assert!(script.contains("/etc/sudoers.d/nas-dev"));
        assert!(script.contains("NOPASSWD"));
        assert!(script.contains("/var/www/nas-web-dev"));
        assert!(script.contains("/var/lib/nas-dev/dev-sync"));
        assert!(script.contains("visudo -cf"));
    }

    #[test]
    fn interpreta_codigos_de_healthcheck() {
        assert!(health_ok("200"));
        assert!(health_ok("302\n"));
        assert!(!health_ok("500"));
        assert!(!health_ok(""));
    }
}
