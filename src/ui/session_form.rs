use crate::kube_config::{filter_targets, get_current_context, get_namespaces, get_targets, parse_kube_config, KubeContext, KubeTarget};
use crate::models::{Session, SessionType};
use crate::ssh_config::{filter_hosts, parse_ssh_config, SshHost};
use crate::ui::FormMode;
use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph},
    Frame,
};
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, PartialEq)]
pub enum FormStep {
    SelectType,
    FillFields,
}

pub struct FormState {
    pub step: FormStep,
    pub type_selection: usize,
    pub name: String,
    pub session_type: SessionType,
    pub context_field: String,
    pub namespace_field: String,
    pub target: String,
    pub local_port: String,
    pub remote_port: String,
    pub focused_field: usize,
    pub cursor_pos: usize,
    pub ssh_hosts: Vec<SshHost>,
    pub filtered_hosts: Vec<SshHost>,
    pub selected_suggestion: usize,
    pub show_suggestions: bool,
    pub kube_contexts: Vec<KubeContext>,
    pub filtered_kube_contexts: Vec<KubeContext>,
    pub namespaces: Vec<String>,
    pub filtered_namespaces: Vec<String>,
    pub kube_targets: Vec<KubeTarget>,
    pub filtered_kube_targets: Vec<KubeTarget>,
    pub available_ports: Vec<u16>,
    pub scroll_offset: usize,
    pub loading_targets: bool,
    target_receiver: Option<Receiver<Vec<KubeTarget>>>,
}

impl FormState {
    pub fn new() -> Self {
        let ssh_hosts = parse_ssh_config();
        let filtered_hosts = ssh_hosts.clone();

        let (kube_context, kube_contexts) = parse_kube_config().unwrap_or_else(|| {
            let ctx = get_current_context().unwrap_or_default();
            (ctx, Vec::new())
        });

        let filtered_kube_contexts = kube_contexts.clone();
        let namespaces = get_namespaces(Some(&kube_context));
        let filtered_namespaces = namespaces.clone();

        Self {
            step: FormStep::SelectType,
            type_selection: 0,
            name: String::new(),
            session_type: SessionType::SSH,
            context_field: kube_context,
            namespace_field: String::new(),
            target: String::new(),
            local_port: String::new(),
            remote_port: String::new(),
            focused_field: 0,
            cursor_pos: 0,
            ssh_hosts,
            filtered_hosts,
            selected_suggestion: 0,
            show_suggestions: false,
            kube_contexts,
            filtered_kube_contexts,
            namespaces,
            filtered_namespaces,
            kube_targets: Vec::new(),
            filtered_kube_targets: Vec::new(),
            available_ports: Vec::new(),
            scroll_offset: 0,
            loading_targets: false,
            target_receiver: None,
        }
    }

    pub fn from_session(session: &Session) -> Self {
        let ssh_hosts = parse_ssh_config();
        let filtered_hosts = ssh_hosts.clone();
        let name_len = session.name.len();

        let (default_context, kube_contexts) = parse_kube_config().unwrap_or_else(|| {
            let ctx = get_current_context().unwrap_or_default();
            (ctx, Vec::new())
        });

        let filtered_kube_contexts = kube_contexts.clone();
        let context_field = session.kube_context.clone().unwrap_or(default_context);
        let namespace_field = session.kube_namespace.clone().unwrap_or_default();

        // Don't load namespaces immediately - will be lazy loaded when namespace field is focused
        let namespaces = Vec::new();
        let filtered_namespaces = Vec::new();

        // For kubectl type, focus on name (field 1), for others focus on name (field 0)
        let focused_field = if session.session_type == SessionType::Kubectl { 1 } else { 0 };

        Self {
            step: FormStep::FillFields,
            type_selection: 0,
            name: session.name.clone(),
            session_type: session.session_type.clone(),
            context_field,
            namespace_field,
            target: session.target.clone(),
            local_port: session.local_port.to_string(),
            remote_port: session.remote_port.map(|p| p.to_string()).unwrap_or_default(),
            focused_field,
            cursor_pos: name_len,
            ssh_hosts,
            filtered_hosts,
            selected_suggestion: 0,
            show_suggestions: false,
            kube_contexts,
            filtered_kube_contexts,
            namespaces,
            filtered_namespaces,
            kube_targets: Vec::new(),
            filtered_kube_targets: Vec::new(),
            available_ports: Vec::new(),
            scroll_offset: 0,
            loading_targets: false,
            target_receiver: None,
        }
    }

