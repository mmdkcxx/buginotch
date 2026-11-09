// SPDX-License-Identifier: GPL-3.0-or-later
//! The borderless, top-most, never-activated notch window.

use isle_logic::layout::{notch_x, target};
use windows::{
    Win32::{
        Foundation::*,
        Graphics::Gdi::*,
        System::LibraryLoader::GetModuleHandleW,
        UI::{HiDpi::*, WindowsAndMessaging::*},
    },
    core::*,
};

pub const WM_APP_WAKE: u32 = WM_APP + 1;
pub const WM_APP_MEDIA: u32 = WM_APP + 2;
pub const WM_APP_LYRICS: u32 = WM_APP + 3;

pub struct Window {
    hwnd: HWND,
}

impl Window {
    pub fn create() -> Result<Window> {
        unsafe {
            let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
            let instance = GetModuleHandleW(None)?;
            let class = w!("IsleWinNotch");
            let wc = WNDCLASSW {
                lpfnWndProc: Some(wndproc),
                hInstance: instance.into(),
                lpszClassName: class,
                hCursor: LoadCursorW(None, IDC_ARROW)?,
                hbrBackground: HBRUSH(GetStockObject(BLACK_BRUSH).0),
                ..Default::default()
            };
            RegisterClassW(&wc);
            let hwnd = CreateWindowExW(
                WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
                class,
                w!("Isle"),
                WS_POPUP,
                0,
                0,
                96,
                26,
                None,
                None,
                Some(instance.into()),
                None,
            )?;
            let window = Window { hwnd };
            let t = target(false, false, false).size;
            let scale = scale_of(hwnd);
            window.set_size_and_pos((t.w * scale).round() as i32, (t.h * scale).round() as i32);
            let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
            Ok(window)
        }
    }

    pub fn hwnd(&self) -> HWND {
        self.hwnd
    }

    /// Sizes the window and centres it at the top of the primary monitor.
    pub fn set_size_and_pos(&self, w_px: i32, h_px: i32) {
        place(self.hwnd, w_px, h_px);
    }
}

fn scale_of(hwnd: HWND) -> f32 {
    let dpi = unsafe { GetDpiForWindow(hwnd) };
    if dpi == 0 { 1.0 } else { dpi as f32 / 96.0 }
}

fn place(hwnd: HWND, w_px: i32, h_px: i32) {
    unsafe {
        let monitor = MonitorFromPoint(POINT { x: 0, y: 0 }, MONITOR_DEFAULTTOPRIMARY);
        let mut info = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
        if !GetMonitorInfoW(monitor, &mut info).as_bool() {
            return;
        }
        let r = info.rcMonitor;
        let x = r.left + notch_x(r.right - r.left, w_px);
        let _ = SetWindowPos(hwnd, Some(HWND_TOPMOST), x, r.top, w_px, h_px, SWP_NOACTIVATE);
    }
}

/// Re-centres the closed notch after a DPI, resolution or monitor change.
fn reposition_closed(hwnd: HWND) {
    let t = target(false, false, false).size;
    let s = scale_of(hwnd);
    place(hwnd, (t.w * s).round() as i32, (t.h * s).round() as i32);
}

extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
        WM_DPICHANGED | WM_DISPLAYCHANGE | WM_SETTINGCHANGE => {
            reposition_closed(hwnd);
            LRESULT(0)
        }
        WM_DESTROY => {
            unsafe { PostQuitMessage(0) };
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

pub fn run() -> Result<()> {
    let _window = Window::create()?;
    unsafe {
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
    Ok(())
}
