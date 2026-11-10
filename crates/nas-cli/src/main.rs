//! CLI unificada del servidor NAS.
//!
//! Sustituye a `bin/nas-cli` (PHP) manteniendo el contrato `--json` con la
//! envoltura `{success, message, data}` que consumen el panel y los scripts.

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Args, Parser, Subcommand};
use nas_core::{ApiResponse, NasError, NasResult};

mod commands;
mod helper;
use helper::{run_helper, HelperAction};

#[derive(Debug, Parser)]
#[command(
    name = "nas",
    version,
    about = "Administración del servidor NAS Debian 13",
    long_about = "CLI del servidor NAS: estado, red, servicios, diagnóstico y auditoría.\n\
                  Todas las salidas admiten --json con la envoltura {success, message, data}."
)]
struct Cli {
    /// Emite la respuesta en JSON (contrato estable para automatización).
    #[arg(long, global = true)]
    json: bool,

    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Métricas del sistema (CPU, memoria, uptime, IP).
    Status,
    /// Inventario de red (interfaces, ruta, DNS, Samba, puertos).
    Network,
    /// Estado de los demonios clave (smbd, nmbd, wsdd2, nginx).
    Services,
    /// Diagnóstico integral con código de salida significativo.
    Diag,
    /// Gestión de la bitácora de auditoría.
    Audit {
        #[command(subcommand)]
        action: AuditAction,
    },
    /// Estado del retiro del stack anterior.
    Migrate {
        #[command(subcommand)]
        action: MigrateAction,
    },
    /// Tareas de respaldo instaladas en el servidor.
    Backup {
        #[command(subcommand)]
        action: BackupAction,
    },
    /// Verificación y planificación de actualizaciones firmadas.
    Update {
        #[command(subcommand)]
        action: Option<UpdateAction>,
    },
    /// Planificación del despliegue del servidor.
    Deploy {
        #[command(subcommand)]
        action: DeployAction,
    },
    /// Inventario de almacenamiento.
    Storage {
        #[command(subcommand)]
        action: StorageAction,
    },
    /// Desinstala la plataforma Rust (no elimina /srv/nas).
    Uninstall {
        /// Confirma la desinstalación.
        #[arg(long)]
        yes: bool,
        /// Elimina también el estado del panel (`/var/lib/nas` y `/etc/nas`).
        #[arg(long)]
        purge: bool,
        /// Alcance: `panel` (por defecto) o `os` (incluye ajustes del sistema).
        #[arg(long, default_value = "panel")]
        scope: String,
        /// Elevar privilegios con `sudo -n`.
        #[arg(long)]
        sudo: bool,
        /// Solo muestra el plan.
        #[arg(long)]
        dry_run: bool,
    },
    /// Inventario de usuarios y grupos.
    Users {
        #[command(subcommand)]
        action: UsersAction,
    },
    /// Operaciones privilegiadas de helpers acotados para el panel y sistema.
    Helper {
        #[command(subcommand)]
        action: HelperAction,
    },
}

#[derive(Debug, Subcommand)]
enum StorageAction {
    /// Lista los dispositivos de bloque detectados.
    List,
}

#[derive(Debug, Subcommand)]
enum UsersAction {
    /// Lista usuarios y grupos locales.
    List,
}

