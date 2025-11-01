use crate::models::SessionStatus;
use crate::ui::AppState;
use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Cell, Clear, Paragraph, Row, Table},
};
use std::time::{SystemTime, UNIX_EPOCH};

pub fn render(frame: &mut Frame, state: &AppState, area: Rect) {
    let chunks = Layout::vertical([
        Constraint::Length(3),
        Constraint::Min(0),
        Constraint::Length(3),
    ])
    .split(area);

    render_title(frame, chunks[0]);
    render_table(frame, state, chunks[1]);
    render_help(frame, state, chunks[2]);

    // Render confirmation dialog on top if active
    if state.delete_confirmation.is_some() {
        render_delete_confirmation(frame, state, area);
    }
}

fn render_title(frame: &mut Frame, area: Rect) {
    let title = Paragraph::new("Port-Forwarding Manager")
        .style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )
        .block(Block::default().borders(Borders::ALL));
    frame.render_widget(title, area);
}

fn render_table(frame: &mut Frame, state: &AppState, area: Rect) {
    let filtered = state.filtered_sessions();

    let header = Row::new(vec![
        Cell::from("Name").style(Style::default().add_modifier(Modifier::BOLD)),
        Cell::from("Type").style(Style::default().add_modifier(Modifier::BOLD)),
        Cell::from("Target").style(Style::default().add_modifier(Modifier::BOLD)),
        Cell::from("Ports").style(Style::default().add_modifier(Modifier::BOLD)),
        Cell::from("Status").style(Style::default().add_modifier(Modifier::BOLD)),
        Cell::from("Uptime").style(Style::default().add_modifier(Modifier::BOLD)),
    ])
    .height(1);

    let rows: Vec<Row> = filtered
        .iter()
        .enumerate()
        .map(|(idx, (_, session))| {
            let (status_text, status_color) = match &session.status {
                SessionStatus::Running => (session.status.as_str().to_string(), Color::Green),
                SessionStatus::Stopped => (session.status.as_str().to_string(), Color::Gray),
                SessionStatus::Error(msg) => {
                    let short_msg = if msg.len() > 30 {
                        format!("{}...", &msg[..27])
                    } else {
                        msg.clone()
                    };
                    (format!("Error: {}", short_msg), Color::Red)
                }
            };

            let style = if idx == state.selected_index {
                Style::default()
                    .bg(Color::DarkGray)
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };

            Row::new(vec![
                Cell::from(session.name.clone()),
                Cell::from(session.session_type.as_str()),
                Cell::from(session.target.clone()),
                Cell::from(session.port_mapping()),
                Cell::from(status_text).style(Style::default().fg(status_color)),
                Cell::from(session.uptime_string()),
            ])
            .style(style)
        })
        .collect();

    let widths = [
        Constraint::Percentage(20),
        Constraint::Percentage(10),
        Constraint::Percentage(25),
        Constraint::Percentage(15),
        Constraint::Percentage(15),
        Constraint::Percentage(15),
    ];

    // Add blinking cursor to search query in title
    let title = if state.search_mode {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis();
        let show_cursor = (now / 500) % 2 == 0;

        let cursor_pos = state.search_cursor_pos.min(state.search_query.len());
        let search_display = if show_cursor {
            format!(
                "{}█{}",
                &state.search_query[..cursor_pos],
                &state.search_query[cursor_pos..]
            )
        } else {
            state.search_query.clone()
        };
        format!("Sessions (Search: {})", search_display)
    } else {
        format!("Sessions ({})", filtered.len())
    };

    let table = Table::new(rows, widths)
        .header(header)
        .block(Block::default().borders(Borders::ALL).title(title))
        .row_highlight_style(Style::default().add_modifier(Modifier::BOLD));

    frame.render_widget(table, area);
}

fn render_help(frame: &mut Frame, state: &AppState, area: Rect) {
    let help_text = if state.delete_confirmation.is_some() {
        Line::from(vec![
            Span::styled("y", Style::default().fg(Color::Yellow)),
            Span::raw(" confirm | "),
            Span::styled("n", Style::default().fg(Color::Yellow)),
            Span::raw(" cancel | "),
            Span::styled("Esc", Style::default().fg(Color::Yellow)),
            Span::raw(" cancel"),
        ])
    } else if state.search_mode {
        Line::from(vec![
            Span::raw("Type to search | "),
            Span::styled("Esc", Style::default().fg(Color::Yellow)),
            Span::raw(" cancel | "),
            Span::styled("Enter", Style::default().fg(Color::Yellow)),
            Span::raw(" apply"),
        ])
    } else {
        Line::from(vec![
            Span::styled("c", Style::default().fg(Color::Yellow)),
            Span::raw(" create | "),
            Span::styled("e", Style::default().fg(Color::Yellow)),
            Span::raw(" edit | "),
            Span::styled("d", Style::default().fg(Color::Yellow)),
            Span::raw(" delete | "),
            Span::styled("s", Style::default().fg(Color::Yellow)),
            Span::raw(" start/stop | "),
            Span::styled("l", Style::default().fg(Color::Yellow)),
            Span::raw(" view logs | "),
            Span::styled("/", Style::default().fg(Color::Yellow)),
            Span::raw(" search | "),
            Span::styled("q", Style::default().fg(Color::Yellow)),
            Span::raw(" quit"),
        ])
    };

    let help = Paragraph::new(help_text).block(Block::default().borders(Borders::ALL));
    frame.render_widget(help, area);
}

fn render_delete_confirmation(frame: &mut Frame, state: &AppState, area: Rect) {
    if let Some(idx) = state.delete_confirmation {
        let session_name = state
            .sessions
            .get(idx)
            .map(|s| s.name.as_str())
            .unwrap_or("Unknown");

        // Create centered popup
        let popup_width = 50.min(area.width - 4);
        let popup_height = 7;
        let popup_x = (area.width.saturating_sub(popup_width)) / 2;
        let popup_y = (area.height.saturating_sub(popup_height)) / 2;

        let popup_area = Rect {
            x: area.x + popup_x,
            y: area.y + popup_y,
            width: popup_width,
            height: popup_height,
        };

        let text = vec![
            Line::from(""),
            Line::from(Span::styled(
                "Delete Session?",
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
            )),
            Line::from(""),
            Line::from(Span::raw(format!("Session: {}", session_name))),
            Line::from(""),
        ];

        let paragraph = Paragraph::new(text).alignment(Alignment::Center).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Red))
                .style(Style::default().bg(Color::Black)),
        );

        frame.render_widget(Clear, popup_area);
        frame.render_widget(paragraph, popup_area);
    }
}
