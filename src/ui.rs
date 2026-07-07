//! System tray, today summary, and Preferences window. SUPABASE_SYNC Sections 5, 6.

#![cfg(windows)]

use crate::models::{
    AppSettings,
    SummaryPeriod,
    format_seconds_display,
    get_today_summary,
    summarize_period,
};
use crate::storage::{
    clear_all_data,
    export_data_snapshot,
    load_screen_time_data,
    reset_app_data,
    save_settings,
};
use crate::startup;
use crate::supabase;
use crate::models::ScreenTimeData;
use native_windows_gui as nwg;
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

/// Resolve path to icon.ico (next to exe or in current dir).
fn icon_path() -> std::path::PathBuf {
    if std::path::Path::new("icon.ico").exists() {
        return std::path::PathBuf::from("icon.ico");
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let p = dir.join("icon.ico");
            if p.exists() {
                return p;
            }
        }
    }
    std::path::PathBuf::from("icon.ico")
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

/// Show the "Today's summary" dashboard (total time + per-app/site list). Blocks until closed.
pub fn show_today_window(data: Arc<std::sync::Mutex<ScreenTimeData>>) {
    let (total_seconds, lines) = get_today_summary(&data.lock().unwrap());
    let total_text = format_seconds_display(total_seconds);
    let list_text: String = lines
        .iter()
        .map(|l| format!("{} — {}", l.name, format_seconds_display(l.total_seconds)))
        .collect::<Vec<_>>()
        .join("\r\n");

    let mut window = nwg::Window::default();
    let mut icon = nwg::Icon::default();
    let mut label_total = nwg::Label::default();
    let mut label_header = nwg::Label::default();
    let mut label_list = nwg::Label::default();
    let mut layout = nwg::GridLayout::default();

    let icon_path = icon_path();
    let icon_loaded = icon_path.exists()
        && nwg::Icon::builder()
            .source_file(Some(icon_path.to_str().unwrap_or("icon.ico")))
            .strict(false)
            .build(&mut icon)
            .is_ok();

    let mut win_builder = nwg::Window::builder()
        .size((400, 420))
        .position((350, 250))
        .title("Chronos – Today");
    if icon_loaded {
        win_builder = win_builder.icon(Some(&icon));
    }
    win_builder
        .flags(nwg::WindowFlags::WINDOW | nwg::WindowFlags::VISIBLE)
        .build(&mut window)
        .expect("today window");

    nwg::Label::builder()
        .text(&format!("Total today: {}", total_text))
        .parent(&window)
        .build(&mut label_total)
        .expect("today total label");

    nwg::Label::builder()
        .text("By app / site")
        .parent(&window)
        .build(&mut label_header)
        .expect("today header label");

    let list_content = if list_text.is_empty() {
        "No activity yet today.".to_string()
    } else {
        list_text
    };
    nwg::Label::builder()
        .text(&list_content)
        .parent(&window)
        .build(&mut label_list)
        .expect("today list label");

    nwg::GridLayout::builder()
        .parent(&window)
        .spacing(10)
        .child_item(nwg::GridLayoutItem::new(&label_total, 0, 0, 1, 1))
        .child_item(nwg::GridLayoutItem::new(&label_header, 0, 1, 1, 1))
        .child_item(nwg::GridLayoutItem::new(&label_list, 0, 2, 1, 1))
        .build(&mut layout)
        .expect("today layout");

    let window_handle = window.handle.clone();
    nwg::full_bind_event_handler(&window_handle, move |evt, _evt_data, handle| {
        if evt == nwg::Event::OnWindowClose && handle == window_handle {
            nwg::stop_thread_dispatch();
        }
    });

    nwg::dispatch_thread_events();
}

