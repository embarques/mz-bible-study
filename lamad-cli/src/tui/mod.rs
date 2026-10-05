//! Ratatui TUI — terminal init/restore + event loop.

mod app;
mod ui;

use anyhow::{Context, Result};
use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;
use std::io::{self, Stdout};
use std::time::Duration;

use crate::config::Config;
use crate::job::PrepareJob;
use crate::tui::app::{Message, Model, Phase};

/// Run the interactive prepare form. Returns a PrepareJob on submit, or None if quit.
pub fn run_form(cfg: &Config) -> Result<Option<PrepareJob>> {
    let pdfs = crate::scans::list_pending_pdfs().unwrap_or_default();
    let mut model = Model::new(pdfs, cfg.audience_enum());

    let mut terminal = setup()?;
    let result = loop {
        terminal.draw(|f| ui::view(f, &model))?;

        if model.phase == Phase::Done {
            break Ok(model.into_job());
        }
        if model.quit {
            break Ok(None);
        }

        if event::poll(Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    model = app::update(model, Message::Key(key.code));
                }
            }
        }
    };
    restore(&mut terminal)?;
    result
}

/// Full TUI prepare: form → run job with status log (async work via runtime handle).
pub async fn run_prepare_tui(cfg: Config) -> Result<()> {
    let pdfs = crate::scans::list_pending_pdfs().unwrap_or_default();
    if pdfs.is_empty() {
        anyhow::bail!(
            "No PDF in scans/. Put a scan PDF in scans/ (not pending/, complete/, or error/), then retry."
        );
    }
    let mut model = Model::new(pdfs, cfg.audience_enum());

    let mut terminal = setup()?;
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<Message>();

    let outcome = loop {
        terminal.draw(|f| ui::view(f, &model))?;

        // Drain async messages
        while let Ok(msg) = rx.try_recv() {
            model = app::update(model, msg);
        }

        if model.quit && model.phase != Phase::Running {
            break Ok(());
        }

        if matches!(model.phase, Phase::Done) && model.job_finished {
            // show done briefly
            if event::poll(Duration::from_millis(50))? {
                if let Event::Key(key) = event::read()? {
                    if key.kind == KeyEventKind::Press
                        && matches!(key.code, KeyCode::Char('q') | KeyCode::Esc | KeyCode::Enter)
                    {
                        break Ok(());
                    }
                }
            }
            continue;
        }

        if event::poll(Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                if key.kind != KeyEventKind::Press {
                    continue;
                }
                let prev_phase = model.phase;
                model = app::update(model, Message::Key(key.code));

                if model.phase == Phase::Running && prev_phase == Phase::Form {
                    if let Some(job) = model.job_snapshot() {
                        let cfg2 = cfg.clone();
                        let tx2 = tx.clone();
                        tokio::spawn(async move {
                            let res = crate::run(job, &cfg2).await;
                            let _ = match res {
                                Ok(()) => tx2.send(Message::Finished),
                                Err(e) => tx2.send(Message::Failed(format!("{e:#}"))),
                            };
                        });
                    }
                }
            }
        }
    };

    restore(&mut terminal)?;
    outcome
}

fn setup() -> Result<Terminal<CrosstermBackend<Stdout>>> {
    enable_raw_mode().context("enable raw mode")?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen).context("enter alt screen")?;
    let backend = CrosstermBackend::new(stdout);
    Terminal::new(backend).context("terminal")
}

fn restore(terminal: &mut Terminal<CrosstermBackend<Stdout>>) -> Result<()> {
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    Ok(())
}
