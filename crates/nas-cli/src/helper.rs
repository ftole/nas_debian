//! Implementación del comando `nas helper` y sus subcomandos fuertemente tipados.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::Subcommand;
use nas_core::{ApiResponse, NasError, NasResult};
use nas_engine::helpers::{accounts, backup, samba, share, storage, terminal};
use nas_exec::Runner;

/// Acciones disponibles bajo `nas helper`.
#[derive(Debug, Subcommand)]
pub enum HelperAction {
    /// Aplica una configuración validada de Samba desde staging.
    SambaApply {
        /// Archivo de configuración en staging (/var/lib/nas/staging/smbconf_*).
        staged_file: PathBuf,
    },
    /// Crea el directorio de un recurso compartido con permisos 2770.
    ShareDir {
        /// Nombre del recurso.
        #[arg(long, short)]
        name: Option<String>,
        /// Unidad de almacenamiento (por defecto: principal).
        #[arg(long, short)]
        unit: Option<String>,
        /// Nombre posicional (compatibilidad con wrappers).
        pos_name: Option<String>,
        /// Unidad posicional (compatibilidad con wrappers).
        pos_unit: Option<String>,
    },
    /// Aplica o revoca ACLs POSIX sobre un recurso compartido.
    ShareAcl {
        /// Ruta dentro de /srv/nas/.
        #[arg(long)]
        path: Option<String>,
        /// Grupo o usuario (ej: grp_sistemas o juan).
        #[arg(long)]
        principal: Option<String>,
        /// Nivel de acceso (read, write, none).
        #[arg(long)]
        level: Option<String>,
        /// Ruta posicional (compatibilidad).
        pos_path: Option<String>,
        /// Principal posicional (compatibilidad).
        pos_principal: Option<String>,
        /// Nivel posicional (compatibilidad).
        pos_level: Option<String>,
    },
    /// Gestión de usuarios y grupos corporativos.
    Accounts {
        #[command(subcommand)]
        action: AccountsAction,
    },
    /// Gestión y control de tareas de respaldo.
    BackupManage {
        #[command(subcommand)]
        action: BackupManageAction,
    },
    /// Operaciones de almacenamiento físico, LVM y Btrfs.
    StorageApply {
        #[command(subcommand)]
        action: StorageApplyAction,
    },
    /// Sesiones interactivas de terminal PTY sobre tmux.
    Terminal {
        /// Usuario POSIX autenticado.
        #[arg(short, long)]
        user: String,
        #[command(subcommand)]
        action: TerminalAction,
    },
}

/// Subcomandos de gestión de cuentas (`nas helper accounts`).
#[derive(Debug, Subcommand)]
pub enum AccountsAction {
    /// Crea un usuario nuevo.
    CreateUser {
        username: String,
        full_name: String,
        #[arg(default_value = "-")]
        password: String,
        #[arg(default_value = "0")]
        admin: String,
        #[arg(default_value = "0")]
        web: String,
        #[arg(default_value = "0")]
        samba: String,
        #[arg(default_value = "")]
        groups: String,
    },
    /// Actualiza un usuario existente.
    UpdateUser {
        username: String,
        full_name: String,
        #[arg(default_value = "--keep-password")]
        password: String,
        #[arg(default_value = "0")]
        admin: String,
        #[arg(default_value = "0")]
        web: String,
        #[arg(default_value = "0")]
        samba: String,
        #[arg(default_value = "")]
        groups: String,
    },
    /// Activa o suspende una cuenta de usuario.
    ToggleUser {
        username: String,
        #[arg(default_value = "1")]
        enable: String,
    },
    /// Elimina una cuenta de usuario.
    DeleteUser { username: String },
    /// Crea un grupo corporativo.
    CreateGroup { group: String },
    /// Renombra un grupo corporativo.
    RenameGroup { old: String, new: String },
    /// Elimina un grupo corporativo.
    DeleteGroup { group: String },
    /// Añade un usuario a un grupo.
    #[command(alias = "add-member")]
    AddGroupMember { username: String, group: String },
    /// Quita un usuario de un grupo.
    #[command(alias = "remove-member")]
    RemoveGroupMember { username: String, group: String },
    /// Reporta el estado TSV de las cuentas gestionables.
    Status,
}

