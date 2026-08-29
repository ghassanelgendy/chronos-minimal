//! Active window tracking and idle monitoring (Windows Win32 API & Linux AT-SPI / GNOME / X11).

use crate::models::{CurrentActivity, date_key, week_key};
use crate::models::{AppDailyData, DayData, ScreenTimeData, WebsiteDailyData};
use crate::models::AppSettings;
use crate::category::{get_category_for_app, get_category_for_website};
use chrono::{Datelike, Local, NaiveDate, Utc};
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

#[cfg(windows)]
use windows::core::VARIANT;
#[cfg(windows)]
use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED};

#[cfg(not(windows))]
use x11rb::protocol::xproto::ConnectionExt;

/// Poll interval (seconds). Align with spec: 1–5 s.
pub const POLL_INTERVAL_SECS: u64 = 1;

pub static IS_CHRONOS_FOCUSED: AtomicBool = AtomicBool::new(false);

const FILE_EXTENSIONS_BLACKLIST: &[&str] = &[
    "txt", "gz", "tar", "zip", "7z", "rar", "bz2", "xz", "zst",
    "pdf", "doc", "docx", "xls", "xlsx", "ppt", "pptx", "odt", "ods", "odp",
    "json", "yaml", "yml", "toml", "xml", "csv", "tsv", "log", "md", "markdown",
    "py", "c", "cpp", "cc", "cxx", "h", "hpp", "go", "java", "kt", "scala",
    "js", "ts", "jsx", "tsx", "html", "htm", "css", "scss", "sass", "less",
    "sh", "bash", "zsh", "fish", "bat", "cmd", "ps1",
    "png", "jpg", "jpeg", "gif", "webp", "svg", "ico", "bmp", "tiff",
    "mp3", "mp4", "mkv", "avi", "wav", "flac", "ogg", "m4a", "mov", "webm",
    "exe", "dll", "so", "dylib", "bin", "deb", "rpm", "apk", "iso", "img",
    "conf", "cfg", "ini", "env", "lock", "sql", "db", "sqlite", "bak", "tmp",
];

fn is_valid_ipv4(host: &str) -> bool {
    let parts: Vec<&str> = host.split('.').collect();
    if parts.len() != 4 {
        return false;
    }
    for part in parts {
        if part.is_empty() || part.len() > 3 {
            return false;
        }
        match part.parse::<u8>() {
            Ok(_) => {}
            Err(_) => return false,
        }
    }
    true
}

fn normalize_domain(input: &str) -> Option<String> {
    let trimmed = input.trim().trim_matches('"').trim_matches('[').trim_matches(']').trim_matches('\'').trim_matches('`');
    if trimmed.is_empty() {
        return None;
    }

    let without_scheme = if let Some(idx) = trimmed.find("://") {
        &trimmed[(idx + 3)..]
    } else {
        trimmed
    };

    if without_scheme.starts_with("localhost") {
        let host_and_port = without_scheme
            .split('/')
            .next()
            .unwrap_or(without_scheme)
            .split('?')
            .next()
            .unwrap_or(without_scheme)
            .split('#')
            .next()
            .unwrap_or(without_scheme)
            .trim();
        if host_and_port.starts_with("localhost") && !host_and_port.contains(' ') {
            return Some(host_and_port.to_lowercase());
        }
    }

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

    // Allow valid IPv4 address (e.g. 192.168.1.1)
    if is_valid_ipv4(canonical) {
        return Some(canonical.to_string());
    }

    let segments: Vec<&str> = canonical.split('.').collect();
    if segments.len() < 2 {
        return None;
    }

    let tld = segments.last().unwrap();
    // Reject common file extensions
    if FILE_EXTENSIONS_BLACKLIST.contains(tld) {
        return None;
    }

    // TLD must consist of 2..24 ASCII alphabetic characters
    if tld.len() < 2 || tld.len() > 24 || !tld.chars().all(|c| c.is_ascii_alphabetic()) {
        return None;
    }

    // Check all segments
    for seg in &segments {
        if seg.is_empty()
            || seg.starts_with('-')
            || seg.ends_with('-')
            || !seg.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
        {
            return None;
        }
    }

    Some(canonical.to_string())
}

fn favicon_url_for_domain(domain: &str) -> String {
    format!("https://www.google.com/s2/favicons?sz=64&domain={}", domain)
}

/// Walk all descendant windows of `root` looking for one whose class name equals `target`.
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
#[cfg(windows)]
fn get_browser_url(hwnd: windows::Win32::Foundation::HWND) -> Option<String> {
    use windows::Win32::UI::WindowsAndMessaging::GetWindowTextW;
    for class in ["Chrome_OmniboxView", "Chrome_AutocompleteEditView"] {
        if let Some(omnibox) = find_child_by_class(hwnd, class) {
            let mut buf = [0u16; 2048];
            let len = unsafe { GetWindowTextW(omnibox, &mut buf) } as usize;
            if len > 0 {
                let raw = String::from_utf16_lossy(&buf[..len]);
                let raw = raw.trim();
                if !raw.is_empty() {
                    if let Some(domain) = normalize_domain(raw)
                        .or_else(|| normalize_domain(&format!("https://{}", raw)))
                    {
                        return Some(domain);
                    }
                }
            }
        }
    }
    get_browser_url_via_uia(hwnd)
}

/// Use UI Automation to read the browser's address bar value.
#[cfg(windows)]
fn get_browser_url_via_uia(hwnd: windows::Win32::Foundation::HWND) -> Option<String> {
    use windows::Win32::UI::Accessibility::*;
    unsafe {
        let coinit = CoInitializeEx(None, COINIT_MULTITHREADED);
        let automation: IUIAutomation = CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER).ok()?;
        let root = automation.ElementFromHandle(hwnd).ok()?;
        let value: VARIANT = (UIA_EditControlTypeId.0 as i32).into();
        let cond = automation
            .CreatePropertyCondition(UIA_ControlTypePropertyId, &value)
            .ok()?;
        let edits = root.FindAll(TreeScope_Subtree, &cond).ok()?;
        let length = edits.Length().unwrap_or(0);
        for i in 0..length {
            if let Ok(el) = edits.GetElement(i) {
                let name = el.CurrentName().unwrap_or_default().to_string().to_lowercase();
                let looks_like_address = name.contains("address") || name.contains("search") || name.contains("omnibox");
                if let Ok(vp) = el.GetCurrentPatternAs::<IUIAutomationValuePattern>(UIA_ValuePatternId) {
                    if let Ok(val_bstr) = vp.CurrentValue() {
                        let val = val_bstr.to_string();
                        if let Some(d) = normalize_domain(val.as_str())
                            .or_else(|| normalize_domain(&format!("https://{}", val)))
                        {
                            if coinit.is_ok() {
                                CoUninitialize();
                            }
                            return Some(d);
                        }
                    }
                }
                if looks_like_address {
                    continue;
                }
            }
        }
        if coinit.is_ok() {
            CoUninitialize();
        }
        None
    }
}

