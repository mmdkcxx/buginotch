// SPDX-License-Identifier: GPL-3.0-or-later
//! Notification-area icon with a small menu (autostart, quit).

use crate::system::{is_autostart, set_autostart};
use windows::{
    Win32::{
        Foundation::*,
        UI::{Shell::*, WindowsAndMessaging::*},
    },
    core::*,
};

pub const WM_APP_TRAY: u32 = WM_APP + 4;
pub const ID_AUTOSTART: usize = 1001;
pub const ID_QUIT: usize = 1002;
pub const ID_UPDATE: usize = 1003;
pub const ID_AUTOUPDATE: usize = 1004;
pub const ID_GLASS: usize = 1005;
pub const ID_GLASS_LIGHT: usize = 1006;
const TRAY_UID: u32 = 1;

pub struct Tray {
    hwnd: HWND,
}

impl Tray {
    pub fn add(hwnd: HWND) -> Result<Tray> {
        unsafe {
            let mut nid = NOTIFYICONDATAW {
                cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
                hWnd: hwnd,
                uID: TRAY_UID,
                uFlags: NIF_ICON | NIF_MESSAGE | NIF_TIP,
                uCallbackMessage: WM_APP_TRAY,
                hIcon: LoadIconW(None, IDI_APPLICATION)?,
                ..Default::default()
            };
            let tip: Vec<u16> = "Buginotch".encode_utf16().collect();
            nid.szTip[..tip.len()].copy_from_slice(&tip);
            if !Shell_NotifyIconW(NIM_ADD, &nid).as_bool() {
                return Err(Error::from(E_FAIL));
            }
            Ok(Tray { hwnd })
        }
    }

    pub fn remove(&self) {
        unsafe {
            let nid = NOTIFYICONDATAW { cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32, hWnd: self.hwnd, uID: TRAY_UID, ..Default::default() };
            let _ = Shell_NotifyIconW(NIM_DELETE, &nid);
        }
    }
}

/// Shows the right-click menu at the cursor.
pub fn show_menu(hwnd: HWND) {
    unsafe {
        let Ok(menu) = CreatePopupMenu() else { return };
        let check = if is_autostart() { MF_CHECKED } else { MF_UNCHECKED };
        let auto_update = if crate::settings_io::load().auto_update { MF_CHECKED } else { MF_UNCHECKED };
        let _ = AppendMenuW(menu, MF_STRING, ID_UPDATE, w!("Periksa pembaruan…"));
        let st = crate::settings_io::load();
        let (dark, light) = (st.glass && !st.glass_light, st.glass && st.glass_light);
        let glass = if dark { MF_CHECKED } else { MF_UNCHECKED };
        let glass_light = if light { MF_CHECKED } else { MF_UNCHECKED };
        let _ = AppendMenuW(menu, MF_STRING | auto_update, ID_AUTOUPDATE, w!("Perbarui otomatis"));
        let _ = AppendMenuW(menu, MF_STRING | glass, ID_GLASS, w!("Notch kaca gelap"));
        let _ = AppendMenuW(menu, MF_STRING | glass_light, ID_GLASS_LIGHT, w!("Notch kaca terang"));
        let _ = AppendMenuW(menu, MF_SEPARATOR, 0, None);
        let _ = AppendMenuW(menu, MF_STRING | check, ID_AUTOSTART, w!("Mulai dengan Windows"));
        let _ = AppendMenuW(menu, MF_SEPARATOR, 0, None);
        let _ = AppendMenuW(menu, MF_STRING, ID_QUIT, w!("Keluar"));
        let mut pt = POINT::default();
        let _ = GetCursorPos(&mut pt);
        let _ = SetForegroundWindow(hwnd);
        let _ = TrackPopupMenu(menu, TPM_RIGHTBUTTON, pt.x, pt.y, None, hwnd, None);
        let _ = PostMessageW(Some(hwnd), WM_NULL, WPARAM(0), LPARAM(0));
        let _ = DestroyMenu(menu);
    }
}

pub fn toggle_autostart() {
    let _ = set_autostart(!is_autostart());
}
