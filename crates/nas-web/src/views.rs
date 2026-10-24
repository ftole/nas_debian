//! Vistas HTML del panel (Slate UI, sin dependencias externas).

use nas_engine::diagnostics::Check;
use nas_engine::parsers::smb::Share;
use nas_engine::services::ServiceStatus;
use nas_engine::storage::{Disk, StorageOverview};
use nas_engine::tasks::BackupTask;
use nas_engine::users::{GroupEntry, UserEntry};
use nas_engine::SystemMetrics;

use crate::files::Entry;

/// Secciones de la navegación lateral.
pub const SECTIONS: &[(&str, &str)] = &[
    ("dashboard", "Dashboard"),
    ("shares", "Recursos"),
    ("users", "Usuarios"),
    ("backups", "Respaldos"),
    ("services", "Servicios"),
    ("diagnostics", "Diagnóstico"),
    ("storage", "Almacenamiento"),
    ("files", "Archivos"),
];

/// Escapa texto para HTML.
pub fn esc(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

const CSS: &str = "\
*{box-sizing:border-box}body{margin:0;font-family:system-ui,-apple-system,'Segoe UI',sans-serif;background:#0f172a;color:#e2e8f0;font-size:14px}
a{color:#38bdf8;text-decoration:none}.app{display:flex;min-height:100vh}
.sidebar{width:216px;flex:0 0 216px;background:#111c2e;border-right:1px solid #1f2b3d;padding:18px 12px;display:flex;flex-direction:column;gap:6px}
.brand{font-weight:700;font-size:16px;letter-spacing:.5px;color:#38bdf8;padding:4px 8px 14px}
.nav a{display:block;padding:9px 10px;border-radius:8px;color:#cbd5e1;font-size:13.5px}
.nav a:hover{background:#1b2940}.nav a.active{background:#1d4ed8;color:#fff;font-weight:600}
main{flex:1;min-width:0;display:flex;flex-direction:column}
.topbar{display:flex;justify-content:space-between;align-items:center;gap:16px;padding:14px 24px;background:#111c2e;border-bottom:1px solid #1f2b3d}
.topbar h1{font-size:17px;margin:0;font-weight:600}
.topbar .right{display:flex;align-items:center;gap:12px}
.badge{display:inline-block;padding:2px 8px;border-radius:999px;font-size:11.5px;font-weight:600;background:#1e293b;border:1px solid #334155;color:#cbd5e1}
.badge.ok{background:#052e16;border-color:#166534;color:#86efac}.badge.err{background:#450a0a;border-color:#991b1b;color:#fca5a5}.badge.warn{background:#422006;border-color:#a16207;color:#fde68a}
.content{padding:22px 24px;display:flex;flex-direction:column;gap:18px}
.grid{display:grid;gap:14px;grid-template-columns:repeat(auto-fit,minmax(210px,1fr))}
.card{background:#111c2e;border:1px solid #1f2b3d;border-radius:12px;padding:16px}
.card h2{margin:0 0 8px;font-size:12px;text-transform:uppercase;letter-spacing:.6px;color:#94a3b8;font-weight:600}
.card .value{font-size:20px;font-weight:600;color:#e2e8f0;word-break:break-word}
.panel{background:#111c2e;border:1px solid #1f2b3d;border-radius:12px;overflow:hidden}
.panel-head{display:flex;justify-content:space-between;align-items:center;padding:13px 16px;border-bottom:1px solid #1f2b3d}
.panel-head h2{margin:0;font-size:13.5px}
table{width:100%;border-collapse:collapse}th,td{padding:9px 14px;text-align:left;border-bottom:1px solid #16233a;font-size:13px;vertical-align:top}
th{background:#0d1727;color:#94a3b8;font-weight:600;font-size:11.5px;text-transform:uppercase;letter-spacing:.5px}
tr:last-child td{border-bottom:none}
.btn{display:inline-block;background:#1e293b;border:1px solid #334155;color:#e2e8f0;border-radius:8px;padding:6px 11px;font-size:12.5px;cursor:pointer}
.btn:hover{background:#24344d}.btn.primary{background:#1d4ed8;border-color:#1d4ed8;color:#fff}.btn.danger{background:#7f1d1d;border-color:#991b1b;color:#fecaca}
.row{display:flex;gap:8px;flex-wrap:wrap;align-items:center}
input,select{background:#0d1727;border:1px solid #334155;border-radius:8px;color:#e2e8f0;padding:7px 9px;font-size:13px}
pre{background:#0b1220;border:1px solid #1f2b3d;border-radius:10px;padding:12px;overflow:auto;font-size:12px;max-height:420px}
code{color:#7dd3fc}.muted{color:#94a3b8;font-size:12.5px}
form.inline{display:inline}";

const JS: &str = "\
async function api(url,body){try{const r=await fetch(url,{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify(body)});const j=await r.json();if(!j.success){alert(j.error||j.message||'Error');return null;}location.reload();return j;}catch(e){alert('Error de red: '+e);return null;}}
function planBackup(id){api('/api/backups/plan',{path:id}).then(j=>{if(j)alert(JSON.stringify(j.data.steps,null,2));});}
function runBackup(id){if(confirm('¿Ejecutar el respaldo '+id+' ahora?'))api('/api/backups/run',{path:id,confirm:true});}
function svc(name,action){api('/api/services/manage',{service:name,action:action});}
function delFile(path){if(confirm('¿Mover a la papelera: '+path+'?'))api('/api/files/delete',{path:path});}
function mkdir(){const p=document.getElementById('newdir').value.trim();if(p)api('/api/files/mkdir',{path:p});}
function upload(){const f=document.getElementById('upfile').files[0];const dir=document.getElementById('curdir').value;if(!f)return;const fd=new FormData();fd.append('path',dir);fd.append('file',f);fetch('/api/files/upload',{method:'POST',body:fd}).then(r=>r.json()).then(j=>{if(!j.success)alert(j.error||j.message);else location.reload();});}
function joinDomain(){const d=document.getElementById('dom').value.trim();const u=document.getElementById('domuser').value.trim();const p=document.getElementById('dompass').value;api('/api/domain/join',{domain:d,user:u,password:p});}";

/// Plantilla base con barra lateral y cabecera.
pub fn layout(active: &str, user: &str, role: &str, title: &str, body: &str) -> String {
    let nav: String = SECTIONS
        .iter()
        .map(|(slug, label)| {
            let class = if *slug == active {
                " class=\"active\""
            } else {
                ""
            };
            format!("<a href=\"/{slug}\"{class}>{label}</a>")
        })
        .collect();

    format!(
        r#"<!DOCTYPE html>
<html lang="es"><head><meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>{title} · Panel NAS</title><style>{CSS}</style></head>
<body><div class="app">
<aside class="sidebar"><div class="brand">NAS · Debian 13</div><nav class="nav">{nav}</nav></aside>
<main>
<header class="topbar"><h1>{title}</h1><div class="right">
<span class="badge">{role}</span>
<form method="post" action="/logout"><button class="btn" type="submit">Salir ({user})</button></form>
</div></header>
<section class="content">{body}</section>
</main></div><script>{JS}</script></body></html>"#,
        title = esc(title),
        user = esc(user),
        role = esc(role)
    )
}

/// Tarjeta simple de valor.
fn card(label: &str, value: &str) -> String {
    format!(
        "<div class=\"card\"><h2>{}</h2><div class=\"value\">{}</div></div>",
        esc(label),
        value
    )
}

/// Dashboard con métricas del sistema.
pub fn dashboard(metrics: &SystemMetrics, role: &str) -> String {
    let body = format!(
        "<div class=\"grid\">{}{}{}{}{}{}</div>",
        card("Hostname", &esc(&metrics.hostname)),
        card("Sistema", &esc(&metrics.os)),
        card("Uptime", &esc(&metrics.uptime)),
        card(
            "CPU",
            &format!("{} · {:.1}%", esc(&metrics.cpu_model), metrics.cpu_usage_pct)
        ),
        card(
            "Memoria",
            &format!(
                "{:.2} / {:.2} GiB ({:.1}%)",
                metrics.ram_used_gb, metrics.ram_total_gb, metrics.ram_usage_pct
            )
        ),
        card("IPv4", &esc(metrics.ip.as_deref().unwrap_or("N/D"))),
    ) + &format!(
        "<div class=\"panel\"><div class=\"panel-head\"><h2>Rol del servidor</h2></div><div class=\"card\"><div class=\"value\">{}</div></div></div>",
        esc(role)
    );

    body
}

/// Tabla genérica con encabezados y filas ya escapadas.
fn table(headers: &[&str], rows: &[String]) -> String {
    let head: String = headers
        .iter()
        .map(|header| format!("<th>{}</th>", esc(header)))
        .collect();
    let body: String = if rows.is_empty() {
        format!(
            "<tr><td colspan=\"{}\" class=\"muted\">Sin datos disponibles.</td></tr>",
            headers.len()
        )
    } else {
        rows.concat()
    };
    format!("<table><thead><tr>{head}</tr></thead><tbody>{body}</tbody></table>")
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
                if share.read_only { "solo lectura" } else { "lectura/escritura" },
                if share.browseable { "visible" } else { "oculto ($)" },
                esc(&share.valid_users.join(", "))
            )
        })
        .collect();

    format!(
        "<div class=\"panel\"><div class=\"panel-head\"><h2>Recursos compartidos ({})</h2></div>{}</div>",
        shares.len(),
        table(&["Recurso", "Ruta", "Permiso", "Visibilidad", "Usuarios"], &rows)
    )
}

/// Usuarios y grupos.
pub fn users(users: &[UserEntry], groups: &[GroupEntry]) -> String {
    let user_rows: Vec<String> = users
        .iter()
        .map(|user| {
            format!(
                "<tr><td>{}</td><td>{}</td><td>{}</td><td><code>{}</code></td><td>{}</td></tr>",
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
                "<tr><td>{}</td><td>{}</td><td>{}</td></tr>",
                esc(&group.name),
                group.gid,
                esc(&group.members.join(", "))
            )
        })
        .collect();

    format!(
        "<div class=\"panel\"><div class=\"panel-head\"><h2>Usuarios ({})</h2></div>{}</div>\
         <div class=\"panel\"><div class=\"panel-head\"><h2>Grupos corporativos ({})</h2></div>{}</div>",
        users.len(),
        table(&["Usuario", "UID", "GID", "Home", "Shell"], &user_rows),
        groups.len(),
        table(&["Grupo", "GID", "Miembros"], &group_rows)
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
                "<tr><td><strong>{}</strong></td><td>{}</td><td><code>{}</code></td><td>{}</td><td>{}</td><td class=\"row\">\
                 <button class=\"btn\" onclick=\"planBackup('{id}')\">Plan</button>\
                 <button class=\"btn primary\" onclick=\"runBackup('{id}')\">Ejecutar</button></td></tr>",
                esc(&task.id),
                esc(task.kind.label()),
                esc(&origin),
                task.retention,
                esc(last.as_deref().unwrap_or("sin snapshots")),
                id = esc(&task.id)
            )
        })
        .collect();

    format!(
        "<div class=\"panel\"><div class=\"panel-head\"><h2>Tareas de respaldo ({})</h2></div>{}</div>",
        tasks.len(),
        table(
            &["Tarea", "Tipo", "Origen", "Retención", "Último snapshot", "Acciones"],
            &rows
        )
    )
}

/// Estado de servicios con acciones.
pub fn services(list: &[ServiceStatus]) -> String {
    let rows: Vec<String> = list
        .iter()
        .map(|service| {
            let badge = if service.active {
                "<span class=\"badge ok\">activo</span>"
            } else {
                "<span class=\"badge err\">inactivo</span>"
            };
            format!(
                "<tr><td><strong>{}</strong></td><td>{}</td><td class=\"row\">\
                 <button class=\"btn\" onclick=\"svc('{name}','reload')\">Reload</button>\
                 <button class=\"btn danger\" onclick=\"svc('{name}','restart')\">Restart</button></td></tr>",
                esc(&service.service),
                badge,
                name = esc(&service.service)
            )
        })
        .collect();

    format!(
        "<div class=\"panel\"><div class=\"panel-head\"><h2>Demonios del sistema</h2></div>{}</div>",
        table(&["Servicio", "Estado", "Acciones"], &rows)
    )
}

/// Diagnóstico integral.
pub fn diagnostics(checks: &[Check]) -> String {
    let rows: Vec<String> = checks
        .iter()
        .map(|check| {
            let badge = if check.ok {
                "<span class=\"badge ok\">OK</span>"
            } else {
                "<span class=\"badge err\">FALLO</span>"
            };
            format!(
                "<tr><td><strong>{}</strong></td><td>{}</td><td><code>{}</code></td></tr>",
                esc(&check.name),
                badge,
                esc(&check.detail)
            )
        })
        .collect();

    format!(
        "<div class=\"panel\"><div class=\"panel-head\"><h2>Diagnóstico ({})</h2></div>{}</div>",
        checks.len(),
        table(&["Comprobación", "Resultado", "Detalle"], &rows)
    )
}

/// Almacenamiento: resumen y discos.
pub fn storage(overview: &StorageOverview, disks: &[Disk]) -> String {
    let summary = format!(
        "<div class=\"grid\">{}{}{}{}{}</div>",
        card("Ruta", &esc(&overview.path)),
        card("Sistema de archivos", &esc(&overview.filesystem)),
        card(
            "Uso",
            &format!(
                "{:.2} / {:.2} GiB ({:.1}%)",
                overview.used_gb, overview.total_gb, overview.usage_percent
            )
        ),
        card("Libre", &format!("{:.2} GiB", overview.free_gb)),
        card(
            "Dispositivo",
            &esc(overview.device.as_deref().unwrap_or("N/D"))
        ),
    );

    let rows: Vec<String> = disks
        .iter()
        .map(|disk| {
            let state = if disk.protected {
                "<span class=\"badge warn\">SO · protegido</span>"
            } else if disk.in_use {
                "<span class=\"badge\">en uso</span>"
            } else {
                "<span class=\"badge ok\">disponible</span>"
            };
            format!(
                "<tr><td><strong>{}</strong></td><td>{}</td><td>{}</td><td><code>{}</code></td><td>{}</td><td>{}</td></tr>",
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
        "{summary}<div class=\"panel\"><div class=\"panel-head\"><h2>Discos ({})</h2></div>{}</div>",
        disks.len(),
        table(
            &["Disco", "Tamaño", "Modelo", "FS", "Montaje", "Estado"],
            &rows
        )
    )
}

/// Explorador de archivos.
pub fn files(current: &str, entries: &[Entry]) -> String {
    let parent = current.rsplit_once('/').map_or("", |(parent, _)| parent);
    let breadcrumb = if current.is_empty() {
        "<span class=\"muted\">/srv/nas</span>".to_string()
    } else {
        format!(
            "<a href=\"/files\">/srv/nas</a> / <span>{}</span>",
            esc(current)
        )
    };

    let mut rows: Vec<String> = Vec::new();
    if !current.is_empty() {
        rows.push(format!(
            "<tr><td><a href=\"/files?path={}\">..</a></td><td class=\"muted\">directorio superior</td><td></td><td></td></tr>",
            esc(&percent_encode(parent))
        ));
    }

    for entry in entries {
        if entry.is_dir {
            rows.push(format!(
                "<tr><td><a href=\"/files?path={}\">📁 {}</a></td><td>directorio</td><td></td><td></td></tr>",
                esc(&percent_encode(&entry.path)),
                esc(&entry.name)
            ));
        } else {
            rows.push(format!(
                "<tr><td>📄 {}</td><td>{} bytes</td><td><a class=\"btn\" href=\"/api/files/download?path={}\">Descargar</a></td>\
                 <td><button class=\"btn danger\" onclick=\"delFile('{}')\">Borrar</button></td></tr>",
                esc(&entry.name),
                entry.size,
                esc(&percent_encode(&entry.path)),
                esc(&entry.path)
            ));
        }
    }

    format!(
        "<div class=\"panel\"><div class=\"panel-head\"><h2>{breadcrumb}</h2>\
         <div class=\"row\"><input id=\"newdir\" placeholder=\"nueva carpeta\"><button class=\"btn\" onclick=\"mkdir()\">Crear</button>\
         <input type=\"file\" id=\"upfile\"><button class=\"btn primary\" onclick=\"upload()\">Subir</button></div></div>\
         <input type=\"hidden\" id=\"curdir\" value=\"{}\">{}</div>",
        esc(current),
        table(&["Nombre", "Tipo", "Acciones", ""], &rows)
    )
}

/// Dominio (formulario de unión).
pub fn domain(status: &nas_engine::domain::DomainStatus) -> String {
    let state = if status.joined {
        "<span class=\"badge ok\">unido</span>"
    } else {
        "<span class=\"badge\">sin dominio</span>"
    };
    let details = if status.details.is_empty() {
        "<span class=\"muted\">Sin información de realm.</span>".to_string()
    } else {
        format!("<pre>{}</pre>", esc(&status.details.join("\n")))
    };

    format!(
        "<div class=\"panel\"><div class=\"panel-head\"><h2>Active Directory</h2>{state}</div>\
         <div class=\"card\"><div class=\"row\">\
         <input id=\"dom\" placeholder=\"dominio (team.local)\">\
         <input id=\"domuser\" placeholder=\"usuario\">\
         <input id=\"dompass\" type=\"password\" placeholder=\"contraseña\">\
         <button class=\"btn primary\" onclick=\"joinDomain()\">Unir al dominio</button></div></div>\
         <div class=\"card\">{details}</div></div>"
    )
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
    use super::{esc, percent_encode};

    #[test]
    fn escapa_html() {
        assert_eq!(esc("<b>"), "&lt;b&gt;");
        assert_eq!(esc("a&b"), "a&amp;b");
        assert_eq!(esc("\"'"), "&quot;&#39;");
    }

    #[test]
    fn codifica_rutas_para_url() {
        assert_eq!(percent_encode("SISTEMAS/2026"), "SISTEMAS/2026");
        assert_eq!(percent_encode("a b.txt"), "a%20b.txt");
        assert_eq!(percent_encode("ñ.txt"), "%C3%B1.txt");
    }
}