fn strip_browser_suffix(title: &str) -> &str {
    let t = title.trim();
    let browser_suffixes = [
        " - Google Chrome",
        " — Google Chrome",
        " | Google Chrome",
        " - Chromium",
        " — Chromium",
        " | Chromium",
        " - Mozilla Firefox",
        " — Mozilla Firefox",
        " | Mozilla Firefox",
        " - Firefox",
        " — Firefox",
        " - Brave",
        " — Brave",
        " | Brave",
        " - Microsoft​ Edge",
        " - Microsoft Edge",
        " — Microsoft Edge",
        " - Opera",
        " — Opera",
        " - Vivaldi",
        " — Vivaldi",
        " - Zen Browser",
        " — Zen Browser",
        " - Tor Browser",
        " — Tor Browser",
        " - Arc",
        " — Arc",
        " - Safari",
        " — Safari",
        " - Waterfox",
        " — Waterfox",
        " - LibreWolf",
        " — LibreWolf",
        " - Floorp",
        " — Floorp",
        " - Web",
        " — Web",
        " - Epiphany",
        " — Epiphany",
        " - Yandex",
        " — Yandex",
    ];

    for suffix in browser_suffixes {
        if let Some(stripped) = t.strip_suffix(suffix) {
            return stripped.trim();
        }
    }
    t
}

pub fn is_internal_browser_page(clean_title: &str) -> bool {
    let lower = clean_title.to_lowercase();
    lower == "new tab"
        || lower == "settings"
        || lower == "downloads"
        || lower == "extensions"
        || lower == "bookmarks"
        || lower == "history"
        || lower == "about:blank"
        || lower.starts_with("chrome://")
        || lower.starts_with("edge://")
        || lower.starts_with("brave://")
        || lower.starts_with("about:")
        || lower.starts_with("opera://")
        || lower.starts_with("vivaldi://")
}

fn is_github_repo_title(segment: &str) -> bool {
    let s = segment.trim();
    let candidate = s.split(':').next().unwrap_or(s).split(" · ").next().unwrap_or(s).trim();
    if let Some((user, rest)) = candidate.split_once('/') {
        let repo = rest.split_whitespace().next().unwrap_or(rest).trim_matches(':');
        if !user.is_empty()
            && !repo.is_empty()
            && !user.contains(' ')
            && !repo.contains(' ')
            && user.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
            && repo.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
        {
            return true;
        }
    }
    false
}

fn brand_token_to_domain(token: &str) -> Option<&'static str> {
    let t = token.trim().to_lowercase();
    match t.as_str() {
        "github" => Some("github.com"),
        "gitlab" => Some("gitlab.com"),
        "bitbucket" => Some("bitbucket.org"),
        "youtube" | "youtube music" | "youtube studio" => Some("youtube.com"),
        "gemini" | "google gemini" => Some("gemini.google.com"),
        "chatgpt" | "openai" => Some("chatgpt.com"),
        "claude" | "claude ai" | "anthropic" => Some("claude.ai"),
        "perplexity" | "perplexity ai" => Some("perplexity.ai"),
        "deepseek" | "deepseek ai" => Some("deepseek.com"),
        "copilot" | "github copilot" | "microsoft copilot" => Some("copilot.microsoft.com"),
        "grok" | "x.ai" => Some("x.ai"),
        "google search" | "google" => Some("google.com"),
        "google meet" | "meet" => Some("meet.google.com"),
        "google docs" | "docs" => Some("docs.google.com"),
        "google drive" | "drive" | "my drive" => Some("drive.google.com"),
        "google sheets" | "sheets" => Some("sheets.google.com"),
        "google slides" | "slides" => Some("slides.google.com"),
        "gmail" | "google mail" => Some("mail.google.com"),
        "google calendar" | "calendar" => Some("calendar.google.com"),
        "google maps" | "maps" => Some("maps.google.com"),
        "google photos" | "photos" => Some("photos.google.com"),
        "google translate" | "translate" => Some("translate.google.com"),
        "google cloud" | "console.cloud.google" => Some("console.cloud.google.com"),
        "supabase" => Some("supabase.com"),
        "vercel" => Some("vercel.com"),
        "netlify" => Some("netlify.app"),
        "aws" | "amazon web services" | "aws management console" => Some("aws.amazon.com"),
        "azure" | "azure portal" | "portal.azure" => Some("portal.azure.com"),
        "cloudflare" => Some("cloudflare.com"),
        "digitalocean" => Some("digitalocean.com"),
        "heroku" => Some("heroku.com"),
        "railway" | "railway.app" => Some("railway.app"),
        "render" | "render.com" => Some("render.com"),
        "fly.io" => Some("fly.io"),
        "replit" => Some("replit.com"),
        "codesandbox" => Some("codesandbox.io"),
        "codepen" => Some("codepen.io"),
        "jsfiddle" => Some("jsfiddle.net"),
        "stackoverflow" | "stack overflow" => Some("stackoverflow.com"),
        "superuser" | "super user" => Some("superuser.com"),
        "serverfault" | "server fault" => Some("serverfault.com"),
        "stack exchange" | "stackexchange" => Some("stackexchange.com"),
        "hacker news" | "y combinator" | "ycombinator" => Some("news.ycombinator.com"),
        "product hunt" | "producthunt" => Some("producthunt.com"),
        "hugging face" | "huggingface" => Some("huggingface.co"),
        "kaggle" => Some("kaggle.com"),
        "notion" => Some("notion.so"),
        "figma" => Some("figma.com"),
        "canva" => Some("canva.com"),
        "miro" => Some("miro.com"),
        "trello" => Some("trello.com"),
        "asana" => Some("asana.com"),
        "linear" => Some("linear.app"),
        "jira" | "atlassian" => Some("atlassian.net"),
        "confluence" => Some("atlassian.net"),
        "monday" | "monday.com" => Some("monday.com"),
        "clickup" => Some("clickup.com"),
        "airtable" => Some("airtable.com"),
        "slack" => Some("slack.com"),
        "discord" => Some("discord.com"),
        "microsoft teams" | "teams" => Some("teams.microsoft.com"),
        "zoom" => Some("zoom.us"),
        "dropbox" => Some("dropbox.com"),
        "spotify" => Some("spotify.com"),
        "netflix" => Some("netflix.com"),
        "twitch" => Some("twitch.tv"),
        "prime video" | "amazon prime video" => Some("primevideo.com"),
        "disney+" | "disney plus" => Some("disneyplus.com"),
        "reddit" => Some("reddit.com"),
        "twitter" | "x" => Some("x.com"),
        "linkedin" => Some("linkedin.com"),
        "facebook" => Some("facebook.com"),
        "instagram" => Some("instagram.com"),
        "threads" => Some("threads.net"),
        "tiktok" => Some("tiktok.com"),
        "pinterest" => Some("pinterest.com"),
        "whatsapp" | "whatsapp web" => Some("web.whatsapp.com"),
        "telegram" | "telegram web" => Some("web.telegram.org"),
        "bluesky" | "bsky" => Some("bsky.app"),
        "amazon" => Some("amazon.com"),
        "ebay" => Some("ebay.com"),
        "aliexpress" => Some("aliexpress.com"),
        "walmart" => Some("walmart.com"),
        "target" => Some("target.com"),
        "wikipedia" => Some("wikipedia.org"),
        "medium" => Some("medium.com"),
        "substack" => Some("substack.com"),
        "dev.to" => Some("dev.to"),
        "arxiv" => Some("arxiv.org"),
        "duckduckgo" => Some("duckduckgo.com"),
        "bing" => Some("bing.com"),
        "fast.com" => Some("fast.com"),
        "speedtest" => Some("speedtest.net"),
        _ => None,
    }
}

