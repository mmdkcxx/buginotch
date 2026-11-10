// SPDX-License-Identifier: GPL-3.0-or-later
//! Watches the system volume (default output) and the built-in panel's brightness on a
//! worker thread, and wakes the UI thread when either changes.

use crate::window::WM_APP_MEDIA;
use isle_logic::osd::Reading;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use windows::{
    Win32::{
        Foundation::*,
        Media::Audio::{Endpoints::IAudioEndpointVolume, *},
        Storage::FileSystem::*,
        System::{Com::*, IO::DeviceIoControl, Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS}},
        UI::WindowsAndMessaging::{FindWindowW, IsWindowVisible, PostMessageW, SW_HIDE, ShowWindow},
    },
    core::w,
};

/// How often the system is asked. Two cheap calls; a key press shows within this long.
const POLL: Duration = Duration::from_millis(125);
/// The default output can change (headphones plugged in); look it up again this often.
const REFRESH_EVERY: u32 = 16;
/// `IOCTL_VIDEO_QUERY_DISPLAY_BRIGHTNESS`
const IOCTL_QUERY_BRIGHTNESS: u32 = 0x0023_0498;

static HIDE_NATIVE: AtomicBool = AtomicBool::new(false);

/// Turns on or off hiding Windows' own volume / brightness popup (best effort).
pub fn set_hide_native(on: bool) {
    HIDE_NATIVE.store(on, Ordering::Relaxed);
}

/// The popup lives in a window of class `NativeHWNDHost` that the shell shows on each key
/// press. Hiding it right after it appears is the only known way; newer Windows builds may
/// host it elsewhere, in which case this finds nothing and does no harm.
fn hide_native_popup() {
    unsafe {
        if let Ok(w) = FindWindowW(w!("NativeHWNDHost"), None) {
            if IsWindowVisible(w).as_bool() {
                let _ = ShowWindow(w, SW_HIDE);
            }
        }
    }
}

#[derive(Clone, Default)]
pub struct OsdWatcher {
    latest: Arc<Mutex<Reading>>,
}

impl OsdWatcher {
    pub fn spawn(hwnd: HWND) -> Self {
        let watcher = Self::default();
        let slot = Arc::clone(&watcher.latest);
        let hwnd = hwnd.0 as isize;
        std::thread::spawn(move || {
            let _ = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
            let mut endpoint: Option<IAudioEndpointVolume> = None;
            let mut tick = 0u32;
            let mut prev: Option<Reading> = None;
            loop {
                if endpoint.is_none() || tick % REFRESH_EVERY == 0 {
                    endpoint = default_endpoint();
                }
                tick = tick.wrapping_add(1);
                let now = Reading { volume: endpoint.as_ref().and_then(read_volume), brightness: read_brightness() };
                if prev != Some(now) {
                    *slot.lock().unwrap() = now;
                    // The very first reading is only a baseline; waking the UI for it is harmless.
                    unsafe {
                        let _ = PostMessageW(Some(HWND(hwnd as *mut _)), WM_APP_MEDIA, WPARAM(0), LPARAM(0));
                    }
                    prev = Some(now);
                }
                if HIDE_NATIVE.load(Ordering::Relaxed) {
                    hide_native_popup();
                }
                std::thread::sleep(POLL);
            }
        });
        watcher
    }

    pub fn latest(&self) -> Reading {
        *self.latest.lock().unwrap()
    }
}

fn default_endpoint() -> Option<IAudioEndpointVolume> {
    unsafe {
        let enumerator: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).ok()?;
        let device = enumerator.GetDefaultAudioEndpoint(eRender, eMultimedia).ok()?;
        device.Activate::<IAudioEndpointVolume>(CLSCTX_ALL, None).ok()
    }
}

fn read_volume(ep: &IAudioEndpointVolume) -> Option<(u8, bool)> {
    unsafe {
        let level = ep.GetMasterVolumeLevelScalar().ok()?;
        let muted = ep.GetMute().ok()?.as_bool();
        Some(((level * 100.0).round().clamp(0.0, 100.0) as u8, muted))
    }
}

/// Brightness (0..=100) of the built-in panel for the current power source, or `None` on a
/// machine without one (a desktop, or an external monitor only).
fn read_brightness() -> Option<u8> {
    unsafe {
        let h = CreateFileW(
            w!("\\\\.\\LCD"),
            0x8000_0000, // GENERIC_READ
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            None,
            OPEN_EXISTING,
            FILE_ATTRIBUTE_NORMAL,
            None,
        )
        .ok()?;
        let mut out = [0u8; 3]; // policy, AC level, DC level
        let mut got = 0u32;
        let ok = DeviceIoControl(h, IOCTL_QUERY_BRIGHTNESS, None, 0, Some(out.as_mut_ptr().cast()), 3, Some(&mut got), None).is_ok();
        let _ = CloseHandle(h);
        if !ok || got < 3 {
            return None;
        }
        let mut s = SYSTEM_POWER_STATUS::default();
        let on_ac = GetSystemPowerStatus(&mut s).is_ok() && s.ACLineStatus == 1;
        Some(if on_ac { out[1] } else { out[2] }.min(100))
    }
}
