//! Windows startup registration helpers (HKCU\Run).
#![cfg(windows)]

use std::io::ErrorKind;
use winreg::enums::{HKEY_CURRENT_USER, KEY_READ, KEY_SET_VALUE};
use winreg::RegKey;

const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const VALUE_NAME: &str = "ChronosScreentime";

fn current_exe_normalized() -> Option<String> {
    let exe = std::env::current_exe().ok()?;
    Some(exe.to_string_lossy().replace('/', "\\"))
}

fn normalize_path(value: &str) -> String {
    value.trim_matches('"').replace('/', "\\")
}

/// Returns true if the HKCU Run entry exists and points to this exe.
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

/// Enable or disable "Start with Windows" using HKCU\Run.
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
        // Ignore missing value; bubble up other errors.
        if e.kind() != ErrorKind::NotFound {
            return Err(format!("registry delete: {}", e));
        }
    }
    Ok(())
}
