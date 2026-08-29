//! Load/save screentime data and settings (Section 3 paths).

use crate::models::{AppSettings, ScreenTimeData};
use directories::ProjectDirs;
use std::path::PathBuf;

// %AppData%\ChronosScreenTime (SUPABASE_SYNC Section 3)
const QUALIFIER: &str = "";
const ORG: &str = "ChronosScreenTime";
const APP: &str = "ChronosScreenTime";
const DATA_FILE: &str = "screentime_data.json";
const SETTINGS_FILE: &str = "settings.json";

fn project_dirs() -> Option<ProjectDirs> {
    ProjectDirs::from(QUALIFIER, ORG, APP)
}

/// Base directory: %AppData%\ChronosScreenTime on Windows, ~/.config/ChronosScreenTime on Linux.
pub fn data_dir() -> Option<PathBuf> {
    if let Some(proj) = project_dirs() {
        Some(proj.config_dir().to_path_buf())
    } else if let Some(base) = directories::BaseDirs::new() {
        Some(base.config_dir().join(ORG))
    } else {
        None
    }
}

fn ensure_data_dir() -> Option<PathBuf> {
    let dir = data_dir()?;
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir)
}

pub fn data_file_path() -> Option<PathBuf> {
    ensure_data_dir().map(|d| d.join(DATA_FILE))
}

pub fn settings_file_path() -> Option<PathBuf> {
    ensure_data_dir().map(|d| d.join(SETTINGS_FILE))
}

fn legacy_data_path() -> Option<PathBuf> {
    directories::BaseDirs::new().map(|b| b.config_dir().join(DATA_FILE))
}

fn legacy_settings_path() -> Option<PathBuf> {
    directories::BaseDirs::new().map(|b| b.config_dir().join(SETTINGS_FILE))
}

pub fn load_screen_time_data() -> ScreenTimeData {
    let path = match data_file_path() {
        Some(p) => p,
        None => return ScreenTimeData::default(),
    };
    if let Ok(bytes) = std::fs::read(&path) {
        return serde_json::from_slice(&bytes).unwrap_or_default();
    }
    // Check legacy path if primary doesn't exist
    if let Some(legacy) = legacy_data_path() {
        if legacy != path && legacy.exists() {
            if let Ok(bytes) = std::fs::read(&legacy) {
                let data: ScreenTimeData = serde_json::from_slice(&bytes).unwrap_or_default();
                save_screen_time_data(&data);
                return data;
            }
        }
    }
    ScreenTimeData::default()
}

pub fn save_screen_time_data(data: &ScreenTimeData) {
    let path = match data_file_path() {
        Some(p) => p,
        None => return,
    };
    if let Ok(json) = serde_json::to_string_pretty(data) {
        let _ = std::fs::write(path, json);
    }
}

pub fn clear_all_data() {
    save_screen_time_data(&ScreenTimeData::default());
}

pub fn reset_app_data(app_name: &str) -> usize {
    if app_name.trim().is_empty() {
        return 0;
    }
    let mut data = load_screen_time_data();
    let target = app_name.trim().to_lowercase();
    let mut removed = 0usize;

    for year in data.years.values_mut() {
        for month in year.months.values_mut() {
            for week in month.weeks.values_mut() {
                for day in week.days.values_mut() {
                    let before = day.apps.len();
                    day.apps
                        .retain(|_, a| a.app_name.trim().to_lowercase() != target);
                    let after = day.apps.len();
                    removed += before.saturating_sub(after);
                    day.total_apps = day.apps.len() as u32;
                }
            }
        }
    }

    save_screen_time_data(&data);
    removed
}

pub fn export_data_snapshot(data: &ScreenTimeData) -> Result<PathBuf, String> {
    let dir = ensure_data_dir().ok_or_else(|| "AppData directory unavailable".to_string())?;
    let export_dir = dir.join("exports");
    std::fs::create_dir_all(&export_dir)
        .map_err(|e| format!("Failed creating export directory: {}", e))?;

    let timestamp = chrono::Local::now().format("%Y%m%d-%H%M%S").to_string();
    let path = export_dir.join(format!("chronos-export-{}.json", timestamp));
    let json = serde_json::to_string_pretty(data)
        .map_err(|e| format!("Failed serializing export: {}", e))?;
    std::fs::write(&path, json).map_err(|e| format!("Failed writing export: {}", e))?;
    Ok(path)
}

pub fn load_settings() -> AppSettings {
    let path = match settings_file_path() {
        Some(p) => p,
        None => return AppSettings::default(),
    };
    if let Ok(bytes) = std::fs::read(&path) {
        return serde_json::from_slice(&bytes).unwrap_or_default();
    }
    // Check legacy path if primary doesn't exist
    if let Some(legacy) = legacy_settings_path() {
        if legacy != path && legacy.exists() {
            if let Ok(bytes) = std::fs::read(&legacy) {
                let settings: AppSettings = serde_json::from_slice(&bytes).unwrap_or_default();
                save_settings(&settings);
                return settings;
            }
        }
    }
    AppSettings::default()
}

pub fn save_settings(settings: &AppSettings) {
    let path = match settings_file_path() {
        Some(p) => p,
        None => return,
    };
    if let Ok(json) = serde_json::to_string_pretty(settings) {
        let _ = std::fs::write(path, json);
    }
}

/// Cache path for Supabase upload (Section 9).
pub fn supabase_cache_path() -> Option<PathBuf> {
    ensure_data_dir().map(|d| d.join("supabase_upload_cache.json"))
}