/// Extract domain from window title (e.g. "Gemini - Chromium" -> gemini.google.com, "GitHub - Chrome" -> github.com).
pub fn domain_from_title(title: &str) -> Option<String> {
    let raw_trimmed = title.trim();
    if raw_trimmed.is_empty() {
        return None;
    }

    let clean_title = strip_browser_suffix(raw_trimmed);

    // If it's a blank or internal browser page (e.g. New Tab, Settings), it's not a website.
    if is_internal_browser_page(clean_title) {
        return None;
    }

    // 1. Direct domain check on the cleaned title or full title
    if let Some(d) = normalize_domain(clean_title) {
        return Some(d);
    }
    if let Some(d) = normalize_domain(raw_trimmed) {
        return Some(d);
    }

    // 2. Check if clean_title as a whole is a known brand (e.g. "Gemini", "ChatGPT", "Claude", "GitHub")
    if let Some(d) = brand_token_to_domain(clean_title) {
        return Some(d.to_string());
    }

    // 3. GitHub repository / issue / PR pattern (e.g. "ghassanelgendy/chronos-minimal: description")
    if is_github_repo_title(clean_title) || is_github_repo_title(raw_trimmed) {
        return Some("github.com".to_string());
    }

    // 4. Split clean_title and raw_trimmed by standard web title delimiters: " · ", " - ", " — ", " | ", " : ", " • "
    let separators = [" · ", " - ", " — ", " | ", " : ", " • "];
    for sep in separators {
        let parts: Vec<&str> = clean_title.split(sep).map(|s| s.trim()).filter(|s| !s.is_empty()).collect();
        if parts.len() >= 2 {
            // Check rightmost part first (standard site suffix, e.g. "Page - YouTube", "Issues · GitHub")
            if let Some(right) = parts.last() {
                if is_github_repo_title(right) {
                    return Some("github.com".to_string());
                }
                if let Some(d) = normalize_domain(right) {
                    return Some(d);
                }
                if let Some(d) = brand_token_to_domain(right) {
                    return Some(d.to_string());
                }
                if right.to_lowercase().starts_with("r/") {
                    return Some("reddit.com".to_string());
                }
            }

            // Check leftmost part next (standard site prefix, e.g. "GitHub - repo", "Supabase | Dashboard")
            if let Some(left) = parts.first() {
                if is_github_repo_title(left) {
                    return Some("github.com".to_string());
                }
                if let Some(d) = normalize_domain(left) {
                    return Some(d);
                }
                if let Some(d) = brand_token_to_domain(left) {
                    return Some(d.to_string());
                }
                if left.to_lowercase().starts_with("r/") {
                    return Some("reddit.com".to_string());
                }
            }

            // Check all other middle parts for explicit domains or brand tokens
            for part in &parts {
                if is_github_repo_title(part) {
                    return Some("github.com".to_string());
                }
                if let Some(d) = normalize_domain(part) {
                    return Some(d);
                }
                if let Some(d) = brand_token_to_domain(part) {
                    return Some(d.to_string());
                }
            }
        }
    }

    // 5. Check if title starts with standard prefixes: "GitHub", "YouTube", "Reddit", etc.
    let lower = clean_title.to_lowercase();
    if lower.starts_with("github") || lower.ends_with("github") {
        return Some("github.com".to_string());
    }
    if lower.starts_with("youtube") || lower.ends_with("youtube") {
        return Some("youtube.com".to_string());
    }
    if lower.starts_with("reddit") || lower.ends_with("reddit") || lower.starts_with("r/") {
        return Some("reddit.com".to_string());
    }
    if lower.starts_with("google search") || lower.ends_with("google search") {
        return Some("google.com".to_string());
    }
    if lower.starts_with("meet - ") || lower.starts_with("google meet") {
        return Some("meet.google.com".to_string());
    }
    if lower.starts_with("gmail") || lower.contains(" - gmail") || lower.starts_with("inbox (") {
        return Some("mail.google.com".to_string());
    }
    if lower.starts_with("supabase") || lower.ends_with("supabase") {
        return Some("supabase.com".to_string());
    }
    if lower.starts_with("chatgpt") || lower.ends_with("chatgpt") {
        return Some("chatgpt.com".to_string());
    }
    if lower.starts_with("claude") || lower.ends_with("claude") {
        return Some("claude.ai".to_string());
    }
    if lower.starts_with("gemini") || lower.ends_with("gemini") {
        return Some("gemini.google.com".to_string());
    }
    if lower.starts_with("perplexity") || lower.ends_with("perplexity") {
        return Some("perplexity.ai".to_string());
    }
    if lower.starts_with("deepseek") || lower.ends_with("deepseek") {
        return Some("deepseek.com".to_string());
    }

    // 6. Scan words / tokens in title for explicit domain formats (e.g. "github.com", "docs.rs", "youtu.be")
    for word in clean_title.split_whitespace() {
        let cleaned = word.trim_matches(|c: char| !c.is_alphanumeric() && c != '.' && c != '-');
        if let Some(d) = normalize_domain(cleaned) {
            return Some(d);
        }
    }

    None
}

/// Like domain_from_title, but guarantees that ANY browser window is tracked as a website —
/// even if the real domain cannot be identified. Falls back to the cleaned page title
/// (browser suffix stripped) so time is NEVER silently counted as the browser app name.
pub fn domain_from_browser_title(title: &str) -> Option<String> {
    let raw_trimmed = title.trim();
    if raw_trimmed.is_empty() {
        return None;
    }

    let clean_title = strip_browser_suffix(raw_trimmed);

    // Internal browser pages (New Tab, Settings…) should NOT be tracked as websites.
    if is_internal_browser_page(clean_title) {
        return None;
    }

    // Try to extract a real domain first.
    if let Some(d) = domain_from_title(raw_trimmed) {
        return Some(d);
    }

    // Fallback: cleaned page title as a label for unknown sites.
    // e.g. "My Dashboard - Chromium" → "My Dashboard"
    // e.g. "Checkout — Brave" → "Checkout"
    let label = clean_title.trim();
    if label.is_empty() {
        return None;
    }

    Some(label.to_string())
}


pub fn is_browser_process(app_name: &str) -> bool {
    let a = app_name.to_lowercase();
    a.contains("chrome")
        || a.contains("chromium")
        || a.contains("msedge")
        || a.contains("edge")
        || a.contains("firefox")
        || a.contains("opera")
        || a.contains("brave")
        || a.contains("vivaldi")
        || a.contains("arc")
        || a.contains("zen")
        || a.contains("safari")
        || a.contains("tor")
        || a.contains("waterfox")
        || a.contains("librewolf")
        || a.contains("floorp")
        || a.contains("epiphany")
        || a.contains("browser")
        || a.contains("navigator")
        || a.contains("yandex")
        || a.contains("sidekick")
        || a.contains("orion")
        || a.contains("qutebrowser")
        || a.contains("falkon")
        || a.contains("ladybird")
        || a.contains("mullvad")
}