/// Subcomandos de tareas de respaldo (`nas helper backup-manage`).
#[derive(Debug, Subcommand)]
pub enum BackupManageAction {
    /// Instala una tarea preparada desde /var/lib/nas/staging.
    Install { id: String },
    /// Escribe o actualiza el cron de una tarea.
    WriteCron {
        id: String,
        #[arg(long)]
        file: Option<PathBuf>,
        #[arg(long)]
        content: Option<String>,
    },
    /// Elimina el cron de una tarea.
    DeleteCron { id: String },
    /// Escribe o actualiza el archivo de credenciales.
    WriteCred {
        id: String,
        #[arg(long)]
        file: Option<PathBuf>,
        #[arg(long)]
        content: Option<String>,
    },
    /// Elimina el archivo de credenciales.
    DeleteCred { id: String },
    /// Ejecuta manualmente una tarea de respaldo en segundo plano.
    #[command(alias = "run")]
    RunManual { id: String },
    /// Detiene una tarea de respaldo en ejecución.
    Stop { id: String },
    /// Muestra las últimas líneas de la bitácora.
    Logs {
        id: String,
        #[arg(default_value = "200")]
        lines: usize,
    },
    /// Elimina una tarea conservando sus respaldos.
    Delete { id: String },
    /// Prueba la conectividad de un recurso CIFS.
    TestCifs {
        host: String,
        share: String,
        user: String,
        #[arg(long)]
        password: Option<String>,
    },
    /// Prueba la conectividad SSH.
    TestSsh {
        host: String,
        port: u16,
        user: String,
        #[arg(long)]
        password: Option<String>,
    },
    /// Prueba de conectividad genérica (cifs|ssh|local).
    Test {
        proto: String,
        host: String,
        target: String,
        #[arg(default_value = "22")]
        port: u16,
        #[arg(default_value = "")]
        user: String,
    },
}

/// Subcomandos de almacenamiento (`nas helper storage-apply`).
#[derive(Debug, Subcommand)]
pub enum StorageApplyAction {
    /// Limpia firmas y particiones de un disco.
    WipeDisk { confirm: String, device: String },
    /// Formatea un disco/partición y lo monta en /srv/nas.
    #[command(alias = "format")]
    FormatSingle {
        confirm: String,
        device: String,
        #[arg(default_value = "ext4")]
        fstype: String,
        #[arg(default_value = "0")]
        unmount: String,
    },
    /// Configura LVM sobre un disco y lo monta en /srv/nas.
    #[command(alias = "lvm")]
    SetupLvm {
        confirm: String,
        disk: String,
        vg: String,
        lv: String,
        #[arg(default_value = "100%FREE")]
        size: String,
        #[arg(default_value = "ext4")]
        fstype: String,
        #[arg(default_value = "0")]
        unmount: String,
    },
    /// Extiende un grupo de volúmenes LVM existente.
    ExtendPool { confirm: String, device: String },
    /// Crea un subvolumen Btrfs en /srv/nas.
    #[command(alias = "subvolume")]
    CreateSubvolume { confirm: String, name: String },
    /// Elimina un subvolumen Btrfs en /srv/nas.
    DeleteSubvolume { confirm: String, name: String },
    /// Formatea y monta una unidad en /srv/nas/unidades/<unidad>.
    #[command(alias = "add-unit")]
    MountUnit {
        confirm: String,
        device: String,
        unit: String,
        #[arg(default_value = "ext4")]
        fstype: String,
    },
    /// Desmonta una unidad y la retira de /etc/fstab.
    UmountUnit { confirm: String, unit: String },
}

/// Subcomandos de terminal (`nas helper terminal`).
#[derive(Debug, Subcommand)]
pub enum TerminalAction {
    /// Inicia la sesión tmux.
    Start {
        #[arg(default_value = "200")]
        cols: u16,
        #[arg(default_value = "50")]
        rows: u16,
    },
    /// Envía teclas o texto a la sesión.
    Keys {
        input: String,
        #[arg(long, default_value_t = false)]
        enter: bool,
    },
    /// Envía texto sin Enter (compatibilidad con wrapper legado).
    Raw { input: String },
    /// Envía texto con Enter (compatibilidad con wrapper legado).
    RawEnter { input: String },
    /// Envía una tecla especial (Enter, Tab, C-c, etc.).
    Key { key_name: String },
    /// Captura la pantalla del terminal y directorio actual.
    Capture,
    /// Redimensiona la geometría del terminal.
    Resize {
        #[arg(default_value = "200")]
        cols: u16,
        #[arg(default_value = "50")]
        rows: u16,
    },
    /// Termina la sesión tmux.
    Kill,
    /// Comprueba si la sesión está activa.
    Exists,
}

fn read_stdin_line() -> String {
    let mut buffer = String::new();
    let _ = std::io::stdin().read_line(&mut buffer);
    buffer.trim_end_matches(['\r', '\n']).to_string()
}

