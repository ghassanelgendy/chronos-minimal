//! Startup registration helpers (HKCU\Run on Windows, ~/.config/autostart desktop entry on Linux).

#[cfg(windows)]
use winreg::enums::{HKEY_CURRENT_USER, KEY_READ, KEY_SET_VALUE};
#[cfg(windows)]
use winreg::RegKey;

#[cfg(windows)]
const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
#[cfg(windows)]
const VALUE_NAME: &str = "ChronosScreentime";

#[cfg(windows)]
fn current_exe_normalized() -> Option<String> {
    let exe = std::env::current_exe().ok()?;
    Some(exe.to_string_lossy().replace('/', "\\"))
}

#[cfg(windows)]
fn normalize_path(value: &str) -> String {
    value.trim_matches('"').replace('/', "\\")
}

#[cfg(windows)]
pub fn is_run_at_startup_enabled() -> bool {
    let Ok(hkcu) = RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey_with_flags(RUN_KEY, KEY_READ)
    else {
        return false;
    };
    let Ok(value) = hkcu.get_value::<String, _>(VALUE_NAME) else {
        return false;
    };
    if let Some(exe) = current_exe_normalized() {
        return normalize_path(&value).eq_ignore_ascii_case(&exe);
    }
    false
}

#[cfg(windows)]
pub fn set_run_at_startup(enabled: bool) -> Result<(), String> {
    let exe = current_exe_normalized().ok_or_else(|| "Cannot resolve exe path".to_string())?;
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let (key, _) = hkcu
        .create_subkey_with_flags(RUN_KEY, KEY_SET_VALUE)
        .map_err(|e| format!("registry open: {}", e))?;

    if enabled {
        let quoted = format!("\"{}\"", exe);
        key.set_value(VALUE_NAME, &quoted)
            .map_err(|e| format!("registry set: {}", e))?;
    } else if let Err(e) = key.delete_value(VALUE_NAME) {
        if e.kind() != std::io::ErrorKind::NotFound {
            return Err(format!("registry delete: {}", e));
        }
    }
    Ok(())
}

/// Windows stub – app-drawer concept doesn't apply.
#[cfg(windows)]
pub fn is_app_entry_installed() -> bool { false }

/// Windows stub – app-drawer concept doesn't apply.
#[cfg(windows)]
pub fn install_app_entry() -> Result<(), String> { Ok(()) }

/// Windows stub.
#[cfg(windows)]
pub fn uninstall_app_entry() -> Result<(), String> { Ok(()) }

// ── Linux helpers ──────────────────────────────────────────────────────────────

#[cfg(not(windows))]
const APP_ID: &str = "chronos-screentime";

/// Path to the bundled PNG icon (next to the executable or in the project root).
#[cfg(not(windows))]
pub fn find_bundled_icon() -> Option<std::path::PathBuf> {
    // 1. Next to the running binary (installed location).
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let p = dir.join("icon-9.png");
            if p.exists() {
                return Some(p);
            }
        }
    }
    // 2. Current working dir (development / cargo run).
    let p = std::path::PathBuf::from("icon-9.png");
    if p.exists() {
        return Some(p);
    }
    None
}

/// Install the icon to standard Freedesktop icon theme paths in all common resolutions.
#[cfg(not(windows))]
pub fn ensure_icon_installed() -> String {
    let icon_bytes = include_bytes!("../icon-9.png");
    if let Some(base) = directories::BaseDirs::new() {
        let local_data = base.data_local_dir();
        let primary_icon = local_data.join("icons").join("hicolor").join("256x256").join("apps").join(format!("{}.png", APP_ID));

        if let Ok(img) = image::load_from_memory_with_format(icon_bytes, image::ImageFormat::Png) {
            let sizes = [16, 22, 24, 32, 48, 64, 128, 256];
            for size in sizes {
                let path = local_data.join("icons").join("hicolor").join(format!("{}x{}", size, size)).join("apps").join(format!("{}.png", APP_ID));
                if let Some(parent) = path.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                if size == 256 {
                    let _ = std::fs::write(&path, icon_bytes);
                } else {
                    let resized = image::imageops::resize(&img, size, size, image::imageops::FilterType::Triangle);
                    let mut buf = Vec::new();
                    if resized.write_to(&mut std::io::Cursor::new(&mut buf), image::ImageFormat::Png).is_ok() {
                        let _ = std::fs::write(&path, buf);
                    }
                }
            }
        }

        let extra_targets = [
            local_data.join("pixmaps").join(format!("{}.png", APP_ID)),
            local_data.join("icons").join(format!("{}.png", APP_ID)),
        ];

        for target in &extra_targets {
            if let Some(parent) = target.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let _ = std::fs::write(target, icon_bytes);
        }

        let hicolor_dir = local_data.join("icons").join("hicolor");
        let _ = std::process::Command::new("gtk-update-icon-cache")
            .args(["-f", "-t", &hicolor_dir.to_string_lossy()])
            .status();

        return primary_icon.to_string_lossy().to_string();
    }
    APP_ID.to_string()
}