#[derive(Debug, Subcommand)]
enum UpdateAction {
    /// Muestra el estado del actualizador y versión instalada.
    Status,
    /// Comprueba si existe una versión más reciente en GitHub Releases.
    Check {
        /// Repositorio en GitHub (`propietario/repo`).
        #[arg(long, default_value = "ftole/nas_debian")]
        repo: String,
    },
    /// Verifica SHA-256 y firma Ed25519 del manifiesto.
    Verify {
        /// Manifiesto JSON de la release.
        manifest: PathBuf,
        /// Artefacto a verificar (por defecto, el declarado en el manifiesto).
        #[arg(long)]
        artifact: Option<PathBuf>,
        /// Clave pública Ed25519 en hexadecimal.
        #[arg(long)]
        pubkey: String,
    },
    /// Aplica la actualización verificada (requiere --yes).
    Apply {
        /// Manifiesto JSON de la release (opcional si se descarga de GitHub).
        manifest: Option<PathBuf>,
        /// Artefacto descargado (opcional si se descarga de GitHub).
        #[arg(long)]
        artifact: Option<PathBuf>,
        /// Repositorio en GitHub (`propietario/repo`) si se descarga por red.
        #[arg(long, default_value = "ftole/nas_debian")]
        repo: String,
        /// Raíz del despliegue.
        #[arg(long, default_value = "/opt/nas_debian")]
        root: String,
        /// Versión instalada actualmente (para el rollback).
        #[arg(long)]
        previous: Option<String>,
        /// Clave pública Ed25519 en hexadecimal o ruta al archivo pubkey.
        #[arg(long)]
        pubkey: Option<String>,
        /// Confirma la ejecución real.
        #[arg(long)]
        yes: bool,
        /// Elevar privilegios con `sudo -n`.
        #[arg(long)]
        sudo: bool,
    },
    /// Firma un manifiesto con una clave privada Ed25519 (hex de 32 bytes).
    Sign {
        /// Manifiesto JSON de la release.
        manifest: PathBuf,
        /// Archivo con la clave privada Ed25519 en hexadecimal.
        #[arg(long)]
        key: PathBuf,
        /// Archivo de salida (por defecto, sobrescribe el manifiesto).
        #[arg(long)]
        out: Option<PathBuf>,
    },
    /// Deriva la clave pública (hex) de una clave privada Ed25519.
    Pubkey {
        /// Archivo con la clave privada Ed25519 en hexadecimal.
        #[arg(long)]
        key: PathBuf,
    },
    /// Genera el plan A/B de la actualización.
    Plan {
        /// Manifiesto JSON de la release.
        manifest: PathBuf,
        /// Artefacto descargado.
        #[arg(long)]
        artifact: PathBuf,
        /// Raíz del despliegue.
        #[arg(long, default_value = "/opt/nas_debian")]
        root: String,
        /// Versión instalada actualmente (para el rollback).
        #[arg(long)]
        previous: Option<String>,
    },
}

#[derive(Debug, Subcommand)]
enum DeployAction {
    /// Muestra el plan de despliegue validado.
    Plan {
        #[command(flatten)]
        args: DeployArgs,
    },
    /// Ejecuta el despliegue (requiere --yes; formatear exige --format-token).
    Apply {
        #[command(flatten)]
        args: DeployArgs,
        /// Confirma la ejecución real.
        #[arg(long)]
        yes: bool,
        /// Permite formatear un disco montado o en uso.
        #[arg(long)]
        ignore_in_use: bool,
        /// Token textual obligatorio para formatear (`SI-FORMATEAR`).
        #[arg(long)]
        format_token: Option<String>,
        /// Elevar privilegios con `sudo -n`.
        #[arg(long)]
        sudo: bool,
    },
}

/// Argumentos comunes de despliegue.
#[derive(Debug, Args)]
pub struct DeployArgs {
    /// Disco dedicado (`/dev/sdb`) o `LOCAL`.
    #[arg(long)]
    pub disk: String,
    /// Grupo de trabajo.
    #[arg(long)]
    pub workgroup: String,
    /// Nombre NetBIOS.
    #[arg(long)]
    pub netbios: String,
    /// Usuario administrador.
    #[arg(long)]
    pub admin: String,
    /// Contraseña del usuario administrador.
    #[arg(long)]
    pub password: Option<String>,
    /// Lee la contraseña del administrador desde STDIN.
    #[arg(long)]
    pub password_stdin: bool,
    /// Rol del servidor.
    #[arg(long, default_value = "ARCHIVOS")]
    pub role: String,
    /// Reutiliza datos existentes sin formatear.
    #[arg(long)]
    pub keep_data: bool,
    /// Fuerza ext4 (por defecto se deriva del rol).
    #[arg(long)]
    pub ext4: bool,
    /// Fuerza Btrfs (por defecto se deriva del rol).
    #[arg(long)]
    pub btrfs: bool,
    /// El disco es SSD (afecta a las opciones de montaje).
    #[arg(long)]
    pub ssd: bool,
}