/// Ejecuta una acción de helper.
pub fn run_helper(action: HelperAction, json: bool) -> NasResult<ExitCode> {
    let runner = Runner::new();

    let output_msg = match action {
        HelperAction::SambaApply { staged_file } => {
            samba::apply_staged_samba(&runner, &staged_file)?
        }
        HelperAction::ShareDir {
            name,
            unit,
            pos_name,
            pos_unit,
        } => {
            let final_name = name.or(pos_name).ok_or_else(|| {
                NasError::Invalid("falta nombre de recurso (--name o posicional)".into())
            })?;
            let final_unit = unit.or(pos_unit);
            share::create_share_dir(&runner, &final_name, final_unit.as_deref())?
        }
        HelperAction::ShareAcl {
            path,
            principal,
            level,
            pos_path,
            pos_principal,
            pos_level,
        } => {
            let final_path = path.or(pos_path).ok_or_else(|| {
                NasError::Invalid("falta ruta de recurso (--path o posicional)".into())
            })?;
            let final_principal = principal.or(pos_principal).ok_or_else(|| {
                NasError::Invalid("falta principal (--principal o posicional)".into())
            })?;
            let final_level = level
                .or(pos_level)
                .ok_or_else(|| NasError::Invalid("falta nivel (--level o posicional)".into()))?;
            share::apply_share_acl(&runner, &final_path, &final_principal, &final_level)?
        }
        HelperAction::Accounts { action } => match action {
            AccountsAction::CreateUser {
                username,
                full_name,
                password,
                admin,
                web,
                samba,
                groups,
            } => {
                let pass = if password == "-" {
                    read_stdin_line()
                } else {
                    password
                };
                let group_vec: Vec<String> = groups
                    .split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect();
                accounts::create_user(
                    &runner,
                    &accounts::CreateUserArgs {
                        username,
                        full_name,
                        password: pass,
                        admin: admin == "1",
                        web: web == "1",
                        samba: samba == "1",
                        groups: group_vec,
                    },
                )?
            }
            AccountsAction::UpdateUser {
                username,
                full_name,
                password,
                admin,
                web,
                samba,
                groups,
            } => {
                let pass = if password == "-" {
                    let s = read_stdin_line();
                    if s.is_empty() {
                        None
                    } else {
                        Some(s)
                    }
                } else if password == "--keep-password" || password.is_empty() {
                    None
                } else {
                    Some(password)
                };
                let group_vec: Vec<String> = groups
                    .split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect();
                accounts::update_user(
                    &runner,
                    &accounts::UpdateUserArgs {
                        username,
                        full_name,
                        password: pass,
                        admin: admin == "1",
                        web: web == "1",
                        samba: samba == "1",
                        groups: group_vec,
                    },
                )?
            }
            AccountsAction::ToggleUser { username, enable } => {
                let is_enable = enable != "0";
                accounts::toggle_user(&runner, &username, is_enable)?
            }
            AccountsAction::DeleteUser { username } => accounts::delete_user(&runner, &username)?,
            AccountsAction::CreateGroup { group } => accounts::create_group(&runner, &group)?,
            AccountsAction::RenameGroup { old, new } => {
                accounts::rename_group(&runner, &old, &new)?
            }
            AccountsAction::DeleteGroup { group } => accounts::delete_group(&runner, &group)?,
            AccountsAction::AddGroupMember { username, group } => {
                accounts::add_group_member(&runner, &username, &group)?
            }
            AccountsAction::RemoveGroupMember { username, group } => {
                accounts::remove_group_member(&runner, &username, &group)?
            }
            AccountsAction::Status => accounts::accounts_status(&runner)?,
        },
        HelperAction::BackupManage { action } => match action {
            BackupManageAction::Install { id } => backup::install_backup_task(&runner, &id)?,
            BackupManageAction::WriteCron { id, file, content } => {
                backup::write_cron(&id, file.as_deref(), content.as_deref())?
            }
            BackupManageAction::DeleteCron { id } => backup::delete_cron(&id)?,
            BackupManageAction::WriteCred { id, file, content } => {
                backup::write_cred(&id, file.as_deref(), content.as_deref())?
            }
            BackupManageAction::DeleteCred { id } => backup::delete_cred(&id)?,
            BackupManageAction::RunManual { id } => backup::run_backup_task(&runner, &id)?,
            BackupManageAction::Stop { id } => backup::stop_backup_task(&runner, &id)?,
            BackupManageAction::Logs { id, lines } => backup::backup_logs(&id, Some(lines))?,
            BackupManageAction::Delete { id } => backup::delete_backup_task(&id)?,
            BackupManageAction::TestCifs {
                host,
                share,
                user,
                password,
            } => {
                let pass = password.unwrap_or_else(read_stdin_line);
                backup::test_cifs_conn(&runner, &host, &share, &user, &pass)?
            }
            BackupManageAction::TestSsh {
                host,
                port,
                user,
                password,
            } => {
                let pass = password.unwrap_or_else(read_stdin_line);
                backup::test_ssh_conn(&runner, &host, port, &user, &pass)?
            }
            BackupManageAction::Test {
                proto,
                host,
                target,
                port,
                user,
            } => {
                let pass = read_stdin_line();
                match proto.as_str() {
                    "cifs" => backup::test_cifs_conn(&runner, &host, &target, &user, &pass)?,
                    "ssh" => backup::test_ssh_conn(&runner, &host, port, &user, &pass)?,
                    "local" => {
                        let p = Path::new(&target);
                        if !p.is_dir() || target.contains("..") {
                            return Err(NasError::Invalid(format!(
                                "ruta local inexistente o inválida: {target}"
                            )));
                        }
                        format!("ruta local verificada: {target}")
                    }
                    other => {
                        return Err(NasError::Invalid(format!(
                            "protocolo no soportado: {other}"
                        )))
                    }
                }
            }
        },
        HelperAction::StorageApply { action } => match action {
            StorageApplyAction::WipeDisk { confirm, device } => {
                storage::wipe_disk(&runner, &confirm, &device)?
            }
            StorageApplyAction::FormatSingle {
                confirm,
                device,
                fstype,
                unmount,
            } => storage::format_single(&runner, &confirm, &device, Some(&fstype), unmount == "1")?,
            StorageApplyAction::SetupLvm {
                confirm,
                disk,
                vg,
                lv,
                size,
                fstype,
                unmount,
            } => storage::setup_lvm(
                &runner,
                &confirm,
                &disk,
                &vg,
                &lv,
                Some(&size),
                Some(&fstype),
                unmount == "1",
            )?,
            StorageApplyAction::ExtendPool { confirm, device } => {
                storage::extend_pool(&runner, &confirm, &device)?
            }
            StorageApplyAction::CreateSubvolume { confirm, name } => {
                storage::create_subvolume(&runner, &confirm, &name)?
            }
            StorageApplyAction::DeleteSubvolume { confirm, name } => {
                storage::delete_subvolume(&runner, &confirm, &name)?
            }
            StorageApplyAction::MountUnit {
                confirm,
                device,
                unit,
                fstype,
            } => storage::mount_unit(&runner, &confirm, &device, &unit, Some(&fstype))?,
            StorageApplyAction::UmountUnit { confirm, unit } => {
                storage::umount_unit(&runner, &confirm, &unit)?
            }
        },
        HelperAction::Terminal { user, action } => match action {
            TerminalAction::Start { cols, rows } => {
                terminal::start_terminal(&runner, &user, Some(cols), Some(rows))?
            }
            TerminalAction::Keys { input, enter } => {
                terminal::send_terminal_keys(&runner, &user, &input, enter)?;
                String::new()
            }
            TerminalAction::Raw { input } => {
                terminal::send_terminal_keys(&runner, &user, &input, false)?;
                String::new()
            }
            TerminalAction::RawEnter { input } => {
                terminal::send_terminal_keys(&runner, &user, &input, true)?;
                String::new()
            }
            TerminalAction::Key { key_name } => {
                terminal::send_terminal_special_key(&runner, &user, &key_name)?;
                String::new()
            }
            TerminalAction::Capture => terminal::capture_terminal(&runner, &user)?,
            TerminalAction::Resize { cols, rows } => {
                terminal::resize_terminal(&runner, &user, cols, rows)?;
                String::new()
            }
            TerminalAction::Kill => {
                terminal::kill_terminal(&runner, &user)?;
                String::new()
            }
            TerminalAction::Exists => {
                let exists = terminal::terminal_exists(&runner, &user)?;
                if !exists {
                    return Ok(ExitCode::FAILURE);
                }
                String::new()
            }
        },
    };

    if json {
        let resp = ApiResponse::ok(output_msg.clone(), "Operación de helper completada");
        println!("{}", serde_json::to_string_pretty(&resp)?);
    } else if !output_msg.is_empty() {
        println!("{output_msg}");
    }

    Ok(ExitCode::SUCCESS)
}
