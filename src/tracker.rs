//! Windows foreground window tracking (polling GetForegroundWindow + process info).

#![cfg(windows)]

use crate::models::{CurrentActivity, date_key, week_key};
use crate::models::{AppDailyData, DayData, ScreenTimeData, WebsiteDailyData};
use crate::models::AppSettings;
use crate::category::{get_category_for_app, get_category_for_website};
use chrono::{Datelike, NaiveDate, Utc};
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

/// Poll interval (seconds). Align with spec: 1–5 s.
const POLL_INTERVAL_SECS: u64 = 1;

fn normalize_domain(input: &str) -> Option<String> {
    let trimmed = input.trim().trim_matches('"').trim_matches('[').trim_matches(']');
    if trimmed.is_empty() {
        return None;
    }

    let without_scheme = if let Some(idx) = trimmed.find("://") {
        &trimmed[(idx + 3)..]
    } else {
        trimmed
    };
    let host_only = without_scheme
        .split('/')
        .next()
        .unwrap_or(without_scheme)
        .split('?')
        .next()
        .unwrap_or(without_scheme)
        .split('#')
        .next()
        .unwrap_or(without_scheme)
        .split(':')
        .next()
        .unwrap_or(without_scheme)
        .trim()
        .trim_end_matches('.');

    if host_only.is_empty() || host_only.contains(' ') || !host_only.contains('.') {
        return None;
    }

    let lowered = host_only.to_lowercase();
    let canonical = lowered.strip_prefix("www.").unwrap_or(&lowered);
    if canonical
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-')
    {
        Some(canonical.to_string())
    } else {
        None
    }
}

fn favicon_url_for_domain(domain: &str) -> String {
    format!("https://www.google.com/s2/favicons?sz=64&domain={}", domain)
}

/// Walk all descendant windows of `root` looking for one whose class name equals `target`.
/// Uses iterative DFS via a stack to avoid recursion depth issues.
#[cfg(windows)]
fn find_child_by_class(
    root: windows::Win32::Foundation::HWND,
    target: &str,
) -> Option<windows::Win32::Foundation::HWND> {
    use windows::Win32::UI::WindowsAndMessaging::{GetClassNameW, GetWindow, GW_CHILD, GW_HWNDNEXT};
    let target_u16: Vec<u16> = target.encode_utf16().collect();
    let mut stack = vec![root];
    let mut total = 0usize;
    while let Some(parent) = stack.pop() {
        total += 1;
        if total > 600 {
            break;
        }
        let first = unsafe { GetWindow(parent, GW_CHILD).ok() };
        let Some(first) = first else { continue };
        if first.is_invalid() {
            continue;
        }
        let mut sibling = first;
        let mut sib_count = 0usize;
        loop {
            sib_count += 1;
            if sib_count > 200 {
                break;
            }
            let mut buf = [0u16; 256];
            let len = unsafe { GetClassNameW(sibling, &mut buf) } as usize;
            if len == target_u16.len() && buf[..len] == target_u16[..] {
                return Some(sibling);
            }
            stack.push(sibling);
            sibling = match unsafe { GetWindow(sibling, GW_HWNDNEXT).ok() } {
                Some(h) if !h.is_invalid() => h,
                _ => break,
            };
        }
    }
    None
}

/// Read the actual URL from a Chromium-based browser's omnibox (Chrome, Edge, Brave, Opera).
/// Falls back to None if the omnibox control is not found (Firefox etc.).
#[cfg(windows)]
fn get_browser_url(hwnd: windows::Win32::Foundation::HWND) -> Option<String> {
    use windows::Win32::UI::WindowsAndMessaging::GetWindowTextW;
    let omnibox = find_child_by_class(hwnd, "Chrome_OmniboxView")?;
    let mut buf = [0u16; 2048];
    let len = unsafe { GetWindowTextW(omnibox, &mut buf) } as usize;
    if len == 0 {
        return None;
    }
    let raw = String::from_utf16_lossy(&buf[..len]);
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }
    // Omnibox may contain a full URL or a bare domain; normalise either way.
    normalize_domain(raw).or_else(|| normalize_domain(&format!("https://{}", raw)))
}

/// Extract domain from window title (e.g. "GitHub - Chrome" -> github.com).
fn domain_from_title(title: &str) -> Option<String> {
    let t = title.trim();
    if t.is_empty() {
        return None;
    }
    if let Some(d) = normalize_domain(t) {
        return Some(d);
    }

    // Common pattern: "Page Title - Browser" or "Title | Site"
    for sep in [" - ", " | ", " — "] {
        if let Some(part) = t.split(sep).next() {
            let s = part.trim();
            if let Some(d) = normalize_domain(s) {
                return Some(d);
            }
        }
    }
    None
}