fn refresh_dashboard_view(
    data: &Arc<std::sync::Mutex<ScreenTimeData>>,
    settings: &Arc<std::sync::Mutex<AppSettings>>,
    total_label: &nwg::Label,
    tracking_label: &nwg::Label,
    stats_box: &nwg::TextBox,
    period: SummaryPeriod,
    websites_only: bool,
    tracking_enabled: bool,
) {
    let (totals, lines) = {
        let guard = data.lock().unwrap();
        summarize_period(&guard, period, websites_only)
    };

    let idle_now = get_idle_seconds().unwrap_or(0);
    let idle_threshold = {
        let s = settings.lock().unwrap();
        s.idle_threshold_seconds_clamped() as u64
    };
    let idle_state = if idle_now >= idle_threshold {
        "Idle"
    } else {
        "Active"
    };
    tracking_label.set_text(&format!(
        "Tracking: {} | Idle: {}s / {}s ({})",
        if tracking_enabled { "Running" } else { "Stopped" },
        idle_now,
        idle_threshold,
        idle_state
    ));

    total_label.set_text(&format!(
        "{} | Total: {} | Apps: {} | Switches: {}",
        period.label(),
        format_seconds_display(totals.total_seconds),
        totals.total_apps,
        totals.total_switches
    ));

    let lines_text = if lines.is_empty() {
        "No tracked activity yet today.".to_string()
    } else {
        lines
            .iter()
            .take(12)
            .map(|l| {
                let kind = if l.is_website { "Web" } else { "App" };
                format!(
                    "[{}] {} - {} (sessions: {})",
                    kind,
                    l.name,
                    format_seconds_display(l.total_seconds),
                    l.session_count
                )
            })
            .collect::<Vec<_>>()
            .join("\r\n")
    };
    stats_box.set_text(&lines_text);
}