#[cfg(windows)]
pub(crate) fn get_foreground_info() -> Option<CurrentActivity> {
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
            get_browser_url(hwnd).or_else(|| domain_from_browser_title(&window_title))
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
pub(crate) fn get_idle_seconds() -> Option<u64> {
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

#[cfg(not(windows))]
fn app_name_from_path(path: &str) -> String {
    let p = std::path::Path::new(path);
    if let Some(stem) = p.file_stem().and_then(|s| s.to_str()) {
        stem.to_string()
    } else {
        path.to_string()
    }
}

#[cfg(not(windows))]
fn format_pascal_or_camel_case(s: &str) -> String {
    let mut result = String::new();
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '-' || c == '_' {
            result.push(' ');
        } else {
            if c.is_uppercase() && !result.is_empty() && !result.ends_with(' ') {
                if let Some(&next) = chars.peek() {
                    if next.is_lowercase() {
                        result.push(' ');
                    }
                }
            }
            result.push(c);
        }
    }
    result.trim().to_string()
}

#[cfg(not(windows))]
pub fn clean_linux_app_name(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("unknown") {
        return String::new();
    }
    let lowered = trimmed.to_lowercase();

    // 1. Chronos
    if lowered.contains("chronos") {
        return "Chronos Screentime".to_string();
    }

    // 2. Desktop / Shell / Session
    if lowered == "gnome-shell" || lowered == "desktop" || lowered == "mutter" || lowered == "active session" || lowered == "gjs" || lowered == "gjs-console" {
        return "Desktop".to_string();
    }

    // 3. Terminals
    if lowered.contains("ptyxis") || lowered.contains("terminal") || lowered.contains("alacritty")
        || lowered.contains("kitty") || lowered.contains("konsole") || lowered.contains("wezterm")
        || lowered.contains("blackbox") || lowered.contains("tilix") || lowered.contains("terminator")
        || lowered == "xterm" || lowered.contains("foot")
    {
        return "Terminal".to_string();
    }

    // 4. Web Browsers
    if lowered.contains("firefox") || lowered.contains("navigator") || lowered.contains("firefoxpwa") {
        return "Firefox".to_string();
    }
    if lowered.contains("google-chrome") || (lowered.contains("chrome") && !lowered.contains("chromium")) {
        return "Google Chrome".to_string();
    }
    if lowered.contains("chromium") {
        return "Chromium".to_string();
    }
    if lowered.contains("brave") {
        return "Brave".to_string();
    }
    if lowered.contains("msedge") || lowered == "edge" || lowered.contains("microsoft-edge") {
        return "Microsoft Edge".to_string();
    }
    if lowered.contains("opera") {
        return "Opera".to_string();
    }
    if lowered.contains("zen") {
        return "Zen Browser".to_string();
    }

    // 5. Code Editors & IDEs
    if lowered.contains("code") || lowered.contains("vscode") || lowered.contains("code-oss") {
        return "Visual Studio Code".to_string();
    }
    if lowered.contains("cursor") {
        return "Cursor".to_string();
    }
    if lowered.contains("windsurf") {
        return "Windsurf".to_string();
    }
    if lowered.contains("pycharm") {
        return "PyCharm".to_string();
    }
    if lowered.contains("rustrover") {
        return "RustRover".to_string();
    }
    if lowered.contains("clion") {
        return "CLion".to_string();
    }
    if lowered.contains("idea") || lowered.contains("intellij") {
        return "IntelliJ IDEA".to_string();
    }
    if lowered.contains("sublime") {
        return "Sublime Text".to_string();
    }

    // 6. GNOME & Desktop Core Utilities
    if lowered.contains("nautilus") || lowered.contains("org.gnome.nautilus") || lowered.contains("dolphin") || lowered.contains("thunar") || lowered.contains("nemo") || lowered.contains("caja") {
        return "Files".to_string();
    }
    if lowered.contains("gnome-text-editor") || lowered.contains("gedit") || lowered.contains("kate") || lowered.contains("kwrite") || lowered.contains("mousepad") || lowered.contains("pluma") {
        return "Text Editor".to_string();
    }
    if lowered.contains("gnome-control-center") || lowered.contains("org.gnome.settings") || lowered == "settings" {
        return "Settings".to_string();
    }
    if lowered.contains("gnome-tweaks") || lowered.contains("org.gnome.tweaks") || lowered == "tweaks" {
        return "Tweaks".to_string();
    }
    if lowered.contains("gnome-calendar") || lowered == "calendar" {
        return "Calendar".to_string();
    }
    if lowered.contains("gnome-calculator") || lowered.contains("calculator") || lowered.contains("kcalc") {
        return "Calculator".to_string();
    }
    if lowered.contains("gnome-system-monitor") || lowered == "resources" || lowered == "btop" || lowered == "htop" {
        return "System Monitor".to_string();
    }
    if lowered.contains("gnome-software") || lowered.contains("snap-store") || lowered.contains("discover") {
        return "Software Center".to_string();
    }
    if lowered.contains("extension-manager") {
        return "Extension Manager".to_string();
    }
    if lowered.contains("sticky") {
        return "Sticky Notes".to_string();
    }
    if lowered.contains("showtime") || lowered == "totem" || lowered == "celluloid" || lowered == "mpv" {
        return "Video Player".to_string();
    }
    if lowered.contains("papers") || lowered == "evince" || lowered == "okular" || lowered == "atril" {
        return "Document Viewer".to_string();
    }
    if lowered.contains("loupe") || lowered == "eog" || lowered.contains("shotwell") || lowered == "gwenview" {
        return "Image Viewer".to_string();
    }
    if lowered.contains("snapshot") || lowered == "cheese" || lowered == "kamoso" {
        return "Camera".to_string();
    }
    if lowered.contains("decibels") || lowered == "amberol" || lowered == "rhythmbox" || lowered == "lollypop" {
        return "Audio Player".to_string();
    }

    // 7. LibreOffice Suite
    if lowered.contains("soffice") || lowered.contains("libreoffice") {
        if lowered.contains("impress") {
            return "LibreOffice Impress".to_string();
        } else if lowered.contains("calc") {
            return "LibreOffice Calc".to_string();
        } else if lowered.contains("writer") {
            return "LibreOffice Writer".to_string();
        } else if lowered.contains("draw") {
            return "LibreOffice Draw".to_string();
        }
        return "LibreOffice".to_string();
    }

    // 8. Communication & Media Apps
    if lowered.contains("vlc") {
        return "VLC".to_string();
    }
    if lowered.contains("spotify") {
        return "Spotify".to_string();
    }
    if lowered.contains("discord") || lowered.contains("webcord") || lowered.contains("vesktop") {
        return "Discord".to_string();
    }
    if lowered.contains("slack") {
        return "Slack".to_string();
    }
    if lowered.contains("telegram") {
        return "Telegram".to_string();
    }
    if lowered.contains("whatsapp") {
        return "WhatsApp Web".to_string();
    }
    if lowered.contains("obs") {
        return "OBS Studio".to_string();
    }
    if lowered.contains("gimp") {
        return "GIMP".to_string();
    }
    if lowered.contains("inkscape") {
        return "Inkscape".to_string();
    }
    if lowered.contains("blender") {
        return "Blender".to_string();
    }
    if lowered.contains("steam") {
        return "Steam".to_string();
    }
    if lowered.contains("thunderbird") {
        return "Thunderbird".to_string();
    }
    if lowered.contains("ulauncher") || lowered == "rofi" || lowered == "wofi" || lowered == "albert" {
        return "Ulauncher".to_string();
    }
    if lowered.contains("apport") || lowered.contains("system-crash-notification") || lowered.contains("report a problem") {
        return "System Reporter".to_string();
    }
    if lowered.contains("xdg-desktop-portal") || lowered == "portal" {
        return "Desktop Portal".to_string();
    }
    if lowered == "pyw" || lowered.starts_with("python3") || lowered == "python" {
        return "Python".to_string();
    }

    // 9. Reverse-DNS format (e.g. org.gnome.Calculator -> Calculator, io.github.alainm23.planify -> Planify)
    if trimmed.contains('.') && !trimmed.contains(' ') {
        if let Some(last) = trimmed.split('.').last() {
            if !last.is_empty() {
                let formatted = format_pascal_or_camel_case(last);
                return clean_linux_app_name(&formatted);
            }
        }
    }

    format_pascal_or_camel_case(trimmed)
}

#[cfg(not(windows))]
fn resolve_flatpak_or_snap_name(pid: u32) -> Option<String> {
    if pid == 0 {
        return None;
    }
    // Check environ for FLATPAK_ID or SNAP_NAME
    if let Ok(bytes) = std::fs::read(format!("/proc/{}/environ", pid)) {
        for env_entry in bytes.split(|&b| b == 0) {
            if let Ok(s) = std::str::from_utf8(env_entry) {
                if let Some(id) = s.strip_prefix("FLATPAK_ID=") {
                    let cleaned = clean_linux_app_name(id);
                    if !cleaned.is_empty() && cleaned != "Flatpak" {
                        return Some(cleaned);
                    }
                }
                if let Some(id) = s.strip_prefix("SNAP_NAME=") {
                    let cleaned = clean_linux_app_name(id);
                    if !cleaned.is_empty() {
                        return Some(cleaned);
                    }
                }
            }
        }
    }
    // Check cmdline
    if let Ok(bytes) = std::fs::read(format!("/proc/{}/cmdline", pid)) {
        let args: Vec<&str> = bytes
            .split(|&b| b == 0)
            .filter_map(|b| std::str::from_utf8(b).ok())
            .filter(|s| !s.is_empty())
            .collect();
        for (i, &arg) in args.iter().enumerate() {
            if arg == "run" && i + 1 < args.len() {
                let candidate = args[i + 1];
                let cleaned = clean_linux_app_name(candidate);
                if !cleaned.is_empty() && cleaned != "Flatpak" {
                    return Some(cleaned);
                }
            }
        }
    }
    None
}

#[cfg(not(windows))]
struct AtspiContext {
    _atspi_lib: libloading::Library,
    _gobject_lib: libloading::Library,
    atspi_get_desktop: unsafe extern "C" fn(i32) -> *mut std::ffi::c_void,
    atspi_accessible_get_child_count: unsafe extern "C" fn(*mut std::ffi::c_void, *mut *mut std::ffi::c_void) -> i32,
    atspi_accessible_get_child_at_index: unsafe extern "C" fn(*mut std::ffi::c_void, i32, *mut *mut std::ffi::c_void) -> *mut std::ffi::c_void,
    atspi_accessible_get_state_set: unsafe extern "C" fn(*mut std::ffi::c_void) -> *mut std::ffi::c_void,
    atspi_state_set_contains: unsafe extern "C" fn(*mut std::ffi::c_void, i32) -> i32,
    atspi_accessible_get_name: unsafe extern "C" fn(*mut std::ffi::c_void, *mut *mut std::ffi::c_void) -> *const std::ffi::c_char,
    atspi_accessible_get_process_id: unsafe extern "C" fn(*mut std::ffi::c_void, *mut *mut std::ffi::c_void) -> u32,
    g_object_unref: unsafe extern "C" fn(*mut std::ffi::c_void),
}

#[cfg(not(windows))]
impl AtspiContext {
    fn load() -> Option<Self> {
        unsafe {
            if std::env::var("AT_SPI_BUS_ADDRESS").is_err() {
                if let Ok(conn) = zbus::blocking::Connection::session() {
                    if let Ok(proxy) = zbus::blocking::Proxy::new(
                        &conn,
                        "org.a11y.Bus",
                        "/org/a11y/bus",
                        "org.a11y.Bus",
                    ) {
                        let res: Result<String, _> = proxy.call("GetAddress", &());
                        if let Ok(addr) = res {
                            if !addr.is_empty() {
                                std::env::set_var("AT_SPI_BUS_ADDRESS", addr);
                            }
                        }
                    }
                }
                if std::env::var("AT_SPI_BUS_ADDRESS").is_err() {
                    if let Ok(runtime) = std::env::var("XDG_RUNTIME_DIR") {
                        let candidate = format!("{}/at-spi/bus", runtime);
                        if std::path::Path::new(&candidate).exists() {
                            std::env::set_var("AT_SPI_BUS_ADDRESS", format!("unix:path={}", candidate));
                        }
                    }
                }
            }

            let atspi_lib = libloading::Library::new("libatspi.so.0")
                .or_else(|_| libloading::Library::new("libatspi.so"))
                .ok()?;
            let gobject_lib = libloading::Library::new("libgobject-2.0.so.0")
                .or_else(|_| libloading::Library::new("libgobject-2.0.so"))
                .ok()?;

            std::env::set_var("NO_AT_BRIDGE", "1");
            let atspi_init: libloading::Symbol<unsafe extern "C" fn() -> i32> = atspi_lib.get(b"atspi_init").ok()?;
            atspi_init();

            let atspi_get_desktop: libloading::Symbol<unsafe extern "C" fn(i32) -> *mut std::ffi::c_void> =
                atspi_lib.get(b"atspi_get_desktop").ok()?;
            let atspi_accessible_get_child_count: libloading::Symbol<unsafe extern "C" fn(*mut std::ffi::c_void, *mut *mut std::ffi::c_void) -> i32> =
                atspi_lib.get(b"atspi_accessible_get_child_count").ok()?;
            let atspi_accessible_get_child_at_index: libloading::Symbol<unsafe extern "C" fn(*mut std::ffi::c_void, i32, *mut *mut std::ffi::c_void) -> *mut std::ffi::c_void> =
                atspi_lib.get(b"atspi_accessible_get_child_at_index").ok()?;
            let atspi_accessible_get_state_set: libloading::Symbol<unsafe extern "C" fn(*mut std::ffi::c_void) -> *mut std::ffi::c_void> =
                atspi_lib.get(b"atspi_accessible_get_state_set").ok()?;
            let atspi_state_set_contains: libloading::Symbol<unsafe extern "C" fn(*mut std::ffi::c_void, i32) -> i32> =
                atspi_lib.get(b"atspi_state_set_contains").ok()?;
            let atspi_accessible_get_name: libloading::Symbol<unsafe extern "C" fn(*mut std::ffi::c_void, *mut *mut std::ffi::c_void) -> *const std::ffi::c_char> =
                atspi_lib.get(b"atspi_accessible_get_name").ok()?;
            let atspi_accessible_get_process_id: libloading::Symbol<unsafe extern "C" fn(*mut std::ffi::c_void, *mut *mut std::ffi::c_void) -> u32> =
                atspi_lib.get(b"atspi_accessible_get_process_id").ok()?;
            let g_object_unref: libloading::Symbol<unsafe extern "C" fn(*mut std::ffi::c_void)> =
                gobject_lib.get(b"g_object_unref").ok()?;

            Some(Self {
                atspi_get_desktop: *atspi_get_desktop,
                atspi_accessible_get_child_count: *atspi_accessible_get_child_count,
                atspi_accessible_get_child_at_index: *atspi_accessible_get_child_at_index,
                atspi_accessible_get_state_set: *atspi_accessible_get_state_set,
                atspi_state_set_contains: *atspi_state_set_contains,
                atspi_accessible_get_name: *atspi_accessible_get_name,
                atspi_accessible_get_process_id: *atspi_accessible_get_process_id,
                g_object_unref: *g_object_unref,
                _atspi_lib: atspi_lib,
                _gobject_lib: gobject_lib,
            })
        }
    }
}

#[cfg(not(windows))]
fn get_atspi_active_window(ctx: &AtspiContext) -> Option<(String, String, u32)> {
    unsafe {
        let desktop = (ctx.atspi_get_desktop)(0);
        if desktop.is_null() {
            return None;
        }
        let child_count = (ctx.atspi_accessible_get_child_count)(desktop, std::ptr::null_mut());
        let mut active_result: Option<(String, String, u32)> = None;

        for i in 0..child_count {
            let app = (ctx.atspi_accessible_get_child_at_index)(desktop, i, std::ptr::null_mut());
            if app.is_null() {
                continue;
            }

            let win_count = (ctx.atspi_accessible_get_child_count)(app, std::ptr::null_mut());
            for j in 0..win_count {
                let win = (ctx.atspi_accessible_get_child_at_index)(app, j, std::ptr::null_mut());
                if win.is_null() {
                    continue;
                }

                let state_set = (ctx.atspi_accessible_get_state_set)(win);
                let mut is_active = false;
                if !state_set.is_null() {
                    // ATSPI_STATE_ACTIVE = 1, ATSPI_STATE_FOCUSED = 16
                    is_active = (ctx.atspi_state_set_contains)(state_set, 1) != 0
                        || (ctx.atspi_state_set_contains)(state_set, 16) != 0;
                    (ctx.g_object_unref)(state_set);
                }

                if is_active && active_result.is_none() {
                    let app_name_ptr = (ctx.atspi_accessible_get_name)(app, std::ptr::null_mut());
                    let app_name = if !app_name_ptr.is_null() {
                        std::ffi::CStr::from_ptr(app_name_ptr).to_string_lossy().into_owned()
                    } else {
                        String::new()
                    };

                    let win_name_ptr = (ctx.atspi_accessible_get_name)(win, std::ptr::null_mut());
                    let win_title = if !win_name_ptr.is_null() {
                        std::ffi::CStr::from_ptr(win_name_ptr).to_string_lossy().into_owned()
                    } else {
                        String::new()
                    };

                    let pid = (ctx.atspi_accessible_get_process_id)(app, std::ptr::null_mut());
                    active_result = Some((app_name, win_title, pid));
                }
                (ctx.g_object_unref)(win);
            }
            (ctx.g_object_unref)(app);
        }
        (ctx.g_object_unref)(desktop);

        active_result
    }
}

#[cfg(not(windows))]
fn get_gnome_active_window_file() -> Option<(String, String, u32, String)> {
    let runtime_dir = std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/run/user/1000".to_string());
    let path = std::path::Path::new(&runtime_dir).join("chronos_active_window.json");
    if !path.exists() {
        return None;
    }
    let meta = std::fs::metadata(&path).ok()?;
    let elapsed = meta.modified().ok()?.elapsed().ok()?;
    if elapsed.as_secs() > 10 {
        return None;
    }
    let content = std::fs::read_to_string(&path).ok()?;
    let parsed: serde_json::Value = serde_json::from_str(&content).ok()?;
    let app_name = parsed.get("app_name")?.as_str()?.to_string();
    let title = parsed.get("title").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let pid = parsed.get("pid").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
    let exe = parsed.get("exe").and_then(|v| v.as_str()).unwrap_or("").to_string();
    if !app_name.is_empty() {
        Some((app_name, title, pid, exe))
    } else {
        None
    }
}

#[cfg(not(windows))]
fn get_x11_active_window() -> Option<CurrentActivity> {
    use x11rb::connection::Connection;
    use x11rb::protocol::xproto::AtomEnum;

    let (conn, screen_num) = x11rb::connect(None).ok()?;
    let root = conn.setup().roots.get(screen_num)?.root;

    let net_active_window = conn.intern_atom(false, b"_NET_ACTIVE_WINDOW").ok()?.reply().ok()?.atom;
    let net_wm_pid = conn.intern_atom(false, b"_NET_WM_PID").ok()?.reply().ok()?.atom;
    let net_wm_name = conn.intern_atom(false, b"_NET_WM_NAME").ok()?.reply().ok()?.atom;
    let wm_class = conn.intern_atom(false, b"WM_CLASS").ok()?.reply().ok()?.atom;

    let reply = conn.get_property(false, root, net_active_window, AtomEnum::WINDOW, 0, 1).ok()?.reply().ok()?;
    if reply.type_ == u32::from(AtomEnum::NONE) {
        return None;
    }
    let win_id = reply.value32()?.next()?;
    if win_id == 0 {
        return None;
    }

    // On Wayland, Xwayland preserves stale _NET_ACTIVE_WINDOW when switching to native Wayland apps.
    // Verify that the X11 connection actually has input focus on win_id.
    let is_wayland = std::env::var("WAYLAND_DISPLAY").is_ok()
        || std::env::var("XDG_SESSION_TYPE").as_deref() == Ok("wayland");
    if is_wayland {
        use x11rb::protocol::xproto::ConnectionExt as _;
        if let Ok(focus_reply) = conn.get_input_focus() {
            if let Ok(focus) = focus_reply.reply() {
                // Focus 0 = None, 1 = PointerRoot (indicates Wayland native or root has focus)
                if focus.focus <= 1 || focus.focus != win_id {
                    return None;
                }
            }
        }
    }

    let mut app_name = String::new();
    let mut process_path = String::new();
    let mut window_title = String::new();

    // 1. Try PID
    let mut resolved_pid = 0u32;
    if let Ok(pid_reply) = conn.get_property(false, win_id, net_wm_pid, AtomEnum::CARDINAL, 0, 1) {
        if let Ok(pid_r) = pid_reply.reply() {
            if let Some(pid) = pid_r.value32().and_then(|mut v| v.next()) {
                resolved_pid = pid;
                if let Ok(link) = std::fs::read_link(format!("/proc/{}/exe", pid)) {
                    process_path = link.to_string_lossy().to_string();
                    app_name = clean_linux_app_name(&app_name_from_path(&process_path));
                } else if let Ok(comm) = std::fs::read_to_string(format!("/proc/{}/comm", pid)) {
                    app_name = clean_linux_app_name(comm.trim());
                }
            }
        }
    }

    // 2. Try WM_CLASS
    if app_name.is_empty() || app_name == "Desktop" || process_path.contains("flatpak") {
        if let Ok(class_reply) = conn.get_property(false, win_id, wm_class, AtomEnum::STRING, 0, 1024) {
            if let Ok(class_r) = class_reply.reply() {
                let raw = String::from_utf8_lossy(&class_r.value);
                let parts: Vec<&str> = raw.split('\0').filter(|s| !s.is_empty()).collect();
                if let Some(&last) = parts.last() {
                    let cleaned = clean_linux_app_name(last);
                    if !cleaned.is_empty() {
                        app_name = cleaned;
                    }
                } else if let Some(&first) = parts.first() {
                    let cleaned = clean_linux_app_name(first);
                    if !cleaned.is_empty() {
                        app_name = cleaned;
                    }
                }
            }
        }
    }

    if (app_name.is_empty() || app_name == "Desktop" || process_path.contains("flatpak") || process_path.contains("snap")) && resolved_pid > 0 {
        if let Some(fp_name) = resolve_flatpak_or_snap_name(resolved_pid) {
            app_name = fp_name;
        }
    }

    // 3. Try Window Title
    let utf8_atom = conn.intern_atom(false, b"UTF8_STRING").ok().and_then(|r| r.reply().ok()).map(|r| r.atom);
    let target_atom = utf8_atom.unwrap_or_else(|| AtomEnum::STRING.into());
    if let Ok(name_reply) = conn.get_property(false, win_id, net_wm_name, target_atom, 0, 2048) {
        if let Ok(name_r) = name_reply.reply() {
            window_title = String::from_utf8_lossy(&name_r.value).trim().to_string();
        }
    }
    if window_title.is_empty() {
        if let Ok(wm_name_reply) = conn.get_property(false, win_id, AtomEnum::WM_NAME, AtomEnum::STRING, 0, 2048) {
            if let Ok(wm_r) = wm_name_reply.reply() {
                window_title = String::from_utf8_lossy(&wm_r.value).trim().to_string();
            }
        }
    }

    if app_name.is_empty() {
        return None;
    }

    let is_browser = is_browser_process(&app_name);
    let domain = if is_browser {
        domain_from_browser_title(&window_title)
    } else {
        None
    };

    Some(CurrentActivity {
        app_name,
        process_path,
        domain,
    })
}

#[cfg(not(windows))]
pub(crate) fn get_foreground_info() -> Option<CurrentActivity> {
    // 0. Check if Chronos's own dashboard window has focus
    if IS_CHRONOS_FOCUSED.load(Ordering::SeqCst) {
        return Some(CurrentActivity {
            app_name: "Chronos Screentime".to_string(),
            process_path: std::env::current_exe().map(|p| p.to_string_lossy().to_string()).unwrap_or_default(),
            domain: None,
        });
    }

    // 1. Try GNOME extension active window state file (if available)
    if let Some((app_name_raw, title, pid, exe)) = get_gnome_active_window_file() {
        let mut app_name = clean_linux_app_name(&app_name_raw);
        if (app_name.is_empty() || app_name == "Desktop" || exe.contains("flatpak") || exe.contains("snap")) && pid > 0 {
            if let Some(fp_name) = resolve_flatpak_or_snap_name(pid) {
                app_name = fp_name;
            }
        }
        let process_path = if !exe.is_empty() {
            exe
        } else if pid > 0 {
            std::fs::read_link(format!("/proc/{}/exe", pid))
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_default()
        } else {
            String::new()
        };
        let is_browser = is_browser_process(&app_name);
        let domain = if is_browser {
            domain_from_browser_title(&title)
        } else {
            None
        };
        return Some(CurrentActivity {
            app_name,
            process_path,
            domain,
        });
    }

    // 2. Try AT-SPI2 Accessibility Bus (works across GNOME/KDE/Wayland/X11)
    static ATSPI: std::sync::OnceLock<Option<AtspiContext>> = std::sync::OnceLock::new();
    let atspi_ctx = ATSPI.get_or_init(AtspiContext::load);
    if let Some(ctx) = atspi_ctx {
        if let Some((app_name_raw, title, pid)) = get_atspi_active_window(ctx) {
            let process_path = if pid > 0 {
                std::fs::read_link(format!("/proc/{}/exe", pid))
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_default()
            } else {
                String::new()
            };
            let mut app_name = clean_linux_app_name(&app_name_raw);
            if (app_name.is_empty() || app_name == "Desktop" || process_path.contains("flatpak") || process_path.contains("snap")) && pid > 0 {
                if let Some(fp_name) = resolve_flatpak_or_snap_name(pid) {
                    app_name = fp_name;
                }
            }
            if app_name.is_empty() && !process_path.is_empty() {
                app_name = clean_linux_app_name(&app_name_from_path(&process_path));
            }
            if app_name.is_empty() && pid > 0 {
                if let Ok(comm) = std::fs::read_to_string(format!("/proc/{}/comm", pid)) {
                    app_name = clean_linux_app_name(comm.trim());
                }
            }
            if app_name.is_empty() {
                app_name = "Desktop".to_string();
            }
            let is_browser = is_browser_process(&app_name);
            let domain = if is_browser {
                domain_from_browser_title(&title)
            } else {
                None
            };
            return Some(CurrentActivity {
                app_name,
                process_path,
                domain,
            });
        }
    }

    // 3. Try X11 / Xwayland _NET_ACTIVE_WINDOW
    if let Some(act) = get_x11_active_window() {
        return Some(act);
    }

    // 4. Default fallback
    Some(CurrentActivity {
        app_name: "Desktop".to_string(),
        process_path: String::new(),
        domain: None,
    })
}

#[cfg(not(windows))]
pub(crate) fn get_idle_seconds() -> Option<u64> {
    // 1. Try GNOME Mutter IdleMonitor via DBus
    if let Ok(conn) = zbus::blocking::Connection::session() {
        if let Ok(proxy) = zbus::blocking::Proxy::new(
            &conn,
            "org.gnome.Mutter.IdleMonitor",
            "/org/gnome/Mutter/IdleMonitor/Core",
            "org.gnome.Mutter.IdleMonitor",
        ) {
            let call_res: Result<u64, _> = proxy.call("GetIdletime", &());
            if let Ok(idle_ms) = call_res {
                return Some(idle_ms / 1000);
            }
        }
    }

    // 2. Try X11 screensaver extension
    use x11rb::connection::Connection;
    use x11rb::protocol::screensaver;

    if let Ok((conn, screen_num)) = x11rb::connect(None) {
        if let Some(root_obj) = conn.setup().roots.get(screen_num) {
            if let Ok(reply) = screensaver::query_info(&conn, root_obj.root) {
                if let Ok(info) = reply.reply() {
                    return Some(info.ms_since_user_input as u64 / 1000);
                }
            }
        }
    }

    None
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
    local_date: NaiveDate,
) {
    let day = ensure_day(data, local_date);
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

/// Record one tick of activity (e.g. 1 second) for the current day.
pub fn record_activity(
    data: &mut ScreenTimeData,
    activity: &CurrentActivity,
    now: chrono::DateTime<Utc>,
    local_date: NaiveDate,
) {
    if activity.app_name.is_empty() || activity.is_app_lock() {
        return;
    }
    let day = ensure_day(data, local_date);
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
                last_switch_key = None;
                std::thread::sleep(Duration::from_secs(POLL_INTERVAL_SECS));
                continue;
            }
        }

        let fg_activity = get_foreground_info();

        if let Some(activity) = fg_activity {
            let now = Utc::now();
            let local_date = Local::now().date_naive();
            let current_key = activity_switch_key(&activity);
            {
                let mut guard = data.lock().unwrap();
                record_activity(&mut guard, &activity, now, local_date);
                if let Some(ref key) = current_key {
                    if let Some(ref prev_key) = last_switch_key {
                        if prev_key != key {
                            increment_session_count(&mut guard, &activity, now, local_date);
                            increment_switch_count(&mut guard, local_date);
                        }
                    } else {
                        increment_session_count(&mut guard, &activity, now, local_date);
                    }
                }
            }
            if let Some(key) = current_key {
                last_switch_key = Some(key);
            } else {
                last_switch_key = None;
            }
        } else {
            last_switch_key = None;
        }
        save_counter += 1;
        if save_counter >= 5 {
            save_counter = 0;
            let to_save = data.lock().unwrap().clone();
            save_screen_time_data(&to_save);
        }
        std::thread::sleep(Duration::from_secs(POLL_INTERVAL_SECS));
    }
    let to_save = data.lock().unwrap().clone();
    save_screen_time_data(&to_save);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_domain_normalization() {
        assert_eq!(normalize_domain("https://github.com/rust-lang"), Some("github.com".to_string()));
        assert_eq!(normalize_domain("www.google.com/search?q=test"), Some("google.com".to_string()));
        assert_eq!(normalize_domain("http://sub.example.co.uk:8080/path"), Some("sub.example.co.uk".to_string()));
        assert_eq!(normalize_domain("192.168.1.100"), Some("192.168.1.100".to_string()));
        assert_eq!(normalize_domain("not a domain"), None);
        // Ensure filenames are NEVER recognized as domains
        assert_eq!(normalize_domain("rockyou.txt"), None);
        assert_eq!(normalize_domain("rockyou.txt.gz"), None);
        assert_eq!(normalize_domain("archive.tar.gz"), None);
        assert_eq!(normalize_domain("tar.gz"), None);
        assert_eq!(normalize_domain("test.py"), None);
        assert_eq!(normalize_domain("app.json"), None);
        assert_eq!(normalize_domain("document.pdf"), None);
    }

    #[test]
    fn test_domain_from_title() {
        // GitHub repo with Supabase in the description/title must be github.com, NOT supabase.com!
        assert_eq!(
            domain_from_title("ghassanelgendy/chronos-minimal: Lightweight background screentime tracker with Supabase sync - Chromium"),
            Some("github.com".to_string())
        );
        assert_eq!(
            domain_from_title("Issues · supabase/supabase · GitHub - Chromium"),
            Some("github.com".to_string())
        );
        assert_eq!(
            domain_from_title("GitHub - supabase/supabase: The open source Firebase alternative - Brave"),
            Some("github.com".to_string())
        );

        // YouTube video about Supabase must be youtube.com, NOT supabase.com!
        assert_eq!(
            domain_from_title("How to connect Next.js to Supabase in 5 minutes - YouTube - Google Chrome"),
            Some("youtube.com".to_string())
        );

        // Actual Supabase dashboard
        assert_eq!(
            domain_from_title("Dashboard | Supabase - Chromium"),
            Some("supabase.com".to_string())
        );
        assert_eq!(
            domain_from_title("Supabase | The Open Source Firebase Alternative - Brave"),
            Some("supabase.com".to_string())
        );

        // Google search about Supabase must be google.com
        assert_eq!(
            domain_from_title("supabase query error - Google Search - Google Chrome"),
            Some("google.com".to_string())
        );

        // Standard web applications
        assert_eq!(domain_from_title("Gemini - Chromium"), Some("gemini.google.com".to_string()));
        assert_eq!(domain_from_title("Gemini — Google Chrome"), Some("gemini.google.com".to_string()));
        assert_eq!(domain_from_title("Gemini"), Some("gemini.google.com".to_string()));
        assert_eq!(domain_from_title("GitHub: Where the world builds software — Mozilla Firefox"), Some("github.com".to_string()));
        assert_eq!(domain_from_title("YouTube - Watch Videos - Google Chrome"), Some("youtube.com".to_string()));
        assert_eq!(domain_from_title("ChatGPT — Google Chrome"), Some("chatgpt.com".to_string()));
        assert_eq!(domain_from_title("Claude - Anthropic - Brave"), Some("claude.ai".to_string()));
        assert_eq!(domain_from_title("Perplexity - Chromium"), Some("perplexity.ai".to_string()));
        assert_eq!(domain_from_title("DeepSeek - Chromium"), Some("deepseek.com".to_string()));
        assert_eq!(domain_from_title("erp.servixa-it.com - Dashboard — Mozilla Firefox"), Some("erp.servixa-it.com".to_string()));
        assert_eq!(domain_from_title("https://my-app.vercel.app/demo - Chromium"), Some("my-app.vercel.app".to_string()));
        assert_eq!(domain_from_title("localhost:3000 - Web App - Chromium"), Some("localhost:3000".to_string()));
        // Ensure browser internal pages return None (so counted as browser app, not site)
        assert_eq!(domain_from_title("New Tab - Chromium"), None);
        assert_eq!(domain_from_title("Settings - Google Chrome"), None);
        // Ensure filename in title is not treated as website
        assert_eq!(domain_from_title("rockyou.txt - Text Editor"), None);
    }

    #[test]
    #[cfg(not(windows))]
    fn test_clean_linux_app_names() {
        assert_eq!(clean_linux_app_name("org.gnome.Ptyxis"), "Terminal");
        assert_eq!(clean_linux_app_name("alacritty"), "Terminal");
        assert_eq!(clean_linux_app_name("firefox-bin"), "Firefox");
        assert_eq!(clean_linux_app_name("google-chrome"), "Google Chrome");
        assert_eq!(clean_linux_app_name("code"), "Visual Studio Code");
        assert_eq!(clean_linux_app_name("org.gnome.Nautilus"), "Files");
        assert_eq!(clean_linux_app_name("org.gnome.Calculator"), "Calculator");
        assert_eq!(clean_linux_app_name("gnome-shell"), "Desktop");
        assert_eq!(clean_linux_app_name("chronos-screentime"), "Chronos Screentime");
        assert_eq!(clean_linux_app_name("Unknown"), "");
        assert_eq!(clean_linux_app_name("soffice.bin"), "LibreOffice");
        assert_eq!(clean_linux_app_name("gnome-control-center"), "Settings");
        assert_eq!(clean_linux_app_name("gnome-tweaks"), "Tweaks");
        assert_eq!(clean_linux_app_name("papers"), "Document Viewer");
        assert_eq!(clean_linux_app_name("showtime"), "Video Player");
        assert_eq!(clean_linux_app_name("system-crash-notification"), "System Reporter");
        assert_eq!(clean_linux_app_name("Active Session"), "Desktop");
    }
}
