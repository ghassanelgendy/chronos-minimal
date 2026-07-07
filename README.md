# Chronos Minimal

[![GitHub Release](https://img.shields.io/github/v/tag/ghassanelgendy/chronos-minimal?style=flat-square&color=0066cc&label=release)](https://github.com/ghassanelgendy/chronos-minimal/releases)
[![Downloads](https://img.shields.io/github/downloads/ghassanelgendy/chronos-minimal/total?style=flat-square&color=2ea44f)](https://github.com/ghassanelgendy/chronos-minimal/releases)
[![Rust Version](https://img.shields.io/badge/rust-1.75%2B-e3592c?style=flat-square)](https://www.rust-lang.org)
[![Platform](https://img.shields.io/badge/platform-windows-lightgray?style=flat-square)](#)

<p align="center">
  <em>
    Chronos is also the 
    <a href="https://en.wikipedia.org/wiki/Chronos" target="_blank">
      Greek god of time!
    </a>
  </em>
</p>

Chronos Minimal is a lightweight background screen time tracker for Windows. It is a Rust port of the original [.NET/WPF Chronos](https://github.com/ghassanelgendy/chronos-screentime), rewritten from scratch for near-zero CPU/RAM usage and a minimal system footprint.

The app sits in your system tray, tracks active application usage (including browser domains from window titles), and syncs your logs securely to a Supabase database.

---

### [**Download Latest Windows Release**](https://github.com/ghassanelgendy/chronos-minimal/releases/latest)

*Download and run `chronos-screentime.exe` to start tracking. No installation required.*

---

## Features

- **Resource Efficient:** Uses minimal RAM/CPU by running as a native Windows tray application.
- **Active App Tracking:** Polls the foreground window every 3 seconds using the native Windows API (`GetForegroundWindow`). Logs the process path, executable name, and extracts base domain names from active browser tabs.
- **Supabase Integration:** Syncs tracking logs to your Supabase instance using a time-gated local cache to reduce network calls and database writes.
- **Asynchronous Architecture:** Heavy operations (e.g., connection checks, database uploads) run on background threads to ensure the system tray and UI dashboard remain fully responsive.
- **Local Cache:** Backs up all tracked activity and configurations locally to `%AppData%\ChronosScreenTime\data.json` and `settings.json`.

---

## Dashboard Overview

The app includes a simple three-tab dashboard:

1. **Activity:** Shows today's total screen time, tracking status (pause/resume), activity logs, and historical usage reports.
2. **Cloud Sync:** Configures Supabase backend credentials, test connection status, and user identifiers.
3. **Preferences:** Set idle timeout thresholds (pauses tracking when away), toggle startup on logon (registry key), start minimized to tray, and export or reset tracking data.

---

## Configuration

Settings are stored in `%AppData%\ChronosScreenTime\settings.json`:

- `EnableSupabaseSync` (bool): Toggle cloud database sync.
- `SupabaseUrl` (string): Your Supabase API endpoint.
- `SupabaseAnonKey` (string): Your Supabase anonymous client key.
- `SupabaseUserId` (string): User identifier for filtering/sync.
- `SupabaseUploadIntervalMinutes` (int): Frequency of database sync.
- `IdleThresholdSeconds` (int): Number of seconds of inactivity before tracking pauses automatically.
- `StartWithWindows` (bool): Auto-run the application at logon.
- `StartMinimizedToTray` (bool): Hides the window to the tray on launch.

---

## Building from Source

You will need the Rust toolchain (stable) installed.

### Build Executable

To build the optimized release binary:

```bash
cargo build --release
```

The compiled binary will be at `target\release\chronos-screentime.exe`.

### Running Tests

```bash
cargo test
```

---

## Technical Stack

- **Core Engine:** Rust (edition 2021)
- **GUI & Tray:** `native-windows-gui` (NWG) & `tray-item`
- **Async Runtime:** `tokio`
- **Network Client:** `reqwest`
- **JSON Serialization:** `serde` & `serde_json`
- **Time/Dates:** `chrono`
- **Windows API:** `windows` crate

---

## License

Distributed under the MIT License. See `LICENSE` for details.
