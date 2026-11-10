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
        UI::WindowsAndMessaging::{FindWindowExW, IsIconic, IsWindowVisible, PostMessageW, SW_MINIMIZE, ShowWindow},
    },
    core::{PCWSTR, w},
};

/// How often the system is asked. Two cheap calls; a key press shows within this long.
const POLL: Duration = Duration::from_millis(40);
/// Readings are taken every this many ticks (about 120 ms); the popup check runs every tick.
const READ_EVERY: u32 = 3;
/// The default output can change (headphones plugged in); look it up again this often.
const REFRESH_EVERY: u32 = 16;
/// `IOCTL_VIDEO_QUERY_DISPLAY_BRIGHTNESS`
const IOCTL_QUERY_BRIGHTNESS: u32 = 0x0023_0498;

static HIDE_NATIVE: AtomicBool = AtomicBool::new(false);

/// Turns on or off hiding Windows' own volume / brightness popup (best effort).
pub fn set_hide_native(on: bool) {
    HIDE_NATIVE.store(on, Ordering::Relaxed);
}

/// Windows' own volume / brightness popup is a shell window the system shows on each key
/// press: `NativeHWNDHost` on Windows 10, an untitled `XamlExplorerHostIslandWindow` (with a
/// `DesktopWindowContentBridge` inside) on Windows 11. Minimizing it when it appears hides
/// the popup, the same way the HideVolumeOSD tool does. Finds nothing, and does nothing,
/// where Windows hosts it differently.
fn hide_native_popup() {
    unsafe {
        for (class, inner) in [(w!("NativeHWNDHost"), w!("DirectUIHWND")), (w!("XamlExplorerHostIslandWindow"), w!("Windows.UI.Composition.DesktopWindowContentBridge"))] {
            let mut prev: Option<HWND> = None;
            while let Ok(h) = FindWindowExW(None, prev, class, w!("")) {
                prev = Some(h);
                let is_osd_host = FindWindowExW(Some(h), None, inner, PCWSTR::null()).is_ok();
                if is_osd_host && IsWindowVisible(h).as_bool() && !IsIconic(h).as_bool() {
                    let _ = ShowWindow(h, SW_MINIMIZE);
                }
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
                if HIDE_NATIVE.load(Ordering::Relaxed) {
                    hide_native_popup();
                }
                let hiding = HIDE_NATIVE.load(Ordering::Relaxed);
                if !hiding || tick % READ_EVERY == 0 {
                    if endpoint.is_none() || tick % (READ_EVERY * REFRESH_EVERY) == 0 {
                        endpoint = default_endpoint();
                    }
                    let now = Reading { volume: endpoint.as_ref().and_then(read_volume), brightness: read_brightness() };
                    if prev != Some(now) {
                        *slot.lock().unwrap() = now;
                        // The very first reading is only a baseline; waking the UI for it is harmless.
                        unsafe {
                            let _ = PostMessageW(Some(HWND(hwnd as *mut _)), WM_APP_MEDIA, WPARAM(0), LPARAM(0));
                        }
                        prev = Some(now);
                    }
                }
                tick = tick.wrapping_add(1);
                // Watching for the popup needs a quick beat; otherwise a relaxed one is enough.
                std::thread::sleep(if hiding { POLL } else { POLL * READ_EVERY });
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
