use crate::models::{Session, SessionStatus, SessionType};
use crate::storage::Storage;
use chrono::{DateTime, Utc};
use color_eyre::Result;
use std::process::{Command, Stdio};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;
use sysinfo::{ProcessRefreshKind, RefreshKind, System};
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct StatusUpdate {
    pub session_id: Uuid,
    pub status: SessionStatus,
    pub pid: Option<u32>,
}

#[derive(Clone, Debug)]
pub(crate) struct MonitoredSession {
    id: Uuid,
    pid: Option<u32>,
    started_at: Option<DateTime<Utc>>,
}

pub struct StatusMonitor {
    thread_handle: Option<thread::JoinHandle<()>>,
    shutdown_sender: Sender<()>,
}

impl StatusMonitor {
    pub fn new(
        sessions: Arc<Mutex<Vec<MonitoredSession>>>,
        storage: Storage,
        update_sender: Sender<StatusUpdate>,
    ) -> Self {
        let (shutdown_sender, shutdown_receiver) = mpsc::channel();

        let thread_handle = thread::spawn(move || {
            Self::monitor_loop(sessions, storage, update_sender, shutdown_receiver);
        });

        Self {
            thread_handle: Some(thread_handle),
            shutdown_sender,
        }
    }

    fn monitor_loop(
        sessions: Arc<Mutex<Vec<MonitoredSession>>>,
        storage: Storage,
        update_sender: Sender<StatusUpdate>,
        shutdown_receiver: Receiver<()>,
    ) {
        loop {
            // Get snapshot of sessions
            let sessions_snapshot = {
                let sessions = sessions.lock().unwrap();
                sessions.clone()
            };

            let now = Utc::now();
            let mut sys = System::new_with_specifics(
                RefreshKind::new().with_processes(ProcessRefreshKind::everything()),
            );
            sys.refresh_processes(sysinfo::ProcessesToUpdate::All, true);

            let mut crashed_sessions = Vec::new();

            for session in sessions_snapshot {
                if let Some(pid) = session.pid {
                    // Check if process still exists
                    if sys.process(sysinfo::Pid::from_u32(pid)).is_none() {
                        // Process died - write separator and read error from logs
                        let crashed_at = now;
                        let separator = format!(
                            "\n{}\nSession Crashed/Exited: {} | PID: {}\n{}\n\n",
                            "=".repeat(80),
                            crashed_at.format("%Y-%m-%d %H:%M:%S"),
                            pid,
                            "=".repeat(80)
                        );
                        let _ = storage.append_log(&session.id, &separator);

                        let error_msg = Self::get_last_log_lines(&storage, &session.id, 3)
                            .unwrap_or_else(|_| "Process terminated".to_string());

                        let _ = update_sender.send(StatusUpdate {
                            session_id: session.id,
                            status: SessionStatus::Error(error_msg),
                            pid: None,
                        });

                        crashed_sessions.push(session.id);
                    } else {
                        // Check if recently started (within 15 seconds) - verify it's stable
                        if let Some(started_at) = session.started_at {
                            let elapsed = now - started_at;
                            if elapsed.num_seconds() < 15 {
                                // Still in verification window - ensure it stays running
                                if sys.process(sysinfo::Pid::from_u32(pid)).is_none() {
                                    // Process died early - write separator
                                    let crashed_at = now;
                                    let separator = format!(
                                        "\n{}\nSession Failed Early: {} | PID: {}\n{}\n\n",
                                        "=".repeat(80),
                                        crashed_at.format("%Y-%m-%d %H:%M:%S"),
                                        pid,
                                        "=".repeat(80)
                                    );
                                    let _ = storage.append_log(&session.id, &separator);

                                    let error_msg =
                                        Self::get_last_log_lines(&storage, &session.id, 3)
                                            .unwrap_or_else(|_| {
                                                "Process exited shortly after start".to_string()
                                            });

                                    let _ = update_sender.send(StatusUpdate {
                                        session_id: session.id,
                                        status: SessionStatus::Error(error_msg),
                                        pid: None,
                                    });

                                    crashed_sessions.push(session.id);
                                }
                            }
                        }
                    }
                }
            }

            // Remove crashed sessions and sessions without PIDs from monitoring
            {
                let mut monitored = sessions.lock().unwrap();
                if !crashed_sessions.is_empty() {
                    monitored.retain(|s| !crashed_sessions.contains(&s.id));
                }
                // Also remove any sessions without PIDs (shouldn't happen but good cleanup)
                monitored.retain(|s| s.pid.is_some());
            }

            // Wait for 2 seconds or until shutdown signal (whichever comes first)
            match shutdown_receiver.recv_timeout(Duration::from_secs(2)) {
                Ok(_) | Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => continue,
            }
        }
    }

    fn get_last_log_lines(storage: &Storage, session_id: &Uuid, lines: usize) -> Result<String> {
        let log_content = storage.read_logs(session_id)?;
        if log_content.is_empty() {
            return Ok("Process exited without output".to_string());
        }

        let last_lines: Vec<&str> = log_content
            .lines()
            .rev()
            .take(lines)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();

        let result = last_lines.join(" ");
        if result.len() > 100 {
            Ok(format!("...{}", &result[result.len() - 100..]))
        } else {
            Ok(result)
        }
    }