    pub fn to_session(&self) -> Option<Session> {
        let local_port = self.local_port.parse::<u16>().ok()?;
        let remote_port = if self.session_type == SessionType::Socks5 {
            None
        } else {
            Some(self.remote_port.parse::<u16>().ok()?)
        };

        let mut session = Session::new(
            self.name.clone(),
            self.session_type.clone(),
            self.target.clone(),
            local_port,
            remote_port,
        );

        if self.session_type == SessionType::Kubectl {
            session.kube_context = if self.context_field.is_empty() { None } else { Some(self.context_field.clone()) };
            session.kube_namespace = if self.namespace_field.is_empty() { None } else { Some(self.namespace_field.clone()) };
        }

        Some(session)
    }

    pub fn toggle_type(&mut self) {
        self.session_type = match self.session_type {
            SessionType::SSH => SessionType::Kubectl,
            SessionType::Kubectl => SessionType::Socks5,
            SessionType::Socks5 => SessionType::SSH,
        };
    }

    pub fn confirm_type_selection(&mut self) {
        self.session_type = match self.type_selection {
            0 => SessionType::SSH,
            1 => SessionType::Kubectl,
            2 => SessionType::Socks5,
            _ => SessionType::SSH,
        };
        self.step = FormStep::FillFields;
    }

    pub fn move_type_selection(&mut self, delta: i32) {
        let new_idx = (self.type_selection as i32 + delta).rem_euclid(3);
        self.type_selection = new_idx as usize;
    }

    pub fn field_count(&self) -> usize {
        match self.session_type {
            SessionType::Socks5 => 3, // Name, Target, Local Port
            SessionType::Kubectl => 6, // Name, Context, Namespace, Target, Local Port, Remote Port
            SessionType::SSH => 4, // Name, Target, Local Port, Remote Port
        }
    }

    pub fn update_target(&mut self, target: String) {
        self.target = target;
        let target_field_idx = if self.session_type == SessionType::Kubectl { 3 } else { 1 };

        if self.focused_field == target_field_idx {
            if self.session_type == SessionType::SSH || self.session_type == SessionType::Socks5 {
                self.filtered_hosts = filter_hosts(&self.ssh_hosts, &self.target);
                self.show_suggestions = !self.filtered_hosts.is_empty();
                self.selected_suggestion = 0;
                self.scroll_offset = 0;
            } else if self.session_type == SessionType::Kubectl {
                // Load targets asynchronously if not already loaded
                if self.kube_targets.is_empty() && !self.target.is_empty() && !self.loading_targets {
                    self.start_loading_targets();
                } else {
                    // Filter existing targets
                    self.filtered_kube_targets = filter_targets(&self.kube_targets, &self.target);
                    self.show_suggestions = !self.filtered_kube_targets.is_empty();
                    self.selected_suggestion = 0;
                    self.scroll_offset = 0;
                }
            }
        }
    }

    pub fn update_context(&mut self, context: String) {
        self.context_field = context;

        if self.session_type == SessionType::Kubectl && self.focused_field == 0 {
            self.filter_contexts();
            self.show_suggestions = !self.filtered_kube_contexts.is_empty();
            self.selected_suggestion = 0;
            self.scroll_offset = 0;
        }
    }

    fn filter_contexts(&mut self) {
        if self.context_field.is_empty() {
            self.filtered_kube_contexts = self.kube_contexts.clone();
        } else {
            let query_lower = self.context_field.to_lowercase();
            self.filtered_kube_contexts = self.kube_contexts
                .iter()
                .filter(|ctx| {
                    ctx.name.to_lowercase().contains(&query_lower)
                        || ctx.cluster.to_lowercase().contains(&query_lower)
                        || ctx.namespace.as_ref().map_or(false, |ns| ns.to_lowercase().contains(&query_lower))
                })
                .cloned()
                .collect();
        }
    }

