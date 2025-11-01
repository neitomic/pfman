mod kube_config;
mod models;
mod process;
mod ssh_config;
mod storage;
mod ui;

use color_eyre::Result;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::{DefaultTerminal, Frame};
use ui::session_form::{FormState, FormStep};
use ui::{AppState, FormMode, Screen};

fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;
    let terminal = ratatui::init();
    let result = App::new()?.run(terminal);
    ratatui::restore();
    result
}

pub struct App {
    running: bool,
    state: AppState,
    form_state: Option<FormState>,
}

impl App {
    pub fn new() -> Result<Self> {
        Ok(Self {
            running: true,
            state: AppState::new()?,
            form_state: None,
        })
    }

    pub fn run(mut self, mut terminal: DefaultTerminal) -> Result<()> {
        while self.running {
            terminal.draw(|frame| self.render(frame))?;
            self.handle_crossterm_events()?;

            // Poll for status updates from background monitor
            if self
                .state
                .process_manager
                .poll_status_updates(&mut self.state.sessions)
            {
                // Save updated statuses if any changed
                let _ = self.state.save();
            }

            // Poll for kubectl target updates if in form mode
            if let Some(form_state) = &mut self.form_state {
                form_state.poll_target_updates();
            }
        }
        Ok(())
    }

    fn render(&mut self, frame: &mut Frame) {
        match &self.state.current_screen {
            Screen::Dashboard => ui::dashboard::render(frame, &self.state, frame.area()),
            Screen::LogsViewer(idx) => {
                ui::logs_viewer::render(frame, &self.state, *idx, frame.area())
            }
            Screen::SessionForm(mode) => {
                if let Some(form_state) = &self.form_state {
                    ui::session_form::render(frame, form_state, mode, frame.area());
                }
            }
        }
    }

    fn handle_crossterm_events(&mut self) -> Result<()> {
        if event::poll(std::time::Duration::from_secs(1))? {
            match event::read()? {
                Event::Key(key) if key.kind == KeyEventKind::Press => self.on_key_event(key),
                _ => {}
            }
        }
        Ok(())
    }

    fn on_key_event(&mut self, key: KeyEvent) {
        match &self.state.current_screen {
            Screen::Dashboard => self.handle_dashboard_keys(key),
            Screen::LogsViewer(_) => self.handle_logs_keys(key),
            Screen::SessionForm(_) => self.handle_form_keys(key),
        }
    }

    fn handle_dashboard_keys(&mut self, key: KeyEvent) {
        // Handle delete confirmation dialog
        if self.state.delete_confirmation.is_some() {
            match key.code {
                KeyCode::Char('y') | KeyCode::Char('Y') | KeyCode::Enter => self.confirm_delete(),
                KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => self.cancel_delete(),
                _ => {}
            }
            return;
        }

        if self.state.search_mode {
            match key.code {
                KeyCode::Esc => {
                    self.state.search_mode = false;
                    self.state.search_query.clear();
                    self.state.search_cursor_pos = 0;
                }
                KeyCode::Enter => self.state.search_mode = false,
                KeyCode::Char(c) => {
                    self.state
                        .search_query
                        .insert(self.state.search_cursor_pos, c);
                    self.state.search_cursor_pos += 1;
                }
                KeyCode::Backspace => {
                    if self.state.search_cursor_pos > 0 {
                        self.state
                            .search_query
                            .remove(self.state.search_cursor_pos - 1);
                        self.state.search_cursor_pos -= 1;
                    }
                }
                _ => {}
            }
            return;
        }

        match (key.modifiers, key.code) {
            (KeyModifiers::CONTROL, KeyCode::Char('c') | KeyCode::Char('C'))
            | (_, KeyCode::Char('q')) => self.quit(),
            (_, KeyCode::Up) => self.move_selection(-1),
            (_, KeyCode::Down) => self.move_selection(1),
            (_, KeyCode::Char('c')) => self.create_session(),
            (_, KeyCode::Char('e')) => self.edit_session(),
            (_, KeyCode::Char('d')) => self.delete_session(),
            (_, KeyCode::Char('s')) => self.toggle_session(),
            (_, KeyCode::Char('l')) => self.view_logs(),
            (_, KeyCode::Char('/')) => self.state.search_mode = true,
            _ => {}
        }
    }

