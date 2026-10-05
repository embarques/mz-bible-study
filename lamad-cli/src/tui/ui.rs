//! view() only — draw widgets, no HTTP/OOXML.

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph, Wrap};
use ratatui::Frame;

use crate::tui::app::{Focus, Model, Phase};

pub fn view(f: &mut Frame, model: &Model) {
    let area = f.area();
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(8),
            Constraint::Length(3),
        ])
        .split(area);

    let title = Paragraph::new("lamad — Preparar estudios (Mount Zion Church)")
        .style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))
        .block(Block::default().borders(Borders::ALL));
    f.render_widget(title, chunks[0]);

    match model.phase {
        Phase::Form => draw_form(f, chunks[1], model),
        Phase::Running | Phase::Done => draw_log(f, chunks[1], model),
    }

    let help = match model.phase {
        Phase::Form => {
            "Tab cambia campo · ←/→ elige PDF · Enter prepara · q/Esc sale"
        }
        Phase::Running => "Preparando… (espere)",
        Phase::Done => "Enter / q para salir",
    };
    let footer = Paragraph::new(help).block(Block::default().borders(Borders::ALL));
    f.render_widget(footer, chunks[2]);
}

fn draw_form(f: &mut Frame, area: Rect, model: &Model) {
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(5),
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Min(2),
        ])
        .split(area);

    let pdf_label = if model.pdfs.is_empty() {
        "(ningún PDF en scans/)".into()
    } else {
        model.pdfs[model.selected.min(model.pdfs.len() - 1)]
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default()
    };
    let pdf_style = focus_style(model.focus == Focus::Pdf);
    let pdf = Paragraph::new(format!(
        "PDF ({}/{}): {}",
        if model.pdfs.is_empty() {
            0
        } else {
            model.selected + 1
        },
        model.pdfs.len(),
        pdf_label
    ))
    .style(pdf_style)
    .block(Block::default().borders(Borders::ALL).title("PDF"));
    f.render_widget(pdf, rows[0]);

    let from = Paragraph::new(model.from.as_str())
        .style(focus_style(model.focus == Focus::From))
        .block(Block::default().borders(Borders::ALL).title("Desde (estudio)"));
    f.render_widget(from, rows[1]);

    let to = Paragraph::new(model.to.as_str())
        .style(focus_style(model.focus == Focus::To))
        .block(Block::default().borders(Borders::ALL).title("Hasta (estudio)"));
    f.render_widget(to, rows[2]);

    if let Some(err) = &model.error {
        let e = Paragraph::new(err.as_str())
            .style(Style::default().fg(Color::Red))
            .wrap(Wrap { trim: true });
        f.render_widget(e, rows[3]);
    } else {
        let hint = Paragraph::new(Line::from(vec![
            Span::raw("Audience: "),
            Span::styled(
                model.audience.as_str(),
                Style::default().fg(Color::Yellow),
            ),
            Span::raw(" · último estudio del rango sin Próximo"),
        ]));
        f.render_widget(hint, rows[3]);
    }
}

fn draw_log(f: &mut Frame, area: Rect, model: &Model) {
    let title = match model.phase {
        Phase::Running => "Ejecutando",
        Phase::Done => {
            if model.error.is_some() {
                "Terminado con error"
            } else {
                "Listo"
            }
        }
        Phase::Form => "",
    };
    let items: Vec<ListItem> = model
        .log
        .iter()
        .rev()
        .take(area.height.saturating_sub(2) as usize)
        .rev()
        .map(|l| ListItem::new(l.as_str()))
        .collect();
    let list = List::new(items).block(Block::default().borders(Borders::ALL).title(title));
    f.render_widget(list, area);
}

fn focus_style(focused: bool) -> Style {
    if focused {
        Style::default()
            .fg(Color::Black)
            .bg(Color::Cyan)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default()
    }
}