/// Check if process is typically a browser (for URL extraction from title).
fn is_browser_process(app_name: &str) -> bool {
    let a = app_name.to_lowercase();
    a.contains("chrome") || a.contains("msedge") || a.contains("firefox") || a.contains("opera")
        || a.contains("brave") || a == "edge" || a.contains("browser")
}

#[cfg(windows)]
fn get_foreground_info() -> Option<CurrentActivity> {
    use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowTextW, GetWindowThreadProcessId};

    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.is_invalid() {
            return None;
        }
        let mut pid: u32 = 0;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if pid == 0 {
            return None;
        }
        let mut title_buf = [0u16; 512];
        let len = GetWindowTextW(hwnd, &mut title_buf);
        let window_title = String::from_utf16_lossy(&title_buf[..(len as usize).min(512)]).trim().to_string();

        let process_path = open_process_and_get_path(pid).unwrap_or_default();
        let app_name = app_name_from_path(&process_path);
        let is_browser = is_browser_process(&app_name);
        let domain = if is_browser {
            // Primary: read the real URL from the address bar (Chrome/Edge omnibox).
            // Fallback: parse the window title (works for some sites that include domain).
            get_browser_url(hwnd).or_else(|| domain_from_title(&window_title))
        } else {
            None
        };
        Some(CurrentActivity {
            app_name,
            process_path,
            domain,
        })
    }
}

#[cfg(windows)]
unsafe fn open_process_and_get_path(pid: u32) -> Option<String> {
    use windows::core::PWSTR;
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Threading::QueryFullProcessImageNameW;
    use windows::Win32::System::Threading::{OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION};

    let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
    let mut buf = [0u16; 260];
    let mut size = buf.len() as u32;
    QueryFullProcessImageNameW(
        handle,
        PROCESS_NAME_WIN32,
        PWSTR(buf.as_mut_ptr()),
        &mut size,
    )
    .ok()?;
    let _ = CloseHandle(handle);
    let s = String::from_utf16_lossy(&buf[..size.min(260) as usize]);
    Some(s)
}

#[cfg(windows)]
fn app_name_from_path(path: &str) -> String {
    std::path::Path::new(path)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(path)
        .to_string()
}

#[cfg(windows)]
fn get_idle_seconds() -> Option<u64> {
    use windows::Win32::System::SystemInformation::GetTickCount64;
    use windows::Win32::UI::Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO};

    unsafe {
        let mut last_input = LASTINPUTINFO {
            cbSize: std::mem::size_of::<LASTINPUTINFO>() as u32,
            dwTime: 0,
        };
        if !GetLastInputInfo(&mut last_input).as_bool() {
            return None;
        }
        let now_ms = GetTickCount64();
        let idle_ms = now_ms.saturating_sub(last_input.dwTime as u64);
        Some(idle_ms / 1000)
    }
}

/// Ensure year/month/week/day exist in data for the given date; return mutable ref to DayData.
fn ensure_day(data: &mut ScreenTimeData, date: NaiveDate) -> &mut DayData {
    let y = date.year().to_string();
    let m = date.month().to_string();
    let w = week_key(date);
    let dk = date_key(date);
    data.years
        .entry(y.clone())
        .or_default()
        .months
        .entry(m.clone())
        .or_default()
        .weeks
        .entry(w.clone())
        .or_default()
        .days
        .entry(dk.clone())
        .or_insert_with(|| DayData {
            date: dk,
            apps: BTreeMap::new(),
            websites: BTreeMap::new(),
            total_switches: 0,
            total_apps: 0,
        })
}

fn activity_switch_key(activity: &CurrentActivity) -> Option<String> {
    if activity.app_name.is_empty() || activity.is_app_lock() {
        return None;
    }
    if let Some(domain) = &activity.domain {
        let d = domain.trim();
        if !d.is_empty() {
            return Some(format!("web:{}", d.to_lowercase()));
        }
    }
    Some(format!("app:{}", activity.app_name.to_lowercase()))
}

fn increment_switch_count(data: &mut ScreenTimeData, date: NaiveDate) {
    let day = ensure_day(data, date);
    day.total_switches = day.total_switches.saturating_add(1);
}

fn increment_session_count(
    data: &mut ScreenTimeData,
    activity: &CurrentActivity,
    now: chrono::DateTime<Utc>,
) {
    let day = ensure_day(data, now.date_naive());
    let iso = now.to_rfc3339();

    if let Some(domain) = &activity.domain {
        let d = domain.trim();
        if !d.is_empty() {
            if let Some(site) = day.websites.get_mut(d) {
                site.session_count = site.session_count.saturating_add(1);
                if site.first_seen.is_empty() {
                    site.first_seen = iso;
                }
                return;
            }
        }
    }

    if let Some(app) = day.apps.get_mut(&activity.app_name) {
        app.session_count = app.session_count.saturating_add(1);
        if app.first_seen.is_empty() {
            app.first_seen = iso;
        }
    }
}