#[cfg(not(windows))]
fn get_autostart_desktop_path() -> Option<std::path::PathBuf> {
    let config_dir = directories::BaseDirs::new()?.config_dir().to_path_buf();
    Some(config_dir.join("autostart").join(format!("{}.desktop", APP_ID)))
}

/// Path for the app-drawer entry: ~/.local/share/applications/chronos-screentime.desktop
#[cfg(not(windows))]
fn get_applications_desktop_path() -> Option<std::path::PathBuf> {
    let base = directories::BaseDirs::new()?;
    Some(base.data_local_dir().join("applications").join(format!("{}.desktop", APP_ID)))
}

/// Build the full .desktop file contents.
#[cfg(not(windows))]
fn desktop_entry_content(exe: &std::path::Path, icon: &str) -> String {
    format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Name=Chronos Screentime\n\
         GenericName=Screen Time Tracker\n\
         Comment=Lightweight screen-time tracker with Supabase sync\n\
         Exec=\"{exe}\"\n\
         Icon={icon}\n\
         Terminal=false\n\
         Categories=Utility;Clock;Monitor;\n\
         StartupWMClass=chronos-screentime\n\
         Keywords=screentime;productivity;tracker;chronos;time;\n\
         StartupNotify=true\n",
        exe = exe.display(),
        icon = icon,
    )
}

// ── App-drawer entry ────────────────────────────────────────────────────────

#[cfg(not(windows))]
pub fn is_app_entry_installed() -> bool {
    get_applications_desktop_path().map_or(false, |p| p.exists())
}

/// Install a .desktop entry into ~/.local/share/applications/ so Chronos
/// appears in the GNOME/app drawer. Also installs the icon.
#[cfg(not(windows))]
pub fn install_app_entry() -> Result<(), String> {
    let icon = ensure_icon_installed();
    let desktop_path = get_applications_desktop_path()
        .ok_or_else(|| "Cannot resolve data-local dir".to_string())?;
    if let Some(parent) = desktop_path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("failed creating applications dir: {}", e))?;
    }
    let exe = std::env::current_exe()
        .map_err(|e| format!("failed resolving exe path: {}", e))?;
    let content = desktop_entry_content(&exe, &icon);
    std::fs::write(&desktop_path, &content)
        .map_err(|e| format!("failed writing applications desktop entry: {}", e))?;
    // Notify the shell (best-effort)
    let _ = std::process::Command::new("update-desktop-database")
        .arg(desktop_path.parent().unwrap_or(std::path::Path::new(".")))
        .status();
    Ok(())
}

/// Remove the .desktop entry from ~/.local/share/applications/.
#[cfg(not(windows))]
pub fn uninstall_app_entry() -> Result<(), String> {
    if let Some(p) = get_applications_desktop_path() {
        if p.exists() {
            std::fs::remove_file(&p)
                .map_err(|e| format!("failed removing applications desktop entry: {}", e))?;
            let _ = std::process::Command::new("update-desktop-database")
                .arg(p.parent().unwrap_or(std::path::Path::new(".")))
                .status();
        }
    }
    Ok(())
}

// ── Autostart entry ─────────────────────────────────────────────────────────

#[cfg(not(windows))]
pub fn is_run_at_startup_enabled() -> bool {
    get_autostart_desktop_path().map_or(false, |p| p.exists())
}

#[cfg(not(windows))]
pub fn set_run_at_startup(enabled: bool) -> Result<(), String> {
    let desktop_path = get_autostart_desktop_path()
        .ok_or_else(|| "Cannot resolve config dir".to_string())?;
    if enabled {
        if let Some(parent) = desktop_path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("failed creating autostart dir: {}", e))?;
        }
        let icon = ensure_icon_installed();
        let exe = std::env::current_exe()
            .map_err(|e| format!("failed resolving exe path: {}", e))?;
        let content = desktop_entry_content(&exe, &icon);
        std::fs::write(&desktop_path, content)
            .map_err(|e| format!("failed writing autostart entry: {}", e))?;
    } else if desktop_path.exists() {
        std::fs::remove_file(&desktop_path)
            .map_err(|e| format!("failed removing autostart entry: {}", e))?;
    }
    Ok(())
}

