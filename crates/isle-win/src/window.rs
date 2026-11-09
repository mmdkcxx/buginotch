// SPDX-License-Identifier: GPL-3.0-or-later
//! The borderless, top-most, never-activated notch window.

use crate::renderer::Renderer;
use isle_logic::canvas::{Align, Color, Rect};
use isle_logic::layout::{notch_x, target};
use std::cell::RefCell;
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

thread_local! {
    static RENDERER: RefCell<Option<Renderer>> = const { RefCell::new(None) };
}

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
                ..Default::default()
            };
            RegisterClassW(&wc);
            let hwnd = CreateWindowExW(
                WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_NOREDIRECTIONBITMAP,
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
            let mut rc = RECT::default();
            GetClientRect(hwnd, &mut rc)?;
            let renderer = Renderer::new(hwnd, (rc.right - rc.left) as u32, (rc.bottom - rc.top) as u32, 96.0 * scale)?;
            RENDERER.with(|r| *r.borrow_mut() = Some(renderer));
            let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
            draw(hwnd);
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

/// Draws the notch shape. Only the bottom corners are rounded: the rectangle extends
/// above the window by the radius, so the top corners are cut off.
fn draw(hwnd: HWND) {
    let s = scale_of(hwnd);
    let mut rc = RECT::default();
    if unsafe { GetClientRect(hwnd, &mut rc) }.is_err() {
        return;
    }
    let (w, h) = ((rc.right - rc.left) as f32 / s, (rc.bottom - rc.top) as f32 / s);
    let r = target(false, false, false).radius;
    RENDERER.with(|cell| {
        if let Some(renderer) = cell.borrow_mut().as_mut() {
            let _ = renderer.frame(|c| {
                c.fill_round_rect(Rect { x: 0.0, y: -r, w, h: h + r }, r, Color(0, 0, 0, 255));
                c.draw_text("Isle", Rect { x: 0.0, y: 0.0, w, h }, 12.0, 600, Color(245, 246, 248, 255), Align::Center);
            });
        }
    });
}

fn resize_renderer(hwnd: HWND) {
    let s = scale_of(hwnd);
    let mut rc = RECT::default();
    if unsafe { GetClientRect(hwnd, &mut rc) }.is_err() {
        return;
    }
    RENDERER.with(|cell| {
        if let Some(renderer) = cell.borrow_mut().as_mut() {
            let _ = renderer.resize((rc.right - rc.left) as u32, (rc.bottom - rc.top) as u32, 96.0 * s);
        }
    });
}

extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_ERASEBKGND => LRESULT(1),
        WM_SIZE => {
            resize_renderer(hwnd);
            draw(hwnd);
            LRESULT(0)
        }
        WM_PAINT => {
            unsafe {
                let mut ps = PAINTSTRUCT::default();
                let _ = BeginPaint(hwnd, &mut ps);
                let _ = EndPaint(hwnd, &ps);
            }
            draw(hwnd);
            LRESULT(0)
        }
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
