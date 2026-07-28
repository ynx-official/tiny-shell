[中文](README.md) | [English](README.en.md)

# TinyShell

<p align="center">
  <img src="assets/icons/tiny-shell.png" alt="TinyShell" width="128" />
</p>

<p align="center">
  <strong>A modern, high-performance desktop terminal client built with Rust</strong>
</p>

<p align="center">
  <a href="https://github.com/ynx-official/tiny-shell/releases/latest"><img src="https://img.shields.io/github/v/release/ynx-official/tiny-shell?style=flat-square" alt="Release"></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-GPL--3.0--or--later-blue.svg?style=flat-square" alt="License"></a>
  <a href="https://github.com/ynx-official/tiny-shell"><img src="https://img.shields.io/badge/platform-macOS%20%7C%20Windows%20%7C%20Linux-lightgrey?style=flat-square" alt="Platform"></a>
</p>

---

TinyShell is a desktop terminal client built with the GPUI framework and GPUI Component library, written in Rust. It integrates local terminals, SSH remote connections, SFTP file management, and system monitoring into a single high-performance, visually polished workspace.

## Features

### 🖥️ Terminal Experience
- **Alacritty Terminal Engine** — High-performance terminal emulation powered by `alacritty_terminal`, with full ANSI escape sequence support, true color, cursor styling, and mouse events
- **Multi-Tab & Split Panes** — Multiple tabs with splittable panes in each tab, delivering a tmux-like workflow
- **Built-in Monospace Font** — Ships with Maple Mono NF CN for excellent CJK character and Nerd Font icon support out of the box
- **Global Font Controls** — Adjust terminal font family, size, and line spacing in real time

### 🔗 Remote Connectivity
- **SSH Client** — Password, key file, and inline key authentication, with passphrase-protected private key support
- **Session Management** — Save, edit, delete, and quick-switch SSH sessions with group organization
- **SFTP File Manager** — Built-in graphical SFTP client with upload, download, drag-and-drop, multi-select, remote file editing, and permission management
- **Proxy Support** — SOCKS5 and HTTP proxy support, configurable globally or per session

### 📊 System Monitoring
- **Real-Time Telemetry** — Live CPU, memory, swap, network, and disk usage with historical charts in the sidebar
- **Remote Monitoring** — Automatically collects remote server metrics after SSH connection, no agent installation required
- **Process Viewer** — Built-in process viewer for inspecting and managing processes on remote servers

### 🎨 Theming & Appearance
- **Multiple Themes** — Matrix, Gruvbox, Tokyo Night, Solarized, Phygerr, and more, with light/dark mode support
- **Custom Themes** — Import custom JSON theme files
- **Compact Layout** — Collapsible sidebar for full-screen immersive terminal experience

### ⚙️ Productivity Tools
- **Keybinding System** — Visual shortcut manager with view, edit, and conflict detection
- **Command Palette** — Global shortcut to quickly switch sessions, open files, and execute actions
- **Config Sync** — Sync session configurations across devices via WebDAV or S3
- **Managed Key Import** — Imported SSH keys are managed by the app; deleting the original file does not affect existing connections

### 🔄 Auto-Update
- **Cross-Platform Updates** — Built-in auto-update that checks, downloads, and installs the latest version from GitHub Releases
- **Platform-Optimized** — Atomic replacement on Linux, App Bundle replacement on macOS, batch script on Windows

## Installation

### macOS

#### Homebrew (Recommended)
```bash
brew install ynx-official/taps/tiny-shell --cask
```

To update:
```bash
brew update && brew upgrade tiny-shell --cask
```

