//! Vistas HTML del panel con la identidad visual del panel Slate original.
//!
//! Reutiliza la hoja de estilos y el sprite SVG del panel PHP (incrustados en el
//! binario) para conservar exactamente la misma apariencia, iconografía y
//! estructura: masthead, barra lateral, KPIs, tarjetas y tablas.

use nas_engine::diagnostics::Check;
use nas_engine::parsers::smb::Share;
use nas_engine::services::ServiceStatus;
use nas_engine::storage::{Disk, StorageOverview};
use nas_engine::tasks::BackupTask;
use nas_engine::users::{GroupEntry, UserEntry};
use nas_engine::SystemMetrics;

use crate::files::Entry;

/// Hoja de estilos del panel (idéntica al diseño Slate).
pub const APP_CSS: &str = include_str!("../assets/app.css");
/// Sprite de iconos SVG.
pub const ICONS_SVG: &str = include_str!("../assets/icons.svg");
/// Hoja de estilos de la pantalla de acceso.
pub const LOGIN_CSS: &str = include_str!("../assets/login.css");
/// Favicon del panel.
pub const FAVICON: &[u8] = include_bytes!("../assets/favicon.svg");

/// Secciones de navegación: (slug, etiqueta, icono, grupo).
pub const SECTIONS: &[(&str, &str, &str, &str)] = &[
    ("dashboard", "Vista general", "icon-dashboard", "GENERAL"),
    ("files", "Archivos", "icon-folder", "GENERAL"),
    ("shares", "Recursos", "icon-folder", "GENERAL"),
    ("users", "Usuarios", "icon-users", "GENERAL"),
    ("storage", "Almacenamiento", "icon-hard-drive", "GENERAL"),
    ("backups", "Respaldos", "icon-archive", "OPERACIÓN"),
    ("services", "Servicios", "icon-services", "OPERACIÓN"),
    ("diagnostics", "Diagnóstico", "icon-wrench", "OPERACIÓN"),
    ("domain", "Dominio", "icon-domain", "SISTEMA"),
    ("terminal", "Terminal", "icon-terminal", "SISTEMA"),
    ("audit", "Auditoría", "icon-shield", "SISTEMA"),
];

/// Contexto de la página.
#[derive(Debug)]
pub struct PageCtx<'a> {
    /// Sección activa.
    pub active: &'a str,
    /// Usuario autenticado.
    pub user: &'a str,
    /// Rol del servidor.
    pub role: &'a str,
    /// Hostname del servidor.
    pub hostname: &'a str,
    /// Grupo de trabajo.
    pub workgroup: &'a str,
}

