// SPDX-License-Identifier: GPL-3.0-or-later
//! The borderless, top-most, never-activated notch window.

use crate::app::App;
use crate::renderer::Renderer;
use isle_logic::canvas::{Align, Color, Rect};
use isle_logic::layout::notch_x;
use isle_logic::settings::Settings;
use std::cell::RefCell;
use windows::{
    Win32::{
        Foundation::*,
        Graphics::Gdi::*,
        System::LibraryLoader::GetModuleHandleW,
        UI::{HiDpi::*, Input::KeyboardAndMouse::*, WindowsAndMessaging::*},
    },
    core::*,
};

pub const WM_APP_WAKE: u32 = WM_APP + 1;
pub const WM_APP_MEDIA: u32 = WM_APP + 2;
pub const WM_APP_LYRICS: u32 = WM_APP + 3;
const TIMER_ANIM: usize = 1;
/// Defined in Win32::UI::Controls, which we do not pull in just for this.
const WM_MOUSELEAVE: u32 = 0x02A3;

thread_local! {
    static RENDERER: RefCell<Option<Renderer>> = const { RefCell::new(None) };
    static APP: RefCell<Option<App>> = const { RefCell::new(None) };
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
            let _ = RegisterClassW(&wc);
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
            APP.with(|a| *a.borrow_mut() = Some(App::new(Settings::default())));
            apply_window_size(hwnd);
            let mut rc = RECT::default();
            GetClientRect(hwnd, &mut rc)?;
            let dpi = 96.0 * scale_of(hwnd);
            let renderer = Renderer::new(hwnd, (rc.right - rc.left) as u32, (rc.bottom - rc.top) as u32, dpi)?;
            RENDERER.with(|r| *r.borrow_mut() = Some(renderer));
            let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
            draw(hwnd);
            Ok(Window { hwnd })
        }
    }

    pub fn hwnd(&self) -> HWND {
        self.hwnd
    }
}

fn scale_of(hwnd: HWND) -> f32 {
    let dpi = unsafe { GetDpiForWindow(hwnd) };
    if dpi == 0 { 1.0 } else { dpi as f32 / 96.0 }
}

/// Sizes the window to the open panel or the closed pill (per `App::window_big`)
/// and centres it at the top of the primary monitor.
fn apply_window_size(hwnd: HWND) {
    let (w, h) = APP.with(|a| a.borrow().as_ref().map(|app| app.window_dims(app.window_big)).unwrap_or((96.0, 26.0)));
    let s = scale_of(hwnd);
    let (w_px, h_px) = ((w * s).round() as i32, (h * s).round() as i32);
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

/// Draws the notch shape centred at the top of the window. Only the bottom corners
/// are rounded: the rectangle extends above the window by the radius.
fn draw(hwnd: HWND) {
    let s = scale_of(hwnd);
    let mut rc = RECT::default();
    if unsafe { GetClientRect(hwnd, &mut rc) }.is_err() {
        return;
    }
    let win_w = (rc.right - rc.left) as f32 / s;
    let Some((w, h, r)) = APP.with(|a| a.borrow().as_ref().map(|app| app.dims())) else { return };
    let x = ((win_w - w) / 2.0).max(0.0);
    RENDERER.with(|cell| {
        if let Some(renderer) = cell.borrow_mut().as_mut() {
            let _ = renderer.frame(|c| {
                c.fill_round_rect(Rect { x, y: -r, w, h: h + r }, r, Color(0, 0, 0, 255));
                c.draw_text("Isle", Rect { x, y: 0.0, w, h: 26.0 }, 12.0, 600, Color(245, 246, 248, 255), Align::Center);
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

fn kick(hwnd: HWND) {
    unsafe { SetTimer(Some(hwnd), TIMER_ANIM, 16, None) };
}

/// One animation/hover step. Borrows of `APP` are released before any window call.
fn tick(hwnd: HWND) {
    let step = APP.with(|a| a.borrow_mut().as_mut().map(|app| app.step()));
    let Some(step) = step else { return };
    if step.opened {
        APP.with(|a| a.borrow_mut().as_mut().map(|app| app.window_big = true));
        apply_window_size(hwnd);
    }
    draw(hwnd);
    if step.fully_closed {
        APP.with(|a| a.borrow_mut().as_mut().map(|app| app.window_big = false));
        apply_window_size(hwnd);
        draw(hwnd);
    }
    if !step.keep_ticking {
        unsafe {
            let _ = KillTimer(Some(hwnd), TIMER_ANIM);
        }
    }
}

fn pointer(hwnd: HWND, inside: bool) {
    APP.with(|a| a.borrow_mut().as_mut().map(|app| app.pointer(inside)));
    kick(hwnd);
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
        WM_MOUSEMOVE => {
            let mut tme = TRACKMOUSEEVENT {
                cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
                dwFlags: TME_LEAVE,
                hwndTrack: hwnd,
                dwHoverTime: 0,
            };
            unsafe {
                let _ = TrackMouseEvent(&mut tme);
            }
            pointer(hwnd, true);
            LRESULT(0)
        }
        WM_MOUSELEAVE => {
            pointer(hwnd, false);
            LRESULT(0)
        }
        WM_TIMER if wparam.0 == TIMER_ANIM => {
            tick(hwnd);
            LRESULT(0)
        }
        WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
        WM_DPICHANGED | WM_DISPLAYCHANGE | WM_SETTINGCHANGE => {
            apply_window_size(hwnd);
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