    fn handle_logs_keys(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => self.state.current_screen = Screen::Dashboard,
            KeyCode::Char('s') => {
                if let Screen::LogsViewer(idx) = self.state.current_screen {
                    self.state.selected_index = idx;
                    self.toggle_session();
                }
            }
            KeyCode::Char('r') => {
                if let Screen::LogsViewer(idx) = self.state.current_screen {
                    if let Some(session) = self.state.sessions.get_mut(idx) {
                        let _ = self.state.process_manager.stop_session(session);
                        let _ = self.state.process_manager.start_session(session);
                        let _ = self.state.save();
                    }
                }
            }
            KeyCode::Char('e') => {
                if let Screen::LogsViewer(idx) = self.state.current_screen {
                    self.state.selected_index = idx;
                    self.edit_session();
                }
            }
            _ => {}
        }
    }

    fn handle_form_keys(&mut self, key: KeyEvent) {
        if let Some(form_state) = &mut self.form_state {
            // Type selection step
            if form_state.step == FormStep::SelectType {
                match key.code {
                    KeyCode::Up => form_state.move_type_selection(-1),
                    KeyCode::Down => form_state.move_type_selection(1),
                    KeyCode::Enter => form_state.confirm_type_selection(),
                    KeyCode::Esc => {
                        self.state.current_screen = Screen::Dashboard;
                        self.form_state = None;
                    }
                    _ => {}
                }
                return;
            }

            // Handle suggestions navigation
            if form_state.show_suggestions {
                match key.code {
                    KeyCode::Up => {
                        form_state.move_suggestion(-1);
                        return;
                    }
                    KeyCode::Down => {
                        form_state.move_suggestion(1);
                        return;
                    }
                    KeyCode::Enter => {
                        form_state.select_suggestion();
                        return;
                    }
                    KeyCode::Esc => {
                        form_state.hide_suggestions();
                        return;
                    }
                    _ => {}
                }
            }

            match (key.modifiers, key.code) {
                (_, KeyCode::Esc) => {
                    self.state.current_screen = Screen::Dashboard;
                    self.form_state = None;
                }
                (KeyModifiers::CONTROL, KeyCode::Char('s') | KeyCode::Char('S')) => {
                    self.save_form();
                }
                (_, KeyCode::Tab) => {
                    let is_create_mode = matches!(
                        self.state.current_screen,
                        Screen::SessionForm(FormMode::Create)
                    );
                    let old_field = form_state.focused_field;
                    let field_count = form_state.field_count();

                    // Copy port value if leaving a port field and other port is empty (create mode only)
                    if is_create_mode {
                        if form_state.session_type == models::SessionType::Kubectl {
                            if old_field == 4
                                && !form_state.local_port.is_empty()
                                && form_state.remote_port.is_empty()
                            {
                                form_state.remote_port = form_state.local_port.clone();
                            } else if old_field == 5
                                && !form_state.remote_port.is_empty()
                                && form_state.local_port.is_empty()
                            {
                                form_state.local_port = form_state.remote_port.clone();
                            }
                        } else if field_count == 4 {
                            if old_field == 2
                                && !form_state.local_port.is_empty()
                                && form_state.remote_port.is_empty()
                            {
                                form_state.remote_port = form_state.local_port.clone();
                            } else if old_field == 3
                                && !form_state.remote_port.is_empty()
                                && form_state.local_port.is_empty()
                            {
                                form_state.local_port = form_state.remote_port.clone();
                            }
                        }
                    }

                    form_state.focused_field = (form_state.focused_field + 1) % field_count;
                    form_state.hide_suggestions();
                    form_state.cursor_pos =
                        if form_state.session_type == models::SessionType::Kubectl {
                            match form_state.focused_field {
                                0 => form_state.context_field.len(),
                                1 => form_state.name.len(),
                                2 => form_state.namespace_field.len(),
                                3 => form_state.target.len(),
                                4 => form_state.local_port.len(),
                                5 => form_state.remote_port.len(),
                                _ => 0,
                            }
                        } else {
                            match form_state.focused_field {
                                0 => form_state.name.len(),
                                1 => form_state.target.len(),
                                2 => form_state.local_port.len(),
                                3 => form_state.remote_port.len(),
                                _ => 0,
                            }
                        };
                    form_state.on_focus_change();
                    form_state.show_port_suggestions();
                }
                (_, KeyCode::BackTab) => {
                    let is_create_mode = matches!(
                        self.state.current_screen,
                        Screen::SessionForm(FormMode::Create)
                    );
                    let old_field = form_state.focused_field;
                    let field_count = form_state.field_count();

                    // Copy port value if leaving a port field and other port is empty (create mode only)
                    if is_create_mode {
                        if form_state.session_type == models::SessionType::Kubectl {
                            if old_field == 4
                                && !form_state.local_port.is_empty()
                                && form_state.remote_port.is_empty()
                            {
                                form_state.remote_port = form_state.local_port.clone();
                            } else if old_field == 5
                                && !form_state.remote_port.is_empty()
                                && form_state.local_port.is_empty()
                            {
                                form_state.local_port = form_state.remote_port.clone();
                            }
                        } else if field_count == 4 {
                            if old_field == 2
                                && !form_state.local_port.is_empty()
                                && form_state.remote_port.is_empty()
                            {
                                form_state.remote_port = form_state.local_port.clone();
                            } else if old_field == 3
                                && !form_state.remote_port.is_empty()
                                && form_state.local_port.is_empty()
                            {
                                form_state.local_port = form_state.remote_port.clone();
                            }
                        }
                    }

                    form_state.focused_field = if form_state.focused_field == 0 {
                        field_count - 1
                    } else {
                        form_state.focused_field - 1
                    };
                    form_state.hide_suggestions();
                    form_state.cursor_pos =
                        if form_state.session_type == models::SessionType::Kubectl {
                            match form_state.focused_field {
                                0 => form_state.context_field.len(),
                                1 => form_state.name.len(),
                                2 => form_state.namespace_field.len(),
                                3 => form_state.target.len(),
                                4 => form_state.local_port.len(),
                                5 => form_state.remote_port.len(),
                                _ => 0,
                            }
                        } else {
                            match form_state.focused_field {
                                0 => form_state.name.len(),
                                1 => form_state.target.len(),
                                2 => form_state.local_port.len(),
                                3 => form_state.remote_port.len(),
                                _ => 0,
                            }
                        };
                    form_state.on_focus_change();
                    form_state.show_port_suggestions();
                }
                (_, KeyCode::Char(c)) => {
                    if form_state.session_type == models::SessionType::Kubectl {
                        match form_state.focused_field {
                            0 => {
                                let mut new_context = form_state.context_field.clone();
                                new_context.insert(form_state.cursor_pos, c);
                                form_state.cursor_pos += 1;
                                form_state.update_context(new_context);
                            }
                            1 => {
                                form_state.name.insert(form_state.cursor_pos, c);
                                form_state.cursor_pos += 1;
                            }
                            2 => {
                                let mut new_namespace = form_state.namespace_field.clone();
                                new_namespace.insert(form_state.cursor_pos, c);
                                form_state.cursor_pos += 1;
                                form_state.update_namespace(new_namespace);
                                // Reload targets when namespace changes
                                form_state.kube_targets.clear();
                            }
                            3 => {
                                let mut new_target = form_state.target.clone();
                                new_target.insert(form_state.cursor_pos, c);
                                form_state.cursor_pos += 1;
                                form_state.update_target(new_target);
                            }
                            4 => {
                                form_state.local_port.insert(form_state.cursor_pos, c);
                                form_state.cursor_pos += 1;
                            }
                            5 => {
                                form_state.remote_port.insert(form_state.cursor_pos, c);
                                form_state.cursor_pos += 1;
                            }
                            _ => {}
                        }
                    } else {
                        match form_state.focused_field {
                            0 => {
                                form_state.name.insert(form_state.cursor_pos, c);
                                form_state.cursor_pos += 1;
                            }
                            1 => {
                                let mut new_target = form_state.target.clone();
                                new_target.insert(form_state.cursor_pos, c);
                                form_state.cursor_pos += 1;
                                form_state.update_target(new_target);
                            }
                            2 => {
                                form_state.local_port.insert(form_state.cursor_pos, c);
                                form_state.cursor_pos += 1;
                            }
                            3 => {
                                form_state.remote_port.insert(form_state.cursor_pos, c);
                                form_state.cursor_pos += 1;
                            }
                            _ => {}
                        }
                    }
                }
                (_, KeyCode::Backspace) => {
                    if form_state.session_type == models::SessionType::Kubectl {
                        match form_state.focused_field {
                            0 => {
                                if form_state.cursor_pos > 0 {
                                    form_state.context_field.remove(form_state.cursor_pos - 1);
                                    form_state.cursor_pos -= 1;
                                    form_state.update_context(form_state.context_field.clone());
                                }
                            }
                            1 => {
                                if form_state.cursor_pos > 0 {
                                    form_state.name.remove(form_state.cursor_pos - 1);
                                    form_state.cursor_pos -= 1;
                                }
                            }
                            2 => {
                                if form_state.cursor_pos > 0 {
                                    form_state.namespace_field.remove(form_state.cursor_pos - 1);
                                    form_state.cursor_pos -= 1;
                                    form_state.update_namespace(form_state.namespace_field.clone());
                                    // Reload targets when namespace changes
                                    form_state.kube_targets.clear();
                                }
                            }
                            3 => {
                                if form_state.cursor_pos > 0 {
                                    form_state.target.remove(form_state.cursor_pos - 1);
                                    form_state.cursor_pos -= 1;
                                    form_state.update_target(form_state.target.clone());
                                }
                            }
                            4 => {
                                if form_state.cursor_pos > 0 {
                                    form_state.local_port.remove(form_state.cursor_pos - 1);
                                    form_state.cursor_pos -= 1;
                                }
                            }
                            5 => {
                                if form_state.cursor_pos > 0 {
                                    form_state.remote_port.remove(form_state.cursor_pos - 1);
                                    form_state.cursor_pos -= 1;
                                }
                            }
                            _ => {}
                        }
                    } else {
                        match form_state.focused_field {
                            0 => {
                                if form_state.cursor_pos > 0 {
                                    form_state.name.remove(form_state.cursor_pos - 1);
                                    form_state.cursor_pos -= 1;
                                }
                            }
                            1 => {
                                if form_state.cursor_pos > 0 {
                                    form_state.target.remove(form_state.cursor_pos - 1);
                                    form_state.cursor_pos -= 1;
                                    form_state.update_target(form_state.target.clone());
                                }
                            }
                            2 => {
                                if form_state.cursor_pos > 0 {
                                    form_state.local_port.remove(form_state.cursor_pos - 1);
                                    form_state.cursor_pos -= 1;
                                }
                            }
                            3 => {
                                if form_state.cursor_pos > 0 {
                                    form_state.remote_port.remove(form_state.cursor_pos - 1);
                                    form_state.cursor_pos -= 1;
                                }
                            }
                            _ => {}
                        }
                    }
                }
                _ => {}
            }
        }
    }

    fn move_selection(&mut self, delta: i32) {
        let count = self.state.filtered_sessions().len();
        if count == 0 {
            return;
        }
        let new_idx = (self.state.selected_index as i32 + delta).rem_euclid(count as i32) as usize;
        self.state.selected_index = new_idx;
    }

    fn create_session(&mut self) {
        self.form_state = Some(FormState::new());
        self.state.current_screen = Screen::SessionForm(FormMode::Create);
    }

    fn edit_session(&mut self) {
        let filtered = self.state.filtered_sessions();
        if let Some((real_idx, session)) = filtered.get(self.state.selected_index) {
            self.form_state = Some(FormState::from_session(session));
            self.state.current_screen = Screen::SessionForm(FormMode::Edit(*real_idx));
        }
    }

    fn delete_session(&mut self) {
        let filtered = self.state.filtered_sessions();
        if let Some((real_idx, _)) = filtered.get(self.state.selected_index) {
            self.state.delete_confirmation = Some(*real_idx);
        }
    }

    fn confirm_delete(&mut self) {
        if let Some(idx) = self.state.delete_confirmation {
            self.state.sessions.remove(idx);
            let _ = self.state.save();
            if self.state.selected_index > 0 {
                self.state.selected_index -= 1;
            }
            self.state.delete_confirmation = None;
        }
    }

    fn cancel_delete(&mut self) {
        self.state.delete_confirmation = None;
    }

    fn toggle_session(&mut self) {
        let filtered = self.state.filtered_sessions();
        if let Some((real_idx, _)) = filtered.get(self.state.selected_index) {
            let real_idx = *real_idx;
            if let Some(session) = self.state.sessions.get_mut(real_idx) {
                match session.status {
                    models::SessionStatus::Running => {
                        let _ = self.state.process_manager.stop_session(session);
                    }
                    _ => {
                        let _ = self.state.process_manager.start_session(session);
                    }
                }
                let _ = self.state.save();
            }
        }
    }

    fn view_logs(&mut self) {
        let filtered = self.state.filtered_sessions();
        if let Some((real_idx, _)) = filtered.get(self.state.selected_index) {
            self.state.current_screen = Screen::LogsViewer(*real_idx);
        }
    }

    fn save_form(&mut self) {
        if let Some(form_state) = &self.form_state {
            if let Some(session) = form_state.to_session() {
                if let Screen::SessionForm(FormMode::Edit(idx)) = self.state.current_screen {
                    if let Some(existing) = self.state.sessions.get_mut(idx) {
                        existing.name = session.name;
                        existing.session_type = session.session_type;
                        existing.target = session.target;
                        existing.local_port = session.local_port;
                        existing.remote_port = session.remote_port;
                        existing.kube_context = session.kube_context;
                        existing.kube_namespace = session.kube_namespace;
                    }
                } else {
                    self.state.sessions.push(session);
                }
                let _ = self.state.save();
                self.state.current_screen = Screen::Dashboard;
                self.form_state = None;
            }
        }
    }

    fn quit(&mut self) {
        self.running = false;
    }
}
