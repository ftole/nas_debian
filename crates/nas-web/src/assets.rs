//! Assets originales del panel, servidos byte a byte desde el binario.

/// Comportamiento del panel (sin modificar).
pub const APP_JS: &[u8] = include_bytes!("../assets/js/app.js");
/// Helpers compartidos (lib): API.
pub const LIB_API_JS: &[u8] = include_bytes!("../assets/js/lib/api.js");
/// Helpers compartidos (lib): modales.
pub const LIB_MODAL_JS: &[u8] = include_bytes!("../assets/js/lib/modal.js");
/// Helpers compartidos (lib): utilidades.
pub const LIB_UTILS_JS: &[u8] = include_bytes!("../assets/js/lib/utils.js");
/// Helpers compartidos (lib): toasts.
pub const LIB_TOAST_JS: &[u8] = include_bytes!("../assets/js/lib/toast.js");
/// Módulos por dominio: almacenamiento.
pub const MODULE_STORAGE_JS: &[u8] = concat!(
    include_str!("../assets/js/modules/storage.js"),
    include_str!("../assets/js/modules/storage/wizard.js"),
    include_str!("../assets/js/modules/storage/pools.js"),
)
.as_bytes();
/// Módulos por dominio: usuarios.
pub const MODULE_USERS_JS: &[u8] = concat!(
    include_str!("../assets/js/modules/users.js"),
    include_str!("../assets/js/modules/users/crud.js"),
    include_str!("../assets/js/modules/users/permissions.js"),
)
.as_bytes();
/// Visor de hojas de cálculo.
pub const XLSX_JS: &[u8] = include_bytes!("../assets/js/vendor/xlsx.full.min.js");
/// Compresión en el navegador.
pub const JSZIP_JS: &[u8] = include_bytes!("../assets/js/vendor/jszip.min.js");
/// Vista previa de documentos Word.
pub const DOCX_JS: &[u8] = include_bytes!("../assets/js/vendor/docx-preview.min.js");

/// Módulos por dominio: backups.
pub const MODULE_BACKUPS_JS: &[u8] = concat!(
    include_str!("../assets/js/modules/backups.js"),
    include_str!("../assets/js/modules/backups/actions.js"),
)
.as_bytes();

/// Módulos por dominio: dashboard.
pub const MODULE_DASHBOARD_JS: &[u8] = include_bytes!("../assets/js/modules/dashboard.js");

/// Módulos por dominio: domain.
pub const MODULE_DOMAIN_JS: &[u8] = include_bytes!("../assets/js/modules/domain.js");

/// Módulos por dominio: files.
pub const MODULE_FILES_JS: &[u8] = concat!(
    include_str!("../assets/js/modules/files.js"),
    include_str!("../assets/js/modules/files/render.js"),
    include_str!("../assets/js/modules/files/actions.js"),
    include_str!("../assets/js/modules/files/trash.js"),
    include_str!("../assets/js/modules/files/preview.js"),
    include_str!("../assets/js/modules/files/viewers.js"),
)
.as_bytes();

/// Módulos por dominio: logs.
pub const MODULE_LOGS_JS: &[u8] = include_bytes!("../assets/js/modules/logs.js");

/// Módulos por dominio: misc.
pub const MODULE_MISC_JS: &[u8] = include_bytes!("../assets/js/modules/misc.js");

/// Módulos por dominio: services.
pub const MODULE_SERVICES_JS: &[u8] = include_bytes!("../assets/js/modules/services.js");

/// Módulos por dominio: shares.
pub const MODULE_SHARES_JS: &[u8] = include_bytes!("../assets/js/modules/shares.js");

/// Módulos por dominio: system.
pub const MODULE_SYSTEM_JS: &[u8] = concat!(
    include_str!("../assets/js/modules/system.js"),
    include_str!("../assets/js/modules/system/apps.js"),
)
.as_bytes();

/// Módulos por dominio: terminal.
pub const MODULE_TERMINAL_JS: &[u8] = include_bytes!("../assets/js/modules/terminal.js");
