//! Chronos Screentime – lightweight Windows tracker with Supabase sync.
//! Build: cargo build --release  → target/release/chronos-screentime.exe

#![cfg(windows)]
#![windows_subsystem = "windows"]

mod category;
mod models;
mod storage;
mod supabase;
mod tracker;
mod ui;

use crate::storage::{load_settings, load_screen_time_data, save_screen_time_data};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

fn main() {
    if let Err(e) = native_windows_gui::init() {
        // No GUI - show Win32 message box so user sees something
        show_error_messagebox(&format!("Chronos failed to start: {}", e));
        std::process::exit(1);
    }
    let settings = Arc::new(std::sync::Mutex::new(load_settings()));
    let data = Arc::new(std::sync::Mutex::new(load_screen_time_data()));
    let tracking_enabled = Arc::new(AtomicBool::new(true));
    let tracker_running = Arc::new(AtomicBool::new(true));

    // Tracker thread: poll foreground window, accumulate time
    let data_tracker = data.clone();
    let settings_tracker = settings.clone();
    let tracking_enabled_tracker = tracking_enabled.clone();
    let running_tracker = tracker_running.clone();
    std::thread::spawn(move || {
        crate::tracker::run_tracker_loop(
            data_tracker,
            settings_tracker,
            tracking_enabled_tracker,
            running_tracker,
        );
    });

    // Upload thread: every N minutes, if sync enabled, upload to Supabase
    let data_upload = data.clone();
    let settings_upload = settings.clone();
    std::thread::spawn(move || {
        let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
        rt.block_on(async {
            let mut interval = tokio::time::interval(Duration::from_secs(60));
            let mut initial_delay = 30u64;
            loop {
                tokio::time::sleep(Duration::from_secs(initial_delay)).await;
                initial_delay = 0;
                interval.tick().await;
                let (url, key, user_id, interval_mins, enabled) = {
                    let s = settings_upload.lock().unwrap();
                    (
                        s.supabase_url.clone(),
                        s.supabase_anon_key.clone(),
                        s.supabase_user_id.clone(),
                        s.upload_interval_minutes_clamped(),
                        s.enable_supabase_sync,
                    )
                };
                if !enabled || url.is_empty() || key.is_empty() || user_id.trim().is_empty() {
                    continue;
                }
                let device_id = std::env::var("COMPUTERNAME").unwrap_or_else(|_| "PC".to_string());
                let to_upload = data_upload.lock().unwrap().clone();
                let _ = save_screen_time_data(&to_upload);
                let result = crate::supabase::upload_screentime_data(
                    &to_upload,
                    &url,
                    &key,
                    user_id.trim(),
                    &device_id,
                    interval_mins,
                )
                .await;
                if !result.success {
                    eprintln!("[chronos] upload failed: {:?}", result.error_message);
                }
            }
        });
    });

    // Flag set by tray "Show Dashboard" → main loop reopens the window.
    let show_dashboard_flag = Arc::new(AtomicBool::new(false));

    // Tray: use resource "1" when icon.ico was embedded at build time, else ""
    let data_tray = data.clone();
    let settings_tray = settings.clone();
    let data_today = data.clone();
    let show_flag_tray = Arc::clone(&show_dashboard_flag);
    let tray = tray_item::TrayItem::new("Chronos Screentime", tray_item::IconSource::Resource("1"))
        .or_else(|_| tray_item::TrayItem::new("Chronos Screentime", tray_item::IconSource::Resource("")));
    // IMPORTANT: keep _tray alive for the whole lifetime of main so the icon persists.
    let _tray = match tray {
        Ok(mut t) => {
            let _ = t.add_menu_item("Show Dashboard", move || {
                show_flag_tray.store(true, Ordering::SeqCst);
            });
            let _ = t.add_menu_item("Today's summary", move || {
                crate::ui::show_today_window(Arc::clone(&data_today));
            });
            let _ = t.add_menu_item("Preferences (Supabase sync)", move || {
                crate::ui::open_settings_window_async(
                    Arc::clone(&data_tray),
                    Arc::clone(&settings_tray),
                );
            });
            let running_tray_exit = tracker_running.clone();
            let _ = t.add_menu_item("Exit", move || {
                running_tray_exit.store(false, Ordering::SeqCst);
                std::process::exit(0);
            });
            Some(t)
        }
        Err(_) => None,
    };

    // Show dashboard; reopen whenever the tray requests it.
    // show_dashboard_window blocks on nwg::dispatch_thread_events() and returns
    // when the user closes or minimises-to-tray the window.
    loop {
        show_dashboard_flag.store(false, Ordering::SeqCst);
        crate::ui::show_dashboard_window(
            data.clone(),
            settings.clone(),
            tracking_enabled.clone(),
            tracker_running.clone(),
        );
        // Dashboard closed – spin until tray asks to reopen, or Exit is triggered.
        loop {
            if !tracker_running.load(Ordering::SeqCst) {
                let to_save = data.lock().unwrap().clone();
                let _ = save_screen_time_data(&to_save);
                std::process::exit(0);
            }
            if show_dashboard_flag.load(Ordering::SeqCst) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(200));
        }
    }
}

/// Show error in a Win32 message box (works even if NWG fails).
fn show_error_messagebox(text: &str) {
    use windows::core::PCWSTR;
    use windows::Win32::UI::WindowsAndMessaging::MessageBoxW;
    use windows::Win32::UI::WindowsAndMessaging::MB_OK;
    use std::os::windows::ffi::OsStrExt;
    let msg: Vec<u16> = std::ffi::OsStr::new(text).encode_wide().chain(Some(0)).collect();
    let title: Vec<u16> = std::ffi::OsStr::new("Chronos Screentime").encode_wide().chain(Some(0)).collect();
    unsafe {
        MessageBoxW(None, PCWSTR(msg.as_ptr()), PCWSTR(title.as_ptr()), MB_OK);
    }
}
