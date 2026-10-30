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

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let runner = Runner::new();
    let paths = NasPaths::from_env();
    let mut app = App::new();
    app.refresh(&runner, &paths);

    let result = run_loop(&mut terminal, &mut app, &runner, &paths);

    let _ = disable_raw_mode();
    let _ = execute!(terminal.backend_mut(), LeaveAlternateScreen);
    let _ = terminal.show_cursor();

    result
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
                        app.refresh(runner, paths);
                    }
                    KeyCode::BackTab | KeyCode::Left => {
                        app.previous_section();
                        app.refresh(runner, paths);
                    }
                    KeyCode::Enter => {
                        app.activate();
                        app.refresh(runner, paths);
                    }
                    KeyCode::Backspace => {
                        app.go_up();
                        app.refresh(runner, paths);
                    }
                    KeyCode::F(5) | KeyCode::Char('r') => app.refresh(runner, paths),
                    _ => {}
                }
            }
        }

        if app.should_quit {
            return Ok(());
        }
    }
}