    pub fn update_namespace(&mut self, namespace: String) {
        self.namespace_field = namespace;

        if self.session_type == SessionType::Kubectl && self.focused_field == 2 {
            // Lazy load namespaces if not already loaded
            if self.namespaces.is_empty() {
                self.reload_namespaces();
            }
            self.filter_namespaces();
            self.show_suggestions = !self.filtered_namespaces.is_empty();
            self.selected_suggestion = 0;
            self.scroll_offset = 0;
        }
    }

    fn filter_namespaces(&mut self) {
        if self.namespace_field.is_empty() {
            self.filtered_namespaces = self.namespaces.clone();
        } else {
            let query_lower = self.namespace_field.to_lowercase();
            self.filtered_namespaces = self.namespaces
                .iter()
                .filter(|ns| ns.to_lowercase().contains(&query_lower))
                .cloned()
                .collect();
        }
    }

    pub fn reload_namespaces(&mut self) {
        if self.session_type == SessionType::Kubectl {
            let context = if self.context_field.is_empty() { None } else { Some(self.context_field.as_str()) };
            self.namespaces = get_namespaces(context);
            self.filtered_namespaces = self.namespaces.clone();
        }
    }

    pub fn select_suggestion(&mut self) {
        if !self.show_suggestions {
            return;
        }

        // Handle context field suggestions for kubectl
        if self.session_type == SessionType::Kubectl && self.focused_field == 0 {
            if let Some(context) = self.filtered_kube_contexts.get(self.selected_suggestion) {
                self.context_field = context.name.clone();
                self.cursor_pos = self.context_field.len();
                self.show_suggestions = false;
                // Reload namespaces for the new context
                self.reload_namespaces();
            }
            return;
        }

        // Handle namespace field suggestions for kubectl
        if self.session_type == SessionType::Kubectl && self.focused_field == 2 {
            if let Some(namespace) = self.filtered_namespaces.get(self.selected_suggestion) {
                self.namespace_field = namespace.clone();
                self.cursor_pos = self.namespace_field.len();
                self.show_suggestions = false;
                // Reload targets when namespace changes
                self.kube_targets.clear();
            }
            return;
        }

        // Handle port field suggestions for kubectl
        if self.session_type == SessionType::Kubectl && (self.focused_field == 4 || self.focused_field == 5) {
            if let Some(&port) = self.available_ports.get(self.selected_suggestion) {
                let port_str = port.to_string();
                if self.focused_field == 4 {
                    self.local_port = port_str;
                    self.cursor_pos = self.local_port.len();
                } else {
                    self.remote_port = port_str;
                    self.cursor_pos = self.remote_port.len();
                }
                self.show_suggestions = false;
            }
            return;
        }

        // Handle target field suggestions
        if self.session_type == SessionType::SSH || self.session_type == SessionType::Socks5 {
            if let Some(host) = self.filtered_hosts.get(self.selected_suggestion) {
                self.target = host.connection_string();
                self.show_suggestions = false;
            }
        } else if self.session_type == SessionType::Kubectl {
            if let Some(kube_target) = self.filtered_kube_targets.get(self.selected_suggestion) {
                self.target = kube_target.target_string();
                // Auto-fill namespace
                if self.namespace_field.is_empty() {
                    self.namespace_field = kube_target.namespace.clone();
                }
                // Store available ports for later suggestion
                self.available_ports = kube_target.ports.clone();
                // Auto-fill port if we have exactly one port
                if kube_target.ports.len() == 1 {
                    let port_str = kube_target.ports[0].to_string();
                    if self.local_port.is_empty() {
                        self.local_port = port_str.clone();
                    }
                    if self.remote_port.is_empty() {
                        self.remote_port = port_str;
                    }
                }
                self.show_suggestions = false;
            }
        }
    }

