// SPDX-License-Identifier: GPL-3.0-or-later
//! Small Windows system helpers.

use windows::Win32::System::SystemInformation::GetLocalTime;

pub struct LocalTime {
    pub year: i32,
    pub month: u32,
    pub day: u32,
    /// 0 = Sunday.
    pub weekday: u32,
    pub hour: u32,
    pub minute: u32,
    pub second: u32,
}

pub fn local_time() -> LocalTime {
    let t = unsafe { GetLocalTime() };
    LocalTime {
        year: t.wYear as i32,
        month: t.wMonth as u32,
        day: t.wDay as u32,
        weekday: t.wDayOfWeek as u32,
        hour: t.wHour as u32,
        minute: t.wMinute as u32,
        second: t.wSecond as u32,
    }
}

/// Wall-clock milliseconds since the Unix epoch; the time base for modules.
pub fn epoch_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

use windows::{
    Win32::{
        Foundation::{E_FAIL, ERROR_ALREADY_EXISTS, GetLastError},
        System::{Registry::*, Threading::CreateMutexW},
        UI::Shell::{QUNS_BUSY, QUNS_PRESENTATION_MODE, QUNS_RUNNING_D3D_FULL_SCREEN, SHQueryUserNotificationState},
    },
    core::*,
};

const RUN_KEY: PCWSTR = w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run");
const RUN_VALUE: PCWSTR = w!("IsleWin");

/// Returns false when another Isle is already running (named mutex).
pub fn acquire_single_instance() -> bool {
    unsafe {
        match CreateMutexW(None, false, w!("Local\\IsleWin")) {
            Ok(handle) => {
                let exists = GetLastError() == ERROR_ALREADY_EXISTS;
                // Keep the handle open for the process lifetime.
                std::mem::forget(handle);
                !exists
            }
            Err(_) => true,
        }
    }
}

pub fn is_autostart() -> bool {
    unsafe {
        let mut key = HKEY::default();
        if RegOpenKeyExW(HKEY_CURRENT_USER, RUN_KEY, None, KEY_QUERY_VALUE, &mut key).is_err() {
            return false;
        }
        let found = RegQueryValueExW(key, RUN_VALUE, None, None, None, None).is_ok();
        let _ = RegCloseKey(key);
        found
    }
}

/// Adds or removes `HKCU\...\Run\IsleWin` pointing at this exe.
pub fn set_autostart(enabled: bool) -> Result<()> {
    unsafe {
        let mut key = HKEY::default();
        RegOpenKeyExW(HKEY_CURRENT_USER, RUN_KEY, None, KEY_SET_VALUE, &mut key).ok()?;
        let result = if enabled {
            let exe = std::env::current_exe().map_err(|_| Error::from(E_FAIL))?;
            let quoted = format!("\"{}\"", exe.display());
            let wide: Vec<u16> = quoted.encode_utf16().chain(std::iter::once(0)).collect();
            let bytes = std::slice::from_raw_parts(wide.as_ptr() as *const u8, wide.len() * 2);
            RegSetValueExW(key, RUN_VALUE, None, REG_SZ, Some(bytes)).ok()
        } else {
            let r = RegDeleteValueW(key, RUN_VALUE);
            if r == windows::Win32::Foundation::ERROR_FILE_NOT_FOUND { Ok(()) } else { r.ok() }
        };
        let _ = RegCloseKey(key);
        result
    }
}

/// True while a fullscreen app, game or presentation is in the foreground.
pub fn fullscreen_app_active() -> bool {
    unsafe {
        match SHQueryUserNotificationState() {
            Ok(s) => s == QUNS_BUSY || s == QUNS_RUNNING_D3D_FULL_SCREEN || s == QUNS_PRESENTATION_MODE,
            Err(_) => false,
        }
    }
}

/// `%APPDATA%\IsleWin\<file>`.
pub fn data_path(file: &str) -> std::path::PathBuf {
    let base = std::env::var_os("APPDATA").map(std::path::PathBuf::from).unwrap_or_else(std::env::temp_dir);
    base.join("IsleWin").join(file)
}
