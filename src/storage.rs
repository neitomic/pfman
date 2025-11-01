use crate::models::Session;
use color_eyre::Result;
use std::fs;
use std::path::PathBuf;
use uuid::Uuid;

#[derive(Clone)]
pub struct Storage {
    data_dir: PathBuf,
}

impl Storage {
    pub fn new() -> Result<Self> {
        let data_dir = Self::get_data_dir()?;
        fs::create_dir_all(&data_dir)?;
        fs::create_dir_all(data_dir.join("logs"))?;

        // Also create config directory for sessions file
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
        let config_dir = PathBuf::from(home).join(".config/pfman");
        fs::create_dir_all(config_dir)?;

        Ok(Self { data_dir })
    }

    fn get_data_dir() -> Result<PathBuf> {
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
        Ok(PathBuf::from(home).join(".local/share/pfman"))
    }

    fn sessions_file(&self) -> PathBuf {
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
        PathBuf::from(home).join(".config/pfman/sessions.yaml")
    }

    pub fn log_file(&self, session_id: &Uuid) -> PathBuf {
        self.data_dir
            .join("logs")
            .join(format!("{}.log", session_id))
    }

    pub fn load_sessions(&self) -> Result<Vec<Session>> {
        let file = self.sessions_file();
        if !file.exists() {
            return Ok(Vec::new());
        }
        let content = fs::read_to_string(file)?;
        let sessions: Vec<Session> = serde_yaml::from_str(&content)?;
        Ok(sessions)
    }

    pub fn save_sessions(&self, sessions: &[Session]) -> Result<()> {
        let content = serde_yaml::to_string(sessions)?;
        fs::write(self.sessions_file(), content)?;
        Ok(())
    }

    pub fn read_logs(&self, session_id: &Uuid) -> Result<String> {
        let log_file = self.log_file(session_id);
        if !log_file.exists() {
            return Ok(String::new());
        }
        Ok(fs::read_to_string(log_file)?)
    }

    pub fn append_log(&self, session_id: &Uuid, content: &str) -> Result<()> {
        let log_file = self.log_file(session_id);
        let mut existing = self.read_logs(session_id)?;
        existing.push_str(content);
        fs::write(log_file, existing)?;
        Ok(())
    }
}
