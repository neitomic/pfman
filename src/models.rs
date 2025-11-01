use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum SessionType {
    SSH,
    Kubectl,
    Socks5,
}

impl SessionType {
    pub fn as_str(&self) -> &str {
        match self {
            SessionType::SSH => "SSH",
            SessionType::Kubectl => "kubectl",
            SessionType::Socks5 => "SOCKS5",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum SessionStatus {
    Running,
    Stopped,
    Error(String),
}

impl SessionStatus {
    pub fn as_str(&self) -> &str {
        match self {
            SessionStatus::Running => "Running",
            SessionStatus::Stopped => "Stopped",
            SessionStatus::Error(_) => "Error",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub id: Uuid,
    pub name: String,
    pub session_type: SessionType,
    pub target: String,
    pub local_port: u16,
    pub remote_port: Option<u16>,
    pub status: SessionStatus,
    pub pid: Option<u32>,
    pub created_at: DateTime<Utc>,
    pub last_started: Option<DateTime<Utc>>,
    pub additional_options: Vec<String>,
    #[serde(default)]
    pub kube_context: Option<String>,
    #[serde(default)]
    pub kube_namespace: Option<String>,
}

impl Session {
    pub fn new(
        name: String,
        session_type: SessionType,
        target: String,
        local_port: u16,
        remote_port: Option<u16>,
    ) -> Self {
        Self {
            id: Uuid::new_v4(),
            name,
            session_type,
            target,
            local_port,
            remote_port,
            status: SessionStatus::Stopped,
            pid: None,
            created_at: Utc::now(),
            last_started: None,
            additional_options: Vec::new(),
            kube_context: None,
            kube_namespace: None,
        }
    }

    pub fn uptime(&self) -> Option<chrono::Duration> {
        if let SessionStatus::Running = self.status {
            self.last_started.map(|start| Utc::now() - start)
        } else {
            None
        }
    }

    pub fn uptime_string(&self) -> String {
        if let Some(duration) = self.uptime() {
            let hours = duration.num_hours();
            let minutes = duration.num_minutes() % 60;
            let seconds = duration.num_seconds() % 60;
            format!("{:02}:{:02}:{:02}", hours, minutes, seconds)
        } else if let Some(last) = self.last_started {
            let duration = Utc::now() - last;
            if duration.num_days() > 0 {
                format!("{} days ago", duration.num_days())
            } else if duration.num_hours() > 0 {
                format!("{} hours ago", duration.num_hours())
            } else if duration.num_minutes() > 0 {
                format!("{} min ago", duration.num_minutes())
            } else {
                "Just now".to_string()
            }
        } else {
            "Never".to_string()
        }
    }

    pub fn port_mapping(&self) -> String {
        match self.session_type {
            SessionType::Socks5 => format!("{}", self.local_port),
            _ => format!("{} → {}", self.local_port, self.remote_port.unwrap_or(0)),
        }
    }
}