/// Escapa texto para HTML.
pub fn esc(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

fn icon(name: &str, class: &str) -> String {
    format!("<svg class=\"icon {class}\"><use href=\"#{name}\"></use></svg>")
}

fn badge(kind: &str, text: &str) -> String {
    format!("<span class=\"badge badge-{kind}\">{}</span>", esc(text))
}

/// Estructura completa con masthead, barra lateral y contenido.
pub fn layout(ctx: &PageCtx<'_>, title: &str, body: &str) -> String {
    let mut nav = String::new();
    let mut current_group = "";
    for (slug, label, icon_name, group) in SECTIONS {
        if *group != current_group {
            current_group = group;
            nav.push_str(&format!(
                "<div class=\"nav-section-title\">{}</div>",
                esc(group)
            ));
        }
        let active = if *slug == ctx.active { " active" } else { "" };
        nav.push_str(&format!(
            "<a class=\"nav-item{active}\" href=\"/{slug}\"><div class=\"nav-item-left\">{}{}<span>{}</span></div></a>",
            icon(icon_name, "icon-sm"),
            "",
            esc(label)
        ));
    }

    format!(
        r##"<!DOCTYPE html>
<html lang="es" data-theme="dark"><head>
<meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1">
<title>{title} • Panel NAS {hostname}</title>
<link rel="icon" type="image/svg+xml" href="/favicon.svg">
<script>try{{var t=localStorage.getItem('nas_theme');if(t){{document.documentElement.setAttribute('data-theme',t);}}}}catch(e){{}}</script>
<link rel="stylesheet" href="/css/app.css">
</head><body>
{ICONS_SVG}
<header class="masthead">
  <div class="masthead-main">
    <button class="mobile-toggle" onclick="toggleSidebar()" title="Alternar menú de navegación">
      <svg class="icon"><use href="#icon-menu"></use></svg>
    </button>
    <div class="masthead-brand">
      {server_icon}
      <span class="brand-logo-text"><span>NAS</span><span class="brand-divider">|</span><span class="brand-hostname">{hostname}</span></span>
      <span class="brand-badge">Debian 13</span>
    </div>
  </div>
  <div class="masthead-tools">
    <div class="header-meta-pill"><span>Rol:</span> <strong>{role}</strong></div>
    <div class="header-meta-pill"><span>Workgroup:</span> <strong>{workgroup}</strong></div>
    <button class="btn btn-secondary btn-sm" id="btn-theme-toggle" onclick="toggleTheme()" title="Alternar tema claro/oscuro">
      <svg class="icon icon-sm" id="theme-toggle-icon"><use href="#icon-sun"></use></svg>
    </button>
    <div class="user-pill"><span class="status-dot status-ok"></span><span class="user-name">{user}</span></div>
    <form method="post" action="/logout" style="display:inline">
      <button type="submit" class="btn btn-secondary btn-sm" title="Cerrar sesión">{power_icon}<span class="btn-text-responsive">Cerrar sesión</span></button>
    </form>
  </div>
</header>
<div class="app-layout">
  <aside class="sidebar"><nav class="sidebar-nav">{nav}</nav></aside>
  <div class="sidebar-backdrop" id="sidebar-backdrop" onclick="toggleSidebar()"></div>
  <main class="main-content">
    <section class="view-section active">{body}</section>
  </main>
</div>
<script>
function toggleSidebar(){{
  var sidebar=document.querySelector('.sidebar');
  var backdrop=document.getElementById('sidebar-backdrop');
  if(!sidebar) return;
  sidebar.classList.toggle('open');
  if(backdrop) backdrop.classList.toggle('active');
}}
function toggleTheme(){{
  var root=document.documentElement;
  var next=root.getAttribute('data-theme')==='light'?'dark':'light';
  root.setAttribute('data-theme',next);
  try{{localStorage.setItem('nas_theme',next);}}catch(e){{}}
  swapThemeIcon(next);
}}
function swapThemeIcon(theme){{
  var icon=document.getElementById('theme-toggle-icon');
  if(!icon) return;
  icon.innerHTML='<use href="#'+(theme==='light'?'icon-sun':'icon-moon')+'"></use>';
}}
document.addEventListener('DOMContentLoaded',function(){{
  swapThemeIcon(document.documentElement.getAttribute('data-theme')||'dark');
}});
</script>
</body></html>"##,
        title = esc(title),
        hostname = esc(ctx.hostname),
        role = esc(ctx.role),
        workgroup = esc(ctx.workgroup),
        user = esc(ctx.user),
        server_icon = icon("icon-server", ""),
        power_icon = icon("icon-power", "icon-sm"),
    )
}

fn page_head(title: &str, subtitle: &str, icon_name: &str, actions: &str) -> String {
    format!(
        "<div class=\"page-head\"><div><h2>{} {}</h2><p>{}</p></div><div class=\"page-head-actions\">{}</div></div>",
        icon(icon_name, ""),
        esc(title),
        esc(subtitle),
        actions
    )
}

fn kpi(label: &str, value: &str, icon_name: &str, sub: &str) -> String {
    format!(
        "<div class=\"kpi-card\"><div class=\"kpi-header\"><span>{}</span><div class=\"kpi-icon-wrap\">{}</div></div>\
         <div class=\"kpi-val\">{}</div><div class=\"kpi-sub\"><span>{}</span></div></div>",
        esc(label),
        icon(icon_name, "icon-sm"),
        value,
        esc(sub)
    )
}

fn table(headers: &[&str], rows: &[String]) -> String {
    let head: String = headers
        .iter()
        .map(|header| format!("<th>{}</th>", esc(header)))
        .collect();
    let body: String = if rows.is_empty() {
        format!(
            "<tr><td colspan=\"{}\" style=\"text-align:center; color:var(--text-muted);\">Sin datos disponibles.</td></tr>",
            headers.len()
        )
    } else {
        rows.concat()
    };
    format!(
        "<div class=\"table-responsive\"><table class=\"nas-table\"><thead><tr>{head}</tr></thead><tbody>{body}</tbody></table></div>"
    )
}

fn panel(title: &str, icon_name: &str, content: &str) -> String {
    format!(
        "<div class=\"panel-card\"><div class=\"panel-card-head\"><h3>{} {}</h3></div>{}</div>",
        icon(icon_name, "icon-sm"),
        esc(title),
        content
    )
}

/// Pantalla de acceso con la identidad visual original.
pub fn login(hostname: &str, error: Option<&str>) -> String {
    let alert_class = if error.is_some() {
        "login-alert visible"
    } else {
        "login-alert"
    };
    let alert_text = esc(error.unwrap_or(""));

    format!(
        r##"<!DOCTYPE html>
<html lang="es" data-theme="dark"><head>
<meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1, shrink-to-fit=no">
<title>Iniciar Sesión • {hostname} (Debian 13)</title>
<link rel="icon" type="image/svg+xml" href="/favicon.svg">
<link rel="stylesheet" href="/css/app.css">
<link rel="stylesheet" href="/css/login.css">
</head>
<body class="login-page">
{ICONS_SVG}
<div class="login-container">
  <div class="login-card">
    <div class="login-header">
      <div class="login-brand">
        {server_icon}
        <div class="login-title-group">
          <h1><span>NAS</span> <span style="opacity:0.4; font-weight:300;">|</span> <span style="color:var(--accent-primary);">{hostname}</span></h1>
          <p class="login-subtitle">Servidor NAS Departamental &amp; Central de Respaldos</p>
        </div>
      </div>
    </div>
    <div class="login-body">
      <div id="login-alert" class="{alert_class}">
        {alert_icon}
        <span id="login-alert-text">{alert_text}</span>
      </div>
      <form id="login-form" method="POST" action="/login">
        <div class="form-group" style="margin-bottom:16px;">
          <label for="username">Usuario:</label>
          <div class="input-with-icon">
            <input type="text" id="username" name="username" required autofocus placeholder="usuario del panel" autocomplete="username">
          </div>
        </div>
        <div class="form-group" style="margin-bottom:20px;">
          <label for="password">Contraseña:</label>
          <div class="input-with-icon">
            <input type="password" id="password" name="password" required placeholder="Contraseña de acceso" autocomplete="current-password">
            <button type="button" class="input-addon-btn" id="btn-toggle-pass" title="Mostrar/ocultar contraseña" onclick="togglePassVisibility()">
              <svg class="icon icon-sm" id="icon-pass-toggle"><use href="#icon-eye"></use></svg>
            </button>
          </div>
        </div>
        <button type="submit" class="btn btn-primary" id="btn-submit" style="width:100%; justify-content:center; padding:10px;">
          {lock_icon}<span>Iniciar sesión</span>
        </button>
      </form>
    </div>
    <div class="login-footer"><span>Debian 13 (Trixie)</span><span>Acceso seguro</span></div>
  </div>
</div>
<script>
function togglePassVisibility(){{
  const input=document.getElementById('password');
  const icon=document.getElementById('icon-pass-toggle');
  const hidden=input.type==='password';
  input.type=hidden?'text':'password';
  if(icon) icon.innerHTML='<use href="#'+(hidden?'icon-eye-off':'icon-eye')+'"></use>';
}}
</script>
</body></html>"##,
        hostname = esc(hostname),
        server_icon = icon("icon-server", "login-brand-icon"),
        alert_icon = icon("icon-alert-triangle", "icon-sm"),
        lock_icon = icon("icon-lock", ""),
    )
}

/// Dashboard con KPIs del sistema.
pub fn dashboard(metrics: &SystemMetrics, role: &str) -> String {
    let body = format!(
        "<div class=\"kpi-grid\">{}{}{}{}{}{}</div>{}",
        kpi("Hostname", &esc(&metrics.hostname), "icon-server", "Servidor NAS"),
        kpi("Sistema", &esc(&metrics.os), "icon-info", &metrics.kernel),
        kpi("Uptime", &esc(&metrics.uptime), "icon-clock", "Tiempo encendido"),
        kpi(
            "CPU",
            &format!("{:.1}%", metrics.cpu_usage_pct),
            "icon-cpu",
            &metrics.cpu_model
        ),
        kpi(
            "Memoria",
            &format!("{:.1}%", metrics.ram_usage_pct),
            "icon-hard-drive",
            &format!("{:.2} / {:.2} GiB", metrics.ram_used_gb, metrics.ram_total_gb)
        ),
        kpi(
            "IPv4",
            &esc(metrics.ip.as_deref().unwrap_or("N/D")),
            "icon-network",
            "Dirección primaria"
        ),
        panel(
            "Información del servidor",
            "icon-server",
            &format!(
                "<div class=\"panel-card-body\"><div class=\"info-list\">\
                 <div class=\"info-row\"><span class=\"info-label\">Rol</span><span class=\"info-val\">{}</span></div>\
                 <div class=\"info-row\"><span class=\"info-label\">Kernel</span><span class=\"info-val\"><code>{}</code></span></div>\
                 <div class=\"info-row\"><span class=\"info-label\">Hora del servidor</span><span class=\"info-val\">{} {}</span></div>\
                 </div></div>",
                esc(role),
                esc(&metrics.kernel),
                esc(&metrics.server_time),
                esc(&metrics.timezone_abbr)
            )
        )
    );
    format!(
        "{}{}",
        page_head(
            "Vista general",
            "Estado del servidor, recursos y servicios principales",
            "icon-dashboard",
            ""
        ),
        body
    )
}

/// Recursos compartidos.
pub fn shares(shares: &[Share]) -> String {
    let rows: Vec<String> = shares
        .iter()
        .map(|share| {
            format!(
                "<tr><td><strong>{}</strong></td><td><code>{}</code></td><td>{}</td><td>{}</td><td>{}</td></tr>",
                esc(&share.name),
                esc(share.path.as_deref().unwrap_or("-")),
                if share.read_only { badge("gray", "Solo lectura") } else { badge("ok", "Lectura/Escritura") },
                if share.browseable { badge("ok", "Visible") } else { badge("gray", "Oculto ($)") },
                esc(&share.valid_users.join(", "))
            )
        })
        .collect();

    format!(
        "{}{}",
        page_head(
            "Recursos compartidos",
            &format!("{} recursos definidos en Samba", shares.len()),
            "icon-folder",
            ""
        ),
        panel(
            "Recursos SMB",
            "icon-folder",
            &table(
                &["Recurso", "Ruta", "Permiso", "Visibilidad", "Usuarios"],
                &rows
            )
        )
    )
}

/// Usuarios y grupos.
pub fn users(users: &[UserEntry], groups: &[GroupEntry]) -> String {
    let user_rows: Vec<String> = users
        .iter()
        .map(|user| {
            format!(
                "<tr><td><strong>{}</strong></td><td>{}</td><td>{}</td><td><code>{}</code></td><td><code>{}</code></td></tr>",
                esc(&user.name),
                user.uid,
                user.gid,
                esc(&user.home),
                esc(&user.shell)
            )
        })
        .collect();
    let group_rows: Vec<String> = groups
        .iter()
        .map(|group| {
            format!(
                "<tr><td><strong>{}</strong></td><td>{}</td><td>{}</td></tr>",
                esc(&group.name),
                group.gid,
                esc(&group.members.join(", "))
            )
        })
        .collect();

    format!(
        "{}{}{}",
        page_head(
            "Usuarios y grupos",
            "Cuentas administrables y grupos corporativos",
            "icon-users",
            ""
        ),
        panel(
            &format!("Usuarios ({})", users.len()),
            "icon-users",
            &table(&["Usuario", "UID", "GID", "Home", "Shell"], &user_rows)
        ),
        panel(
            &format!("Grupos corporativos ({})", groups.len()),
            "icon-users",
            &table(&["Grupo", "GID", "Miembros"], &group_rows)
        )
    )
}

/// Tareas de respaldo con acciones.
pub fn backups(tasks: &[(BackupTask, Option<String>)]) -> String {
    let rows: Vec<String> = tasks
        .iter()
        .map(|(task, last)| {
            let origin = match task.kind {
                nas_engine::SourceKind::Cifs => format!(
                    "//{}/{}",
                    task.source_ip.as_deref().unwrap_or("?"),
                    task.share.as_deref().unwrap_or("?")
                ),
                nas_engine::SourceKind::Ssh => format!(
                    "{}@{}:{}",
                    task.user.as_deref().unwrap_or("?"),
                    task.source_ip.as_deref().unwrap_or("?"),
                    task.path.as_deref().unwrap_or("?")
                ),
                nas_engine::SourceKind::Local => {
                    task.path.clone().unwrap_or_else(|| "?".to_string())
                }
            };
            format!(
                "<tr><td><strong>{}</strong></td><td>{}</td><td><code>{}</code></td><td>{}</td><td>{}</td>\
                 <td style=\"text-align:right; white-space:nowrap;\">\
                 <button class=\"btn btn-secondary btn-sm\" onclick=\"planBackup('{id}')\">{plan_icon} Plan</button> \
                 <button class=\"btn btn-primary btn-sm\" onclick=\"runBackup('{id}')\">{play_icon} Ejecutar</button></td></tr>",
                esc(&task.id),
                badge("blue", task.kind.label()),
                esc(&origin),
                task.retention,
                esc(last.as_deref().unwrap_or("sin snapshots")),
                id = esc(&task.id),
                plan_icon = icon("icon-eye", "icon-sm"),
                play_icon = icon("icon-play", "icon-sm")
            )
        })
        .collect();

    format!(
        "{}{}",
        page_head(
            "Respaldos",
            "Tareas programadas, planes y ejecución manual",
            "icon-archive",
            "<button class=\"btn btn-secondary btn-sm\" onclick=\"location.reload()\">
                <svg class=\"icon icon-sm\"><use href=\"#icon-refresh\"></use></svg> Actualizar</button>"
        ),
        panel(
            &format!("Tareas de respaldo ({})", tasks.len()),
            "icon-archive",
            &table(
                &["Tarea", "Tipo", "Origen", "Retención", "Último snapshot", "Acciones"],
                &rows
            )
        )
    )
}

/// Servicios con acciones.
pub fn services(list: &[ServiceStatus]) -> String {
    let rows: Vec<String> = list
        .iter()
        .map(|service| {
            let state = if service.active {
                badge("ok", "activo")
            } else {
                badge("err", "inactivo")
            };
            format!(
                "<tr><td><strong>{}</strong></td><td>{}</td><td style=\"text-align:right; white-space:nowrap;\">\
                 <button class=\"btn btn-secondary btn-sm\" onclick=\"svc('{name}','reload')\">{reload} Reload</button> \
                 <button class=\"btn btn-danger btn-sm\" onclick=\"svc('{name}','restart')\">{power} Restart</button></td></tr>",
                esc(&service.service),
                state,
                name = esc(&service.service),
                reload = icon("icon-refresh", "icon-sm"),
                power = icon("icon-power", "icon-sm")
            )
        })
        .collect();

    format!(
        "{}{}",
        page_head(
            "Servicios",
            "Control de demonios del servidor",
            "icon-services",
            ""
        ),
        panel(
            "Demonios del sistema",
            "icon-services",
            &table(&["Servicio", "Estado", "Acciones"], &rows)
        )
    )
}

/// Diagnóstico.
pub fn diagnostics(checks: &[Check]) -> String {
    let rows: Vec<String> = checks
        .iter()
        .map(|check| {
            let state = if check.ok {
                badge("ok", "OK")
            } else {
                badge("err", "FALLO")
            };
            format!(
                "<tr><td><strong>{}</strong></td><td>{}</td><td><code>{}</code></td></tr>",
                esc(&check.name),
                state,
                esc(&check.detail)
            )
        })
        .collect();

    let failed = checks.iter().filter(|check| !check.ok).count();
    format!(
        "{}{}",
        page_head(
            "Diagnóstico",
            &format!("{} comprobaciones · {} fallos", checks.len(), failed),
            "icon-wrench",
            ""
        ),
        panel(
            "Comprobaciones",
            "icon-wrench",
            &table(&["Comprobación", "Resultado", "Detalle"], &rows)
        )
    )
}

/// Almacenamiento.
pub fn storage(overview: &StorageOverview, disks: &[Disk]) -> String {
    let summary = format!(
        "<div class=\"kpi-grid\">{}{}{}{}</div>",
        kpi(
            "Uso del volumen",
            &format!("{:.1}%", overview.usage_percent),
            "icon-hard-drive",
            &format!("{:.2} / {:.2} GiB", overview.used_gb, overview.total_gb)
        ),
        kpi(
            "Libre",
            &format!("{:.2} GiB", overview.free_gb),
            "icon-archive",
            ""
        ),
        kpi("Filesystem", &esc(&overview.filesystem), "icon-grid", ""),
        kpi(
            "Dispositivo",
            &esc(overview.device.as_deref().unwrap_or("N/D")),
            "icon-hard-drive",
            &overview.device_type
        ),
    );

    let rows: Vec<String> = disks
        .iter()
        .map(|disk| {
            let state = if disk.protected {
                badge("warn", "SO · protegido")
            } else if disk.in_use {
                badge("blue", "en uso")
            } else {
                badge("ok", "disponible")
            };
            format!(
                "<tr><td><strong>{}</strong></td><td>{}</td><td>{}</td><td><code>{}</code></td><td><code>{}</code></td><td>{}</td></tr>",
                esc(&disk.name),
                esc(&disk.size),
                esc(disk.model.as_deref().unwrap_or("-")),
                esc(disk.fstype.as_deref().unwrap_or("-")),
                esc(disk.mountpoint.as_deref().unwrap_or("-")),
                state
            )
        })
        .collect();

    format!(
        "{}{}{}",
        page_head(
            "Almacenamiento",
            "Uso del volumen y discos detectados",
            "icon-hard-drive",
            ""
        ),
        summary,
        panel(
            &format!("Discos ({})", disks.len()),
            "icon-hard-drive",
            &table(
                &["Disco", "Tamaño", "Modelo", "FS", "Montaje", "Estado"],
                &rows
            )
        )
    )
}

/// Explorador de archivos.
pub fn files(current: &str, entries: &[Entry]) -> String {
    let parent = current.rsplit_once('/').map_or("", |(parent, _)| parent);
    let breadcrumb = if current.is_empty() {
        "<code>/srv/nas</code>".to_string()
    } else {
        format!(
            "<a href=\"/files\">/srv/nas</a> / <code>{}</code>",
            esc(current)
        )
    };

    let mut rows: Vec<String> = Vec::new();
    if !current.is_empty() {
        rows.push(format!(
            "<tr><td><a href=\"/files?path={}\">📁 ..</a></td><td>directorio superior</td><td></td><td></td></tr>",
            esc(&percent_encode(parent))
        ));
    }

    for entry in entries {
        if entry.is_dir {
            rows.push(format!(
                "<tr><td>{} <a href=\"/files?path={}\"><strong>{}</strong></a></td><td>{}</td><td></td><td></td></tr>",
                icon("icon-folder", "icon-sm"),
                esc(&percent_encode(&entry.path)),
                esc(&entry.name),
                badge("blue", "directorio")
            ));
        } else {
            rows.push(format!(
                "<tr><td>{} {}</td><td>{}</td><td>{}</td>\
                 <td style=\"text-align:right; white-space:nowrap;\">\
                 <a class=\"btn btn-secondary btn-sm\" href=\"/api/files/download?path={}\">{down} Descargar</a> \
                 <button class=\"btn btn-danger btn-sm\" onclick=\"delFile('{}')\">{trash} Borrar</button></td></tr>",
                icon("icon-file", "icon-sm"),
                esc(&entry.name),
                format_bytes(entry.size),
                badge("gray", &format!("{:.0}%", 0.0)).replace("0%", ""),
                esc(&percent_encode(&entry.path)),
                esc(&entry.path),
                down = icon("icon-download", "icon-sm"),
                trash = icon("icon-trash", "icon-sm")
            ));
        }
    }

    let actions = format!(
        "<div class=\"row\" style=\"gap:8px;\">\
         <input id=\"newdir\" placeholder=\"nueva carpeta\" style=\"width:160px;\">\
         <button class=\"btn btn-secondary btn-sm\" onclick=\"mkdir()\">{plus} Crear</button>\
         <input type=\"file\" id=\"upfile\" style=\"max-width:220px;\">\
         <button class=\"btn btn-primary btn-sm\" onclick=\"upload()\">{up} Subir</button></div>",
        plus = icon("icon-plus", "icon-sm"),
        up = icon("icon-upload", "icon-sm")
    );

    format!(
        "{}{}",
        page_head("Archivos", &breadcrumb, "icon-folder", &actions),
        panel(
            "Contenido",
            "icon-folder",
            &table(&["Nombre", "Tamaño", "Estado", ""], &rows).replace(
                "<div class=\"table-responsive\">",
                &format!(
                    "<input type=\"hidden\" id=\"curdir\" value=\"{}\"><div class=\"table-responsive\">",
                    esc(current)
                )
            )
        )
    )
}

/// Dominio.
pub fn domain(status: &nas_engine::domain::DomainStatus) -> String {
    let state = if status.joined {
        badge("ok", "unido")
    } else {
        badge("gray", "sin dominio")
    };
    let details = if status.details.is_empty() {
        "<span class=\"text-muted\">Sin información de realm.</span>".to_string()
    } else {
        format!("<pre>{}</pre>", esc(&status.details.join("\n")))
    };

    let form = format!(
        "<div class=\"panel-card-body\"><div class=\"row\" style=\"gap:8px; flex-wrap:wrap;\">\
         <input id=\"dom\" placeholder=\"dominio (team.local)\">\
         <input id=\"domuser\" placeholder=\"usuario\">\
         <input id=\"dompass\" type=\"password\" placeholder=\"contraseña\">\
         <button class=\"btn btn-primary btn-sm\" onclick=\"joinDomain()\">{join} Unir al dominio</button></div>\
         <div style=\"margin-top:12px;\">{details}</div></div>",
        join = icon("icon-domain", "icon-sm")
    );

    format!(
        "{}{}",
        page_head(
            "Dominio",
            "Integración con Active Directory",
            "icon-domain",
            &state
        ),
        panel("Active Directory", "icon-domain", &form)
    )
}

/// Auditoría del panel.
pub fn audit(rows: &[crate::db::AuditRow]) -> String {
    let body: Vec<String> = rows
        .iter()
        .map(|row| {
            let when = chrono::DateTime::from_timestamp(row.ts, 0)
                .map(|dt| {
                    dt.with_timezone(&chrono::Local)
                        .format("%Y-%m-%d %H:%M:%S")
                        .to_string()
                })
                .unwrap_or_else(|| row.ts.to_string());
            let result = match row.result.as_str() {
                "SUCCESS" => badge("ok", "SUCCESS"),
                "FAILED" => badge("err", "FAILED"),
                other => badge("gray", other),
            };
            format!(
                "<tr><td><code>{}</code></td><td><strong>{}</strong></td><td>{}</td><td><code>{}</code></td><td>{}</td><td class=\"text-muted\">{}</td></tr>",
                esc(&when),
                esc(&row.username),
                esc(&row.action),
                esc(&row.target),
                result,
                esc(&row.detail)
            )
        })
        .collect();

    format!(
        "{}{}",
        page_head(
            "Auditoría",
            &format!("Últimas {} acciones registradas", rows.len()),
            "icon-shield",
            ""
        ),
        panel(
            "Registro de actividad",
            "icon-shield",
            &table(
                &[
                    "Fecha",
                    "Usuario",
                    "Acción",
                    "Objetivo",
                    "Resultado",
                    "Detalle"
                ],
                &body
            )
        )
    )
}

/// Terminal web (captura periódica del backend tmux).
pub fn terminal() -> String {
    let script = r#"
let nasTimer=null;
function startTerminal(){
  fetch('/api/terminal/session',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({cols:200,rows:50})})
    .then(r=>r.json()).then(j=>{ if(!j.success) alert(j.error||j.message); pollTerminal(); });
}
function pollTerminal(){
  if(nasTimer) clearInterval(nasTimer);
  const refresh=()=>fetch('/api/terminal/capture').then(r=>r.json()).then(j=>{
    const pre=document.getElementById('term');
    if(pre && j.success && j.data) pre.textContent=j.data.output;
  }).catch(()=>{});
  refresh();
  nasTimer=setInterval(refresh,1200);
}
function sendTerminal(){
  const input=document.getElementById('cmd');
  const data=input.value;
  fetch('/api/terminal/send',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({data:data,enter:true})})
    .then(()=>{input.value='';});
}
function killTerminal(){
  if(confirm('¿Cerrar la sesión de terminal?')) fetch('/api/terminal/kill',{method:'POST'}).then(()=>{ if(nasTimer) clearInterval(nasTimer); document.getElementById('term').textContent=''; });
}
window.addEventListener('DOMContentLoaded',pollTerminal);
"#;

    format!(
        "{}{}",
        page_head(
            "Terminal",
            "Sesión tmux aislada por usuario",
            "icon-terminal",
            "<button class=\"btn btn-primary btn-sm\" onclick=\"startTerminal()\">
                <svg class=\"icon icon-sm\"><use href=\"#icon-play\"></use></svg> Iniciar / Reanudar</button>
             <button class=\"btn btn-danger btn-sm\" onclick=\"killTerminal()\">
                <svg class=\"icon icon-sm\"><use href=\"#icon-power\"></use></svg> Cerrar sesión</button>"
        ),
        panel(
            "Consola",
            "icon-terminal",
            &format!(
                "<div class=\"panel-card-body\"><pre id=\"term\" style=\"min-height:300px; background:var(--bg-terminal); color:var(--text-terminal); padding:12px; border-radius:8px; overflow:auto;\">(sin sesión activa)</pre>
                 <div class=\"row\" style=\"margin-top:10px; gap:8px;\">
                   <input id=\"cmd\" placeholder=\"comando y Enter\" style=\"flex:1;\" onkeydown=\"if(event.key==='Enter'){{sendTerminal();}}\">
                   <button class=\"btn btn-secondary btn-sm\" onclick=\"sendTerminal()\">Enviar</button>
                 </div></div><script>{script}</script>"
            )
        )
    )
}

