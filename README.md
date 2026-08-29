# Chronos Minimal

[![GitHub Release](https://img.shields.io/github/v/tag/ghassanelgendy/chronos-minimal?style=flat-square&color=0066cc&label=release)](https://github.com/ghassanelgendy/chronos-minimal/releases)
[![Rust Version](https://img.shields.io/badge/rust-1.75%2B-e3592c?style=flat-square)](https://www.rust-lang.org)
[![Platforms](https://img.shields.io/badge/platform-Linux%20%7C%20Windows-blue?style=flat-square)](#)
[![License: MIT](https://img.shields.io/badge/License-MIT-green.svg?style=flat-square)](LICENSE)

<p align="center">
  <em>
    Chronos is also the 
    <a href="https://en.wikipedia.org/wiki/Chronos" target="_blank">
      Greek god of time!
    </a>
  </em>
</p>

Chronos Minimal is a lightweight background screen time tracker for **Linux** and **Windows**. Written in Rust for near-zero CPU and RAM usage, it tracks your active applications, processes, and visited websites, and syncs your usage logs securely to a Supabase database.

---

### [**Download Latest Release (Linux & Windows)**](https://github.com/ghassanelgendy/chronos-minimal/releases/latest)

---

## ✨ Features

- **Cross-Platform Background Tracking:**
  - **Linux:** Native AT-SPI2 accessibility, X11/XWayland, GNOME Mutter integration, and Freedesktop AppIndicator system tray support.
  - **Windows:** Native Win32 foreground polling and taskbar notification tray.
- **Smart Website & Web App Detection:**
  - Extracts domains for web applications (Gemini, ChatGPT, Claude, Perplexity, DeepSeek, YouTube, GitHub, Google Meet, Drive, Docs, Notion, Linear, Reddit, and 70+ others) without double-counting parent browser time.
  - Development URL support (including `localhost:3000`, `127.0.0.1:8080`).
  - Distinguishes actual websites from internal browser pages (`New Tab`, `Settings`).
- **Interactive UI & Calendar Navigation:**
  - **Sunday-First 7-Day Navigation Strip:** View daily tracked time at a glance (`Sun`..`Sat`).
  - **Date Navigator:** Quick jump controls (`◀ Prev Day`, `Today`, `Next Day ▶`, `« Prev Week`, `Next Week »`).
  - **Aggregated Views:** Filter by `📅 Day`, `📊 This Week`, `⏮ Last Week`, or `📆 This Month`.
  - **Category Filters:** Switch between `All Items`, `📱 Applications`, and `🌐 Websites` with visual share percentage bars.
- **Desktop & Theme Integration:**
  - **GNOME WhiteSur Theme Support:** Automatically detects system GTK theme, dark/light mode, and window button layouts (left-aligned macOS traffic lights or standard controls).
  - **Single Unified Header:** Native window decorations with close-to-indicator behavior (`-` minimizes to dock, `X` closes window into AppIndicator tray).
- **Idle Inactivity Monitoring:**
  - Automatically pauses tracking when no keyboard or mouse input is detected (configurable threshold in seconds).
- **Supabase Cloud Sync:**
  - Syncs cumulative screen time snapshots to Supabase Edge Functions with local time-gated caching to minimize database writes.
- **Data Privacy:**
  - Personal logs, databases, snapshots, and configuration files are kept strictly local in `~/.config/ChronosScreenTime/` (or `%AppData%\ChronosScreenTime\`) and excluded from version control.

---

## 🚀 Quick Start & Installation

### Linux (Ubuntu / Debian / Fedora / Arch)

You can install Chronos with the included installer script:

```bash
git clone https://github.com/ghassanelgendy/chronos-minimal.git
cd chronos-minimal
chmod +x install.sh uninstall.sh
./install.sh
```

Or manually install the release binary:
```bash
cargo build --release
install -m 755 target/release/chronos-screentime ~/.local/bin/chronos-screentime
```

To run Chronos in the background / AppIndicator tray on startup:
```bash
~/.local/bin/chronos-screentime --minimized
```

### Windows

1. Download `chronos-screentime-windows-x86_64.zip` from the [Releases](https://github.com/ghassanelgendy/chronos-minimal/releases).
2. Extract and run `chronos-screentime.exe`.

---

## ⚙️ Configuration

Settings are stored in `~/.config/ChronosScreenTime/settings.json` (Linux) or `%AppData%\ChronosScreenTime\settings.json` (Windows):

| Field | Type | Description |
|---|---|---|
| `EnableSupabaseSync` | `bool` | Toggle Supabase cloud database sync |
| `SupabaseUrl` | `string` | Supabase API endpoint (e.g. `https://xyz.supabase.co`) |
| `SupabaseAnonKey` | `string` | Supabase anonymous / public API key |
| `SupabaseUserId` | `string` | User identifier for database partitioning |
| `SupabaseUploadIntervalMinutes` | `int` | Upload sync interval in minutes |
| `IdleThresholdSeconds` | `int` | Inactivity timeout before pausing tracking (seconds) |
| `StartWithWindows` | `bool` | Auto-start Chronos on user login |
| `StartMinimizedToTray` | `bool` | Start directly into the system tray / AppIndicator |
| `CloseToTray` | `bool` | Clicking `X` closes window into AppIndicator tray |

---

## 🛠️ Building & Testing

### Prerequisites

- **Rust toolchain (stable 1.75+)**
- **Linux Packages:** `libxcb-render0-dev`, `libxcb-shape0-dev`, `libxcb-xfixes0-dev`, `libx11-dev`, `libdbus-1-dev`, `libgtk-3-dev`

```bash
# Ubuntu / Debian dependencies
sudo apt-get update
sudo apt-get install -y libxcb-render0-dev libxcb-shape0-dev libxcb-xfixes0-dev libx11-dev libdbus-1-dev libgtk-3-dev pkg-config
```

### Build

```bash
cargo build --release
```

### Test

```bash
cargo test
```

---

## 📦 Automated CI/CD Releases

Automated cross-platform builds and GitHub releases are managed via GitHub Actions:
- Pushing a new tag (e.g. `v0.2.0`) triggers `.github/workflows/build-and-release.yml`.
- The workflow compiles release binaries for both Linux (`x86_64-unknown-linux-gnu`) and Windows (`x86_64-pc-windows-msvc`) and attaches the release archives.

---

## 📄 License

Distributed under the MIT License. See `LICENSE` for details.

