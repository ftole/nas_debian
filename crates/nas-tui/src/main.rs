//! Arranque del asistente TUI.

use std::io;
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
