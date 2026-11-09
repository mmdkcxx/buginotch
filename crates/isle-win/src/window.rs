// SPDX-License-Identifier: GPL-3.0-or-later
//! The borderless, top-most, never-activated notch window.

use crate::app::App;
use crate::renderer::Renderer;
use crate::tray::{self, ID_AUTOSTART, ID_QUIT, Tray, WM_APP_TRAY};
use isle_logic::layout::notch_x;
use isle_logic::module::Event;
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
/// Posted to ourselves to run the next animation frame, paced by the compositor.
const WM_APP_TICK: u32 = WM_APP + 5;
const TIMER_SECOND: usize = 2;
const TIMER_WAKE: usize = 3;
/// Defined in Win32::UI::Controls, which we do not pull in just for this.
const WM_MOUSELEAVE: u32 = 0x02A3;

thread_local! {
    static TICK_PENDING: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    static TRAY: RefCell<Option<Tray>> = const { RefCell::new(None) };
    static HIDDEN: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
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
            let mut settings = crate::settings_io::load();
            settings.autostart = crate::system::is_autostart();
            APP.with(|a| *a.borrow_mut() = Some(App::new(settings)));
            let watcher = crate::media_win::MediaWatcher::spawn(hwnd);
            APP.with(|a| {
                if let Some(app) = a.borrow_mut().as_mut() {
                    app.registry.insert_page(1, Box::new(crate::modules::music::MusicModule::new(watcher, hwnd.0 as isize)));
                }
            });
            if let Ok(t) = Tray::add(hwnd) {
                TRAY.with(|c| *c.borrow_mut() = Some(t));
            }
            apply_window_size(hwnd);
            let mut rc = RECT::default();
            GetClientRect(hwnd, &mut rc)?;
            let dpi = 96.0 * scale_of(hwnd);
            let renderer = Renderer::new(hwnd, (rc.right - rc.left) as u32, (rc.bottom - rc.top) as u32, dpi)?;
            RENDERER.with(|r| *r.borrow_mut() = Some(renderer));
            SetTimer(Some(hwnd), TIMER_SECOND, 1000, None);
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

/// Sends a drag (or release) to the current page. Returns whether a redraw is needed.
fn forward_pointer(hwnd: HWND, lparam: LPARAM, up: bool) -> bool {
    let s = scale_of(hwnd);
    let (px, py) = ((lparam.0 & 0xFFFF) as i16 as f32, ((lparam.0 >> 16) & 0xFFFF) as i16 as f32);
    let mut rc = RECT::default();
    let _ = unsafe { GetClientRect(hwnd, &mut rc) };
    let win_w = (rc.right - rc.left) as f32 / s;
    APP.with(|a| {
        a.borrow_mut().as_mut().is_some_and(|app| {
            let (w, _, _) = app.dims();
            let (x, y) = (px / s - (win_w - w) / 2.0, py / s);
            app.page_event(&if up { Event::PointerUp(x, y) } else { Event::PointerDrag(x, y) })
        })
    })
}

/// Draws one frame through `App::paint`.
fn draw(hwnd: HWND) {
    let s = scale_of(hwnd);
    let mut rc = RECT::default();
    if unsafe { GetClientRect(hwnd, &mut rc) }.is_err() {
        return;
    }
    let win_w = (rc.right - rc.left) as f32 / s;
    let mut lost = false;
    APP.with(|a| {
        let mut a = a.borrow_mut();
        let Some(app) = a.as_mut() else { return };
        RENDERER.with(|r| {
            if let Some(renderer) = r.borrow_mut().as_mut() {
                if let Err(e) = renderer.frame(|c| app.paint(c, win_w)) {
                    // D2DERR_RECREATE_TARGET, DXGI_ERROR_DEVICE_REMOVED / _RESET: rebuild.
                    if matches!(e.code().0 as u32, 0x8899000C | 0x887A0005 | 0x887A0007) {
                        lost = true;
                    }
                }
            }
        });
    });
    if lost {
        crate::log("graphics device lost; rebuilding renderer");
        if let Ok(r) = Renderer::new(hwnd, (rc.right - rc.left) as u32, (rc.bottom - rc.top) as u32, 96.0 * s) {
            RENDERER.with(|c| *c.borrow_mut() = Some(r));
        }
    }
}

/// Records the registry's autostart state in settings.toml.
fn save_settings() {
    crate::settings_io::update(|s| s.autostart = crate::system::is_autostart());
}

/// Polls the modules, resizes the window if the pill changed width, redraws, and
/// schedules the next module wake-up.
fn poll_and_draw(hwnd: HWND) {
    let poll = APP.with(|a| a.borrow_mut().as_mut().map(|app| app.poll()));
    let Some(poll) = poll else { return };
    if poll.relayout {
        // Open or closed, the window must fit the (possibly wider) target size.
        apply_window_size(hwnd);
        kick(hwnd);
    }
    if poll.redraw {
        draw(hwnd);
    }
    schedule_wake(hwnd);
}

fn schedule_wake(hwnd: HWND) {
    let wake = APP.with(|a| a.borrow().as_ref().and_then(|app| app.next_wake_ms()));
    if let Some(at) = wake {
        if at - crate::system::epoch_ms() <= FRAME_WAKE_MS {
            // Needed within a frame or two (a lyric scroll): run vsync-paced frames.
            kick(hwnd);
        } else {
            let delay = (at - crate::system::epoch_ms()).clamp(16, 60_000) as u32;
            unsafe { SetTimer(Some(hwnd), TIMER_WAKE, delay, None) };
        }
    }
}

/// A module wake-up this close counts as "animating": drive it with vsync frames.
const FRAME_WAKE_MS: i64 = 24;

fn wants_frames() -> bool {
    APP.with(|a| a.borrow().as_ref().and_then(|app| app.next_wake_ms())).is_some_and(|at| at - crate::system::epoch_ms() <= FRAME_WAKE_MS)
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
    if !TICK_PENDING.replace(true) {
        unsafe {
            let _ = PostMessageW(Some(hwnd), WM_APP_TICK, WPARAM(0), LPARAM(0));
        }
    }
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
    if step.keep_ticking || wants_frames() {
        kick(hwnd);
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
            if wparam.0 & 0x0001 != 0 {
                // MK_LBUTTON: dragging (we hold mouse capture since the press).
                if forward_pointer(hwnd, lparam, false) {
                    draw(hwnd);
                }
            }
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            let redraw = forward_pointer(hwnd, lparam, true);
            unsafe {
                let _ = ReleaseCapture();
            }
            // With capture the pointer may have ended outside: then the notch must close.
            let (px, py) = ((lparam.0 & 0xFFFF) as i16 as i32, ((lparam.0 >> 16) & 0xFFFF) as i16 as i32);
            let mut rc = RECT::default();
            let _ = unsafe { GetClientRect(hwnd, &mut rc) };
            if px < 0 || py < 0 || px >= rc.right || py >= rc.bottom {
                pointer(hwnd, false);
            }
            if redraw {
                draw(hwnd);
            }
            LRESULT(0)
        }
        WM_LBUTTONDOWN => {
            unsafe {
                let _ = SetCapture(hwnd);
            }
            let s = scale_of(hwnd);
            let (px, py) = ((lparam.0 & 0xFFFF) as i16 as f32, ((lparam.0 >> 16) & 0xFFFF) as i16 as f32);
            let mut rc = RECT::default();
            let _ = unsafe { GetClientRect(hwnd, &mut rc) };
            let win_w = (rc.right - rc.left) as f32 / s;
            let redraw = APP.with(|a| {
                a.borrow_mut().as_mut().is_some_and(|app| {
                    let (w, _, _) = app.dims();
                    let r = app.click(px / s - (win_w - w) / 2.0, py / s);
                    r || app.sync_lyrics()
                })
            });
            if APP.with(|a| a.borrow().as_ref().is_some_and(|app| app.is_open())) {
                apply_window_size(hwnd);
                kick(hwnd);
            }
            if redraw {
                draw(hwnd);
            }
            LRESULT(0)
        }
        WM_MOUSELEAVE => {
            pointer(hwnd, false);
            LRESULT(0)
        }
        WM_APP_MEDIA | WM_APP_LYRICS => {
            poll_and_draw(hwnd);
            LRESULT(0)
        }
        WM_APP_TRAY => {
            if (lparam.0 & 0xFFFF) as u32 == WM_RBUTTONUP {
                tray::show_menu(hwnd);
            }
            LRESULT(0)
        }
        WM_COMMAND => {
            match wparam.0 & 0xFFFF {
                ID_AUTOSTART => {
                    tray::toggle_autostart();
                    save_settings();
                }
                ID_QUIT => unsafe {
                    let _ = DestroyWindow(hwnd);
                },
                _ => {}
            }
            LRESULT(0)
        }
        WM_TIMER if wparam.0 == TIMER_SECOND => {
            let fullscreen = crate::system::fullscreen_app_active();
            if fullscreen != HIDDEN.get() {
                HIDDEN.set(fullscreen);
                unsafe {
                    let _ = ShowWindow(hwnd, if fullscreen { SW_HIDE } else { SW_SHOWNOACTIVATE });
                }
            }
            poll_and_draw(hwnd);
            LRESULT(0)
        }
        WM_TIMER if wparam.0 == TIMER_WAKE => {
            unsafe {
                let _ = KillTimer(Some(hwnd), TIMER_WAKE);
            }
            poll_and_draw(hwnd);
            LRESULT(0)
        }
        WM_APP_TICK => {
            TICK_PENDING.set(false);
            // Wait for the next compositor pass so frames land evenly on vsync;
            // a WM_TIMER only fires every ~15.6 ms and gave uneven steps.
            unsafe {
                let _ = windows::Win32::Graphics::Dwm::DwmFlush();
            }
            tick(hwnd);
            LRESULT(0)
        }
        WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
        WM_DPICHANGED | WM_DISPLAYCHANGE | WM_SETTINGCHANGE => {
            apply_window_size(hwnd);
            LRESULT(0)
        }
        WM_DESTROY => {
            TRAY.with(|c| {
                if let Some(t) = c.borrow().as_ref() {
                    t.remove();
                }
            });
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
