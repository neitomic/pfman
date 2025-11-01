pub mod dashboard;
pub mod logs_viewer;
pub mod session_form;

use crate::models::Session;
use crate::process::ProcessManager;
use crate::storage::Storage;

#[derive(Debug, Clone, PartialEq)]
pub enum Screen {
    Dashboard,
    LogsViewer(usize),
    SessionForm(FormMode),
}

#[derive(Debug, Clone, PartialEq)]
pub enum FormMode {
    Create,
    Edit(usize),
}

pub struct AppState {
    pub sessions: Vec<Session>,
    pub current_screen: Screen,
    pub selected_index: usize,
    pub search_query: String,
    pub search_mode: bool,
    pub search_cursor_pos: usize,
    pub storage: Storage,
    pub process_manager: ProcessManager,
    pub delete_confirmation: Option<usize>,
}

impl AppState {
    pub fn new() -> color_eyre::Result<Self> {
        let storage = Storage::new()?;
        let sessions = storage.load_sessions()?;
        let process_manager = ProcessManager::new(Storage::new()?);

        // Sync monitored sessions with loaded sessions
        process_manager.sync_monitored_sessions(&sessions);

        Ok(Self {
            sessions,
            current_screen: Screen::Dashboard,
            selected_index: 0,
            search_query: String::new(),
            search_mode: false,
            search_cursor_pos: 0,
            storage,
            process_manager,
            delete_confirmation: None,
        })
    }

    pub fn save(&self) -> color_eyre::Result<()> {
        self.storage.save_sessions(&self.sessions)
    }

    pub fn filtered_sessions(&self) -> Vec<(usize, &Session)> {
        if self.search_query.is_empty() {
            self.sessions.iter().enumerate().collect()
        } else {
            self.sessions
                .iter()
                .enumerate()
                .filter(|(_, s)| {
                    s.name
                        .to_lowercase()
                        .contains(&self.search_query.to_lowercase())
                        || s.target
                            .to_lowercase()
                            .contains(&self.search_query.to_lowercase())
                        || s.local_port.to_string().contains(&self.search_query)
                        || s.remote_port
                            .map(|p| p.to_string().contains(&self.search_query))
                            .unwrap_or(false)
                })
                .collect()
        }
    }
}
