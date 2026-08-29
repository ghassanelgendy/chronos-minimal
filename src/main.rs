//! Chronos Screentime – lightweight cross-platform screentime tracker with Supabase sync.

#![allow(dead_code)]

mod category;
mod models;
mod storage;
mod supabase;
mod tracker;
mod ui;
mod startup;

use crate::storage::{load_settings, load_screen_time_data, save_screen_time_data};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

pub static STARTUP_TIME: std::sync::OnceLock<chrono::DateTime<chrono::Local>> = std::sync::OnceLock::new();

#[cfg(windows)]
type TrayHICON = isize;

/// Resolve path to icon.ico (next to exe or in current dir).
#[cfg(windows)]
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

#[cfg(windows)]
fn load_tray_icon_handle() -> Option<TrayHICON> {
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::HINSTANCE;
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::UI::WindowsAndMessaging::{LoadImageW, LR_DEFAULTSIZE, LR_LOADFROMFILE, IMAGE_ICON};

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

#[cfg(target_os = "linux")]
struct LinuxTray {
    data: Arc<std::sync::Mutex<crate::models::ScreenTimeData>>,
    settings: Arc<std::sync::Mutex<crate::models::AppSettings>>,
    tracking_enabled: Arc<AtomicBool>,
    tracker_running: Arc<AtomicBool>,
    show_dashboard_flag: Arc<AtomicBool>,
}

#[cfg(target_os = "linux")]
impl ksni::Tray for LinuxTray {
    fn id(&self) -> String {
        "chronos-screentime".to_string()
    }
    fn title(&self) -> String {
        "Chronos Screentime".to_string()
    }
    fn status(&self) -> ksni::Status {
        ksni::Status::Active
    }
    fn icon_name(&self) -> String {
        "chronos-screentime".to_string()
    }
    fn category(&self) -> ksni::Category {
        ksni::Category::ApplicationStatus
    }
    fn activate(&mut self, _x: i32, _y: i32) {
        self.show_dashboard_flag.store(true, Ordering::SeqCst);
    }
    fn icon_theme_path(&self) -> String {
        if let Some(base) = directories::BaseDirs::new() {
            base.data_local_dir().join("icons").to_string_lossy().to_string()
        } else {
            String::new()
        }
    }
    fn icon_pixmap(&self) -> Vec<ksni::Icon> {
        let bytes = include_bytes!("../icon-9.png");
        let Ok(img) = image::load_from_memory_with_format(bytes, image::ImageFormat::Png) else {
            return Vec::new();
        };

        let sizes: [u32; 7] = [16, 22, 24, 32, 48, 64, 256];
        let mut icons = Vec::with_capacity(sizes.len());

        for size in sizes {
            let resized = if img.width() == size && img.height() == size {
                img.to_rgba8()
            } else {
                image::imageops::resize(&img, size, size, image::imageops::FilterType::Triangle)
            };

            let mut argb_data = Vec::with_capacity((size * size * 4) as usize);
            for pixel in resized.pixels() {
                argb_data.push(pixel[3]); // Alpha
                argb_data.push(pixel[0]); // Red
                argb_data.push(pixel[1]); // Green
                argb_data.push(pixel[2]); // Blue
            }

            icons.push(ksni::Icon {
                width: size as i32,
                height: size as i32,
                data: argb_data,
            });
        }

        icons
    }
    fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
        use ksni::menu::*;
        let is_tracking = self.tracking_enabled.load(Ordering::SeqCst);
        let tracking_label = if is_tracking { "Pause Tracking" } else { "Resume Tracking" };

        vec![
            StandardItem {
                label: "Chronos Screentime".to_string(),
                enabled: false,
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            StandardItem {
                label: "Open Dashboard".to_string(),
                activate: Box::new(|this: &mut Self| {
                    this.show_dashboard_flag.store(true, Ordering::SeqCst);
                }),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: tracking_label.to_string(),
                activate: Box::new(|this: &mut Self| {
                    let current = this.tracking_enabled.load(Ordering::SeqCst);
                    this.tracking_enabled.store(!current, Ordering::SeqCst);
                }),
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            StandardItem {
                label: "Quit Chronos".to_string(),
                activate: Box::new(|this: &mut Self| {
                    this.tracker_running.store(false, Ordering::SeqCst);
                    std::process::exit(0);
                }),
                ..Default::default()
            }
            .into(),
        ]
    }
}

#[cfg(target_os = "linux")]
fn init_linux_tray(
    data: Arc<std::sync::Mutex<crate::models::ScreenTimeData>>,
    settings: Arc<std::sync::Mutex<crate::models::AppSettings>>,
    tracking_enabled: Arc<AtomicBool>,
    tracker_running: Arc<AtomicBool>,
    show_dashboard_flag: Arc<AtomicBool>,
) {
    use ksni::TrayMethods;
    let tray = LinuxTray {
        data,
        settings,
        tracking_enabled,
        tracker_running,
        show_dashboard_flag,
    };
    std::thread::spawn(move || {
        if let Ok(rt) = tokio::runtime::Runtime::new() {
            rt.block_on(async move {
                match tray.spawn().await {
                    Ok(_handle) => {
                        println!("[chronos] Linux AppIndicator tray registered successfully");
                        std::future::pending::<()>().await;
                    }
                    Err(e) => {
                        eprintln!("[chronos] Warning: Could not register AppIndicator tray: {:?}", e);
                    }
                }
            });
        }
    });
}

fn main() {
    STARTUP_TIME.set(chrono::Local::now()).ok();
    let args: Vec<String> = std::env::args().collect();
    if args.contains(&"--test-sync".to_string()) {
        println!("Running sync test...");
        let settings = load_settings();
        let data = load_screen_time_data();
        let device_id = std::env::var("COMPUTERNAME")
            .or_else(|_| std::env::var("HOSTNAME"))
            .unwrap_or_else(|_| "PC".to_string());
        println!(
            "Settings: URL={}, Key_len={}, User={}",
            settings.supabase_url,
            settings.supabase_anon_key.len(),
            settings.supabase_user_id
        );
        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(crate::supabase::upload_screentime_data(
            &data,
            &settings.supabase_url,
            &settings.supabase_anon_key,
            &settings.supabase_user_id,
            &device_id,
            0,
        ));
        println!(
            "Result: success={}, error={:?}, apps={}, webs={}",
            result.success, result.error_message, result.apps_inserted, result.websites_inserted
        );
        return;
    }
    if args.contains(&"--test-tracker".to_string()) {
        println!("Testing live tracker for 5 seconds...");
        for i in 1..=5 {
            let idle = crate::tracker::get_idle_seconds();
            let fg = crate::tracker::get_foreground_info();
            println!("[Tick {}] Idle: {:?}s, Activity: {:?}", i, idle, fg);
            std::thread::sleep(Duration::from_secs(1));
        }
        return;
    }

    let settings = Arc::new(std::sync::Mutex::new(load_settings()));
    let data = Arc::new(std::sync::Mutex::new(load_screen_time_data()));
    let tracking_enabled = Arc::new(AtomicBool::new(true));
    let tracker_running = Arc::new(AtomicBool::new(true));

    #[cfg(not(windows))]
    crate::startup::ensure_icon_installed();

    // Ensure autostart entry matches saved preference.
    {
        let desired_startup = settings.lock().unwrap().start_with_windows;
        if startup::is_run_at_startup_enabled() != desired_startup {
            if let Err(e) = startup::set_run_at_startup(desired_startup) {
                eprintln!("[chronos] startup registration failed: {}", e);
            }
        }
    }

    // Tracker thread: poll active window, accumulate time
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
                let device_id = std::env::var("COMPUTERNAME")
                    .or_else(|_| std::env::var("HOSTNAME"))
                    .unwrap_or_else(|_| "PC".to_string());
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

    let show_dashboard_flag = Arc::new(AtomicBool::new(false));
    let select_tab_index = Arc::new(std::sync::atomic::AtomicUsize::new(0));

    #[cfg(windows)]
    let _tray = init_tray(
        Arc::clone(&data),
        Arc::clone(&settings),
        Arc::clone(&show_dashboard_flag),
        Arc::clone(&select_tab_index),
        Arc::clone(&tracker_running),
    );

    #[cfg(target_os = "linux")]
    let _linux_tray = init_linux_tray(
        Arc::clone(&data),
        Arc::clone(&settings),
        Arc::clone(&tracking_enabled),
        Arc::clone(&tracker_running),
        Arc::clone(&show_dashboard_flag),
    );

    let start_minimized = args.contains(&"--minimized".to_string())
        || settings.lock().unwrap().start_minimized_to_tray;

    // Launch Dashboard window
    let tab_to_select = select_tab_index.load(Ordering::SeqCst);
    crate::ui::show_dashboard_window(
        data.clone(),
        settings.clone(),
        tracking_enabled.clone(),
        tracker_running.clone(),
        Arc::clone(&show_dashboard_flag),
        Some(tab_to_select),
        start_minimized,
    );
}
