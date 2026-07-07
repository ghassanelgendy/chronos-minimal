# Chronos Screentime

[![GitHub Release](https://img.shields.io/github/v/tag/ghassanelgendy/chronos-minimal?style=flat-square&color=0066cc&label=release)](https://github.com/ghassanelgendy/chronos-minimal/releases)
[![Downloads](https://img.shields.io/github/downloads/ghassanelgendy/chronos-minimal/total?style=flat-square&color=2ea44f)](https://github.com/ghassanelgendy/chronos-minimal/releases)
[![Rust Version](https://img.shields.io/badge/rust-1.75%2B-e3592c?style=flat-square)](https://www.rust-lang.org)
[![Platform](https://img.shields.io/badge/platform-windows-lightgray?style=flat-square)](#)

Chronos Screentime is a lightweight, background-running Windows screentime tracking application. This version is a performance-boosted port of the original [chronos](https://github.com/ghassanelgendy/chronos-screentime) rewritten in Rust for minimal resource consumption and system footprint.

The application sits quietly in your system tray, records your active application usage, extracts browser domains from window titles, and syncs this data to a Supabase backend database securely and efficiently.

***

### [**Download Latest Windows Executable**](https://github.com/ghassanelgendy/chronos-minimal/releases/latest)

*Simply download and run `chronos-screentime.exe` from the latest release to start tracking immediately. No installer required.*

***

## Core Features

- **Precise Application Tracking**: Polls the foreground window every 3 seconds using the native Windows API (`GetForegroundWindow`). It records the process path, executable name, and resolves browser tabs to their base domain names.
- **Local Data Storage**: Persists tracked activity logs and settings locally in `%AppData%\ChronosScreenTime\data.json` and `settings.json`.
- **Intelligent Supabase Sync**: Uploads screentime records to your Supabase instance using a time-gated local cache. It compares usage times and session counts before uploading, reducing payload size and network calls.
- **Non-Blocking GUI**: Built on a background-threaded architecture. Heavy operations like database connection checks and uploads run asynchronously, keeping the system tray and tabbed dashboard fully responsive.
- **Three-Tab Dashboard**:
  - **Activity**: Shows real-time today total screen time, pause/resume tracking switches, time-period reports, and an activity log explorer.
  - **Cloud Sync**: Manages Supabase sync configuration, secure masked key fields, logon session startup tracking, and manual connection diagnostics.
  - **Preferences**: Configures Windows startup registration (Run at logon), start minimized, idle thresholds, data exports, and selective application data resetting.

## System Configuration

Configuration is managed automatically through the settings dashboard and stored in `%AppData%\ChronosScreenTime\settings.json`.

Available parameters:
- `EnableSupabaseSync`: (Boolean) Toggles remote server synchronization.
- `SupabaseUrl`: (String) Your Supabase API endpoint.
- `SupabaseAnonKey`: (String) Your Supabase anonymous client key.
- `SupabaseUserId`: (String/UUID) Your LifeOS user identifier.
- `SupabaseUploadIntervalMinutes`: (Integer) Time window between automated uploads.
- `IdleThresholdSeconds`: (Integer) Time without input before tracking automatically pauses.
- `StartWithWindows`: (Boolean) Registers the app to run on system logon.
- `StartMinimizedToTray`: (Boolean) Hides the main dashboard window to the system tray on startup.

## Development and Build Requirements

To build Chronos Screentime from source, you will need the Rust toolchain installed on a Windows machine.

### Build Executable

Run the cargo build command with the release profile:

```bash
cargo build --release
```

The compiled executable will be generated at `target\release\chronos-screentime.exe`.

### Running Tests

Run unit tests covering the serialization, local caching, and payload synchronization logic:

```bash
cargo test
```

## Technical Stack

- **Core Engine**: Rust (edition 2021)
- **Windows Integration**: Windows API (`windows` crate)
- **Graphical User Interface**: `native-windows-gui` (NWG) for the dashboard, `tray-item` for system tray controls
- **Asynchronous Runtime**: `tokio` for background tasks, `reqwest` for HTTP synchronization requests
- **Data Handling**: `serde` and `serde_json` for serialization/deserialization, `chrono` for timezone-aware time management