/// Record one tick of activity (e.g. 3 seconds) for the current day.
pub fn record_activity(
    data: &mut ScreenTimeData,
    activity: &CurrentActivity,
    now: chrono::DateTime<Utc>,
) {
    if activity.app_name.is_empty() || activity.is_app_lock() {
        return;
    }
    let date = now.date_naive();
    let day = ensure_day(data, date);
    let iso = now.to_rfc3339();

    if let Some(ref domain) = activity.domain {
        if !domain.is_empty() {
            let entry = day.websites.entry(domain.clone()).or_insert_with(|| WebsiteDailyData {
                domain: domain.clone(),
                category: get_category_for_website(domain),
                total_time_seconds: 0,
                session_count: 0,
                first_seen: iso.clone(),
                last_seen: iso.clone(),
                last_active_time: iso.clone(),
                favicon_url: favicon_url_for_domain(domain),
            });
            entry.total_time_seconds += POLL_INTERVAL_SECS;
            entry.last_seen = iso.clone();
            entry.last_active_time = iso.clone();
            if entry.favicon_url.trim().is_empty() {
                entry.favicon_url = favicon_url_for_domain(domain);
            }
            return;
        }
    }

    let app_key = activity.app_name.clone();
    let entry = day.apps.entry(app_key.clone()).or_insert_with(|| AppDailyData {
        app_name: activity.app_name.clone(),
        category: get_category_for_app(&activity.app_name),
        process_path: activity.process_path.clone(),
        total_time_seconds: 0,
        session_count: 0,
        first_seen: iso.clone(),
        last_seen: iso.clone(),
        last_active_time: iso.clone(),
    });
    entry.total_time_seconds += POLL_INTERVAL_SECS;
    entry.last_seen = iso.clone();
    entry.last_active_time = iso.clone();
    day.total_apps = day.apps.len() as u32;
}

/// Run the tracker loop: poll foreground window, merge into shared data, save periodically.
pub fn run_tracker_loop(
    data: Arc<std::sync::Mutex<ScreenTimeData>>,
    settings: Arc<std::sync::Mutex<AppSettings>>,
    tracking_enabled: Arc<AtomicBool>,
    running: Arc<AtomicBool>,
) {
    use crate::storage::{load_screen_time_data, save_screen_time_data};

    // Load initial data
    {
        let mut guard = data.lock().unwrap();
        *guard = load_screen_time_data();
    }
    let mut last_switch_key: Option<String> = None;
    let mut save_counter: u32 = 0;
    while running.load(Ordering::SeqCst) {
        if !tracking_enabled.load(Ordering::SeqCst) {
            last_switch_key = None;
            std::thread::sleep(Duration::from_secs(1));
            continue;
        }

        let idle_threshold_seconds = {
            let s = settings.lock().unwrap();
            s.idle_threshold_seconds_clamped() as u64
        };
        if let Some(idle_seconds) = get_idle_seconds() {
            if idle_seconds >= idle_threshold_seconds {
                // Treat idle/lock periods as non-trackable; next active event starts a new session.
                last_switch_key = None;
                std::thread::sleep(Duration::from_secs(POLL_INTERVAL_SECS));
                continue;
            }
        }

        if let Some(activity) = get_foreground_info() {
            let now = Utc::now();
            let current_key = activity_switch_key(&activity);
            {
                let mut guard = data.lock().unwrap();
                record_activity(&mut guard, &activity, now);
                if let Some(ref key) = current_key {
                    if let Some(ref prev_key) = last_switch_key {
                        if prev_key != key {
                            increment_session_count(&mut guard, &activity, now);
                            increment_switch_count(&mut guard, now.date_naive());
                        }
                    } else {
                        // First active item seen after app start/pause counts as session start.
                        increment_session_count(&mut guard, &activity, now);
                    }
                }
            }
            if let Some(key) = current_key {
                last_switch_key = Some(key);
            } else {
                last_switch_key = None;
            }
        } else {
            // No valid foreground app - avoid stitching this gap into a continuous session.
            last_switch_key = None;
        }
        save_counter += 1;
        if save_counter >= 20 {
            save_counter = 0;
            let to_save = data.lock().unwrap().clone();
            save_screen_time_data(&to_save);
        }
        std::thread::sleep(Duration::from_secs(POLL_INTERVAL_SECS));
    }
    let to_save = data.lock().unwrap().clone();
    save_screen_time_data(&to_save);
}
