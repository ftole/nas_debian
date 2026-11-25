//! Arranque del asistente TUI.

use std::io::{self, IsTerminal};
use std::time::Duration;

use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use nas_core::NasPaths;
use nas_exec::Runner;
use nas_tui::ui;
use nas_tui::App;
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;

fn main() -> io::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--version" || a == "-V") {
        println!("nas-tui {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!(
            "nas-tui {}\nAsistente visual de terminal (TUI) para el servidor NAS Debian 13\n\nUSO:\n    nas-tui\n\nOPCIONES:\n    -h, --help       Imprime esta ayuda\n    -V, --version    Imprime la versión",
            env!("CARGO_PKG_VERSION")
        );
        return Ok(());
    }

    if !io::stdout().is_terminal() {
        eprintln!("Error: nas-tui requiere una terminal interactiva (TTY)");
        std::process::exit(1);
    }

    struct TtyRestoreGuard;
    impl Drop for TtyRestoreGuard {
        fn drop(&mut self) {
            let _ = disable_raw_mode();
            let mut out = io::stdout();
            let _ = execute!(out, LeaveAlternateScreen, crossterm::cursor::Show);
        }
    }

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let _tty_guard = TtyRestoreGuard;

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let runner = Runner::new();
    let paths = NasPaths::from_env();
    let mut app = App::new();
    app.refresh(&runner, &paths);

    run_loop(&mut terminal, &mut app, &runner, &paths)
}

fn run_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
    runner: &Runner,
    paths: &NasPaths,
) -> io::Result<()> {
    loop {
        terminal.draw(|frame| ui::draw(frame, app))?;

        if event::poll(Duration::from_millis(250))? {
            if let Event::Key(key) = event::read()? {
                if key.kind != KeyEventKind::Press {
                    continue;
                }

                match key.code {
                    KeyCode::Char('q') => app.should_quit = true,
                    KeyCode::Esc => {
                        if app.overlay.take().is_none() {
                            app.should_quit = true;
                        }
                    }
                    KeyCode::Down | KeyCode::Char('j') => app.move_down(app.item_count()),
                    KeyCode::Up | KeyCode::Char('k') => app.move_up(),
                    KeyCode::Tab | KeyCode::Right => {
                        app.next_section();
                        if app.current() == nas_tui::Section::Quit {
                            app.status = "Pulsa Enter o 'q' para salir".to_string();
                        } else {
                            app.refresh(runner, paths);
                        }
                    }
                    KeyCode::BackTab | KeyCode::Left => {
                        app.previous_section();
                        if app.current() == nas_tui::Section::Quit {
                            app.status = "Pulsa Enter o 'q' para salir".to_string();
                        } else {
                            app.refresh(runner, paths);
                        }
                    }
                    KeyCode::Enter => {
                        let prev_path = app.path.clone();
                        app.activate();
                        if app.current() == nas_tui::Section::Files {
                            if app.path != prev_path {
                                app.refresh(runner, paths);
                            }
                        } else if app.current() == nas_tui::Section::Services {
                            let status_after_activate = app.status.clone();
                            app.refresh(runner, paths);
                            app.status = status_after_activate;
                        }
                    }
                    KeyCode::Backspace => {
                        app.go_up();
                        if app.current() != nas_tui::Section::Quit {
                            app.refresh(runner, paths);
                        }
                    }
                    KeyCode::F(5) | KeyCode::Char('r')
                        if app.current() != nas_tui::Section::Quit =>
                    {
                        app.refresh(runner, paths);
                    }
                    _ => {}
                }
            }
        }

        if app.should_quit {
            return Ok(());
        }
    }
}
