//! Implementación del comando `nas helper` y sus subcomandos fuertemente tipados.

use std::path::PathBuf;

use clap::Subcommand;

mod dispatch;

pub use dispatch::run_helper;

/// Acciones disponibles bajo `nas helper`.
#[derive(Debug, Subcommand)]
pub enum HelperAction {
    /// Aplica una configuración validada de Samba desde staging.
    #[command(alias = "samba_apply")]
    SambaApply {
        /// Archivo de configuración en staging (/var/lib/nas/staging/smbconf_*).
        staged_file: PathBuf,
    },
    /// Crea el directorio de un recurso compartido con permisos 2770.
    #[command(alias = "share_dir")]
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
    #[command(alias = "share_acl")]
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
    #[command(alias = "backup_manage")]
    BackupManage {
        #[command(subcommand)]
        action: BackupManageAction,
    },
    /// Operaciones de almacenamiento físico, LVM y Btrfs.
    #[command(alias = "storage_apply")]
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
    /// Purga archivos en las papeleras de reciclaje Samba (.recycle).
    #[command(alias = "purge_recycle")]
    PurgeRecycle {
        /// Días de antigüedad para purgar (por defecto: 15).
        #[arg(long, default_value_t = 15)]
        days: u32,
    },
}

/// Subcomandos de gestión de cuentas (`nas helper accounts`).
#[derive(Debug, Subcommand)]
pub enum AccountsAction {
    /// Crea un usuario nuevo.
    #[command(alias = "create_user")]
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
    #[command(alias = "update_user")]
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
    #[command(alias = "toggle_user")]
    ToggleUser {
        username: String,
        #[arg(default_value = "1")]
        enable: String,
    },
    /// Elimina una cuenta de usuario.
    #[command(alias = "delete_user")]
    DeleteUser { username: String },
    /// Crea un grupo corporativo.
    #[command(alias = "create_group")]
    CreateGroup { group: String },
    /// Renombra un grupo corporativo.
    #[command(alias = "rename_group")]
    RenameGroup { old: String, new: String },
    /// Elimina un grupo corporativo.
    #[command(alias = "delete_group")]
    DeleteGroup { group: String },
    /// Añade un usuario a un grupo.
    #[command(alias = "add-member", alias = "add_group_member", alias = "add_member")]
    AddGroupMember { username: String, group: String },
    /// Quita un usuario de un grupo.
    #[command(
        alias = "remove-member",
        alias = "remove_group_member",
        alias = "remove_member"
    )]
    RemoveGroupMember { username: String, group: String },
    /// Reporta el estado TSV de las cuentas gestionables.
    Status,
}

/// Subcomandos de tareas de respaldo (`nas helper backup-manage`).
#[derive(Debug, Subcommand)]
pub enum BackupManageAction {
    /// Instala una tarea preparada desde /var/lib/nas/staging.
    Install { id: String },
    /// Valida e instala una tarea a partir de su especificación JSON por entrada estándar.
    #[command(alias = "install_spec")]
    InstallSpec,
    /// Escribe o actualiza el cron de una tarea.
    #[command(alias = "write_cron")]
    WriteCron {
        id: String,
        #[arg(long)]
        file: Option<PathBuf>,
        #[arg(long)]
        content: Option<String>,
        /// Archivo o '-' para stdin posicional (compatibilidad).
        pos_file: Option<PathBuf>,
    },
    /// Elimina el cron de una tarea.
    #[command(alias = "delete_cron")]
    DeleteCron { id: String },
    /// Escribe o actualiza el archivo de credenciales.
    #[command(alias = "write_cred")]
    WriteCred {
        id: String,
        #[arg(long)]
        file: Option<PathBuf>,
        #[arg(long)]
        content: Option<String>,
        /// Archivo o '-' para stdin posicional (compatibilidad).
        pos_file: Option<PathBuf>,
    },
    /// Elimina el archivo de credenciales.
    #[command(alias = "delete_cred")]
    DeleteCred { id: String },
    /// Ejecuta manualmente una tarea de respaldo en segundo plano.
    #[command(alias = "run", alias = "run_manual")]
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
    #[command(alias = "test_cifs")]
    TestCifs {
        host: String,
        share: String,
        user: String,
        #[arg(long)]
        password: Option<String>,
    },
    /// Prueba la conectividad SSH.
    #[command(alias = "test_ssh")]
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
    /// Congela el último snapshot como respaldo permanente inmutable.
    Freeze {
        /// Identificador de la tarea.
        id: String,
        /// Etiqueta descriptiva opcional.
        tag: Option<String>,
    },
    /// Lista los respaldos permanentes inmutables (Golden Snapshots).
    #[command(alias = "list_permanent")]
    ListPermanent {
        /// Identificador de la tarea para filtrar.
        id: Option<String>,
    },
    /// Restaura un snapshot hacia el origen de red o destino local.
    Restore {
        /// Identificador de la tarea.
        id: String,
        /// Nombre o ruta del snapshot a restaurar.
        snapshot: String,
        /// Destino local personalizado opcional.
        #[arg(long)]
        target: Option<String>,
    },
    /// Elimina un respaldo permanente inmutable.
    #[command(alias = "delete_permanent")]
    DeletePermanent {
        /// Identificador de la tarea.
        id: String,
        /// Nombre de la carpeta del respaldo (ej. FULL_2026-10-30_190000).
        name: String,
    },
}

/// Subcomandos de almacenamiento (`nas helper storage-apply`).
#[derive(Debug, Subcommand)]
pub enum StorageApplyAction {
    /// Limpia firmas y particiones de un disco.
    #[command(alias = "wipe_disk")]
    WipeDisk { confirm: String, device: String },
    /// Formatea un disco/partición y lo monta en /srv/nas.
    #[command(alias = "format", alias = "format_single")]
    FormatSingle {
        confirm: String,
        device: String,
        #[arg(default_value = "ext4")]
        fstype: String,
        #[arg(default_value = "0")]
        unmount: String,
    },
    /// Configura LVM sobre un disco y lo monta en /srv/nas.
    #[command(alias = "lvm", alias = "setup_lvm")]
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
    #[command(alias = "extend_pool")]
    ExtendPool { confirm: String, device: String },
    /// Crea un subvolumen Btrfs en /srv/nas.
    #[command(alias = "subvolume", alias = "create_subvolume")]
    CreateSubvolume { confirm: String, name: String },
    /// Elimina un subvolumen Btrfs en /srv/nas.
    #[command(alias = "delete_subvolume")]
    DeleteSubvolume { confirm: String, name: String },
    /// Formatea y monta una unidad en /srv/nas/unidades/<unidad>.
    #[command(alias = "add-unit", alias = "add_unit", alias = "mount_unit")]
    MountUnit {
        confirm: String,
        device: String,
        unit: String,
        #[arg(default_value = "ext4")]
        fstype: String,
    },
    /// Desmonta una unidad y la retira de /etc/fstab.
    #[command(alias = "umount_unit")]
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
    #[command(alias = "raw_enter")]
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
