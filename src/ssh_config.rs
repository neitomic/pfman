use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

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

/// In-progress Host block; may list several concrete names sharing options.
struct HostBlock {
    names: Vec<String>,
    hostname: Option<String>,
    user: Option<String>,
    port: Option<u16>,
}

impl HostBlock {
    fn into_hosts(self) -> Vec<SshHost> {
        self.names
            .into_iter()
            .map(|name| SshHost {
                name,
                hostname: self.hostname.clone(),
                user: self.user.clone(),
                port: self.port,
            })
            .collect()
    }
}

pub fn parse_ssh_config() -> Vec<SshHost> {
    let config_path = get_ssh_config_path();
    let mut visited = HashSet::new();
    parse_ssh_config_file(&config_path, &mut visited)
}

fn parse_ssh_config_file(path: &Path, visited: &mut HashSet<PathBuf>) -> Vec<SshHost> {
    let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    if !visited.insert(canonical) {
        return Vec::new();
    }

    if !path.exists() {
        return Vec::new();
    }

    let content = match fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };

    // Relative Include paths in user config resolve under ~/.ssh (OpenSSH).
    let include_base = get_ssh_config_dir();

    let mut hosts = Vec::new();
    let mut current: Option<HostBlock> = None;

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
                if let Some(block) = current.take() {
                    hosts.extend(block.into_hosts());
                }

                let names: Vec<String> = parts[1..]
                    .iter()
                    .filter(|n| !n.contains('*') && !n.contains('?'))
                    .map(|s| (*s).to_string())
                    .collect();

                if !names.is_empty() {
                    current = Some(HostBlock {
                        names,
                        hostname: None,
                        user: None,
                        port: None,
                    });
                }
            }
            "hostname" => {
                if let Some(ref mut block) = current {
                    if parts.len() > 1 {
                        block.hostname = Some(parts[1].to_string());
                    }
                }
            }
            "user" => {
                if let Some(ref mut block) = current {
                    if parts.len() > 1 {
                        block.user = Some(parts[1].to_string());
                    }
                }
            }
            "port" => {
                if let Some(ref mut block) = current {
                    if parts.len() > 1 {
                        block.port = parts[1].parse().ok();
                    }
                }
            }
            "include" => {
                // Include does not close the current Host/Match block.
                for pattern in &parts[1..] {
                    for included_path in expand_include_pattern(pattern, &include_base) {
                        hosts.extend(parse_ssh_config_file(&included_path, visited));
                    }
                }
            }
            _ => {}
        }
    }

    if let Some(block) = current {
        hosts.extend(block.into_hosts());
    }

    hosts
}

/// Expand an Include pattern into concrete file paths (lexical order).
///
/// Supports `~` expansion, relative paths under `base`, and simple globs in
/// the final path component (e.g. `config.d/*`).
fn expand_include_pattern(pattern: &str, base: &Path) -> Vec<PathBuf> {
    let expanded = expand_tilde(pattern);
    let path = {
        let p = Path::new(&expanded);
        if p.is_absolute() {
            p.to_path_buf()
        } else {
            base.join(p)
        }
    };

    let path_str = path.to_string_lossy();
    if path_str.contains('*') || path_str.contains('?') {
        expand_glob(&path)
    } else if path.is_file() {
        vec![path]
    } else {
        Vec::new()
    }
}

fn expand_tilde(pattern: &str) -> String {
    if pattern == "~" {
        return home_dir().to_string_lossy().into_owned();
    }
    if let Some(rest) = pattern.strip_prefix("~/") {
        return home_dir().join(rest).to_string_lossy().into_owned();
    }
    pattern.to_string()
}

/// Expand a path whose final component may contain `*` / `?` globs.
fn expand_glob(path: &Path) -> Vec<PathBuf> {
    let Some(file_name) = path.file_name().and_then(|s| s.to_str()) else {
        return Vec::new();
    };
    let Some(parent) = path.parent() else {
        return Vec::new();
    };

    if !parent.is_dir() {
        return Vec::new();
    }

    let mut matches: Vec<PathBuf> = match fs::read_dir(parent) {
        Ok(entries) => entries
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.is_file())
            .filter(|p| {
                p.file_name()
                    .and_then(|n| n.to_str())
                    .map(|n| glob_match(file_name, n))
                    .unwrap_or(false)
            })
            .collect(),
        Err(_) => return Vec::new(),
    };

    matches.sort();
    matches
}

