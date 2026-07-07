//! Supabase upload service: payload build, cache, time gating, POST to upload-screentime (SUPABASE_SYNC Sections 4, 7, 9).

use crate::models::ScreenTimeData;
use crate::category::{get_category_for_app, get_category_for_website};
use crate::storage::supabase_cache_path;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

const PLATFORM: &str = "windows";
const SOURCE: &str = "pc";
const APP_LOCK_NAME: &str = "AppLock";

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct CacheEntry {
    pub total_time_seconds: u64,
    pub session_count: u32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct SummaryCacheEntry {
    pub total_switches: u32,
    pub total_apps: u32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct UploadCache {
    #[serde(default)]
    pub uploaded_apps: HashMap<String, CacheEntry>,
    #[serde(default)]
    pub uploaded_websites: HashMap<String, CacheEntry>,
    #[serde(default)]
    pub uploaded_daily_summaries: HashMap<String, SummaryCacheEntry>,
    #[serde(default)]
    pub last_upload_time_utc: String,
}

fn cache_key_summary(user_id: &str, date: &str, device_id: &str) -> String {
    format!("{}|{}|{}|{}|summary", user_id, date, SOURCE, device_id)
}

fn cache_key_app(user_id: &str, date: &str, device_id: &str, app_name: &str) -> String {
    format!("{}|{}|{}|{}|{}|{}", user_id, date, SOURCE, device_id, PLATFORM, app_name)
}

fn cache_key_website(user_id: &str, date: &str, device_id: &str, domain: &str) -> String {
    format!("{}|{}|{}|{}|{}|{}", user_id, date, SOURCE, device_id, PLATFORM, domain)
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
fn build_payload(
    data: &ScreenTimeData,
    user_id: &str,
    device_id: &str,
    cache: &UploadCache,
) -> serde_json::Value {
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
                        total_apps_no_lock += 1;

                        let app_key = cache_key_app(user_id, &day.date, device_id, &app.app_name);
                        let is_changed = match cache.uploaded_apps.get(&app_key) {
                            Some(entry) => {
                                entry.total_time_seconds != app.total_time_seconds
                                    || entry.session_count != app.session_count
                            }
                            None => true,
                        };

                        if is_changed {
                            let category = if app.category.is_empty() {
                                get_category_for_app(&app.app_name)
                            } else {
                                app.category.clone()
                            };
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
                    }

                    let mut websites = Vec::new();
                    for (_, site) in &day.websites {
                        let website_key = cache_key_website(user_id, &day.date, device_id, &site.domain);
                        let is_changed = match cache.uploaded_websites.get(&website_key) {
                            Some(entry) => {
                                entry.total_time_seconds != site.total_time_seconds
                                    || entry.session_count != site.session_count
                            }
                            None => true,
                        };

                        if is_changed {
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

                    let summary_key = cache_key_summary(user_id, &day.date, device_id);
                    let summary_changed = match cache.uploaded_daily_summaries.get(&summary_key) {
                        Some(entry) => {
                            entry.total_switches != total_switches
                                || entry.total_apps != total_apps_no_lock
                        }
                        None => true,
                    };

                    if !apps.is_empty() || !websites.is_empty() || summary_changed {
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

    let body = build_payload(data, user_id, device_id, &cache);
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
                        let mut total_apps_no_lock: u32 = 0;
                        for app in day.apps.values() {
                            if !app.app_name.eq_ignore_ascii_case(APP_LOCK_NAME) {
                                total_apps_no_lock += 1;
                            }
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

                        let summary_key = cache_key_summary(user_id, date, device_id);
                        cache.uploaded_daily_summaries.insert(
                            summary_key,
                            SummaryCacheEntry {
                                total_switches,
                                total_apps: total_apps_no_lock,
                            },
                        );

                        for (app_name, app) in &day.apps {
                            let app_key = cache_key_app(user_id, date, device_id, app_name);
                            cache.uploaded_apps.insert(
                                app_key,
                                CacheEntry {
                                    total_time_seconds: app.total_time_seconds,
                                    session_count: app.session_count,
                                },
                            );
                        }
                        for (domain, site) in &day.websites {
                            let website_key = cache_key_website(user_id, date, device_id, domain);
                            cache.uploaded_websites.insert(
                                website_key,
                                CacheEntry {
                                    total_time_seconds: site.total_time_seconds,
                                    session_count: site.session_count,
                                },
                            );
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

    #[test]
    fn test_cache_serialization_and_deserialization() {
        let mut cache = UploadCache::default();
        cache.last_upload_time_utc = "2026-07-07T00:00:00Z".to_string();
        cache.uploaded_apps.insert(
            "user1|2026-07-07|pc|device1|windows|Chrome".to_string(),
            CacheEntry {
                total_time_seconds: 120,
                session_count: 5,
            },
        );
        cache.uploaded_daily_summaries.insert(
            "user1|2026-07-07|pc|device1|summary".to_string(),
            SummaryCacheEntry {
                total_switches: 10,
                total_apps: 2,
            },
        );

        let json = serde_json::to_string(&cache).unwrap();
        let decoded: UploadCache = serde_json::from_str(&json).unwrap();

        assert_eq!(decoded.last_upload_time_utc, "2026-07-07T00:00:00Z");
        assert_eq!(
            decoded.uploaded_apps.get("user1|2026-07-07|pc|device1|windows|Chrome").unwrap().total_time_seconds,
            120
        );
        assert_eq!(
            decoded.uploaded_daily_summaries.get("user1|2026-07-07|pc|device1|summary").unwrap().total_switches,
            10
        );
    }

    #[test]
    fn test_legacy_cache_fallback() {
        // Construct a legacy cache JSON where uploaded_apps is a string array (HashSet<String>)
        let legacy_json = r#"{
            "uploaded_apps": ["user1|2026-07-07|pc|device1|windows|Chrome"],
            "uploaded_websites": [],
            "uploaded_daily_summaries": [],
            "last_upload_time_utc": "2026-07-07T00:00:00Z"
        }"#;

        // Deserialization as UploadCache should fail due to type mismatch (array instead of object),
        // but load_cache's unwrap_or_default() pattern resolves this. Here we verify that a standard deserialization
        // of type mismatched json results in an error (confirming why unwrap_or_default fallback works).
        let decoded: Result<UploadCache, _> = serde_json::from_str(legacy_json);
        assert!(decoded.is_err());
    }

    fn dummy_data() -> ScreenTimeData {
        use crate::models::{YearData, MonthData, WeekData, DayData, AppDailyData, WebsiteDailyData};
        use std::collections::BTreeMap;

        let mut app_map = BTreeMap::new();
        app_map.insert(
            "Chrome".to_string(),
            AppDailyData {
                app_name: "Chrome".to_string(),
                category: "Browsing".to_string(),
                process_path: "chrome.exe".to_string(),
                total_time_seconds: 120,
                session_count: 5,
                first_seen: "2026-07-07T00:00:00Z".to_string(),
                last_seen: "2026-07-07T00:02:00Z".to_string(),
                last_active_time: "2026-07-07T00:02:00Z".to_string(),
            },
        );

        let mut web_map = BTreeMap::new();
        web_map.insert(
            "google.com".to_string(),
            WebsiteDailyData {
                domain: "google.com".to_string(),
                category: "Search".to_string(),
                total_time_seconds: 60,
                session_count: 2,
                first_seen: "2026-07-07T00:00:00Z".to_string(),
                last_seen: "2026-07-07T00:01:00Z".to_string(),
                last_active_time: "2026-07-07T00:01:00Z".to_string(),
                favicon_url: "https://google.com/favicon.ico".to_string(),
            },
        );

        let mut day_map = BTreeMap::new();
        day_map.insert(
            "2026-07-07".to_string(),
            DayData {
                date: "2026-07-07".to_string(),
                apps: app_map,
                websites: web_map,
                total_switches: 7,
                total_apps: 1,
            },
        );

        let mut week_map = BTreeMap::new();
        week_map.insert(
            "2026-W28".to_string(),
            WeekData { days: day_map },
        );

        let mut month_map = BTreeMap::new();
        month_map.insert(
            "07".to_string(),
            MonthData { weeks: week_map },
        );

        let mut year_map = BTreeMap::new();
        year_map.insert(
            "2026".to_string(),
            YearData { months: month_map },
        );

        ScreenTimeData { years: year_map }
    }

    #[test]
    fn test_build_payload_filtering() {
        let user_id = "test-user";
        let device_id = "test-device";
        let data = dummy_data();

        // Scenario 1: Empty cache -> everything should be included
        let cache_empty = UploadCache::default();
        let payload_full = build_payload(&data, user_id, device_id, &cache_empty);
        
        let snapshots = payload_full.get("snapshots").unwrap().as_array().unwrap();
        assert_eq!(snapshots.len(), 1);
        
        let day_snap = &snapshots[0];
        let apps = day_snap.get("apps").unwrap().as_array().unwrap();
        let websites = day_snap.get("websites").unwrap().as_array().unwrap();
        assert_eq!(apps.len(), 1);
        assert_eq!(websites.len(), 1);

        // Scenario 2: Cache is fully up-to-date -> payload snapshots should be empty
        let mut cache_synced = UploadCache::default();
        let app_key = cache_key_app(user_id, "2026-07-07", device_id, "Chrome");
        cache_synced.uploaded_apps.insert(
            app_key,
            CacheEntry {
                total_time_seconds: 120,
                session_count: 5,
            },
        );
        let web_key = cache_key_website(user_id, "2026-07-07", device_id, "google.com");
        cache_synced.uploaded_websites.insert(
            web_key,
            CacheEntry {
                total_time_seconds: 60,
                session_count: 2,
            },
        );
        let summary_key = cache_key_summary(user_id, "2026-07-07", device_id);
        cache_synced.uploaded_daily_summaries.insert(
            summary_key,
            SummaryCacheEntry {
                total_switches: 7,
                total_apps: 1,
            },
        );

        let payload_empty = build_payload(&data, user_id, device_id, &cache_synced);
        let snapshots_empty = payload_empty.get("snapshots").unwrap().as_array().unwrap();
        assert_eq!(snapshots_empty.len(), 0);

        // Scenario 3: Cache has stale values for apps -> only the modified app should be in the payload
        let mut cache_stale_app = cache_synced.clone();
        // Modify Chrome entry in cache to have less time
        cache_stale_app.uploaded_apps.insert(
            cache_key_app(user_id, "2026-07-07", device_id, "Chrome"),
            CacheEntry {
                total_time_seconds: 100, // Stale time!
                session_count: 5,
            },
        );

        let payload_partial = build_payload(&data, user_id, device_id, &cache_stale_app);
        let snapshots_partial = payload_partial.get("snapshots").unwrap().as_array().unwrap();
        assert_eq!(snapshots_partial.len(), 1);
        
        let partial_day = &snapshots_partial[0];
        let partial_apps = partial_day.get("apps").unwrap().as_array().unwrap();
        let partial_websites = partial_day.get("websites").unwrap().as_array().unwrap();
        
        assert_eq!(partial_apps.len(), 1);
        assert_eq!(partial_apps[0].get("app_name").unwrap().as_str().unwrap(), "Chrome");
        assert_eq!(partial_websites.len(), 0); // Unchanged website is filtered out!
    }
}