#[derive(Debug, Subcommand)]
enum MigrateAction {
    /// Muestra qué componentes legacy siguen presentes y su sustituto.
    Status,
}

#[derive(Debug, Subcommand)]
enum BackupAction {
    /// Lista las tareas de respaldo instaladas (`backup_*.sh`).
    List {
        /// Directorio de runners.
        #[arg(long, default_value = "/usr/local/bin")]
        dir: PathBuf,
        /// Muestra respaldos permanentes inmutables (Golden Snapshots).
        #[arg(long)]
        permanent: bool,
    },
    /// Muestra el plan de ejecución de una tarea.
    Plan {
        /// Identificador de la tarea (sin prefijo `backup_`).
        id: String,
        /// Directorio de runners.
        #[arg(long, default_value = "/usr/local/bin")]
        dir: PathBuf,
    },
    /// Ejecuta una tarea de respaldo (requiere --yes).
    Run {
        /// Identificador de la tarea.
        id: String,
        /// Directorio de runners.
        #[arg(long, default_value = "/usr/local/bin")]
        dir: PathBuf,
        /// Confirma la ejecución real.
        #[arg(long)]
        yes: bool,
        /// Simula la ejecución mostrando el plan y las guardas.
        #[arg(long)]
        dry_run: bool,
        /// Elevar privilegios con `sudo -n`.
        #[arg(long)]
        sudo: bool,
        /// Directorio de bloqueos.
        #[arg(long, default_value = "/var/lock")]
        lock_dir: PathBuf,
    },
    /// Congela el último snapshot de una tarea como respaldo permanente inmutable (Golden Snapshot a costo 0).
    Freeze {
        /// Identificador de la tarea.
        id: String,
        /// Etiqueta descriptiva opcional.
        tag: Option<String>,
        /// Directorio de runners.
        #[arg(long, default_value = "/usr/local/bin")]
        dir: PathBuf,
    },
    /// Lista los respaldos permanentes inmutables (Golden Snapshots).
    Permanent {
        /// Identificador opcional de la tarea para filtrar.
        id: Option<String>,
    },
    /// Restaura un snapshot hacia el origen de red o destino local alternativo.
    Restore {
        /// Identificador de la tarea.
        id: String,
        /// Nombre o ruta del snapshot a restaurar.
        snapshot: String,
        /// Destino local alternativo opcional.
        #[arg(long)]
        dest: Option<String>,
        /// Confirmación obligatoria de restauración.
        #[arg(long)]
        yes: bool,
    },
}

#[derive(Debug, Subcommand)]
enum AuditAction {
    /// Verifica la integridad de la cadena de auditoría.
    Verify,
}

fn main() -> ExitCode {
    let cli = Cli::parse();

    match run(cli) {
        Ok(code) => code,
        Err(error) => {
            report_error(&error);
            ExitCode::from(u8::try_from(error.exit_code()).unwrap_or(1))
        }
    }
}

fn report_error(error: &NasError) {
    let payload = ApiResponse::<serde_json::Value>::error(error.to_string());
    match serde_json::to_string_pretty(&payload) {
        Ok(json) => eprintln!("{json}"),
        Err(_) => eprintln!("{{\"success\":false,\"error\":\"{error}\"}}"),
    }
}