    pub fn move_suggestion(&mut self, delta: i32) {
        if !self.show_suggestions {
            return;
        }

        let count = if self.session_type == SessionType::Kubectl && self.focused_field == 0 {
            // Context field suggestions
            if self.filtered_kube_contexts.is_empty() {
                return;
            }
            self.filtered_kube_contexts.len()
        } else if self.session_type == SessionType::Kubectl && self.focused_field == 2 {
            // Namespace field suggestions
            if self.filtered_namespaces.is_empty() {
                return;
            }
            self.filtered_namespaces.len()
        } else if self.session_type == SessionType::Kubectl && (self.focused_field == 4 || self.focused_field == 5) {
            // Port field suggestions
            if self.available_ports.is_empty() {
                return;
            }
            self.available_ports.len()
        } else if self.session_type == SessionType::Kubectl {
            // Target field suggestions
            if self.filtered_kube_targets.is_empty() {
                return;
            }
            self.filtered_kube_targets.len()
        } else {
            // SSH host suggestions
            if self.filtered_hosts.is_empty() {
                return;
            }
            self.filtered_hosts.len()
        };

        let new_idx = (self.selected_suggestion as i32 + delta).rem_euclid(count as i32) as usize;
        self.selected_suggestion = new_idx;

        // Adjust scroll offset to keep selection visible (assume 8 visible items)
        const VISIBLE_ITEMS: usize = 8;
        if self.selected_suggestion < self.scroll_offset {
            self.scroll_offset = self.selected_suggestion;
        } else if self.selected_suggestion >= self.scroll_offset + VISIBLE_ITEMS {
            self.scroll_offset = self.selected_suggestion.saturating_sub(VISIBLE_ITEMS - 1);
        }
    }

    pub fn hide_suggestions(&mut self) {
        self.show_suggestions = false;
    }

    pub fn show_port_suggestions(&mut self) {
        if self.session_type == SessionType::Kubectl
            && !self.available_ports.is_empty()
            && (self.focused_field == 4 || self.focused_field == 5)
        {
            self.show_suggestions = true;
            self.selected_suggestion = 0;
            self.scroll_offset = 0;
        }
    }

    pub fn on_focus_change(&mut self) {
        // Lazy load namespaces when namespace field gets focus
        if self.session_type == SessionType::Kubectl && self.focused_field == 2 && self.namespaces.is_empty() {
            self.reload_namespaces();
        }
    }

    pub fn start_loading_targets(&mut self) {
        if self.session_type != SessionType::Kubectl {
            return;
        }

        let context = if self.context_field.is_empty() { None } else { Some(self.context_field.clone()) };
        let namespace = if self.namespace_field.is_empty() { None } else { Some(self.namespace_field.clone()) };
        let (tx, rx) = mpsc::channel();

        self.target_receiver = Some(rx);
        self.loading_targets = true;

        thread::spawn(move || {
            let targets = get_targets(context.as_deref(), namespace.as_deref());
            let _ = tx.send(targets);
        });
    }

    pub fn poll_target_updates(&mut self) -> bool {
        if let Some(ref rx) = self.target_receiver {
            if let Ok(targets) = rx.try_recv() {
                self.kube_targets = targets;
                self.filtered_kube_targets = filter_targets(&self.kube_targets, &self.target);
                self.show_suggestions = !self.filtered_kube_targets.is_empty() && self.focused_field == 3;
                self.selected_suggestion = 0;
                self.scroll_offset = 0;
                self.loading_targets = false;
                self.target_receiver = None;
                return true;
            }
        }
        false
    }
}

pub fn render(frame: &mut Frame, form_state: &FormState, mode: &FormMode, area: Rect) {
    if form_state.step == FormStep::SelectType {
        render_type_selection(frame, form_state, area);
    } else {
        let chunks = if form_state.show_suggestions {
            Layout::vertical([
                Constraint::Length(3),
                Constraint::Min(0),
                Constraint::Length(10),
                Constraint::Length(3),
            ])
            .split(area)
        } else {
            Layout::vertical([
                Constraint::Length(3),
                Constraint::Min(0),
                Constraint::Length(3),
            ])
            .split(area)
        };

        let title = match mode {
            FormMode::Create => "Create Session",
            FormMode::Edit(_) => "Edit Session",
        };

        render_title(frame, title, chunks[0]);
        render_form(frame, form_state, chunks[1]);

        if form_state.show_suggestions {
            render_suggestions(frame, form_state, chunks[2]);
            render_help(frame, chunks[3]);
        } else {
            render_help(frame, chunks[2]);
        }
    }
}

fn render_title(frame: &mut Frame, title: &str, area: Rect) {
    let title_widget = Paragraph::new(title)
        .style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))
        .block(Block::default().borders(Borders::ALL));
    frame.render_widget(title_widget, area);
}

