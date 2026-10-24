//! Estado de la aplicación TUI y carga de datos desde el motor.

use nas_core::NasPaths;
use nas_engine::diagnostics::Check;
use nas_engine::domain::DomainStatus;
use nas_engine::parsers::smb::Share;
use nas_engine::services::ServiceStatus;
use nas_engine::storage::{Disk, StorageOverview};
use nas_engine::tasks::BackupTask;
use nas_engine::users::{GroupEntry, UserEntry};
use nas_engine::{NetworkOverview, SystemMetrics};
use nas_exec::Runner;

/// Secciones del asistente.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Section {
    /// Métricas del sistema.
    Dashboard,
    /// Inventario de red.
    Network,
    /// Recursos compartidos.
    Shares,
    /// Usuarios y grupos.
    Users,
    /// Tareas de respaldo.
    Backups,
    /// Demonios del sistema.
    Services,
    /// Diagnóstico integral.
    Diagnostics,
    /// Almacenamiento y discos.
    Storage,
    /// Explorador de archivos.
    Files,
    /// Integración con Active Directory.
    Domain,
    /// Terminal interactivo (vía panel web).
    Terminal,
    /// Salir del asistente.
    Quit,
}

impl Section {
    /// Todas las secciones, en orden de menú.
    pub const ALL: [Section; 12] = [
        Section::Dashboard,
        Section::Network,
        Section::Shares,
        Section::Users,
        Section::Backups,
        Section::Services,
        Section::Diagnostics,
        Section::Storage,
        Section::Files,
        Section::Domain,
        Section::Terminal,
        Section::Quit,
    ];

    /// Título mostrado en el menú.
    pub fn title(self) -> &'static str {
        match self {
            Section::Dashboard => "Dashboard",
            Section::Network => "Red",
            Section::Shares => "Recursos",
            Section::Users => "Usuarios",
            Section::Backups => "Respaldos",
            Section::Services => "Servicios",
            Section::Diagnostics => "Diagnóstico",
            Section::Storage => "Almacenamiento",
            Section::Files => "Archivos",
            Section::Domain => "Dominio",
            Section::Terminal => "Terminal",
            Section::Quit => "Salir",
        }
    }
}

/// Entrada del explorador de archivos.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// Nombre del archivo o carpeta.
    pub name: String,
    /// Ruta relativa a la raíz.
    pub path: String,
    /// Indica si es un directorio.
    pub is_dir: bool,
}

/// Lista un directorio relativo a `/srv/nas`, rechazando escapes.
pub fn list_files(root: &std::path::Path, relative: &str) -> nas_core::NasResult<Vec<Entry>> {
    use nas_core::NasError;

    let relative = relative.trim();
    if relative.starts_with('/') || relative.split('/').any(|part| part == "..") {
        return Err(NasError::Denied("ruta fuera del almacenamiento".into()));
    }

    let base = std::fs::canonicalize(root).map_err(|e| NasError::io(root, e))?;
    let dir = if relative.is_empty() {
        base.clone()
    } else {
        let joined = base.join(relative);
        std::fs::canonicalize(&joined).map_err(|e| NasError::io(&joined, e))?
    };
    if !dir.starts_with(&base) {
        return Err(NasError::Denied("ruta fuera del almacenamiento".into()));
    }

    let mut entries = Vec::new();
    for item in std::fs::read_dir(&dir).map_err(|e| NasError::io(&dir, e))? {
        let item = item.map_err(|e| NasError::io(&dir, e))?;
        let name = item.file_name().to_string_lossy().to_string();
        if name == ".trash" {
            continue;
        }
        let is_dir = item.metadata().map(|m| m.is_dir()).unwrap_or(false);
        let path = if relative.is_empty() {
            name.clone()
        } else {
            format!("{relative}/{name}")
        };
        entries.push(Entry { name, path, is_dir });
    }
    entries.sort_by(|a, b| b.is_dir.cmp(&a.is_dir).then(a.name.cmp(&b.name)));
    Ok(entries)
}

