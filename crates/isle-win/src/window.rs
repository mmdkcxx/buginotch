// SPDX-License-Identifier: GPL-3.0-or-later
//! The borderless, top-most, never-activated notch window.

use crate::app::App;
use crate::renderer::Renderer;
use crate::tray::{self, ID_AUTOSTART, ID_AUTOUPDATE, ID_QUIT, ID_UPDATE, Tray, WM_APP_TRAY};
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
    static REGION: std::cell::Cell<(i32, i32, i32)> = const { std::cell::Cell::new((0, 0, 0)) };
    static LAST_POLL_MS: std::cell::Cell<i64> = const { std::cell::Cell::new(0) };
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
            let _ = windows::Win32::System::DataExchange::AddClipboardFormatListener(hwnd);
            crate::shell_win::init();
            crate::shell_win::register_drop_target(hwnd, crate::shell_win::DropHandlers { hover: drag_hover, files: files_dropped });
            let mut settings = crate::settings_io::load();
            settings.autostart = crate::system::is_autostart();
            APP.with(|a| *a.borrow_mut() = Some(App::new(settings)));
            let lead = APP.with(|a| a.borrow().as_ref().map_or(500, |app| app.settings.lyrics_lead_ms));
            let watcher = crate::media_win::MediaWatcher::spawn(hwnd);
            let headphones = crate::headphones_win::HeadphoneWatcher::spawn(hwnd);
            APP.with(|a| {
                if let Some(app) = a.borrow_mut().as_mut() {
                    app.registry.add(Box::new(crate::modules::headphones::HeadphonesModule::new(headphones)));
                    app.registry.insert_page(1, Box::new(crate::modules::music::MusicModule::new(watcher, hwnd.0 as isize, lead)));
                }
            });
            if let Ok(t) = Tray::add(hwnd) {
                TRAY.with(|c| *c.borrow_mut() = Some(t));
            }
            place_window(hwnd);
            apply_window_size(hwnd);
            let mut rc = RECT::default();
            GetClientRect(hwnd, &mut rc)?;
            let dpi = 96.0 * scale_of(hwnd);
            let (bw, bh) = buffer_px(scale_of(hwnd));
            let renderer = Renderer::new(hwnd, bw, bh, dpi)?;
            RENDERER.with(|r| *r.borrow_mut() = Some(renderer));
            SetTimer(Some(hwnd), TIMER_SECOND, 1000, None);
            crate::updater::set_hwnd(hwnd);
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

