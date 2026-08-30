//! Load/save screentime data and settings (Section 3 paths).

use crate::models::{AppSettings, ScreenTimeData};
use directories::ProjectDirs;
use std::path::PathBuf;

// %AppData%\ChronosScreenTime on Windows, ~/.config/chronosscreentime on Linux
const QUALIFIER: &str = "";
#[cfg(windows)]
const ORG: &str = "ChronosScreenTime";
#[cfg(not(windows))]
const ORG: &str = "chronosscreentime";
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
        let mut data: ScreenTimeData = serde_json::from_slice(&bytes).unwrap_or_default();
        if sanitize_screen_time_data(&mut data) {
            save_screen_time_data(&data);
        }
        return data;
    }
    // Check legacy path if primary doesn't exist
    if let Some(legacy) = legacy_data_path() {
        if legacy != path && legacy.exists() {
            if let Ok(bytes) = std::fs::read(&legacy) {
                let mut data: ScreenTimeData = serde_json::from_slice(&bytes).unwrap_or_default();
                sanitize_screen_time_data(&mut data);
                save_screen_time_data(&data);
                return data;
            }
        }
    }
    ScreenTimeData::default()
}

/// True when a stored website name is junk that should never have been a site: local
/// file paths, raw URLs, URL-encoded shell content, or absurdly long page titles that
/// the old tracker stored verbatim when it couldn't extract a domain.
fn is_garbage_website_name(name: &str) -> bool {
    let lower = name.to_lowercase();
    if lower.contains("file://")
        || lower.contains("%20")
        || lower.contains("://")
        || name.starts_with('/')
        || name.len() > 120
    {
        return true;
    }
    // Filename-style labels tracked as websites (e.g. "rockyou.txt", "tar.gz").
    crate::tracker::is_filename_like_label(name)
}