/// Minimal glob: `*` any sequence, `?` single char. No character classes.
fn glob_match(pattern: &str, name: &str) -> bool {
    let p: Vec<char> = pattern.chars().collect();
    let n: Vec<char> = name.chars().collect();
    glob_match_chars(&p, &n)
}

fn glob_match_chars(pattern: &[char], name: &[char]) -> bool {
    let (mut pi, mut ni) = (0, 0);
    let mut star_p = None;
    let mut star_n = 0;

    while ni < name.len() {
        if pi < pattern.len() && (pattern[pi] == '?' || pattern[pi] == name[ni]) {
            pi += 1;
            ni += 1;
        } else if pi < pattern.len() && pattern[pi] == '*' {
            star_p = Some(pi);
            star_n = ni;
            pi += 1;
        } else if let Some(sp) = star_p {
            pi = sp + 1;
            star_n += 1;
            ni = star_n;
        } else {
            return false;
        }
    }

    while pi < pattern.len() && pattern[pi] == '*' {
        pi += 1;
    }
    pi == pattern.len()
}

fn home_dir() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    PathBuf::from(home)
}

fn get_ssh_config_dir() -> PathBuf {
    home_dir().join(".ssh")
}

fn get_ssh_config_path() -> PathBuf {
    get_ssh_config_dir().join("config")
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn write_file(path: &Path, content: &str) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        let mut f = fs::File::create(path).unwrap();
        f.write_all(content.as_bytes()).unwrap();
    }

    #[test]
    fn glob_match_basic() {
        assert!(glob_match("*", "anything"));
        assert!(glob_match("*.conf", "foo.conf"));
        assert!(!glob_match("*.conf", "foo.cfg"));
        assert!(glob_match("host?", "host1"));
        assert!(!glob_match("host?", "host12"));
    }

    #[test]
    fn parses_include_and_globs() {
        let dir = std::env::temp_dir().join(format!("pfman_ssh_test_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("config.d")).unwrap();

        write_file(
            &dir.join("config.d").join("work"),
            r#"
Host work-box
  HostName work.example.com
  User bob
  Port 2222
"#,
        );
        write_file(
            &dir.join("extra"),
            r#"
Host extra-host
  HostName extra.example.com
"#,
        );
        write_file(
            &dir.join("config"),
            &format!(
                r#"
Host main
  HostName main.example.com
  User alice

Include {}
Include {}
"#,
                dir.join("config.d").join("*").display(),
                dir.join("extra").display(),
            ),
        );

        let mut visited = HashSet::new();
        let hosts = parse_ssh_config_file(&dir.join("config"), &mut visited);
        let names: Vec<_> = hosts.iter().map(|h| h.name.as_str()).collect();
        assert!(names.contains(&"main"));
        assert!(names.contains(&"work-box"));
        assert!(names.contains(&"extra-host"));

        let work = hosts.iter().find(|h| h.name == "work-box").unwrap();
        assert_eq!(work.hostname.as_deref(), Some("work.example.com"));
        assert_eq!(work.user.as_deref(), Some("bob"));
        assert_eq!(work.port, Some(2222));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn include_cycle_does_not_loop() {
        let dir = std::env::temp_dir().join(format!("pfman_ssh_cycle_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();

        let a = dir.join("a");
        let b = dir.join("b");
        write_file(&a, &format!("Host from-a\nInclude {}\n", b.display()));
        write_file(&b, &format!("Host from-b\nInclude {}\n", a.display()));

        let mut visited = HashSet::new();
        let hosts = parse_ssh_config_file(&a, &mut visited);
        let names: Vec<_> = hosts.iter().map(|h| h.name.as_str()).collect();
        assert_eq!(names.iter().filter(|n| **n == "from-a").count(), 1);
        assert_eq!(names.iter().filter(|n| **n == "from-b").count(), 1);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn multi_name_host_shares_options() {
        let dir = std::env::temp_dir().join(format!("pfman_ssh_multi_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();

        write_file(
            &dir.join("config"),
            r#"
Host alpha beta
  HostName shared.example.com
  User deploy
"#,
        );

        let mut visited = HashSet::new();
        let hosts = parse_ssh_config_file(&dir.join("config"), &mut visited);
        assert_eq!(hosts.len(), 2);
        assert!(hosts.iter().all(|h| h.hostname.as_deref() == Some("shared.example.com")));
        assert!(hosts.iter().all(|h| h.user.as_deref() == Some("deploy")));

        let _ = fs::remove_dir_all(&dir);
    }
}