/// Datos cargados para la sección activa.
#[derive(Debug, Clone, Default)]
pub enum Data {
    /// Sin cargar.
    #[default]
    Empty,
    /// Métricas del sistema.
    Dashboard(SystemMetrics),
    /// Inventario de red.
    Network(NetworkOverview),
    /// Recursos compartidos.
    Shares(Vec<Share>),
    /// Usuarios y grupos.
    Users(Vec<UserEntry>, Vec<GroupEntry>),
    /// Tareas de respaldo con su último snapshot.
    Backups(Vec<(BackupTask, Option<String>)>),
    /// Estado de servicios.
    Services(Vec<ServiceStatus>),
    /// Diagnóstico.
    Diagnostics(Vec<Check>),
    /// Almacenamiento.
    Storage(Option<StorageOverview>, Vec<Disk>),
    /// Explorador de archivos.
    Files(Vec<Entry>),
    /// Dominio.
    Domain(DomainStatus),
    /// Mensaje informativo.
    Message(String),
}

/// Estado de la aplicación.
#[derive(Debug)]
pub struct App {
    /// Índice de la sección activa.
    pub section: usize,
    /// Selección dentro del listado actual.
    pub selected: usize,
    /// Ruta actual del explorador.
    pub path: String,
    /// Datos de la sección.
    pub data: Data,
    /// Mensaje de estado.
    pub status: String,
    /// Overlay con texto extra (plan de respaldo, ayuda).
    pub overlay: Option<String>,
    /// Indica que el usuario pidió salir.
    pub should_quit: bool,
}

impl Default for App {
    fn default() -> Self {
        Self::new()
    }
}

impl App {
    /// Crea la aplicación en la primera sección.
    pub fn new() -> Self {
        Self {
            section: 0,
            selected: 0,
            path: String::new(),
            data: Data::Empty,
            status: "F5 actualiza · Enter abre · Retroceso sube · q sale".to_string(),
            overlay: None,
            should_quit: false,
        }
    }

    /// Sección activa.
    pub fn current(&self) -> Section {
        Section::ALL[self.section.min(Section::ALL.len() - 1)]
    }

    /// Avanza a la sección siguiente.
    pub fn next_section(&mut self) {
        self.section = (self.section + 1) % Section::ALL.len();
        self.selected = 0;
        self.overlay = None;
    }

    /// Retrocede a la sección anterior.
    pub fn previous_section(&mut self) {
        self.section = if self.section == 0 {
            Section::ALL.len() - 1
        } else {
            self.section - 1
        };
        self.selected = 0;
        self.overlay = None;
    }

    /// Mueve la selección hacia abajo.
    pub fn move_down(&mut self, len: usize) {
        if len > 0 {
            self.selected = (self.selected + 1).min(len - 1);
        }
    }

    /// Mueve la selección hacia arriba.
    pub fn move_up(&mut self) {
        self.selected = self.selected.saturating_sub(1);
    }

    /// Número de elementos del listado actual.
    pub fn item_count(&self) -> usize {
        match &self.data {
            Data::Shares(items) => items.len(),
            Data::Users(users, _) => users.len(),
            Data::Backups(items) => items.len(),
            Data::Services(items) => items.len(),
            Data::Diagnostics(items) => items.len(),
            Data::Storage(_, disks) => disks.len(),
            Data::Files(items) => items.len(),
            _ => 0,
        }
    }

