//! Renderizado de la interfaz TUI.

use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap};
use ratatui::Frame;

use crate::app::{App, Data, Section};

/// Dibuja la interfaz completa.
pub fn draw(frame: &mut Frame, app: &App) {
    let outer = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(5), Constraint::Length(2)])
        .split(frame.area());

    let body = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(24), Constraint::Min(20)])
        .split(outer[0]);

    let items: Vec<ListItem> = Section::ALL
        .iter()
        .map(|section| ListItem::new(section.title()))
        .collect();

    let mut state = ListState::default();
    state.select(Some(app.section.min(Section::ALL.len() - 1)));

    let menu = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" NAS · Debian 13 "),
        )
        .highlight_style(
            Style::default()
                .bg(Color::Blue)
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("› ");

    frame.render_stateful_widget(menu, body[0], &mut state);

    let content = Paragraph::new(content_lines(app))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(format!(" {} ", app.current().title())),
        )
        .wrap(Wrap { trim: false });

    frame.render_widget(content, body[1]);

    let status = Paragraph::new(app.status.as_str())
        .style(Style::default().fg(Color::Gray))
        .block(Block::default().borders(Borders::TOP));
    frame.render_widget(status, outer[1]);

    if let Some(text) = &app.overlay {
        let area = centered_rect(80, 70, frame.area());
        frame.render_widget(Clear, area);
        let popup = Paragraph::new(text.as_str())
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" Detalle (Esc cierra) "),
            )
            .wrap(Wrap { trim: false });
        frame.render_widget(popup, area);
    }
}

fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(area);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(vertical[1])[1]
}

fn kv(label: &str, value: &str) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!("{label:<18}"), Style::default().fg(Color::Gray)),
        Span::raw(value.to_string()),
    ])
}

fn content_lines(app: &App) -> Vec<Line<'static>> {
    match &app.data {
        Data::Empty => vec![Line::from("Cargando…")],
        Data::Message(message) => vec![Line::from(message.clone())],
        Data::Dashboard(metrics) => vec![
            kv("Hostname", &metrics.hostname),
            kv("Sistema", &metrics.os),
            kv("Kernel", &metrics.kernel),
            kv("Uptime", &metrics.uptime),
            kv(
                "CPU",
                &format!("{} ({:.1}%)", metrics.cpu_model, metrics.cpu_usage_pct),
            ),
            kv(
                "Memoria",
                &format!(
                    "{:.2} / {:.2} GiB ({:.1}%)",
                    metrics.ram_used_gb, metrics.ram_total_gb, metrics.ram_usage_pct
                ),
            ),
            kv("IPv4", metrics.ip.as_deref().unwrap_or("N/D")),
            kv("Hora", &metrics.server_time),
        ],
        Data::Network(overview) => {
            let mut lines = vec![
                kv("Hostname", &overview.hostname),
                kv("NetBIOS", &overview.netbios),
                kv("Workgroup", overview.workgroup.as_deref().unwrap_or("N/D")),
                kv("Gateway", overview.gateway.as_deref().unwrap_or("N/D")),
                kv(
                    "DNS",
                    &if overview.dns.is_empty() {
                        "N/D".to_string()
                    } else {
                        overview.dns.join(", ")
                    },
                ),
            ];
            for iface in &overview.interfaces {
                lines.push(kv(
                    "Interfaz",
                    &format!(
                        "{} {} {}/{}",
                        iface.name,
                        iface.state,
                        iface.ipv4.as_deref().unwrap_or("-"),
                        iface.prefix.unwrap_or(0)
                    ),
                ));
            }
            lines.push(kv("Puertos", &overview.listening.len().to_string()));
            lines
        }
        Data::Shares(shares) => {
            let mut lines = vec![Line::from(Span::styled(
                format!("{:<24} {:<10} {}", "RECURSO", "PERMISO", "RUTA"),
                Style::default().add_modifier(Modifier::BOLD),
            ))];
            for (index, share) in shares.iter().enumerate() {
                let marker = if index == app.selected { "›" } else { " " };
                lines.push(Line::from(format!(
                    "{marker} {:<22} {:<10} {}",
                    share.name,
                    if share.read_only { "RO" } else { "RW" },
                    share.path.as_deref().unwrap_or("-")
                )));
            }
            lines
        }
        Data::Users(users, groups) => {
            let mut lines = vec![Line::from(Span::styled(
                format!("USUARIOS ({})", users.len()),
                Style::default().add_modifier(Modifier::BOLD),
            ))];
            for (index, user) in users.iter().enumerate() {
                let marker = if index == app.selected { "›" } else { " " };
                lines.push(Line::from(format!(
                    "{marker} {:<20} uid={:<6} {}",
                    user.name, user.uid, user.home
                )));
            }
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                format!("GRUPOS ({})", groups.len()),
                Style::default().add_modifier(Modifier::BOLD),
            )));
            for group in groups {
                lines.push(Line::from(format!(
                    "  {:<20} gid={:<6} {}",
                    group.name,
                    group.gid,
                    group.members.join(", ")
                )));
            }
            lines
        }
        Data::Backups(tasks) => {
            let mut lines = vec![Line::from(Span::styled(
                "Enter: ver plan · tecla r: actualizar",
                Style::default().fg(Color::Gray),
            ))];
            for (index, (task, last)) in tasks.iter().enumerate() {
                let marker = if index == app.selected { "›" } else { " " };
                lines.push(Line::from(format!(
                    "{marker} {:<18} {:<10} retención={:<4} último={}",
                    task.id,
                    task.kind.label(),
                    task.retention,
                    last.as_deref().unwrap_or("-")
                )));
            }
            lines
        }
        Data::Services(services) => {
            let mut lines = vec![Line::from(Span::styled(
                "Enter: reiniciar servicio seleccionado",
                Style::default().fg(Color::Gray),
            ))];
            for (index, service) in services.iter().enumerate() {
                let marker = if index == app.selected { "›" } else { " " };
                lines.push(Line::from(vec![
                    Span::raw(format!("{marker} {:<12}", service.service)),
                    Span::styled(
                        if service.active { "activo" } else { "inactivo" },
                        Style::default().fg(if service.active {
                            Color::Green
                        } else {
                            Color::Red
                        }),
                    ),
                ]));
            }
            lines
        }
        Data::Diagnostics(checks) => {
            let mut lines = Vec::new();
            for (index, check) in checks.iter().enumerate() {
                let marker = if index == app.selected { "›" } else { " " };
                lines.push(Line::from(vec![
                    Span::raw(format!("{marker} {:<18}", check.name)),
                    Span::styled(
                        if check.ok { "OK" } else { "FALLO" },
                        Style::default().fg(if check.ok { Color::Green } else { Color::Red }),
                    ),
                    Span::raw(format!("  {}", check.detail)),
                ]));
            }
            lines
        }
        Data::Storage(overview, disks) => {
            let mut lines = Vec::new();
            if let Some(overview) = overview {
                lines.push(kv("Ruta", &overview.path));
                lines.push(kv("Filesystem", &overview.filesystem));
                lines.push(kv(
                    "Uso",
                    &format!(
                        "{:.2} / {:.2} GiB ({:.1}%)",
                        overview.used_gb, overview.total_gb, overview.usage_percent
                    ),
                ));
                lines.push(kv(
                    "Dispositivo",
                    overview.device.as_deref().unwrap_or("N/D"),
                ));
                lines.push(Line::from(""));
            }
            for disk in disks {
                lines.push(Line::from(format!(
                    "{} {:<10} {:<8} {} {}",
                    if disk.protected { "🔒" } else { "  " },
                    disk.name,
                    disk.size,
                    disk.fstype.as_deref().unwrap_or("-"),
                    disk.mountpoint.as_deref().unwrap_or("-")
                )));
            }
            lines
        }
        Data::Files(entries) => {
            let mut lines = vec![Line::from(Span::styled(
                format!("/srv/nas/{}", app.path),
                Style::default().add_modifier(Modifier::BOLD),
            ))];
            for (index, entry) in entries.iter().enumerate() {
                let marker = if index == app.selected { "›" } else { " " };
                lines.push(Line::from(format!(
                    "{marker} {} {}",
                    if entry.is_dir { "📁" } else { "📄" },
                    entry.name
                )));
            }
            lines
        }
        Data::Domain(status) => {
            let mut lines = vec![
                kv("Unido", if status.joined { "sí" } else { "no" }),
                kv("Realm", status.realm.as_deref().unwrap_or("N/D")),
                kv("Dominio", status.domain.as_deref().unwrap_or("N/D")),
                kv(
                    "SSSD",
                    if status.sssd_active {
                        "activo"
                    } else {
                        "inactivo"
                    },
                ),
                Line::from(""),
            ];
            for detail in &status.details {
                lines.push(Line::from(detail.clone()));
            }
            lines
        }
    }
}

