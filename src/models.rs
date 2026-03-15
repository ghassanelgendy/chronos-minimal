//! Data models for screentime and Supabase sync (aligned with SUPABASE_SYNC.md).

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};

/// App-level settings including Supabase sync (Section 2). Serialized as PascalCase for compatibility.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct AppSettings {
    #[serde(default)]
    pub enable_supabase_sync: bool,
    #[serde(default)]
    pub supabase_url: String,
    #[serde(default)]
    pub supabase_anon_key: String,
    #[serde(default)]
    pub supabase_user_id: String,
    #[serde(default = "default_upload_interval")]
    pub supabase_upload_interval_minutes: u32,
    #[serde(default = "default_idle_threshold_seconds")]
    pub idle_threshold_seconds: u32,
}

fn default_upload_interval() -> u32 {
    30
}

fn default_idle_threshold_seconds() -> u32 {
    120
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            enable_supabase_sync: false,
            supabase_url: String::new(),
            supabase_anon_key: String::new(),
            supabase_user_id: String::new(),
            supabase_upload_interval_minutes: 30,
            idle_threshold_seconds: 120,
        }
    }
}

impl AppSettings {
    pub fn upload_interval_minutes_clamped(&self) -> u32 {
        if self.supabase_upload_interval_minutes == 0 {
            30
        } else {
            self.supabase_upload_interval_minutes.min(10080)
        }
    }

    pub fn idle_threshold_seconds_clamped(&self) -> u32 {
        if self.idle_threshold_seconds == 0 {
            120
        } else {
            self.idle_threshold_seconds.clamp(10, 3600)
        }
    }
}

