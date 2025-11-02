# pfman - Port-Forward Manager

A terminal-based UI application for managing SSH and Kubernetes port-forwarding sessions.

## Features

**Session Management**
- Create, edit, delete port-forwarding sessions
- Support for SSH, kubectl, and SOCKS5 tunnels
- Sessions persist in background even when app is closed
- Real-time status monitoring and process tracking

**Session Types**
- SSH: Standard SSH port forwarding
- kubectl: Kubernetes service/pod port forwarding with context/namespace selection
- SOCKS5: SSH SOCKS5 proxy tunnels

**User Interface**
- Terminal UI built with Ratatui
- Live session status (Running/Stopped/Error)
- Session uptime tracking
- Search and filter sessions
- Autocomplete for SSH hosts and Kubernetes resources
- Live log viewer for each session

**Smart Features**
- Auto-detects SSH hosts from ~/.ssh/config
- Auto-detects Kubernetes contexts and namespaces
- Autocomplete for pods/services when creating kubectl sessions
- Auto-copy port values between local/remote fields
- Session logs stored and viewable

## Installation
### With cargo
```bash
cargo install --path .
```

### With homebrew
```bash
brew install neitomic/tap/pfman
```

## Usage

```bash
pfman
```

**Dashboard Controls**
- `c` - Create new session
- `e` - Edit selected session
- `d` - Delete session
- `s` - Start/stop session
- `l` - View session logs
- `/` - Search sessions
- `q` or `Ctrl+C` - Quit

**Form Controls**
- `Tab/Shift+Tab` - Navigate fields
- `Ctrl+S` - Save session
- `Esc` - Cancel

**Log Viewer**
- `s` - Start/stop session
- `r` - Restart session
- `e` - Edit session
- `Esc` - Back to dashboard

## Configuration

Sessions stored in: `~/.config/pfman/sessions.yaml`
Logs stored in: `~/.local/share/pfman/logs/`

## Requirements

- SSH client (for SSH/SOCKS5 sessions)
- kubectl (for Kubernetes sessions)

## License

This project is licensed under the MIT license ([LICENSE](./LICENSE) or <http://opensource.org/licenses/MIT>)