/// Texto de ayuda mostrado al pie.
pub fn help_line() -> Line<'static> {
    Line::from(vec![Span::styled(
        "↑↓ navegar · Tab sección · Enter abrir · F5 actualizar · q salir",
        Style::default().fg(Color::DarkGray),
    )])
}

/// Dibuja sólo la ayuda (usado en pruebas).
pub fn helper(frame: &mut Frame) {
    let area = frame.area();
    let paragraph = Paragraph::new(help_line()).alignment(Alignment::Left);
    frame.render_widget(paragraph, area);
}

#[cfg(test)]
mod tests {
    use super::{draw, helper};
    use crate::app::{App, Data};
    use nas_engine::SystemMetrics;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    fn buffer_text(terminal: &Terminal<TestBackend>) -> String {
        let buffer = terminal.backend().buffer();
        buffer
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>()
    }

    #[test]
    fn dibuja_menu_y_dashboard() {
        let mut app = App::new();
        app.data = Data::Dashboard(SystemMetrics {
            hostname: "SRV-NAS".into(),
            uptime: "1d".into(),
            kernel: "6.12".into(),
            os: "Debian 13".into(),
            cpu_model: "CPU".into(),
            cpu_usage_pct: 1.0,
            ram_total_gb: 8.0,
            ram_used_gb: 1.0,
            ram_usage_pct: 12.5,
            server_time: "2026-01-01 00:00:00".into(),
            timezone: "UTC".into(),
            timezone_abbr: "UTC".into(),
            timezone_offset: 0,
            ip: Some("10.10.1.2".into()),
        });

        let backend = TestBackend::new(100, 30);
        let mut terminal = Terminal::new(backend).expect("terminal");
        terminal.draw(|frame| draw(frame, &app)).expect("draw");
        let text = buffer_text(&terminal);

        assert!(text.contains("NAS · Debian 13"));
        assert!(text.contains("Dashboard"));
        assert!(text.contains("SRV-NAS"));
        assert!(text.contains("Recursos"));
    }

    #[test]
    fn dibuja_ayuda() {
        let backend = TestBackend::new(80, 5);
        let mut terminal = Terminal::new(backend).expect("terminal");
        terminal.draw(helper).expect("draw");
        assert!(buffer_text(&terminal).contains("navegar"));
    }
}
