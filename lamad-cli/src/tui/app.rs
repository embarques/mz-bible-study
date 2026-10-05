//! MVU Model + Message + update().

use crossterm::event::KeyCode;

use crate::job::{Audience, PrepareJob};
use crate::scans;
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Form,
    Running,
    Done,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    Pdf,
    From,
    To,
}

#[derive(Debug, Clone)]
pub struct Model {
    pub pdfs: Vec<PathBuf>,
    pub selected: usize,
    pub from: String,
    pub to: String,
    pub focus: Focus,
    pub phase: Phase,
    pub log: Vec<String>,
    pub audience: Audience,
    pub quit: bool,
    pub job_finished: bool,
    pub error: Option<String>,
}

#[allow(dead_code)]
pub enum Message {
    Key(KeyCode),
    AgentLine(String),
    Finished,
    Failed(String),
    Submit,
}

impl Model {
    pub fn new(pdfs: Vec<PathBuf>, audience: Audience) -> Self {
        let focus = if pdfs.len() > 1 {
            Focus::Pdf
        } else {
            Focus::From
        };
        Self {
            pdfs,
            selected: 0,
            from: String::new(),
            to: String::new(),
            focus,
            phase: Phase::Form,
            log: Vec::new(),
            audience,
            quit: false,
            job_finished: false,
            error: None,
        }
    }

    pub fn into_job(self) -> Option<PrepareJob> {
        self.job_snapshot()
    }

    pub fn job_snapshot(&self) -> Option<PrepareJob> {
        let from: u32 = self.from.parse().ok()?;
        let to: u32 = self.to.parse().ok()?;
        if to < from {
            return None;
        }
        let pdf = if self.pdfs.is_empty() {
            None
        } else {
            Some(self.pdfs[self.selected.min(self.pdfs.len() - 1)].clone())
        };
        Some(PrepareJob {
            pdf,
            pdf_from: from,
            from,
            to,
            discover: false,
            audience: self.audience,
            prepare_only: false,
            force_prepare: false,
            gen_images: false,
            export_pdf: true,
            review: false,
            template: None,
            stream: false,
            stop_on_error: true,
        })
    }
}

pub fn update(mut model: Model, msg: Message) -> Model {
    match msg {
        Message::Finished => {
            model.phase = Phase::Done;
            model.job_finished = true;
            model.log.push("Listo.".into());
        }
        Message::Failed(e) => {
            model.phase = Phase::Done;
            model.job_finished = true;
            model.error = Some(e.clone());
            model.log.push(format!("Error: {e}"));
        }
        Message::AgentLine(line) => {
            model.log.push(line);
            if model.log.len() > 200 {
                model.log.drain(0..model.log.len() - 200);
            }
        }
        Message::Submit => {
            if let Some(err) = validate_form(&model) {
                model.error = Some(err);
            } else {
                model.error = None;
                model.phase = Phase::Running;
                model.log.push("Iniciando…".into());
            }
        }
        Message::Key(code) => match model.phase {
            Phase::Form => handle_form_key(&mut model, code),
            Phase::Running => {
                // ignore most keys while running
                if matches!(code, KeyCode::Char('q') | KeyCode::Esc) {
                    // cannot cancel easily; ignore
                }
            }
            Phase::Done => {
                if matches!(code, KeyCode::Char('q') | KeyCode::Esc | KeyCode::Enter) {
                    model.quit = true;
                }
            }
        },
    }
    model
}

fn validate_form(model: &Model) -> Option<String> {
    if model.pdfs.is_empty() {
        return Some("No hay PDF en scans/.".into());
    }
    let from: u32 = match model.from.parse() {
        Ok(n) if n >= 1 => n,
        _ => return Some("Desde: número de estudio inválido.".into()),
    };
    let to: u32 = match model.to.parse() {
        Ok(n) if n >= 1 => n,
        _ => return Some("Hasta: número de estudio inválido.".into()),
    };
    if to < from {
        return Some("Hasta debe ser ≥ Desde.".into());
    }
    let _ = scans::list_pending_pdfs;
    None
}

fn handle_form_key(model: &mut Model, code: KeyCode) {
    match code {
        KeyCode::Esc | KeyCode::Char('q') => model.quit = true,
        KeyCode::Tab | KeyCode::Down => {
            model.focus = match model.focus {
                Focus::Pdf => Focus::From,
                Focus::From => Focus::To,
                Focus::To => {
                    if model.pdfs.len() > 1 {
                        Focus::Pdf
                    } else {
                        Focus::From
                    }
                }
            };
        }
        KeyCode::BackTab | KeyCode::Up => {
            model.focus = match model.focus {
                Focus::Pdf => Focus::To,
                Focus::From => {
                    if model.pdfs.len() > 1 {
                        Focus::Pdf
                    } else {
                        Focus::To
                    }
                }
                Focus::To => Focus::From,
            };
        }
        KeyCode::Enter => {
            if model.focus == Focus::To || (model.focus == Focus::From && !model.to.is_empty()) {
                // submit via update Submit
                if let Some(err) = validate_form(model) {
                    model.error = Some(err);
                } else {
                    model.error = None;
                    model.phase = Phase::Running;
                    model.log.push("Iniciando…".into());
                }
            } else {
                model.focus = match model.focus {
                    Focus::Pdf => Focus::From,
                    Focus::From => Focus::To,
                    Focus::To => Focus::To,
                };
            }
        }
        KeyCode::Left | KeyCode::Char('h') if model.focus == Focus::Pdf => {
            if model.selected > 0 {
                model.selected -= 1;
            }
        }
        KeyCode::Right | KeyCode::Char('l') if model.focus == Focus::Pdf => {
            if model.selected + 1 < model.pdfs.len() {
                model.selected += 1;
            }
        }
        KeyCode::Backspace => match model.focus {
            Focus::From => {
                model.from.pop();
            }
            Focus::To => {
                model.to.pop();
            }
            Focus::Pdf => {}
        },
        KeyCode::Char(c) if c.is_ascii_digit() => match model.focus {
            Focus::From => model.from.push(c),
            Focus::To => model.to.push(c),
            Focus::Pdf => {}
        },
        _ => {}
    }
}