/// Show a dashboard window with realtime tracking stats and quick actions.
pub fn show_dashboard_window(
    data: Arc<std::sync::Mutex<ScreenTimeData>>,
    settings: Arc<std::sync::Mutex<AppSettings>>,
    tracking_enabled: Arc<AtomicBool>,
    running: Arc<AtomicBool>,
    tray_restore_flag: Arc<AtomicBool>,
    default_tab: Option<usize>,
) {
    nwg::Font::set_global_family("Segoe UI").ok();

    let mut window = nwg::Window::default();
    let mut icon = nwg::Icon::default();

    // Tab Container & Tabs
    let mut tabs = nwg::TabsContainer::default();
    let mut tab_activity = nwg::Tab::default();
    let mut tab_supabase = nwg::Tab::default();
    let mut tab_prefs = nwg::Tab::default();

    // Controls for Tab 1: Activity
    let mut header = nwg::Label::default();
    let mut total_label = nwg::Label::default();
    let mut tracking_label = nwg::Label::default();
    let mut stats_box = nwg::TextBox::default();
    let mut period_btn = nwg::Button::default();
    let mut web_toggle_btn = nwg::Button::default();
    let mut tracking_btn = nwg::Button::default();
    let mut today_btn = nwg::Button::default();
    let mut layout_activity = nwg::GridLayout::default();

    // Controls for Tab 2: Cloud Sync (Supabase)
    let mut enable_sync = nwg::CheckBox::default();
    let mut lbl_url = nwg::Label::default();
    let mut url = nwg::TextInput::default();
    let mut lbl_key = nwg::Label::default();
    let mut anon_key = nwg::TextInput::default();
    let mut lbl_uid = nwg::Label::default();
    let mut user_id = nwg::TextInput::default();
    let mut lbl_interval = nwg::Label::default();
    let mut interval = nwg::TextInput::default();
    let mut lifeos_link_btn = nwg::Button::default();
    let mut test_btn = nwg::Button::default();
    let mut save_btn = nwg::Button::default();
    let mut logon_time_lbl = nwg::Label::default();
    let mut last_upload_lbl = nwg::Label::default();
    let mut layout_supabase = nwg::GridLayout::default();

    // Controls for Tab 3: Preferences & Tools
    let mut start_with_windows = nwg::CheckBox::default();
    let mut start_minimized = nwg::CheckBox::default();
    let mut lbl_idle = nwg::Label::default();
    let mut idle_input = nwg::TextInput::default();
    let mut idle_apply_btn = nwg::Button::default();
    let mut minimize_tray_btn = nwg::Button::default();
    let mut restore_tray_btn = nwg::Button::default();
    let mut reset_app_input = nwg::TextInput::default();
    let mut reset_app_btn = nwg::Button::default();
    let mut export_btn = nwg::Button::default();
    let mut reset_all_btn = nwg::Button::default();
    let mut exit_btn = nwg::Button::default();
    let mut layout_prefs = nwg::GridLayout::default();

    // Timer & main layout
    let mut timer = nwg::AnimationTimer::default();
    let mut main_layout = nwg::GridLayout::default();

    let current_period = Rc::new(RefCell::new(SummaryPeriod::Today));
    let websites_only = Rc::new(RefCell::new(false));
    let reset_all_confirm_armed = Rc::new(RefCell::new(false));

    let icon_path = icon_path();
    let icon_loaded = icon_path.exists()
        && nwg::Icon::builder()
            .source_file(Some(icon_path.to_str().unwrap_or("icon.ico")))
            .strict(false)
            .build(&mut icon)
            .is_ok();

    let mut win_builder = nwg::Window::builder()
        .size((780, 680))
        .position((280, 160))
        .title("Chronos Dashboard");
    if icon_loaded {
        win_builder = win_builder.icon(Some(&icon));
    }
    win_builder
        .flags(nwg::WindowFlags::WINDOW | nwg::WindowFlags::VISIBLE)
        .build(&mut window)
        .expect("Build dashboard window");

    // Build Tabs
    nwg::TabsContainer::builder()
        .parent(&window)
        .build(&mut tabs)
        .expect("tabs container");

    nwg::Tab::builder()
        .parent(&tabs)
        .text("Activity Dashboard")
        .build(&mut tab_activity)
        .expect("tab activity");

    nwg::Tab::builder()
        .parent(&tabs)
        .text("Supabase Cloud Sync")
        .build(&mut tab_supabase)
        .expect("tab supabase");

    nwg::Tab::builder()
        .parent(&tabs)
        .text("Preferences & Maintenance")
        .build(&mut tab_prefs)
        .expect("tab preferences");

    // --- TAB 1 (Activity) BUILD ---
    nwg::Label::builder()
        .text("Realtime Screentime Activity Stats")
        .parent(&tab_activity)
        .build(&mut header)
        .expect("dashboard header");

    nwg::Label::builder()
        .text("Today total: 0s")
        .parent(&tab_activity)
        .build(&mut total_label)
        .expect("dashboard total");

    nwg::Label::builder()
        .text("Tracking: Running")
        .parent(&tab_activity)
        .build(&mut tracking_label)
        .expect("dashboard tracking status");

    nwg::Button::builder()
        .text("Period: Today")
        .parent(&tab_activity)
        .build(&mut period_btn)
        .expect("dashboard period btn");

    nwg::Button::builder()
        .text("View: Combined")
        .parent(&tab_activity)
        .build(&mut web_toggle_btn)
        .expect("dashboard view btn");

    nwg::Button::builder()
        .text("Pause Tracking")
        .parent(&tab_activity)
        .build(&mut tracking_btn)
        .expect("dashboard tracking btn");

    nwg::Button::builder()
        .text("Today's Summary")
        .parent(&tab_activity)
        .build(&mut today_btn)
        .expect("dashboard today btn");

    nwg::TextBox::builder()
        .parent(&tab_activity)
        .readonly(true)
        .flags(
            nwg::TextBoxFlags::VISIBLE
                | nwg::TextBoxFlags::TAB_STOP
                | nwg::TextBoxFlags::VSCROLL
                | nwg::TextBoxFlags::AUTOVSCROLL,
        )
        .text("Loading...")
        .build(&mut stats_box)
        .expect("dashboard stats");

    nwg::GridLayout::builder()
        .parent(&tab_activity)
        .spacing(8)
        .child_item(nwg::GridLayoutItem::new(&header, 0, 0, 4, 1))
        .child_item(nwg::GridLayoutItem::new(&total_label, 0, 1, 4, 1))
        .child_item(nwg::GridLayoutItem::new(&tracking_label, 0, 2, 4, 1))
        .child_item(nwg::GridLayoutItem::new(&period_btn, 0, 3, 1, 1))
        .child_item(nwg::GridLayoutItem::new(&web_toggle_btn, 1, 3, 1, 1))
        .child_item(nwg::GridLayoutItem::new(&tracking_btn, 2, 3, 1, 1))
        .child_item(nwg::GridLayoutItem::new(&today_btn, 3, 3, 1, 1))
        .child_item(nwg::GridLayoutItem::new(&stats_box, 0, 4, 4, 4))
        .build(&mut layout_activity)
        .expect("activity layout");

    // --- TAB 2 (Supabase Cloud Sync) BUILD ---
    nwg::CheckBox::builder()
        .text("Enable Supabase Sync")
        .parent(&tab_supabase)
        .build(&mut enable_sync)
        .expect("enable sync check");

    nwg::Label::builder()
        .text("Supabase URL")
        .parent(&tab_supabase)
        .build(&mut lbl_url)
        .expect("label url");

    nwg::TextInput::builder()
        .parent(&tab_supabase)
        .build(&mut url)
        .expect("url input");

    nwg::Label::builder()
        .text("Anon Key")
        .parent(&tab_supabase)
        .build(&mut lbl_key)
        .expect("label key");

    nwg::TextInput::builder()
        .parent(&tab_supabase)
        .password(Some('*'))
        .build(&mut anon_key)
        .expect("key input");

    nwg::Label::builder()
        .text("User ID (LifeOS UUID)")
        .parent(&tab_supabase)
        .build(&mut lbl_uid)
        .expect("label uid");

    nwg::TextInput::builder()
        .parent(&tab_supabase)
        .password(Some('*'))
        .build(&mut user_id)
        .expect("user id input");

    nwg::Button::builder()
        .text("🌐 Open LifeOS GitHub")
        .parent(&tab_supabase)
        .build(&mut lifeos_link_btn)
        .expect("lifeos link btn");

    nwg::Label::builder()
        .text("Upload Interval (minutes)")
        .parent(&tab_supabase)
        .build(&mut lbl_interval)
        .expect("label interval");

    nwg::TextInput::builder()
        .parent(&tab_supabase)
        .build(&mut interval)
        .expect("interval input");

    nwg::Button::builder()
        .text("🔌 Test Connection")
        .parent(&tab_supabase)
        .build(&mut test_btn)
        .expect("test btn");

    nwg::Button::builder()
        .text("Save Settings")
        .parent(&tab_supabase)
        .build(&mut save_btn)
        .expect("save btn");

    nwg::Label::builder()
        .text("Logon Time: Loading...")
        .parent(&tab_supabase)
        .build(&mut logon_time_lbl)
        .expect("logon label");

    nwg::Label::builder()
        .text("Last Upload: Loading...")
        .parent(&tab_supabase)
        .build(&mut last_upload_lbl)
        .expect("last upload label");

    nwg::GridLayout::builder()
        .parent(&tab_supabase)
        .spacing(8)
        .child_item(nwg::GridLayoutItem::new(&enable_sync, 0, 0, 3, 1))
        .child_item(nwg::GridLayoutItem::new(&lbl_url, 0, 1, 1, 1))
        .child_item(nwg::GridLayoutItem::new(&url, 1, 1, 2, 1))
        .child_item(nwg::GridLayoutItem::new(&lbl_key, 0, 2, 1, 1))
        .child_item(nwg::GridLayoutItem::new(&anon_key, 1, 2, 2, 1))
        .child_item(nwg::GridLayoutItem::new(&lbl_uid, 0, 3, 1, 1))
        .child_item(nwg::GridLayoutItem::new(&user_id, 1, 3, 2, 1))
        .child_item(nwg::GridLayoutItem::new(&lbl_interval, 0, 4, 1, 1))
        .child_item(nwg::GridLayoutItem::new(&interval, 1, 4, 2, 1))
        .child_item(nwg::GridLayoutItem::new(&lifeos_link_btn, 0, 5, 1, 1))
        .child_item(nwg::GridLayoutItem::new(&test_btn, 1, 5, 1, 1))
        .child_item(nwg::GridLayoutItem::new(&save_btn, 2, 5, 1, 1))
        .child_item(nwg::GridLayoutItem::new(&logon_time_lbl, 0, 6, 3, 1))
        .child_item(nwg::GridLayoutItem::new(&last_upload_lbl, 0, 7, 3, 1))
        .build(&mut layout_supabase)
        .expect("supabase layout");

    // --- TAB 3 (Preferences & Tools) BUILD ---
    nwg::CheckBox::builder()
        .text("Start with Windows (Run at logon)")
        .parent(&tab_prefs)
        .build(&mut start_with_windows)
        .expect("start with windows check");

    nwg::CheckBox::builder()
        .text("Start minimized to tray")
        .parent(&tab_prefs)
        .build(&mut start_minimized)
        .expect("start minimized check");

    nwg::Label::builder()
        .text("Idle threshold (sec)")
        .parent(&tab_prefs)
        .build(&mut lbl_idle)
        .expect("label idle");

    nwg::TextInput::builder()
        .parent(&tab_prefs)
        .build(&mut idle_input)
        .expect("idle threshold input");

    nwg::Button::builder()
        .text("Apply Idle")
        .parent(&tab_prefs)
        .build(&mut idle_apply_btn)
        .expect("idle apply btn");

    nwg::TextInput::builder()
        .text("App name to reset")
        .parent(&tab_prefs)
        .build(&mut reset_app_input)
        .expect("reset app name input");

    nwg::Button::builder()
        .text("Reset App Data")
        .parent(&tab_prefs)
        .build(&mut reset_app_btn)
        .expect("reset app btn");

    nwg::Button::builder()
        .text("Export JSON")
        .parent(&tab_prefs)
        .build(&mut export_btn)
        .expect("export btn");

    nwg::Button::builder()
        .text("Reset All Data")
        .parent(&tab_prefs)
        .build(&mut reset_all_btn)
        .expect("reset all btn");

    nwg::Button::builder()
        .text("Exit App")
        .parent(&tab_prefs)
        .build(&mut exit_btn)
        .expect("exit app btn");

    nwg::Button::builder()
        .text("Minimize to Tray")
        .parent(&tab_prefs)
        .build(&mut minimize_tray_btn)
        .expect("minimize btn");

    nwg::Button::builder()
        .text("Restore Tray Icon")
        .parent(&tab_prefs)
        .build(&mut restore_tray_btn)
        .expect("restore tray btn");

    nwg::GridLayout::builder()
        .parent(&tab_prefs)
        .spacing(8)
        .child_item(nwg::GridLayoutItem::new(&start_with_windows, 0, 0, 4, 1))
        .child_item(nwg::GridLayoutItem::new(&start_minimized, 0, 1, 4, 1))
        .child_item(nwg::GridLayoutItem::new(&lbl_idle, 0, 2, 1, 1))
        .child_item(nwg::GridLayoutItem::new(&idle_input, 1, 2, 2, 1))
        .child_item(nwg::GridLayoutItem::new(&idle_apply_btn, 3, 2, 1, 1))
        .child_item(nwg::GridLayoutItem::new(&reset_app_input, 0, 3, 3, 1))
        .child_item(nwg::GridLayoutItem::new(&reset_app_btn, 3, 3, 1, 1))
        .child_item(nwg::GridLayoutItem::new(&export_btn, 0, 4, 1, 1))
        .child_item(nwg::GridLayoutItem::new(&reset_all_btn, 1, 4, 1, 1))
        .child_item(nwg::GridLayoutItem::new(&exit_btn, 2, 4, 1, 1))
        .child_item(nwg::GridLayoutItem::new(&minimize_tray_btn, 0, 5, 2, 1))
        .child_item(nwg::GridLayoutItem::new(&restore_tray_btn, 2, 5, 2, 1))
        .build(&mut layout_prefs)
        .expect("preferences layout");

    // --- MAIN WINDOW LAYOUT BUILD ---
    nwg::GridLayout::builder()
        .parent(&window)
        .spacing(4)
        .child_item(nwg::GridLayoutItem::new(&tabs, 0, 0, 1, 1))
        .build(&mut main_layout)
        .expect("main window layout");

    // Populate data
    {
        let s = settings.lock().unwrap();
        // Tab 2
        enable_sync.set_check_state(if s.enable_supabase_sync { nwg::CheckBoxState::Checked } else { nwg::CheckBoxState::Unchecked });
        url.set_text(&s.supabase_url);
        anon_key.set_text(&s.supabase_anon_key);
        user_id.set_text(&s.supabase_user_id);
        interval.set_text(&s.supabase_upload_interval_minutes.to_string());
        
        // Tab 3
        start_with_windows.set_check_state(if s.start_with_windows { nwg::CheckBoxState::Checked } else { nwg::CheckBoxState::Unchecked });
        start_minimized.set_check_state(if s.start_minimized_to_tray { nwg::CheckBoxState::Checked } else { nwg::CheckBoxState::Unchecked });
        idle_input.set_text(&s.idle_threshold_seconds_clamped().to_string());
    }

    // Set startup logon time
    let startup_time_str = match crate::STARTUP_TIME.get() {
        Some(t) => t.format("%Y-%m-%d %H:%M:%S").to_string(),
        None => "Unknown".to_string(),
    };
    logon_time_lbl.set_text(&format!("Logon Time: {}", startup_time_str));

    // Set last upload time
    let last_upload = supabase::get_last_upload_time();
    last_upload_lbl.set_text(&format!("Last Upload: {}", last_upload));

    // Preselect default tab if provided
    if let Some(idx) = default_tab {
        tabs.set_selected_tab(idx);
    }

    nwg::AnimationTimer::builder()
        .parent(&window)
        .interval(Duration::from_millis(1000))
        .active(true)
        .build(&mut timer)
        .expect("dashboard timer");

    let initial_tracking_enabled = tracking_enabled.load(Ordering::SeqCst);
    tracking_btn.set_text(if initial_tracking_enabled {
        "Pause Tracking"
    } else {
        "Resume Tracking"
    });

    refresh_dashboard_view(
        &data,
        &settings,
        &total_label,
        &tracking_label,
        &stats_box,
        *current_period.borrow(),
        *websites_only.borrow(),
        initial_tracking_enabled,
    );

    let window_handle = window.handle.clone();
    let timer_handle = timer.handle.clone();
    let period_btn_handle = period_btn.handle.clone();
    let web_toggle_btn_handle = web_toggle_btn.handle.clone();
    let tracking_btn_handle = tracking_btn.handle.clone();
    let today_btn_handle = today_btn.handle.clone();
    let reset_all_btn_handle = reset_all_btn.handle.clone();
    let idle_apply_btn_handle = idle_apply_btn.handle.clone();
    let minimize_tray_btn_handle = minimize_tray_btn.handle.clone();
    let restore_tray_btn_handle = restore_tray_btn.handle.clone();
    let reset_app_btn_handle = reset_app_btn.handle.clone();
    let export_btn_handle = export_btn.handle.clone();
    let exit_btn_handle = exit_btn.handle.clone();
    let test_btn_handle = test_btn.handle.clone();
    let save_btn_handle = save_btn.handle.clone();
    let lifeos_link_btn_handle = lifeos_link_btn.handle.clone();

    let data_for_timer = Arc::clone(&data);
    let data_for_today = Arc::clone(&data);
    let data_test = Arc::clone(&data);
    let settings_for_idle = Arc::clone(&settings);
    let settings_for_save = Arc::clone(&settings);
    let tracking_enabled_for_ui = Arc::clone(&tracking_enabled);
    let running_for_exit = Arc::clone(&running);

    let test_result: Arc<std::sync::Mutex<Option<Result<supabase::UploadResult, String>>>> = Arc::new(std::sync::Mutex::new(None));
    let test_result_for_timer = Arc::clone(&test_result);
    let test_result_for_test = Arc::clone(&test_result);

    let current_period_ref = Rc::clone(&current_period);
    let websites_only_ref = Rc::clone(&websites_only);
    let reset_all_confirm_ref = Rc::clone(&reset_all_confirm_armed);

    nwg::full_bind_event_handler(&window_handle, move |evt, _evt_data, handle| {
        if evt == nwg::Event::OnWindowClose && handle == window_handle {
            nwg::stop_thread_dispatch();
            return;
        }

        if evt == nwg::Event::OnTimerTick && handle == timer_handle {
            refresh_dashboard_view(
                &data_for_timer,
                &settings_for_idle,
                &total_label,
                &tracking_label,
                &stats_box,
                *current_period_ref.borrow(),
                *websites_only_ref.borrow(),
                tracking_enabled_for_ui.load(Ordering::SeqCst),
            );

            // Check if test connection background task finished
            let test_opt = {
                let mut guard = test_result_for_timer.lock().unwrap();
                guard.take()
            };
            if let Some(res) = test_opt {
                test_btn.set_enabled(true);
                test_btn.set_text("🔌 Test Connection");
                match res {
                    Ok(result) => {
                        if result.success {
                            let msg = format!(
                                "Upload OK.\nApps: {} / {}\nWebsites: {} / {}",
                                result.apps_inserted,
                                result.total_apps,
                                result.websites_inserted,
                                result.total_websites,
                            );
                            nwg::modal_info_message(&window_handle, "Test Connection", &msg);
                            
                            let last_upload = supabase::get_last_upload_time();
                            last_upload_lbl.set_text(&format!("Last Upload: {}", last_upload));
                        } else {
                            let msg = result.error_message.unwrap_or_else(|| "Unknown error".to_string());
                            nwg::modal_info_message(&window_handle, "Upload Failed", &msg);
                        }
                    }
                    Err(err_msg) => {
                        nwg::modal_info_message(&window_handle, "Runtime Error", &err_msg);
                    }
                }
            }
            return;
        }

        if evt != nwg::Event::OnButtonClick {
            return;
        }

        if handle == period_btn_handle {
            let mut p = current_period_ref.borrow_mut();
            *p = p.next();
            period_btn.set_text(&format!("Period: {}", p.label()));
            return;
        }

        if handle == web_toggle_btn_handle {
            let mut web_only = websites_only_ref.borrow_mut();
            *web_only = !*web_only;
            web_toggle_btn.set_text(if *web_only {
                "View: Websites"
            } else {
                "View: Combined"
            });
            return;
        }

        if handle == tracking_btn_handle {
            let next = !tracking_enabled_for_ui.load(Ordering::SeqCst);
            tracking_enabled_for_ui.store(next, Ordering::SeqCst);
            tracking_btn.set_text(if next {
                "Pause Tracking"
            } else {
                "Resume Tracking"
            });
            return;
        }

        if handle == today_btn_handle {
            show_today_window(Arc::clone(&data_for_today));
            return;
        }

        if handle == reset_all_btn_handle {
            let mut armed = reset_all_confirm_ref.borrow_mut();
            if !*armed {
                *armed = true;
                reset_all_btn.set_text("Confirm Reset All");
                nwg::modal_info_message(&window_handle, "Confirm", "Click Reset All again to confirm destructive reset.");
                return;
            }

            clear_all_data();
            {
                let mut guard = data_for_timer.lock().unwrap();
                *guard = load_screen_time_data();
            }
            *armed = false;
            reset_all_btn.set_text("Reset All Data");
            nwg::modal_info_message(&window_handle, "Reset", "All tracked data has been reset.");
            return;
        }

        if handle == idle_apply_btn_handle {
            let parsed = idle_input.text().trim().parse::<u32>();
            let input = match parsed {
                Ok(v) => v,
                Err(_) => {
                    lbl_idle.set_text("Idle threshold (sec) [invalid!]");
                    return;
                }
            };
            let mut updated = {
                let guard = settings_for_idle.lock().unwrap();
                guard.clone()
            };
            updated.idle_threshold_seconds = input;
            let clamped = updated.idle_threshold_seconds_clamped();
            updated.idle_threshold_seconds = clamped;
            {
                let mut guard = settings_for_idle.lock().unwrap();
                *guard = updated.clone();
            }
            save_settings(&updated);
            idle_input.set_text(&clamped.to_string());
            lbl_idle.set_text(&format!("Idle (sec) ✓{}s", clamped));
            return;
        }

        if handle == minimize_tray_btn_handle {
            nwg::stop_thread_dispatch();
            return;
        }

        if handle == restore_tray_btn_handle {
            tray_restore_flag.store(true, Ordering::SeqCst);
            return;
        }

        if handle == reset_app_btn_handle {
            let app = reset_app_input.text();
            if app.trim().is_empty() || app == "App name to reset" {
                nwg::modal_info_message(&window_handle, "Reset App", "Enter an app name first.");
                return;
            }
            let removed = reset_app_data(&app);
            {
                let mut guard = data_for_timer.lock().unwrap();
                *guard = load_screen_time_data();
            }
            nwg::modal_info_message(
                &window_handle,
                "Reset App",
                &format!("Reset completed for app: {}. Entries removed: {}", app, removed),
            );
            return;
        }

        if handle == export_btn_handle {
            let snapshot = data_for_timer.lock().unwrap().clone();
            match export_data_snapshot(&snapshot) {
                Ok(path) => {
                    let msg = format!("Data exported successfully to:\n{}", path.to_string_lossy());
                    nwg::modal_info_message(&window_handle, "Export JSON", &msg);
                }
                Err(e) => {
                    let msg = format!("Failed to export data: {}", e);
                    nwg::modal_info_message(&window_handle, "Export Failed", &msg);
                }
            }
            return;
        }

        if handle == exit_btn_handle {
            running_for_exit.store(false, Ordering::SeqCst);
            nwg::stop_thread_dispatch();
            return;
        }

        if handle == lifeos_link_btn_handle {
            std::process::Command::new("cmd")
                .args(["/C", "start", "https://github.com/ghassanelgendy/lifeOS/"])
                .spawn()
                .ok();
            return;
        }

        if handle == save_btn_handle {
            let s = AppSettings {
                enable_supabase_sync: enable_sync.check_state() == nwg::CheckBoxState::Checked,
                supabase_url: url.text(),
                supabase_anon_key: anon_key.text(),
                supabase_user_id: user_id.text(),
                supabase_upload_interval_minutes: interval.text().parse().unwrap_or(30),
                idle_threshold_seconds: idle_input.text().parse().unwrap_or(120),
                start_with_windows: start_with_windows.check_state() == nwg::CheckBoxState::Checked,
                start_minimized_to_tray: start_minimized.check_state() == nwg::CheckBoxState::Checked,
            };
            {
                let mut guard = settings_for_save.lock().unwrap();
                *guard = s.clone();
            }
            save_settings(&s);
            if let Err(e) = startup::set_run_at_startup(s.start_with_windows) {
                eprintln!("[chronos] set startup failed: {}", e);
            }
            let clamped = s.idle_threshold_seconds_clamped();
            idle_input.set_text(&clamped.to_string());
            nwg::modal_info_message(&window_handle, "Saved", "Settings saved successfully.");
            return;
        }

        if handle == test_btn_handle {
            let s = AppSettings {
                enable_supabase_sync: enable_sync.check_state() == nwg::CheckBoxState::Checked,
                supabase_url: url.text(),
                supabase_anon_key: anon_key.text(),
                supabase_user_id: user_id.text(),
                supabase_upload_interval_minutes: interval.text().parse().unwrap_or(30),
                idle_threshold_seconds: idle_input.text().parse().unwrap_or(120),
                start_with_windows: start_with_windows.check_state() == nwg::CheckBoxState::Checked,
                start_minimized_to_tray: start_minimized.check_state() == nwg::CheckBoxState::Checked,
            };
            if s.supabase_url.is_empty() || s.supabase_anon_key.is_empty() || s.supabase_user_id.is_empty() {
                nwg::modal_info_message(&window_handle, "Error", "Please set URL, Anon Key, and User ID.");
                return;
            }
            if uuid::Uuid::parse_str(s.supabase_user_id.trim()).is_err() {
                nwg::modal_info_message(&window_handle, "Error", "User ID must be a valid UUID.");
                return;
            }

            test_btn.set_text("Testing...");
            test_btn.set_enabled(false);

            let to_upload = data_test.lock().unwrap().clone();
            let device_id = std::env::var("COMPUTERNAME").unwrap_or_else(|_| "PC".to_string());
            let test_result_clone = Arc::clone(&test_result_for_test);

            std::thread::spawn(move || {
                let rt = match tokio::runtime::Runtime::new() {
                    Ok(r) => r,
                    Err(e) => {
                        let mut guard = test_result_clone.lock().unwrap();
                        *guard = Some(Err(format!("Failed to start runtime: {}", e)));
                        return;
                    }
                };
                let result = rt.block_on(supabase::upload_screentime_data(
                    &to_upload,
                    &s.supabase_url,
                    &s.supabase_anon_key,
                    &s.supabase_user_id,
                    &device_id,
                    0, // Bypass time-gating checks
                ));
                let mut guard = test_result_clone.lock().unwrap();
                *guard = Some(Ok(result));
            });
        }
    });

    nwg::dispatch_thread_events();
}