#### Manual Install
Download `tiny-shell-*-macos-*.zip` from the [Releases page](https://github.com/ynx-official/tiny-shell/releases/latest), unzip, and drag `TinyShell.app` to your Applications folder.

> If macOS warns the app is "damaged" on first launch:
> ```bash
> sudo xattr -cr /Applications/TinyShell.app
> ```

### Windows

Choose either package from the [Releases page](https://github.com/ynx-official/tiny-shell/releases/latest):

- **Installer**: Download `tiny-shell-*-windows-x86_64-setup.exe` and follow the setup wizard. It provides a Start Menu shortcut, an optional desktop shortcut, and a standard uninstall entry.
- **Portable**: Download `tiny-shell-*-windows-x86_64-portable.zip`, extract it, and run `tiny-shell.exe` without installation.

### Linux

#### Debian/Ubuntu (.deb)
Download the `.deb` package from the [Releases page](https://github.com/ynx-official/tiny-shell/releases/latest):
```bash
sudo dpkg -i tiny-shell_*.deb
```

#### Generic Linux (tar.gz)
```bash
tar -xzf tiny-shell-*-linux-x86_64.tar.gz
cd tiny-shell-*-linux-x86_64
./tiny-shell
```

## Building from Source

### Prerequisites
- Rust toolchain 1.85+
- Linux: `pkg-config` `libfontconfig1-dev` `libxcb1-dev` and other X11/Wayland development libraries
- macOS: Xcode Command Line Tools
- Windows: MSVC Build Tools

### Build & Run
```bash
# Clone the repository
git clone https://github.com/ynx-official/tiny-shell.git
cd tiny-shell

# Run
cargo run --release

# Package macOS App Bundle
./scripts/package-macos-app.sh

# Package Linux .deb
cargo install cargo-deb
cargo deb
```

## Version History

### v1.1.7
- Moved quick connect, group operations, connection moves, and archive import/export into independent system windows while keeping the main window interactive
- Added multiple isolated SSH create/edit windows with independent drafts, atomic saves, and optimistic concurrency conflict detection
- Reduced the SSH editor size with a compact scrollable layout and added `Esc` window closing
- Automatically close the quick connect window after connecting by double-click or menu action

### v1.1.6
- Upgraded quick connect into an independent tree-based connection manager
- Added connection and group search, sorting, expand/collapse, context actions, and moving
- Added soft deletion, recycle-bin restoration, and synchronized tombstones for connections and groups
- Added password-encrypted TinyShell JSON import/export with no plaintext sensitive fields

### v1.1.5
- Added privacy password verification for sensitive data sync to detect missing or mismatched passwords early
- Enhanced sync settings and user prompts to make configuration and synchronization workflows clearer
- Added the latest sync status to the SFTP footer and improved related visibility settings

### v1.1.4
- Added preflight checks and user guidance for sync uploads to prevent failures caused by invalid configuration
- Improved WebDAV connection verification, configuration handling, and error messages
- Expanded Chinese and English localization for sync workflows
- Improved cross-platform release workflows and automated release note generation

### v1.1.3
- Fixed Windows release workflow parsing of the version returned by `cargo pkgid`
- Fixed the update loop caused by a mismatch between the release tag and the application version
- Enforced release-time validation that the Git tag matches the `Cargo.toml` version
- Added SHA-256 integrity verification for online update downloads

### v1.0.1
- Rebranded from ashell to TinyShell, repository moved to [ynx-official/tiny-shell](https://github.com/ynx-official/tiny-shell)
- Added cross-platform auto-update for Linux, macOS, and Windows
- Added auto-update UI prompts in both English and Chinese
- Updated all brand assets: icons, desktop entries, config directories, environment variables

### v0.4.x
- Visual keybinding management with conflict detection
- Multi-pane split tabs (tmux-like experience)
- Enhanced SFTP transfer history
- SSH private key passphrase support
- Complete Block Elements rendering in terminal

### v0.3.x
- Global font family and size controls
- Concurrent SFTP transfers
- Persistent layout state
- Hot-swappable English/Chinese i18n
- Terminal right-click copy/paste

## Tech Stack

| Component | Technology |
|-----------|------------|
| GUI Framework | [GPUI](https://github.com/zed-industries/zed) |
| Component Library | [gpui-component](https://github.com/longbridge/gpui-component) |
| Terminal Engine | [alacritty_terminal](https://github.com/alacritty/alacritty) |
| SSH Protocol | [russh](https://github.com/warp-tech/russh) |
| i18n | [rust-i18n](https://github.com/longbridge/rust-i18n) |
| System Info | [sysinfo](https://github.com/GuillaumeGomez/sysinfo) |

## License

This project is licensed under the [GPL-3.0-or-later](LICENSE) license.