fn render_type_selection(frame: &mut Frame, form_state: &FormState, area: Rect) {
    let chunks = Layout::vertical([
        Constraint::Length(3),
        Constraint::Min(0),
        Constraint::Length(3),
    ])
    .split(area);

    render_title(frame, "Select Session Type", chunks[0]);

    let types = vec![
        ("SSH", "Standard SSH port forwarding"),
        ("kubectl", "Kubernetes port forwarding"),
        ("SOCKS5", "SOCKS5 proxy via SSH"),
    ];

    let mut lines = vec![Line::from("")];
    for (idx, (name, desc)) in types.iter().enumerate() {
        let style = if idx == form_state.type_selection {
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default()
        };

        let prefix = if idx == form_state.type_selection {
            "> "
        } else {
            "  "
        };

        lines.push(Line::from(vec![
            Span::raw(prefix),
            Span::styled(format!("{:10}", name), style.add_modifier(Modifier::BOLD)),
            Span::raw("  "),
            Span::styled(*desc, Style::default().fg(Color::Gray)),
        ]));
        lines.push(Line::from(""));
    }

    let type_list = Paragraph::new(lines).block(
        Block::default()
            .borders(Borders::ALL)
            .title("Choose a type"),
    );
    frame.render_widget(type_list, chunks[1]);

    let help_text = Line::from(vec![
        Span::styled("↑↓", Style::default().fg(Color::Yellow)),
        Span::raw(" navigate | "),
        Span::styled("Enter", Style::default().fg(Color::Yellow)),
        Span::raw(" confirm | "),
        Span::styled("Esc", Style::default().fg(Color::Yellow)),
        Span::raw(" cancel"),
    ]);
    let help = Paragraph::new(help_text).block(Block::default().borders(Borders::ALL));
    frame.render_widget(help, chunks[2]);
}