/// Places the window, always at the size of the largest open panel, centred at the top
/// of the primary monitor. It never changes size otherwise: resizing the window (and the
/// swap chain with it) blinked. What is shown and clickable is set by `apply_window_size`.
fn place_window(hwnd: HWND) {
    let s = scale_of(hwnd);
    let (w_px, h_px) = buffer_px(s);
    let (w_px, h_px) = (w_px as i32, h_px as i32);
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

/// Limits the window to the notch's current footprint (the open panel or the closed pill)
/// with a window region, so the transparent rest neither shows nor takes mouse input.
fn apply_window_size(hwnd: HWND) {
    let (w, h) = APP.with(|a| a.borrow().as_ref().map(|app| app.region_dims()).unwrap_or((96.0, 26.0)));
    let s = scale_of(hwnd);
    let mut rc = RECT::default();
    if unsafe { GetClientRect(hwnd, &mut rc) }.is_err() {
        return;
    }
    let win_w = rc.right - rc.left;
    let (w_px, h_px) = ((w * s).round() as i32, (h * s).round() as i32);
    let left = (win_w - w_px) / 2;
    // Skip identical regions (this runs every animation frame).
    if REGION.replace((left, w_px, h_px)) == (left, w_px, h_px) {
        return;
    }
    unsafe {
        let rgn = CreateRectRgn(left, 0, left + w_px, h_px);
        // The window owns the region after a successful call.
        if SetWindowRgn(hwnd, Some(rgn), false) == 0 {
            let _ = DeleteObject(rgn.into());
        }
    }
}

/// Carries out file requests a page made during a pointer event: copying a file to the
/// clipboard, or dragging it out of the notch (which runs until the button is released).
fn run_file_actions(hwnd: HWND) {
    if let Some(paths) = APP.with(|a| a.borrow_mut().as_mut().and_then(|app| app.take_copy_files())) {
        crate::shell_win::copy_files(&paths);
    }
    if let Some(paths) = APP.with(|a| a.borrow_mut().as_mut().and_then(|app| app.take_drag())) {
        unsafe {
            let _ = ReleaseCapture();
        }
        crate::shell_win::drag_files(&paths);
        // The drag swallowed the button release and any mouse-leave: check where we ended.
        let mut pt = POINT::default();
        let mut rc = RECT::default();
        unsafe {
            let _ = GetCursorPos(&mut pt);
            let _ = ScreenToClient(hwnd, &mut pt);
            let _ = GetClientRect(hwnd, &mut rc);
        }
        if pt.x < 0 || pt.y < 0 || pt.x >= rc.right || pt.y >= rc.bottom {
            pointer(hwnd, false);
        }
        draw(hwnd);
    }
}

/// Files dragged over the notch open it on the Shelf.
fn drag_hover(hwnd: HWND, over: bool) {
    APP.with(|a| a.borrow_mut().as_mut().map(|app| app.drag_hover(over)));
    apply_window_size(hwnd);
    kick(hwnd);
}

/// Files were dropped on the notch.
fn files_dropped(hwnd: HWND, paths: Vec<String>) {
    let changed = APP.with(|a| a.borrow_mut().as_mut().is_some_and(|app| app.files_dropped(&paths)));
    if changed {
        draw(hwnd);
        kick(hwnd);
    }
}

/// Sends a drag (or release) to the current page. Returns whether a redraw is needed.
thread_local! {
    /// The window has been made activatable so the text box can take the keyboard.
    static INPUT_ON: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// The window that had the keyboard before we took it.
    static PREV_FOREGROUND: std::cell::Cell<isize> = const { std::cell::Cell::new(0) };
    static HIGH_SURROGATE: std::cell::Cell<u16> = const { std::cell::Cell::new(0) };
}

fn text_focus() -> bool {
    APP.with(|a| a.borrow().as_ref().is_some_and(|app| app.text_focus()))
}

/// Takes the keyboard when a text box gets focus and gives it back when it ends.
fn sync_input(hwnd: HWND) {
    let want = text_focus();
    if want == INPUT_ON.get() {
        return;
    }
    INPUT_ON.set(want);
    unsafe {
        let style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        if want {
            PREV_FOREGROUND.set(GetForegroundWindow().0 as isize);
            SetWindowLongPtrW(hwnd, GWL_EXSTYLE, style & !(WS_EX_NOACTIVATE.0 as isize));
            let _ = SetForegroundWindow(hwnd);
            let _ = SetFocus(Some(hwnd));
        } else {
            SetWindowLongPtrW(hwnd, GWL_EXSTYLE, style | WS_EX_NOACTIVATE.0 as isize);
            let prev = HWND(PREV_FOREGROUND.get() as *mut _);
            if !prev.0.is_null() && prev != hwnd && IsWindow(Some(prev)).as_bool() {
                let _ = SetForegroundWindow(prev);
            }
        }
    }
}

/// Sends a keyboard event to the page with the text box and carries out what it asks.
fn send_text_event(hwnd: HWND, e: Event) {
    let reaction = APP.with(|a| a.borrow_mut().as_mut().map(|app| app.page_reaction(&e)));
    if let Some(isle_logic::module::Reaction::Copy(text)) = &reaction {
        crate::input_win::write_clipboard(hwnd, text);
    }
    sync_input(hwnd);
    if reaction.is_some_and(|r| r != isle_logic::module::Reaction::None) {
        draw(hwnd);
        kick(hwnd);
    }
}

/// Moves the IME's window next to the caret.
fn place_ime(hwnd: HWND) {
    let s = scale_of(hwnd);
    let mut rc = RECT::default();
    let _ = unsafe { GetClientRect(hwnd, &mut rc) };
    let win_w = (rc.right - rc.left) as f32 / s;
    let at = APP.with(|a| a.borrow().as_ref().and_then(|app| app.ime_anchor().map(|p| (p, app.dims().0))));
    if let Some(((x, y), w)) = at {
        crate::input_win::place_ime(hwnd, (((win_w - w) / 2.0 + x) * s) as i32, (y * s) as i32);
    }
}

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
    let client_px = (rc.right - rc.left) as f32;
    let mut lost = false;
    APP.with(|a| {
        let mut a = a.borrow_mut();
        let Some(app) = a.as_mut() else { return };
        RENDERER.with(|r| {
            if let Some(renderer) = r.borrow_mut().as_mut() {
                // Paint centred in the (fixed-size) buffer, and keep the buffer centred on the window.
                let buf_px = renderer.size().0 as f32;
                renderer.set_offset_x((client_px - buf_px) / 2.0);
                if let Err(e) = renderer.frame(|c| app.paint(c, buf_px / s)) {
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
        let (bw, bh) = buffer_px(s);
        if let Ok(r) = Renderer::new(hwnd, bw, bh, 96.0 * s) {
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
    // A text box can lose focus without a key (a button press, a page change): hand the keyboard back.
    sync_input(hwnd);
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

/// The swap chain is always as big as the largest open panel (plus slack), so growing or
/// shrinking the window never reallocates it: reallocating cleared the frame and blinked.
fn buffer_px(scale: f32) -> (u32, u32) {
    ((680.0 * scale).ceil() as u32, (190.0 * scale).ceil() as u32)
}

fn resize_renderer(hwnd: HWND) {
    let s = scale_of(hwnd);
    let mut rc = RECT::default();
    if unsafe { GetClientRect(hwnd, &mut rc) }.is_err() {
        return;
    }
    RENDERER.with(|cell| {
        if let Some(renderer) = cell.borrow_mut().as_mut() {
            let (bw, bh) = buffer_px(s);
            let _ = renderer.resize(bw, bh, 96.0 * s);
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
    apply_window_size(hwnd);
    draw(hwnd);
    sync_input(hwnd);
    if step.fully_closed {
        APP.with(|a| a.borrow_mut().as_mut().map(|app| app.window_big = false));
        apply_window_size(hwnd);
        draw(hwnd);
    }
    if step.keep_ticking || wants_frames() {
        kick(hwnd);
    }
}

/// Dispatches the keyboard, timer and mouse messages that are already waiting. A posted message
/// (our frame tick) is always served before these, so a frame loop has to let them through itself.
fn pump_input() {
    // WM_KEYFIRST (0x100) ..= WM_MOUSELEAVE (0x2A3): keys, IME, timers, (non-client) mouse.
    let mut msg = MSG::default();
    for _ in 0..64 {
        if !unsafe { PeekMessageW(&mut msg, None, 0x0100, WM_MOUSELEAVE, PM_REMOVE) }.as_bool() {
            break;
        }
        unsafe {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
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
            if wparam.0 & 0x0001 != 0 {
                // MK_LBUTTON: dragging (we hold mouse capture since the press).
                if forward_pointer(hwnd, lparam, false) {
                    draw(hwnd);
                }
                run_file_actions(hwnd);
            }
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            let redraw = forward_pointer(hwnd, lparam, true);
            unsafe {
                let _ = ReleaseCapture();
            }
            run_file_actions(hwnd);
            sync_input(hwnd);
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
                    // Not `r || sync`: sync_lyrics must always run after a click.
                    let resized = app.sync_lyrics();
                    r || resized
                })
            });
            if let Some(text) = APP.with(|a| a.borrow_mut().as_mut().and_then(|app| app.take_copy())) {
                crate::input_win::write_clipboard(hwnd, &text);
            }
            sync_input(hwnd);
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
        WM_CLIPBOARDUPDATE => {
            if let Some(text) = crate::input_win::clipboard_for_history(hwnd) {
                let changed = APP.with(|a| a.borrow_mut().as_mut().is_some_and(|app| app.clipboard_changed(&text)));
                if changed && APP.with(|a| a.borrow().as_ref().is_some_and(|app| app.is_open())) {
                    draw(hwnd);
                }
            }
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
                ID_UPDATE => crate::updater::check_now(),
                ID_AUTOUPDATE => crate::settings_io::update(|s| s.auto_update = !s.auto_update),
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
            if !fullscreen {
                // Another window can slip above the notch (Windows does not always keep a topmost
                // window first); raise it again without moving or activating it.
                unsafe {
                    let _ = SetWindowPos(hwnd, Some(HWND_TOPMOST), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
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
            // Back-to-back frames would starve the lower-priority messages (mouse, keys, timers):
            // handle those now, or the notch could never see the pointer leave.
            pump_input();
            // The modules still need their polling (a peek expiring, new data) while frames run.
            let now = crate::system::epoch_ms();
            if now - LAST_POLL_MS.get() >= 100 {
                LAST_POLL_MS.set(now);
                poll_and_draw(hwnd);
            }
            tick(hwnd);
            LRESULT(0)
        }
        WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
        WM_KEYDOWN if text_focus() => {
            if let Some(e) = crate::input_win::key_event(hwnd, wparam.0 as u16) {
                send_text_event(hwnd, e);
            }
            LRESULT(0)
        }
        WM_CHAR if text_focus() => {
            let unit = wparam.0 as u16;
            let text = if (0xD800..0xDC00).contains(&unit) {
                HIGH_SURROGATE.set(unit);
                None
            } else if (0xDC00..0xE000).contains(&unit) {
                char::decode_utf16([HIGH_SURROGATE.replace(0), unit]).next().and_then(|r| r.ok()).map(String::from)
            } else if unit >= 0x20 && unit != 0x7F {
                char::from_u32(unit as u32).map(String::from)
            } else {
                None
            };
            if let Some(t) = text {
                send_text_event(hwnd, Event::Text(t));
            }
            LRESULT(0)
        }
        WM_IME_STARTCOMPOSITION if text_focus() => {
            place_ime(hwnd);
            LRESULT(0)
        }
        WM_IME_COMPOSITION if text_focus() => {
            for e in crate::input_win::ime_events(hwnd, lparam.0 as u32) {
                send_text_event(hwnd, e);
            }
            place_ime(hwnd);
            LRESULT(0)
        }
        WM_IME_ENDCOMPOSITION if text_focus() => {
            send_text_event(hwnd, Event::Compose(None));
            LRESULT(0)
        }
        WM_KILLFOCUS if INPUT_ON.get() => {
            // The user clicked elsewhere: end the edit.
            send_text_event(hwnd, Event::Edit { key: isle_logic::module::EditKey::Escape, shift: false });
            unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
        }
        WM_MOUSEWHEEL => {
            let steps = ((wparam.0 >> 16) as i16) as f32 / 120.0;
            let redraw = APP.with(|a| a.borrow_mut().as_mut().is_some_and(|app| app.page_event(&Event::PointerWheel(steps))));
            if redraw {
                draw(hwnd);
            }
            LRESULT(0)
        }
        WM_DPICHANGED | WM_DISPLAYCHANGE | WM_SETTINGCHANGE => {
            place_window(hwnd);
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
