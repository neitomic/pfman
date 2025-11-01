use crate::models::Session;
use crate::ui::AppState;
use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame,
};

pub fn render(frame: &mut Frame, state: &AppState, session_idx: usize, area: Rect) {
    if let Some(session) = state.sessions.get(session_idx) {
        // Adjust header height based on whether there's an error
        let header_height = if matches!(session.status, crate::models::SessionStatus::Error(_)) {
            6
        } else {
            5
        };

        let chunks = Layout::vertical([
            Constraint::Length(header_height),
            Constraint::Min(0),
            Constraint::Length(3),
        ])
        .split(area);

        render_header(frame, session, chunks[0]);
        render_logs(frame, state, session, chunks[1]);
        render_help(frame, chunks[2]);
    }
}

fn render_header(frame: &mut Frame, session: &Session, area: Rect) {
    let mut header_text = vec![
        Line::from(vec![
            Span::styled("Name: ", Style::default().add_modifier(Modifier::BOLD)),
            Span::raw(&session.name),
        ]),
        Line::from(vec![
            Span::styled("Type: ", Style::default().add_modifier(Modifier::BOLD)),
            Span::raw(session.session_type.as_str()),
            Span::raw("  "),
            Span::styled("Target: ", Style::default().add_modifier(Modifier::BOLD)),
            Span::raw(&session.target),
        ]),
        Line::from(vec![
            Span::styled("Ports: ", Style::default().add_modifier(Modifier::BOLD)),
            Span::raw(session.port_mapping()),
            Span::raw("  "),
            Span::styled("Status: ", Style::default().add_modifier(Modifier::BOLD)),
            Span::styled(
                session.status.as_str(),
                Style::default().fg(match &session.status {
                    crate::models::SessionStatus::Running => Color::Green,
                    crate::models::SessionStatus::Stopped => Color::Gray,
                    crate::models::SessionStatus::Error(_) => Color::Red,
                }),
            ),
        ]),
    ];

    // Add error message line if status is Error
    if let crate::models::SessionStatus::Error(msg) = &session.status {
        header_text.push(Line::from(vec![
            Span::styled("Error: ", Style::default().add_modifier(Modifier::BOLD).fg(Color::Red)),
            Span::styled(msg, Style::default().fg(Color::Red)),
        ]));
    }

    let header = Paragraph::new(header_text)
        .block(Block::default().borders(Borders::ALL).title("Session Details"));
    frame.render_widget(header, area);
}

fn render_logs(frame: &mut Frame, state: &AppState, session: &Session, area: Rect) {
    let logs = state
        .storage
        .read_logs(&session.id)
        .unwrap_or_else(|_| "Failed to read logs".to_string());

    let log_text = if logs.is_empty() {
        match &session.status {
            crate::models::SessionStatus::Stopped => {
                if session.last_started.is_some() {
                    "Session was stopped. No logs were generated.".to_string()
                } else {
                    "Session has never been started. No logs available.".to_string()
                }
            }
            crate::models::SessionStatus::Error(_) => {
                "Session failed. Check error message above or logs may be empty.".to_string()
            }
            crate::models::SessionStatus::Running => {
                "Session is running but no output yet...".to_string()
            }
        }
    } else {
        logs
    };

    let title = match &session.status {
        crate::models::SessionStatus::Running => "Logs (Live)",
        crate::models::SessionStatus::Stopped => "Logs (Historical)",
        crate::models::SessionStatus::Error(_) => "Logs (Error)",
    };

    // Calculate scroll position to show bottom (tail behavior)
    let total_lines = log_text.lines().count() as u16;
    let visible_height = area.height.saturating_sub(2); // Subtract borders
    let scroll_offset = total_lines.saturating_sub(visible_height);

    let logs_widget = Paragraph::new(log_text)
        .block(Block::default().borders(Borders::ALL).title(title))
        .wrap(Wrap { trim: false })
        .scroll((scroll_offset, 0));

    frame.render_widget(logs_widget, area);
}

fn render_help(frame: &mut Frame, area: Rect) {
    let help_text = Line::from(vec![
        Span::styled("s", Style::default().fg(Color::Yellow)),
        Span::raw(" start/stop | "),
        Span::styled("r", Style::default().fg(Color::Yellow)),
        Span::raw(" restart | "),
        Span::styled("e", Style::default().fg(Color::Yellow)),
        Span::raw(" edit | "),
        Span::styled("Esc", Style::default().fg(Color::Yellow)),
        Span::raw(" back"),
    ]);

    let help = Paragraph::new(help_text).block(Block::default().borders(Borders::ALL));
    frame.render_widget(help, area);
}