fn render_form(frame: &mut Frame, form_state: &FormState, area: Rect) {
    let type_str = form_state.session_type.as_str().to_string();
    let mut fields = vec![];

    // Add kubectl-specific fields with context first
    if form_state.session_type == SessionType::Kubectl {
        fields.push(("Context", &form_state.context_field, 0));
        fields.push(("Name", &form_state.name, 1));
        fields.push(("Namespace", &form_state.namespace_field, 2));
        fields.push(("Target", &form_state.target, 3));
        fields.push(("Local Port", &form_state.local_port, 4));
        fields.push(("Remote Port", &form_state.remote_port, 5));
    } else {
        fields.push(("Name", &form_state.name, 0));
        fields.push(("Target", &form_state.target, 1));
        fields.push(("Local Port", &form_state.local_port, 2));
        if form_state.session_type != SessionType::Socks5 {
            fields.push(("Remote Port", &form_state.remote_port, 3));
        }
    }

    // Calculate blinking cursor visibility (500ms on, 500ms off)
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis();
    let show_cursor = (now / 500) % 2 == 0;

    let mut lines = vec![
        Line::from(vec![
            Span::styled("Type: ", Style::default().add_modifier(Modifier::BOLD)),
            Span::styled(
                type_str,
                Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(""),
    ];

    // Show loading status for kubectl
    if form_state.session_type == SessionType::Kubectl && form_state.loading_targets {
        lines.push(Line::from(vec![
            Span::styled("Status: ", Style::default().add_modifier(Modifier::BOLD)),
            Span::styled(
                "Loading targets...",
                Style::default().fg(Color::Yellow),
            ),
        ]));
        lines.push(Line::from(""));
    }

    for (label, value, idx) in fields {
        let style = if idx == form_state.focused_field {
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default()
        };

        let prefix = if idx == form_state.focused_field {
            "> "
        } else {
            "  "
        };

        let display_value = if idx == form_state.focused_field && show_cursor {
            let cursor_pos = form_state.cursor_pos.min(value.len());
            format!("{}█{}", &value[..cursor_pos], &value[cursor_pos..])
        } else {
            value.to_string()
        };

        lines.push(Line::from(vec![
            Span::raw(prefix),
            Span::styled(
                format!("{:12}: ", label),
                Style::default().add_modifier(Modifier::BOLD),
            ),
            Span::styled(display_value, style),
        ]));
        lines.push(Line::from(""));
    }

    let form = Paragraph::new(lines).block(Block::default().borders(Borders::ALL).title("Form"));
    frame.render_widget(form, area);
}

fn render_suggestions(frame: &mut Frame, form_state: &FormState, area: Rect) {
    // Calculate visible items based on area height (subtract 2 for borders)
    let scroll_offset = form_state.scroll_offset;

    let (items, title): (Vec<ListItem>, &str) =
        // Context suggestions for kubectl
        if form_state.session_type == SessionType::Kubectl && form_state.focused_field == 0 {
            let items = form_state
                .filtered_kube_contexts
                .iter()
                .enumerate()
                .skip(scroll_offset)
                .map(|(idx, context)| {
                    let style = if idx == form_state.selected_suggestion {
                        Style::default()
                            .bg(Color::DarkGray)
                            .fg(Color::Yellow)
                            .add_modifier(Modifier::BOLD)
                    } else {
                        Style::default()
                    };
                    ListItem::new(context.display_name()).style(style)
                })
                .collect();
            (items, "Kubernetes Contexts (↑↓ navigate, Enter select, Esc close)")
        }
        // Namespace suggestions for kubectl
        else if form_state.session_type == SessionType::Kubectl && form_state.focused_field == 2 {
            let items = form_state
                .filtered_namespaces
                .iter()
                .enumerate()
                .skip(scroll_offset)
                .map(|(idx, namespace)| {
                    let style = if idx == form_state.selected_suggestion {
                        Style::default()
                            .bg(Color::DarkGray)
                            .fg(Color::Yellow)
                            .add_modifier(Modifier::BOLD)
                    } else {
                        Style::default()
                    };
                    ListItem::new(namespace.clone()).style(style)
                })
                .collect();
            (items, "Kubernetes Namespaces (↑↓ navigate, Enter select, Esc close)")
        }
        // Port suggestions for kubectl
        else if form_state.session_type == SessionType::Kubectl && (form_state.focused_field == 4 || form_state.focused_field == 5) {
            let items = form_state
                .available_ports
                .iter()
                .enumerate()
                .skip(scroll_offset)
                .map(|(idx, &port)| {
                    let style = if idx == form_state.selected_suggestion {
                        Style::default()
                            .bg(Color::DarkGray)
                            .fg(Color::Yellow)
                            .add_modifier(Modifier::BOLD)
                    } else {
                        Style::default()
                    };
                    ListItem::new(port.to_string()).style(style)
                })
                .collect();
            (items, "Available Ports (↑↓ navigate, Enter select, Esc close)")
        }
        // Target suggestions for kubectl
        else if form_state.session_type == SessionType::Kubectl {
            let items = form_state
                .filtered_kube_targets
                .iter()
                .enumerate()
                .skip(scroll_offset)
                .map(|(idx, target)| {
                    let style = if idx == form_state.selected_suggestion {
                        Style::default()
                            .bg(Color::DarkGray)
                            .fg(Color::Yellow)
                            .add_modifier(Modifier::BOLD)
                    } else {
                        Style::default()
                    };
                    ListItem::new(target.display_name()).style(style)
                })
                .collect();
            (items, "Kubernetes Targets (↑↓ navigate, Enter select, Esc close)")
        }
        // SSH host suggestions
        else {
            let items = form_state
                .filtered_hosts
                .iter()
                .enumerate()
                .skip(scroll_offset)
                .map(|(idx, host)| {
                    let style = if idx == form_state.selected_suggestion {
                        Style::default()
                            .bg(Color::DarkGray)
                            .fg(Color::Yellow)
                            .add_modifier(Modifier::BOLD)
                    } else {
                        Style::default()
                    };
                    ListItem::new(host.display_name()).style(style)
                })
                .collect();
            (items, "SSH Hosts (↑↓ navigate, Enter select, Esc close)")
        };

    let list = List::new(items).block(
        Block::default()
            .borders(Borders::ALL)
            .title(title),
    );

    frame.render_widget(list, area);
}

fn render_help(frame: &mut Frame, area: Rect) {
    let help_text = Line::from(vec![
        Span::styled("Tab", Style::default().fg(Color::Yellow)),
        Span::raw(" next field | "),
        Span::styled("Ctrl+S", Style::default().fg(Color::Yellow)),
        Span::raw(" save | "),
        Span::styled("Esc", Style::default().fg(Color::Yellow)),
        Span::raw(" cancel"),
    ]);

    let help = Paragraph::new(help_text).block(Block::default().borders(Borders::ALL));
    frame.render_widget(help, area);
}