    pub fn shutdown(mut self) {
        let _ = self.shutdown_sender.send(());
        if let Some(handle) = self.thread_handle.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for StatusMonitor {
    fn drop(&mut self) {
        let _ = self.shutdown_sender.send(());
        if let Some(handle) = self.thread_handle.take() {
            let _ = handle.join();
        }
    }
}

pub struct ProcessManager {
    storage: Storage,
    monitored_sessions: Arc<Mutex<Vec<MonitoredSession>>>,
    update_receiver: Receiver<StatusUpdate>,
    _status_monitor: StatusMonitor,
}

impl ProcessManager {
    pub fn new(storage: Storage) -> Self {
        let monitored_sessions = Arc::new(Mutex::new(Vec::new()));
        let (update_sender, update_receiver) = mpsc::channel();

        let status_monitor = StatusMonitor::new(
            Arc::clone(&monitored_sessions),
            storage.clone(),
            update_sender,
        );

        Self {
            storage,
            monitored_sessions,
            update_receiver,
            _status_monitor: status_monitor,
        }
    }

    pub fn sync_monitored_sessions(&self, sessions: &[Session]) {
        let mut monitored = self.monitored_sessions.lock().unwrap();
        monitored.clear();
        for session in sessions {
            monitored.push(MonitoredSession {
                id: session.id,
                pid: session.pid,
                started_at: session.last_started,
            });
        }
    }

    pub fn poll_status_updates(&self, sessions: &mut [Session]) -> bool {
        let mut updated = false;
        while let Ok(update) = self.update_receiver.try_recv() {
            if let Some(session) = sessions.iter_mut().find(|s| s.id == update.session_id) {
                session.status = update.status;
                session.pid = update.pid;
                updated = true;
            }
        }
        updated
    }

    pub fn start_session(&self, session: &mut Session) -> Result<()> {
        let started_at = Utc::now();

        let mut cmd = match session.session_type {
            SessionType::SSH => self.build_ssh_command(session),
            SessionType::Kubectl => self.build_kubectl_command(session),
            SessionType::Socks5 => self.build_socks5_command(session),
        };

        let log_file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.storage.log_file(&session.id))?;

        cmd.stdout(Stdio::from(log_file.try_clone()?))
            .stderr(Stdio::from(log_file));

        let child = cmd.spawn()?;
        let pid = child.id();
        session.pid = Some(pid);
        session.status = SessionStatus::Running;
        session.last_started = Some(started_at);

        // Write separator with timestamp and PID
        let separator = format!(
            "\n{}\nSession Started: {} | PID: {}\n{}\n",
            "=".repeat(80),
            started_at.format("%Y-%m-%d %H:%M:%S"),
            pid,
            "=".repeat(80)
        );
        let _ = self.storage.append_log(&session.id, &separator);

        // Update monitored sessions immediately
        let mut monitored = self.monitored_sessions.lock().unwrap();
        if let Some(existing) = monitored.iter_mut().find(|s| s.id == session.id) {
            existing.pid = Some(pid);
            existing.started_at = session.last_started;
        } else {
            monitored.push(MonitoredSession {
                id: session.id,
                pid: Some(pid),
                started_at: session.last_started,
            });
        }

        Ok(())
    }

    pub fn stop_session(&self, session: &mut Session) -> Result<()> {
        if let Some(pid) = session.pid {
            #[cfg(unix)]
            {
                use std::process::Command;
                Command::new("kill").arg(pid.to_string()).output()?;
            }
            #[cfg(windows)]
            {
                use std::process::Command;
                Command::new("taskkill")
                    .args(["/PID", &pid.to_string(), "/F"])
                    .output()?;
            }

            // Write separator for manual stop
            let stopped_at = Utc::now();
            let separator = format!(
                "\n{}\nSession Stopped: {} | PID: {}\n{}\n\n",
                "=".repeat(80),
                stopped_at.format("%Y-%m-%d %H:%M:%S"),
                pid,
                "=".repeat(80)
            );
            let _ = self.storage.append_log(&session.id, &separator);
        }
        session.status = SessionStatus::Stopped;
        session.pid = None;

        // Remove from monitored sessions (no need to monitor stopped sessions)
        let mut monitored = self.monitored_sessions.lock().unwrap();
        monitored.retain(|s| s.id != session.id);

        Ok(())
    }

    fn build_ssh_command(&self, session: &Session) -> Command {
        let mut cmd = Command::new("ssh");
        cmd.arg("-L")
            .arg(format!(
                "{}:localhost:{}",
                session.local_port,
                session.remote_port.unwrap_or(0)
            ))
            .arg(&session.target)
            .arg("-N");

        for opt in &session.additional_options {
            cmd.arg(opt);
        }

        cmd
    }

    fn build_kubectl_command(&self, session: &Session) -> Command {
        let mut cmd = Command::new("kubectl");

        // Add context if specified
        if let Some(ctx) = &session.kube_context {
            cmd.arg("--context").arg(ctx);
        }

        // Add namespace if specified
        if let Some(ns) = &session.kube_namespace {
            cmd.arg("--namespace").arg(ns);
        }

        cmd.arg("port-forward").arg(&session.target).arg(format!(
            "{}:{}",
            session.local_port,
            session.remote_port.unwrap_or(0)
        ));

        for opt in &session.additional_options {
            cmd.arg(opt);
        }

        cmd
    }

    fn build_socks5_command(&self, session: &Session) -> Command {
        let mut cmd = Command::new("ssh");
        cmd.arg("-D")
            .arg(session.local_port.to_string())
            .arg(&session.target)
            .arg("-N");

        for opt in &session.additional_options {
            cmd.arg(opt);
        }

        cmd
    }
}
