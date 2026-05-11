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
mod startup;

use crate::models::{AppSettings, ScreenTimeData};
use crate::storage::{load_settings, load_screen_time_data, save_screen_time_data};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use windows::core::PCWSTR;
use windows::Win32::Foundation::HINSTANCE;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{LoadImageW, LR_DEFAULTSIZE, LR_LOADFROMFILE, IMAGE_ICON};

type TrayHICON = isize;

/// Resolve path to icon.ico (next to exe or in current dir).
fn tray_icon_path() -> std::path::PathBuf {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let p = dir.join("icon.ico");
            if p.exists() {
                return p;
            }
        }
    }
    if std::path::Path::new("icon.ico").exists() {
        return std::path::PathBuf::from("icon.ico");
    }
    std::path::PathBuf::from("icon.ico")
}

/// Load icon for the tray: first from the embedded exe resource (numeric ID 1),
/// then fall back to icon.ico on disk.
fn load_tray_icon_handle() -> Option<TrayHICON> {
    // Primary: load from embedded resource set by winres in build.rs.
    // MAKEINTRESOURCEW(1) = pointer value 1, which tells the API to look up numeric ID 1.
    unsafe {
        if let Ok(hmod) = GetModuleHandleW(None) {
            let result = LoadImageW(
                HINSTANCE(hmod.0),
                PCWSTR(1 as *const u16),
                IMAGE_ICON,
                0,
                0,
                LR_DEFAULTSIZE,
            );
            if let Ok(h) = result {
                if !h.is_invalid() {
                    return Some(h.0 as isize);
                }
            }
        }
    }

    // Fallback: load from icon.ico file next to exe or in cwd.
    let path = tray_icon_path();
    if !path.exists() {
        return None;
    }
    use std::os::windows::ffi::OsStrExt;
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    unsafe {
        let handle = LoadImageW(
            None,
            PCWSTR(wide.as_ptr()),
            IMAGE_ICON,
            0,
            0,
            LR_LOADFROMFILE | LR_DEFAULTSIZE,
        );
        if let Ok(h) = handle {
            if !h.is_invalid() {
                return Some(h.0 as isize);
            }
        }
    }
    None
}

fn build_tray(
    data: Arc<std::sync::Mutex<ScreenTimeData>>,
    settings: Arc<std::sync::Mutex<AppSettings>>,
    show_flag: Arc<AtomicBool>,
    tracker_running: Arc<AtomicBool>,
    tray_icon_handle: Option<TrayHICON>,
) -> Option<tray_item::TrayItem> {
    // clones for menu callbacks
    let data_today = Arc::clone(&data);
    let data_tray = Arc::clone(&data);
    let settings_tray = Arc::clone(&settings);
    let show_flag_tray = Arc::clone(&show_flag);

    let tray = if let Some(h) = tray_icon_handle {
        tray_item::TrayItem::new("Chronos Screentime", tray_item::IconSource::RawIcon(h))
            .or_else(|_| tray_item::TrayItem::new("Chronos Screentime", tray_item::IconSource::Resource("1")))
    } else {
        tray_item::TrayItem::new("Chronos Screentime", tray_item::IconSource::Resource("1"))
            .or_else(|_| tray_item::TrayItem::new("Chronos Screentime", tray_item::IconSource::Resource("")))
    };

    match tray {
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
        Err(e) => {
            eprintln!("[chronos] tray create failed: {}", e);
            None
        }
    }
}

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

    // Ensure "Start with Windows" registry entry matches saved preference.
    {
        let desired_startup = settings.lock().unwrap().start_with_windows;
        if startup::is_run_at_startup_enabled() != desired_startup {
            if let Err(e) = startup::set_run_at_startup(desired_startup) {
                eprintln!("[chronos] startup registration failed: {}", e);
            }
        }
    }

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
    let tray_restore_flag = Arc::new(AtomicBool::new(true)); // true so tray initializes on boot
    let tray_icon_handle = load_tray_icon_handle();
    let tray_holder: Arc<std::sync::Mutex<Option<tray_item::TrayItem>>> =
        Arc::new(std::sync::Mutex::new(None));

    // Background thread keeps tray alive and recreates it on request (e.g. after Explorer restart).
    {
        let data_tray = Arc::clone(&data);
        let settings_tray = Arc::clone(&settings);
        let show_flag_tray = Arc::clone(&show_dashboard_flag);
        let running_tray = Arc::clone(&tracker_running);
        let restore_flag = Arc::clone(&tray_restore_flag);
        let tray_store = Arc::clone(&tray_holder);
        std::thread::spawn(move || {
            while running_tray.load(Ordering::SeqCst) {
                let needs_restore = restore_flag.swap(false, Ordering::SeqCst);
                let has_tray = { tray_store.lock().unwrap().is_some() };
                if needs_restore || !has_tray {
                    let mut guard = tray_store.lock().unwrap();
                    *guard = build_tray(
                        Arc::clone(&data_tray),
                        Arc::clone(&settings_tray),
                        Arc::clone(&show_flag_tray),
                        Arc::clone(&running_tray),
                        tray_icon_handle,
                    );
                }
                std::thread::sleep(Duration::from_secs(1));
            }
        });
    }
    {
        let mut guard = tray_holder.lock().unwrap();
        if guard.is_none() {
            *guard = build_tray(
                Arc::clone(&data),
                Arc::clone(&settings),
                Arc::clone(&show_dashboard_flag),
                Arc::clone(&tracker_running),
                tray_icon_handle,
            );
        }
        tray_restore_flag.store(false, Ordering::SeqCst);
    }

    // Tray watcher will recreate icon on demand.

    // Show dashboard; reopen whenever the tray requests it.
    // show_dashboard_window blocks on nwg::dispatch_thread_events() and returns
    // when the user closes or minimises-to-tray the window.
    let start_minimized = { settings.lock().unwrap().start_minimized_to_tray };
    let mut first_cycle = true;
    loop {
        show_dashboard_flag.store(false, Ordering::SeqCst);

        let tray_ready = { tray_holder.lock().unwrap().is_some() };
        let skip_window = first_cycle && start_minimized && tray_ready;
        first_cycle = false;

        if !skip_window {
            crate::ui::show_dashboard_window(
                data.clone(),
                settings.clone(),
                tracking_enabled.clone(),
                tracker_running.clone(),
                Arc::clone(&tray_restore_flag),
            );
        }
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