fn run(cli: Cli) -> NasResult<ExitCode> {
    match cli.command {
        Command::Status => commands::status::status(cli.json),
        Command::Network => commands::network::network(cli.json),
        Command::Services => commands::services::services(cli.json),
        Command::Diag => commands::diag::diag(cli.json),
        Command::Audit { action } => match action {
            AuditAction::Verify => commands::audit::audit_verify(cli.json),
        },
        Command::Backup { action } => match action {
            BackupAction::List { dir, permanent } => {
                if permanent {
                    commands::backup::backup_permanent_list(None, cli.json)
                } else {
                    commands::backup::backup_list(&dir, cli.json)
                }
            }
            BackupAction::Plan { id, dir } => commands::backup::backup_plan(&dir, &id, cli.json),
            BackupAction::Run {
                id,
                dir,
                yes,
                dry_run,
                sudo,
                lock_dir,
            } => commands::backup::backup_run(
                &dir,
                &id,
                commands::backup::RunOptions {
                    dry_run,
                    confirmed: yes,
                    sudo,
                    lock_dir,
                },
                cli.json,
            ),
            BackupAction::Freeze { id, tag, dir: _ } => {
                commands::backup::backup_freeze(&id, tag.as_deref(), cli.json)
            }
            BackupAction::Permanent { id } => {
                commands::backup::backup_permanent_list(id.as_deref(), cli.json)
            }
            BackupAction::Restore {
                id,
                snapshot,
                dest,
                yes,
            } => commands::backup::backup_restore(&id, &snapshot, dest.as_deref(), yes, cli.json),
        },
        Command::Update { action } => match action {
            None | Some(UpdateAction::Status) => commands::update::update_status(cli.json),
            Some(UpdateAction::Check { repo }) => commands::update::update_check(&repo, cli.json),
            Some(UpdateAction::Verify {
                manifest,
                artifact,
                pubkey,
            }) => commands::update::update_verify(
                &manifest,
                artifact.as_deref(),
                &pubkey,
                cli.json,
            ),
            Some(UpdateAction::Apply {
                manifest,
                artifact,
                repo,
                root,
                previous,
                pubkey,
                yes,
                sudo,
            }) => commands::update::update_apply_dispatch(
                commands::update::UpdateDispatchArgs {
                    manifest,
                    artifact,
                    repo,
                    root,
                    previous,
                    pubkey,
                    yes,
                    sudo,
                },
                cli.json,
            ),
            Some(UpdateAction::Sign { manifest, key, out }) => {
                commands::update::update_sign(&manifest, &key, out.as_deref(), cli.json)
            }
            Some(UpdateAction::Pubkey { key }) => commands::update::update_pubkey(&key, cli.json),
            Some(UpdateAction::Plan {
                manifest,
                artifact,
                root,
                previous,
            }) => commands::update::update_plan(
                &manifest,
                &artifact,
                &root,
                previous.as_deref(),
                cli.json,
            ),
        },
        Command::Migrate { action } => match action {
            MigrateAction::Status => commands::migrate::migrate_status(cli.json),
        },
        Command::Uninstall {
            yes,
            purge,
            scope,
            sudo,
            dry_run,
        } => commands::deploy::uninstall_command(yes, purge, &scope, sudo, dry_run, cli.json),
        Command::Storage { action } => match action {
            StorageAction::List => commands::storage::storage_list(cli.json),
        },
        Command::Users { action } => match action {
            UsersAction::List => commands::users::users_list(cli.json),
        },
        Command::Deploy { action } => match action {
            DeployAction::Plan { args } => commands::deploy::deploy_plan(&args, cli.json),
            DeployAction::Apply {
                args,
                yes,
                ignore_in_use,
                format_token,
                sudo,
            } => commands::deploy::deploy_apply(
                &args,
                commands::deploy::DeployRunOptions {
                    confirmed: yes,
                    ignore_in_use,
                    format_token,
                    sudo,
                },
                cli.json,
            ),
        },
        Command::Helper { action } => run_helper(action, cli.json),
    }
}
