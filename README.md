# Chronos Screentime (Rust)

Lightweight **Windows** screentime tracker with **Supabase sync**. Built from the [screentime app spec](screentime_app_spec_prompt_79ad01ba.plan.md) and [SUPABASE_SYNC](SUPABASE_SYNC.md).

## Features

- **Tracking:** Polls foreground window every ~3s via `GetForegroundWindow` + process path; records app name and (for browsers) domain from window title.
- **Storage:** JSON in `%AppData%\ChronosScreenTime\` (screentime data + settings).
- **Supabase sync:** One-way upload to `POST …/functions/v1/upload-screentime` with time gating and cache (see SUPABASE_SYNC.md).

## Build

```bash
cargo build --release
```

Output: `target\release\chronos-screentime.exe`

## Run

1. Run `chronos-screentime.exe`.
2. Icon appears in the system tray.
3. **Preferences (Supabase sync):** Enable sync, set Supabase URL, anon key, LifeOS User ID (UUID), upload interval (minutes), then **Save**. Use **Test Connection** to verify.
4. **Exit:** Tray menu → Exit.

## Config

- **Path:** `%AppData%\ChronosScreenTime\settings.json`
- **Keys (PascalCase):** `EnableSupabaseSync`, `SupabaseUrl`, `SupabaseAnonKey`, `SupabaseUserId`, `SupabaseUploadIntervalMinutes`

## Tech

- **Rust**, Windows API (`windows` crate), **native-windows-gui** (settings), **tray-item** (tray), **reqwest** + **tokio** (HTTP), **chrono**, **serde_json**, **directories** (AppData).