/// Drop legacy junk website entries and migrate brand-titled entries (that the old
/// tracker stored verbatim as a page sentence) to their canonical domain, so names
/// stay unified. Returns true when anything changed.
pub fn sanitize_screen_time_data(data: &mut ScreenTimeData) -> bool {
    let mut changed = false;

    // Small helper templates so the closure doesn't move the reused entry.
    let rekey = |key: &str,
                     entry: crate::models::WebsiteDailyData,
                     sites: &mut std::collections::BTreeMap<String, crate::models::WebsiteDailyData>|
     -> bool {
        let canonical = match crate::tracker::domain_from_title(key) {
            Some(d) if d != key => d,
            _ => return false,
        };
        let (f_first, f_last, f_active) = (
            entry.first_seen.clone(),
            entry.last_seen.clone(),
            entry.last_active_time.clone(),
        );
        let target = sites
            .entry(canonical.clone())
            .or_insert_with(|| crate::models::WebsiteDailyData {
                domain: canonical.clone(),
                category: crate::category::get_category_for_website(&canonical),
                total_time_seconds: 0,
                session_count: 0,
                first_seen: f_first.clone(),
                last_seen: f_last.clone(),
                last_active_time: f_active.clone(),
                favicon_url: crate::tracker::favicon_url_for_domain(&canonical),
            });
        target.total_time_seconds = target.total_time_seconds.saturating_add(entry.total_time_seconds);
        target.session_count = target.session_count.saturating_add(entry.session_count);
        if target.first_seen.is_empty() {
            target.first_seen = entry.first_seen.clone();
        }
        if entry.last_seen > target.last_seen {
            target.last_seen = entry.last_seen.clone();
        }
        if entry.last_active_time > target.last_active_time {
            target.last_active_time = entry.last_active_time.clone();
        }
        true
    };

    for year in data.years.values_mut() {
        for month in year.months.values_mut() {
            for week in month.weeks.values_mut() {
                for day in week.days.values_mut() {
                    // 1. Drop local-resource junk that is clearly not a website.
                    let before = day.websites.len();
                    day.websites.retain(|name, _| !is_garbage_website_name(name));
                    changed |= day.websites.len() != before;

                    // 2. Rename brand-title entries (e.g. "2-Step Verification … sign in")
                    //    to their canonical domain (google.com) and merge their time into it.
                    let keys: Vec<String> = day.websites.keys().cloned().collect();
                    for key in keys {
                        if !key.contains(' ') {
                            continue;
                        }
                        if let Some(entry) = day.websites.remove(&key) {
                            changed |= rekey(&key, entry, &mut day.websites);
                        }
                    }
                }
            }
        }
    }
    changed
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{DayData, ScreenTimeData, WeekData};

    fn day_with_sites(sites: &[(&str, u64)]) -> DayData {
        let mut day = DayData::default();
        for (s, secs) in sites {
            day.websites.insert(
                s.to_string(),
                crate::models::WebsiteDailyData {
                    domain: s.to_string(),
                    total_time_seconds: *secs,
                    ..Default::default()
                },
            );
        }
        day
    }

    fn wrap(days: Vec<DayData>) -> ScreenTimeData {
        let mut data = ScreenTimeData::default();
        let year = data.years.entry("2026".to_string()).or_default();
        let month = year.months.entry("8".to_string()).or_default();
        let mut week = WeekData::default();
        week.days.insert("2026-08-30".to_string(), days[0].clone());
        month.weeks.insert("2026-W35".to_string(), week);
        data
    }

    #[test]
    fn test_sanitize_removes_junk_and_merges_brand_titles() {
        let google_2fa = "2-Step Verification To help keep your account safe Google wants to make sure it's really you trying to sign in";
        let mut data = wrap(vec![day_with_sites(&[
            ("file:///mnt/01DC492C8F8307B0/Github/chronos-minimal%20main%20!1%20❯%20wezterm -vv", 42),
            (google_2fa, 30),
            ("youtube.com", 120),
        ])]);

        assert!(sanitize_screen_time_data(&mut data));

        let day = data
            .years
            .get("2026")
            .and_then(|y| y.months.get("8"))
            .and_then(|m| m.weeks.get("2026-W35"))
            .and_then(|w| w.days.get("2026-08-30"))
            .unwrap();

        // Junk file/URL-encoded title removed.
        assert!(!day.websites.contains_key(
            "file:///mnt/01DC492C8F8307B0/Github/chronos-minimal%20main%20!1%20❯%20wezterm -vv"
        ));
        // Brand-titled entry renamed → merged into its canonical domain.
        assert!(!day.websites.contains_key(google_2fa));
        assert_eq!(day.websites.get("google.com").map(|w| w.total_time_seconds), Some(30));
        // Real domain untouched.
        assert_eq!(day.websites.get("youtube.com").map(|w| w.total_time_seconds), Some(120));

        // A second pass changes nothing.
        assert!(!sanitize_screen_time_data(&mut data));
        assert!(data
            .years
            .get("2026")
            .and_then(|y| y.months.get("8"))
            .and_then(|m| m.weeks.get("2026-W35"))
            .and_then(|w| w.days.get("2026-08-30"))
            .is_some());
    }

    #[test]
    fn test_is_garbage_website_name() {
        assert!(is_garbage_website_name("file:///home/user/x"));
        assert!(is_garbage_website_name("some%20url-encoded%20junk"));
        assert!(is_garbage_website_name("/mnt/something"));
        assert!(is_garbage_website_name(&"x".repeat(121)));
        assert!(is_garbage_website_name("rockyou.txt"));
        assert!(is_garbage_website_name("tar.gz"));
        assert!(is_garbage_website_name("frappe.utils.print_format.download_pdf"));
        assert!(!is_garbage_website_name("youtube.com"));
        assert!(!is_garbage_website_name("erp.servixa-it.com"));
        assert!(!is_garbage_website_name("192.168.1.100"));
        assert!(!is_garbage_website_name(&"x".repeat(120)));
    }
}