fn format_bytes(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{bytes} B")
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KiB", bytes as f64 / 1024.0)
    } else if bytes < 1024 * 1024 * 1024 {
        format!("{:.1} MiB", bytes as f64 / (1024.0 * 1024.0))
    } else {
        format!("{:.2} GiB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
    }
}

fn percent_encode(value: &str) -> String {
    let mut encoded = String::new();
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'/' => {
                encoded.push(byte as char);
            }
            _ => encoded.push_str(&format!("%{byte:02X}")),
        }
    }
    encoded
}

#[cfg(test)]
mod tests {
    use super::{esc, format_bytes, percent_encode};

    #[test]
    fn escapa_html() {
        assert_eq!(esc("<b>"), "&lt;b&gt;");
        assert_eq!(esc("a&b"), "a&amp;b");
    }

    #[test]
    fn codifica_rutas_para_url() {
        assert_eq!(percent_encode("SISTEMAS/2026"), "SISTEMAS/2026");
        assert_eq!(percent_encode("a b.txt"), "a%20b.txt");
    }

    #[test]
    fn formatea_tamanos() {
        assert_eq!(format_bytes(512), "512 B");
        assert_eq!(format_bytes(2048), "2.0 KiB");
        assert_eq!(format_bytes(5 * 1024 * 1024), "5.0 MiB");
    }
}
