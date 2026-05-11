//! Supabase upload service: payload build, cache, time gating, POST to upload-screentime (SUPABASE_SYNC Sections 4, 7, 9).

use crate::models::ScreenTimeData;
use crate::category::{get_category_for_app, get_category_for_website};
use crate::storage::supabase_cache_path;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

const PLATFORM: &str = "windows";
const SOURCE: &str = "pc";
const APP_LOCK_NAME: &str = "AppLock";

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct UploadCache {
    #[serde(default)]
    pub uploaded_apps: HashSet<String>,
    #[serde(default)]
    pub uploaded_websites: HashSet<String>,
    #[serde(default)]
    pub uploaded_daily_summaries: HashSet<String>,
    #[serde(default)]
    pub last_upload_time_utc: String,
}

fn cache_key_summary(user_id: &str, date: &str, device_id: &str) -> String {
    format!("{}|{}|{}|{}|summary", user_id, date, SOURCE, device_id)
}

fn load_cache() -> UploadCache {
    let path = match supabase_cache_path() {
        Some(p) => p,
        None => return UploadCache::default(),
    };
    let Ok(bytes) = std::fs::read(&path) else {
        return UploadCache::default();
    };
    serde_json::from_slice(&bytes).unwrap_or_default()
}

fn save_cache(cache: &UploadCache) {
    let path = match supabase_cache_path() {
        Some(p) => p,
        None => return,
    };
    if let Ok(json) = serde_json::to_string_pretty(cache) {
        let _ = std::fs::write(path, json);
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct UploadResult {
    pub success: bool,
    pub error_message: Option<String>,
    pub apps_inserted: u64,
    pub websites_inserted: u64,
    pub total_apps: u64,
    pub total_websites: u64,
}

#[derive(Debug, Deserialize)]
pub struct UploadResponse {
    pub success: Option<bool>,
    pub inserted: Option<InsertedData>,
    pub total: Option<TotalData>,
    pub error: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct InsertedData {
    pub apps: Option<u64>,
    pub websites: Option<u64>,
}

#[derive(Debug, Deserialize)]
pub struct TotalData {
    pub apps: Option<u64>,
    pub websites: Option<u64>,
}

/// Build payload in flat snapshots format matching the edge function's FlatSnapshot/FlatUsageItem interfaces.
/// Uses plain integer `total_time_seconds`, is_cumulative=true so the server takes the max on upsert.
fn build_payload(data: &ScreenTimeData, user_id: &str, device_id: &str) -> serde_json::Value {
    use serde_json::json;
    let mut snapshots = Vec::new();
    let mut daily_summaries = Vec::new();

    for (_, year) in &data.years {
        for (_, month) in &year.months {
            for (_, week) in &month.weeks {
                for (_, day) in &week.days {
                    let mut apps = Vec::new();
                    let mut total_apps_no_lock: u32 = 0;
                    for (_, app) in &day.apps {
                        if app.app_name.eq_ignore_ascii_case(APP_LOCK_NAME) {
                            continue;
                        }
                        let category = if app.category.is_empty() {
                            get_category_for_app(&app.app_name)
                        } else {
                            app.category.clone()
                        };
                        total_apps_no_lock += 1;
                        apps.push(json!({
                            "app_name": app.app_name,
                            "category": category.clone(),
                            "process_path": app.process_path,
                            "total_time_seconds": app.total_time_seconds,
                            "session_count": app.session_count,
                            "first_seen_at": app.first_seen,
                            "last_seen_at": app.last_seen,
                            "last_active_at": app.last_active_time,
                        }));
                    }
                    let mut websites = Vec::new();
                    for (_, site) in &day.websites {
                        let category = if site.category.is_empty() {
                            get_category_for_website(&site.domain)
                        } else {
                            site.category.clone()
                        };
                        websites.push(json!({
                            "domain": site.domain,
                            "category": category.clone(),
                            "favicon_url": site.favicon_url,
                            "total_time_seconds": site.total_time_seconds,
                            "session_count": site.session_count,
                            "first_seen_at": site.first_seen,
                            "last_seen_at": site.last_seen,
                            "last_active_at": site.last_active_time,
                        }));
                    }
                    let total_switches = if day.total_switches > 0 {
                        day.total_switches
                    } else {
                        day.apps
                            .values()
                            .filter(|a| !a.app_name.eq_ignore_ascii_case(APP_LOCK_NAME))
                            .map(|a| a.session_count)
                            .sum()
                    };
                    if !apps.is_empty() || !websites.is_empty() {
                        daily_summaries.push(json!({
                            "date": day.date,
                            "total_switches": total_switches,
                            "total_apps": total_apps_no_lock,
                        }));
                        snapshots.push(json!({
                            "date": day.date,
                            "apps": apps,
                            "websites": websites,
                            "total_switches": total_switches,
                            "total_apps": total_apps_no_lock,
                        }));
                    }
                }
            }
        }
    }

    json!({
        "user_id": user_id,
        "device_id": device_id,
        "platform": PLATFORM,
        "source": SOURCE,
        "is_cumulative": true,
        "daily_summaries": daily_summaries,
        "snapshots": snapshots,
    })
}

/// Upload screentime data to Supabase. Time-gated by cache; returns result.
pub async fn upload_screentime_data(
    data: &ScreenTimeData,
    supabase_url: &str,
    supabase_anon_key: &str,
    user_id: &str,
    device_id: &str,
    upload_interval_minutes: u32,
) -> UploadResult {
    if user_id.trim().is_empty() {
        return UploadResult {
            success: false,
            error_message: Some("User ID is required".to_string()),
            apps_inserted: 0,
            websites_inserted: 0,
            total_apps: 0,
            total_websites: 0,
        };
    }

    let body = build_payload(data, user_id, device_id);
    let snapshot_count = body.get("snapshots").and_then(|v| v.as_array()).map(|a| a.len()).unwrap_or(0);
    if snapshot_count == 0 {
        return UploadResult {
            success: true,
            error_message: None,
            apps_inserted: 0,
            websites_inserted: 0,
            total_apps: 0,
            total_websites: 0,
        };
    }

    let mut cache = load_cache();
    let now = Utc::now();
    let last_utc = chrono::DateTime::parse_from_rfc3339(&cache.last_upload_time_utc).ok();
    let interval_mins = if upload_interval_minutes == 0 {
        30
    } else {
        upload_interval_minutes
    };
    if let Some(last) = last_utc {
        let next_allowed = last + chrono::Duration::minutes(interval_mins as i64);
        if now < next_allowed {
            return UploadResult {
                success: true,
                error_message: None,
                apps_inserted: 0,
                websites_inserted: 0,
                total_apps: 0,
                total_websites: 0,
            };
        }
    }

    let url = build_upload_url(supabase_url);

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(300))
        .build()
        .unwrap();
    let res = client
        .post(&url)
        .header("apikey", supabase_anon_key)
        .header("Authorization", format!("Bearer {}", supabase_anon_key))
        .header("Content-Type", "application/json")
        .body(body.to_string())
        .send()
        .await;

    let (success, error_message, apps_inserted, websites_inserted, total_apps, total_websites) = match res {
        Err(e) => (
            false,
            Some(e.to_string()),
            0u64,
            0u64,
            0u64,
            0u64,
        ),
        Ok(resp) => {
            let status = resp.status();
            let body_text = resp.text().await.unwrap_or_default();
            if !status.is_success() {
                return UploadResult {
                    success: false,
                    error_message: Some(body_text),
                    apps_inserted: 0,
                    websites_inserted: 0,
                    total_apps: 0,
                    total_websites: 0,
                };
            }
            let parsed: Result<UploadResponse, _> = serde_json::from_str(&body_text);
            match parsed {
                Ok(r) => (
                    r.success.unwrap_or(true),
                    r.error,
                    r.inserted
                        .as_ref()
                        .and_then(|i| i.apps)
                        .unwrap_or(0),
                    r.inserted
                        .as_ref()
                        .and_then(|i| i.websites)
                        .unwrap_or(0),
                    r.total.as_ref().and_then(|t| t.apps).unwrap_or(0),
                    r.total.as_ref().and_then(|t| t.websites).unwrap_or(0),
                ),
                Err(_) => (true, None, 0, 0, 0, 0),
            }
        }
    };

    if success {
        cache.last_upload_time_utc = now.to_rfc3339();
        for (_, year) in &data.years {
            for (_, month) in &year.months {
                for (_, week) in &month.weeks {
                    for (date, day) in &week.days {
                        cache.uploaded_daily_summaries.insert(cache_key_summary(user_id, date, device_id));
                        for app_name in day.apps.keys() {
                            cache.uploaded_apps.insert(format!("{}|{}|{}|{}|{}|{}", user_id, date, SOURCE, device_id, PLATFORM, app_name));
                        }
                        for domain in day.websites.keys() {
                            cache.uploaded_websites.insert(format!("{}|{}|{}|{}|{}|{}", user_id, date, SOURCE, device_id, PLATFORM, domain));
                        }
                    }
                }
            }
        }
        save_cache(&cache);
    }

    UploadResult {
        success,
        error_message,
        apps_inserted,
        websites_inserted,
        total_apps,
        total_websites,
    }
}

/// Build upload URL: if URL already contains /functions/, use as-is; else append /functions/v1/upload-screentime.
fn build_upload_url(supabase_url: &str) -> String {
    let base = supabase_url.trim_end_matches('/');
    if base.contains("/functions/") {
        base.to_string()
    } else {
        format!("{}/functions/v1/upload-screentime", base)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_build_upload_url() {
        assert!(build_upload_url("https://x.supabase.co").ends_with("upload-screentime"));
        assert!(build_upload_url("https://x.supabase.co/functions/v1/foo").eq("https://x.supabase.co/functions/v1/foo"));
    }
}
