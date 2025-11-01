use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct SshHost {
    pub name: String,
    pub hostname: Option<String>,
    pub user: Option<String>,
    pub port: Option<u16>,
}

impl SshHost {
    pub fn display_name(&self) -> String {
        if let Some(user) = &self.user {
            if let Some(hostname) = &self.hostname {
                format!("{}@{} ({})", user, hostname, self.name)
            } else {
                format!("{}@{}", user, self.name)
            }
        } else if let Some(hostname) = &self.hostname {
            format!("{} ({})", hostname, self.name)
        } else {
            self.name.clone()
        }
    }

    pub fn connection_string(&self) -> String {
        if let Some(user) = &self.user {
            format!("{}@{}", user, self.name)
        } else {
            self.name.clone()
        }
    }
}

pub fn parse_ssh_config() -> Vec<SshHost> {
    let config_path = get_ssh_config_path();
    if !config_path.exists() {
        return Vec::new();
    }

    let content = match fs::read_to_string(&config_path) {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };

    let mut hosts = Vec::new();
    let mut current_host: Option<SshHost> = None;

    for line in content.lines() {
        let line = line.trim();

        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.is_empty() {
            continue;
        }

        let key = parts[0].to_lowercase();

        match key.as_str() {
            "host" => {
                if let Some(host) = current_host.take() {
                    hosts.push(host);
                }

                if parts.len() > 1 {
                    let host_name = parts[1..].join(" ");
                    // Skip wildcard hosts
                    if !host_name.contains('*') && !host_name.contains('?') {
                        current_host = Some(SshHost {
                            name: host_name,
                            hostname: None,
                            user: None,
                            port: None,
                        });
                    }
                }
            }
            "hostname" => {
                if let Some(ref mut host) = current_host {
                    if parts.len() > 1 {
                        host.hostname = Some(parts[1].to_string());
                    }
                }
            }
            "user" => {
                if let Some(ref mut host) = current_host {
                    if parts.len() > 1 {
                        host.user = Some(parts[1].to_string());
                    }
                }
            }
            "port" => {
                if let Some(ref mut host) = current_host {
                    if parts.len() > 1 {
                        host.port = parts[1].parse().ok();
                    }
                }
            }
            _ => {}
        }
    }

    if let Some(host) = current_host {
        hosts.push(host);
    }

    hosts
}

fn get_ssh_config_path() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    PathBuf::from(home).join(".ssh").join("config")
}

pub fn filter_hosts(hosts: &[SshHost], query: &str) -> Vec<SshHost> {
    if query.is_empty() {
        return hosts.to_vec();
    }

    let query_lower = query.to_lowercase();
    hosts
        .iter()
        .filter(|h| {
            h.name.to_lowercase().contains(&query_lower)
                || h.hostname
                    .as_ref()
                    .map(|hn| hn.to_lowercase().contains(&query_lower))
                    .unwrap_or(false)
                || h.user
                    .as_ref()
                    .map(|u| u.to_lowercase().contains(&query_lower))
                    .unwrap_or(false)
        })
        .cloned()
        .collect()
}