/// Root screentime data (Section 8).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ScreenTimeData {
    pub years: BTreeMap<String, YearData>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct YearData {
    pub months: BTreeMap<String, MonthData>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MonthData {
    pub weeks: BTreeMap<String, WeekData>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WeekData {
    pub days: BTreeMap<String, DayData>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DayData {
    pub date: String, // yyyy-MM-dd
    #[serde(default)]
    pub apps: BTreeMap<String, AppDailyData>,
    #[serde(default)]
    pub websites: BTreeMap<String, WebsiteDailyData>,
    #[serde(default)]
    pub total_switches: u32,
    #[serde(default)]
    pub total_apps: u32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AppDailyData {
    pub app_name: String,
    #[serde(default)]
    pub category: String,
    #[serde(default)]
    pub process_path: String,
    /// Stored as seconds for simplicity; formatted to "hh:mm:ss.fffffff" for API.
    #[serde(default)]
    pub total_time_seconds: u64,
    #[serde(default)]
    pub session_count: u32,
    #[serde(default)]
    pub first_seen: String,
    #[serde(default)]
    pub last_seen: String,
    #[serde(default)]
    pub last_active_time: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WebsiteDailyData {
    pub domain: String,
    #[serde(default)]
    pub category: String,
    #[serde(default)]
    pub total_time_seconds: u64,
    #[serde(default)]
    pub session_count: u32,
    #[serde(default)]
    pub first_seen: String,
    #[serde(default)]
    pub last_seen: String,
    #[serde(default)]
    pub last_active_time: String,
    #[serde(default)]
    pub favicon_url: String,
}

/// In-memory current session: what is focused and for how long (this second).
#[derive(Debug, Clone)]
pub struct CurrentActivity {
    pub app_name: String,
    pub process_path: String,
    pub domain: Option<String>,
}

impl CurrentActivity {
    pub fn is_app_lock(&self) -> bool {
        self.app_name.eq_ignore_ascii_case("AppLock")
    }
}

/// Date key for a given NaiveDate.
pub fn date_key(d: NaiveDate) -> String {
    d.format("%Y-%m-%d").to_string()
}

/// ISO 8601 week of year (e.g. "2025-W11").
pub fn week_key(d: NaiveDate) -> String {
    use chrono::Datelike;
    let iw = d.iso_week();
    format!("{}-W{:02}", iw.year(), iw.week())
}

/// One line for the today summary: name and formatted time.
#[derive(Debug, Clone)]
pub struct TodayAppLine {
    pub name: String,
    pub total_seconds: u64,
}

/// Get today's total seconds and per-app/per-website lines (excluding AppLock). Sorted by time desc.
pub fn get_today_summary(data: &ScreenTimeData) -> (u64, Vec<TodayAppLine>) {
    use chrono::Datelike;
    let today = chrono::Utc::now().date_naive();
    let dk = date_key(today);
    let year = today.year().to_string();
    let month = today.month().to_string();
    let week = week_key(today);
    let mut total_seconds = 0u64;
    let mut lines = Vec::new();
    if let Some(day) = data
        .years
        .get(&year)
        .and_then(|y| y.months.get(&month))
        .and_then(|m| m.weeks.get(&week))
        .and_then(|w| w.days.get(&dk))
    {
        for (_, app) in &day.apps {
            if app.app_name.eq_ignore_ascii_case("AppLock") {
                continue;
            }
            total_seconds += app.total_time_seconds;
            lines.push(TodayAppLine {
                name: app.app_name.clone(),
                total_seconds: app.total_time_seconds,
            });
        }
        for (_, web) in &day.websites {
            total_seconds += web.total_time_seconds;
            lines.push(TodayAppLine {
                name: web.domain.clone(),
                total_seconds: web.total_time_seconds,
            });
        }
    }
    lines.sort_by(|a, b| b.total_seconds.cmp(&a.total_seconds));
    (total_seconds, lines)
}

/// Format seconds as "Xh Ym Zs" for display.
pub fn format_seconds_display(seconds: u64) -> String {
    let h = seconds / 3600;
    let m = (seconds % 3600) / 60;
    let s = seconds % 60;
    if h > 0 {
        format!("{}h {}m {}s", h, m, s)
    } else if m > 0 {
        format!("{}m {}s", m, s)
    } else {
        format!("{}s", s)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SummaryPeriod {
    Today,
    Yesterday,
    ThisWeek,
    LastWeek,
    ThisMonth,
}

impl SummaryPeriod {
    pub fn label(self) -> &'static str {
        match self {
            SummaryPeriod::Today => "Today",
            SummaryPeriod::Yesterday => "Yesterday",
            SummaryPeriod::ThisWeek => "This Week",
            SummaryPeriod::LastWeek => "Last Week",
            SummaryPeriod::ThisMonth => "This Month",
        }
    }

    pub fn next(self) -> SummaryPeriod {
        match self {
            SummaryPeriod::Today => SummaryPeriod::Yesterday,
            SummaryPeriod::Yesterday => SummaryPeriod::ThisWeek,
            SummaryPeriod::ThisWeek => SummaryPeriod::LastWeek,
            SummaryPeriod::LastWeek => SummaryPeriod::ThisMonth,
            SummaryPeriod::ThisMonth => SummaryPeriod::Today,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct SummaryTotals {
    pub total_seconds: u64,
    pub total_switches: u64,
    pub total_apps: u32,
}

#[derive(Debug, Clone)]
pub struct SummaryLine {
    pub name: String,
    pub total_seconds: u64,
    pub session_count: u64,
    pub is_website: bool,
}

fn in_period(date: NaiveDate, period: SummaryPeriod, now: NaiveDate) -> bool {
    use chrono::Datelike;
    match period {
        SummaryPeriod::Today => date == now,
        SummaryPeriod::Yesterday => date == now - chrono::Duration::days(1),
        SummaryPeriod::ThisWeek => {
            let a = date.iso_week();
            let b = now.iso_week();
            a.year() == b.year() && a.week() == b.week()
        }
        SummaryPeriod::LastWeek => {
            let lw = now - chrono::Duration::weeks(1);
            let a = date.iso_week();
            let b = lw.iso_week();
            a.year() == b.year() && a.week() == b.week()
        }
        SummaryPeriod::ThisMonth => date.year() == now.year() && date.month() == now.month(),
    }
}

/// Build totals + ranked lines for a requested period.
pub fn summarize_period(
    data: &ScreenTimeData,
    period: SummaryPeriod,
    websites_only: bool,
) -> (SummaryTotals, Vec<SummaryLine>) {
    let now = chrono::Utc::now().date_naive();
    let mut totals = SummaryTotals::default();
    let mut app_names = HashSet::<String>::new();
    let mut rollup = HashMap::<(bool, String), (u64, u64)>::new();

    for year in data.years.values() {
        for month in year.months.values() {
            for week in month.weeks.values() {
                for day in week.days.values() {
                    let Ok(date) = NaiveDate::parse_from_str(&day.date, "%Y-%m-%d") else {
                        continue;
                    };
                    if !in_period(date, period, now) {
                        continue;
                    }

                    totals.total_switches = totals
                        .total_switches
                        .saturating_add(day.total_switches as u64);

                    if !websites_only {
                        for app in day.apps.values() {
                            if app.app_name.eq_ignore_ascii_case("AppLock") {
                                continue;
                            }
                            totals.total_seconds =
                                totals.total_seconds.saturating_add(app.total_time_seconds);
                            app_names.insert(app.app_name.to_lowercase());

                            let key = (false, app.app_name.clone());
                            let entry = rollup.entry(key).or_insert((0, 0));
                            entry.0 = entry.0.saturating_add(app.total_time_seconds);
                            entry.1 = entry.1.saturating_add(app.session_count as u64);
                        }
                    }

                    for site in day.websites.values() {
                        totals.total_seconds =
                            totals.total_seconds.saturating_add(site.total_time_seconds);
                        let key = (true, site.domain.clone());
                        let entry = rollup.entry(key).or_insert((0, 0));
                        entry.0 = entry.0.saturating_add(site.total_time_seconds);
                        entry.1 = entry.1.saturating_add(site.session_count as u64);
                    }
                }
            }
        }
    }

    totals.total_apps = app_names.len() as u32;

    let mut lines: Vec<SummaryLine> = rollup
        .into_iter()
        .map(|((is_website, name), (total_seconds, session_count))| SummaryLine {
            name,
            total_seconds,
            session_count,
            is_website,
        })
        .collect();
    lines.sort_by(|a, b| b.total_seconds.cmp(&a.total_seconds));
    (totals, lines)
}