    /// Carga los datos de la sección activa.
    pub fn refresh(&mut self, runner: &Runner, paths: &NasPaths) {
        self.data = match self.current() {
            Section::Dashboard => Data::Dashboard(nas_engine::collect_status(runner)),
            Section::Network => Data::Network(nas_engine::collect_network(runner, paths)),
            Section::Shares => match std::fs::read_to_string(&paths.samba_conf) {
                Ok(text) => Data::Shares(nas_engine::parsers::smb::parse_shares(&text)),
                Err(error) => Data::Message(format!("no se pudo leer smb.conf: {error}")),
            },
            Section::Users => {
                let (mut users, mut groups) = nas_engine::users::load_local();
                users.retain(|user| !user.system);
                groups.retain(|group| group.name.starts_with("grp_"));
                Data::Users(users, groups)
            }
            Section::Backups => {
                let tasks = BackupTask::discover("/usr/local/bin").unwrap_or_default();
                let list = tasks
                    .into_iter()
                    .map(|task| {
                        let last = nas_engine::last_snapshot(&task).map(|path| {
                            path.file_name()
                                .map(|name| name.to_string_lossy().to_string())
                                .unwrap_or_default()
                        });
                        (task, last)
                    })
                    .collect();
                Data::Backups(list)
            }
            Section::Services => Data::Services(nas_engine::services::collect(
                runner,
                nas_engine::services::DEFAULT_SERVICES,
            )),
            Section::Diagnostics => {
                Data::Diagnostics(nas_engine::diagnostics::collect(paths, runner))
            }
            Section::Storage => Data::Storage(
                nas_engine::storage::overview_for(paths, runner).ok(),
                nas_engine::storage::disks(runner).unwrap_or_default(),
            ),
            Section::Files => self.load_files(),
            Section::Domain => Data::Domain(nas_engine::domain::status(runner)),
            Section::Terminal => Data::Message(
                "El terminal interactivo está disponible en el panel web (/terminal).".to_string(),
            ),
            Section::Quit => {
                self.should_quit = true;
                Data::Message("Saliendo…".to_string())
            }
        };

        if self.selected >= self.item_count() {
            self.selected = self.item_count().saturating_sub(1);
        }

        self.status = format!("{} actualizado", self.current().title());
    }

    fn load_files(&self) -> Data {
        match list_files(std::path::Path::new("/srv/nas"), &self.path) {
            Ok(entries) => Data::Files(entries),
            Err(error) => Data::Message(error.to_string()),
        }
    }

    /// Acción de Enter sobre la sección activa.
    pub fn activate(&mut self) {
        match self.current() {
            Section::Files => {
                let target = match &self.data {
                    Data::Files(entries) => entries.get(self.selected).cloned(),
                    _ => None,
                };
                if let Some(entry) = target {
                    if entry.is_dir {
                        self.path = entry.path;
                        self.selected = 0;
                    } else {
                        self.status = format!("Archivo: {}", entry.name);
                    }
                }
            }
            Section::Backups => {
                let plan = match &self.data {
                    Data::Backups(items) => items.get(self.selected).map(|(task, last)| {
                        let plan = task.plan(
                            last.as_ref().map(|_| std::path::Path::new("/dev/null")),
                            &chrono::Local::now(),
                        );
                        plan.iter()
                            .map(|step| nas_core::shell::quote_all(step))
                            .collect::<Vec<_>>()
                            .join("\n")
                    }),
                    _ => None,
                };
                if let Some(plan) = plan {
                    self.overlay = Some(format!("PLAN DE RESPALDO\n\n{plan}"));
                }
            }
            Section::Services => {
                let service = match &self.data {
                    Data::Services(items) => {
                        items.get(self.selected).map(|item| item.service.clone())
                    }
                    _ => None,
                };
                if let Some(service) = service {
                    let result =
                        nas_engine::services::manage(&Runner::new(), &service, "restart", true);
                    self.status = match result {
                        Ok(()) => format!("{service}: reiniciado"),
                        Err(error) => format!("{service}: {error}"),
                    };
                }
            }
            Section::Quit => self.should_quit = true,
            _ => {}
        }
    }

    /// Retrocede al directorio padre en el explorador.
    pub fn go_up(&mut self) {
        if self.current() != Section::Files {
            return;
        }
        if let Some((parent, _)) = self.path.rsplit_once('/') {
            self.path = parent.to_string();
            self.selected = 0;
        } else {
            self.path.clear();
            self.selected = 0;
        }
    }
}
