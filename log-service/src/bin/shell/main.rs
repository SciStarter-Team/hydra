use anyhow::Result;
use clap::Parser;
use log_service::database::storage::Database;
use ratatui::{
    crossterm::{
        ExecutableCommand,
        event::{self, Event, KeyCode, KeyEventKind, KeyModifiers, MouseEventKind},
        event::{DisableMouseCapture, EnableMouseCapture},
        terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
    },
    prelude::*,
};
use std::{io::stdout, sync::Arc};

mod analytics;
mod functions;
mod query_engine;
mod ui;

use self::analytics::AnalyticsEngine;
use self::query_engine::QueryEngine;
use self::ui::App;

#[derive(Parser)]
#[command(name = "analytics-shell")]
#[command(about = "Interactive analytics shell for querying log service data")]
#[command(version)]
struct Args {
    /// Enable debug mode
    #[arg(short, long)]
    debug: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    // Initialize SQLite connection pool for cache tables (same as main log-service)
    common::shared::init("log_cache").await;

    let database_path = std::env::var("DATABASE_PATH").expect("DATABASE_PATH is not set");

    // Initialize database
    let database = Arc::new(Database::new(database_path).await?);

    // Initialize query engine
    let query_engine = QueryEngine::new(database.clone()).await?;

    // Initialize analytics engine
    let analytics_engine = AnalyticsEngine::new(database.clone()).await?;

    // Initialize terminal
    enable_raw_mode()?;
    stdout().execute(EnterAlternateScreen)?;
    stdout().execute(EnableMouseCapture)?;
    stdout().execute(ratatui::crossterm::terminal::Clear(
        ratatui::crossterm::terminal::ClearType::All,
    ))?;
    let mut terminal = Terminal::new(CrosstermBackend::new(stdout()))?;

    // Create app
    let mut app = App::new(query_engine, analytics_engine, args.debug);

    // Main loop
    let result = run_app(&mut terminal, &mut app).await;

    // Cleanup
    disable_raw_mode()?;
    stdout().execute(DisableMouseCapture)?;
    stdout().execute(LeaveAlternateScreen)?;

    match result {
        Ok(_) => println!("Analytics shell exited successfully"),
        Err(e) => eprintln!("Error: {}", e),
    }

    Ok(())
}

async fn run_app<B: Backend>(terminal: &mut Terminal<B>, app: &mut App) -> Result<()> {
    loop {
        // Process any pending async events first
        app.process_async_events();

        terminal.draw(|f| app.render(f))?;

        if event::poll(std::time::Duration::from_millis(100))? {
            match event::read()? {
                Event::Key(key) => {
                    if key.kind == KeyEventKind::Press {
                        // Handle Ctrl-C for cancellation or input clearing
                        if key.code == KeyCode::Char('c')
                            && key.modifiers.contains(KeyModifiers::CONTROL)
                        {
                            app.handle_ctrl_c();
                            continue;
                        }

                        // Handle Ctrl-D (EOF) when input is empty - check modifiers
                        if key.code == KeyCode::Char('d')
                            && key.modifiers.contains(KeyModifiers::CONTROL)
                        {
                            if app.is_input_empty() {
                                break; // Quit on Ctrl-D with empty input
                            }
                            // If input is not empty, ignore Ctrl-D
                            continue;
                        }

                        match key.code {
                            KeyCode::Esc => app.handle_escape(),
                            KeyCode::Enter => {
                                if app.handle_enter()? {
                                    break; // quit command was executed
                                }
                            }
                            KeyCode::Tab => app.handle_tab(),
                            KeyCode::Up => app.handle_up(),
                            KeyCode::Down => app.handle_down(),
                            KeyCode::Left => app.handle_left(),
                            KeyCode::Right => app.handle_right(),
                            KeyCode::PageUp => app.handle_page_up(),
                            KeyCode::PageDown => app.handle_page_down(),
                            KeyCode::Home => app.handle_home(),
                            KeyCode::End => app.handle_end(),
                            KeyCode::Char(c) => app.handle_char(c),
                            KeyCode::Backspace => app.handle_backspace(),
                            _ => {}
                        }
                    }
                }
                Event::Mouse(mouse) => {
                    match mouse.kind {
                        MouseEventKind::ScrollUp => app.handle_mouse_scroll_up(),
                        MouseEventKind::ScrollDown => app.handle_mouse_scroll_down(),
                        // Let other mouse events (clicks, drags) pass through for text selection
                        _ => {}
                    }
                }
                _ => {}
            }
        }
    }
    Ok(())
}
