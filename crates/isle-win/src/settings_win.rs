// SPDX-License-Identifier: GPL-3.0-or-later
//! The settings window: a separate top-level window with its own glass backdrop, drawn by the
//! shared `SettingsView`. One instance; opening it again brings it to the front.

use crate::renderer::Renderer;
use isle_logic::module::Event;
use isle_logic::settings_view::{Act, H, Key, MARGIN, Outcome, SettingsView, W};
use std::cell::RefCell;
use windows::{
    Win32::{
        Foundation::*,
        Graphics::Gdi::*,
        System::LibraryLoader::GetModuleHandleW,
        UI::{HiDpi::*, Input::KeyboardAndMouse::*, Shell::*, WindowsAndMessaging::*},
    },
    core::*,
};

/// Tells the notch window that settings.toml changed.
pub const WM_APP_SETTINGS: u32 = WM_APP + 9;

struct State {
    hwnd: HWND,
    glass: HWND,
    _backdrop: Option<crate::backdrop::Backdrop>,
    renderer: Renderer,
    view: SettingsView,
    scale: f32,
    owner: HWND,
    high_surrogate: u16,
}

thread_local! {
    static STATE: RefCell<Option<State>> = const { RefCell::new(None) };
}

fn scale_of(hwnd: HWND) -> f32 {
    let dpi = unsafe { GetDpiForWindow(hwnd) };
    if dpi == 0 { 1.0 } else { dpi as f32 / 96.0 }
}

/// Opens the window, or brings the open one to the front. `pages` are the reorderable pages.
pub fn open(owner: HWND, pages: Vec<(String, String)>) {
    let existing = STATE.with(|s| s.try_borrow().ok().and_then(|g| g.as_ref().map(|st| st.hwnd)));
    if let Some(h) = existing {
        unsafe {
            let _ = ShowWindow(h, SW_RESTORE);
            let _ = SetForegroundWindow(h);
        }
        return;
    }
    if let Err(e) = create(owner, pages) {
        crate::window::glass_log(&format!("settings window failed: {e}"));
    }
}

fn create(owner: HWND, pages: Vec<(String, String)>) -> Result<()> {
    unsafe {
        let instance = GetModuleHandleW(None)?;
        let class = w!("BuginotchSettings");
        let wc = WNDCLASSW {
            lpfnWndProc: Some(wndproc),
            hInstance: instance.into(),
            lpszClassName: class,
            hCursor: LoadCursorW(None, IDC_ARROW)?,
            hIcon: crate::tray::app_icon(1),
            ..Default::default()
        };
        let _ = RegisterClassW(&wc);

        // The monitor the cursor is on, so the window opens where the user is looking.
        let mut pt = POINT::default();
        let _ = GetCursorPos(&mut pt);
        let mon = MonitorFromPoint(pt, MONITOR_DEFAULTTOPRIMARY);
        let mut dpi_x = 96u32;
        let mut dpi_y = 96u32;
        let _ = GetDpiForMonitor(mon, MDT_EFFECTIVE_DPI, &mut dpi_x, &mut dpi_y);
        let scale = dpi_x as f32 / 96.0;
        let mut info = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
        let _ = GetMonitorInfoW(mon, &mut info);
        let (w_px, h_px) = (((W + MARGIN * 2.0) * scale).round() as i32, ((H + MARGIN * 2.0) * scale).round() as i32);
        let wa = info.rcWork;
        let x = wa.left + ((wa.right - wa.left) - w_px) / 2;
        let y = wa.top + ((wa.bottom - wa.top) - h_px) / 2;

        let hwnd = CreateWindowExW(WS_EX_NOREDIRECTIONBITMAP | WS_EX_APPWINDOW, class, w!("Pengaturan Buginotch"), WS_POPUP, x, y, w_px, h_px, None, None, Some(instance.into()), None)?;
        let dpi = 96.0 * scale_of(hwnd);
        let scale = dpi / 96.0;
        let renderer = Renderer::new(hwnd, w_px as u32, h_px as u32, dpi)?;
        let (glass, backdrop) = crate::window::new_glass(scale).ok_or_else(|| Error::from(E_FAIL))?;
        let r = (16.0 * scale).round() as i32;
        let (gw, gh) = ((W * scale).round() as i32, (H * scale).round() as i32);
        let rgn = CreateRoundRectRgn(0, 0, gw + 1, gh + 1, r * 2, r * 2);
        if SetWindowRgn(glass, Some(rgn), true) == 0 {
            let _ = DeleteObject(rgn.into());
        }

        let mut settings = crate::settings_io::load();
        settings.autostart = crate::system::is_autostart();
        let view = SettingsView::new(settings, pages, env!("CARGO_PKG_VERSION"));
        STATE.with(|s| *s.borrow_mut() = Some(State { hwnd, glass, _backdrop: backdrop, renderer, view, scale, owner, high_surrogate: 0 }));
        sync_glass(hwnd);
        let _ = ShowWindow(hwnd, SW_SHOW);
        let _ = SetForegroundWindow(hwnd);
        draw();
        Ok(())
    }
}

fn draw() {
    STATE.with(|s| {
        if let Ok(mut g) = s.try_borrow_mut() {
            if let Some(st) = g.as_mut() {
                let State { renderer, view, .. } = st;
                let _ = renderer.frame(|c| view.paint(c, MARGIN, MARGIN));
            }
        }
    });
}

/// Puts the glass window right under the settings window, on its content rectangle.
fn sync_glass(hwnd: HWND) {
    let info = STATE.with(|s| s.try_borrow().ok().and_then(|g| g.as_ref().map(|st| (st.glass, st.scale))));
    let Some((glass, scale)) = info else { return };
    unsafe {
        let mut rc = RECT::default();
        if GetWindowRect(hwnd, &mut rc).is_err() {
            return;
        }
        let m = (MARGIN * scale).round() as i32;
        let _ = SetWindowPos(glass, Some(hwnd), rc.left + m, rc.top + m, (W * scale).round() as i32, (H * scale).round() as i32, SWP_NOACTIVATE | SWP_SHOWWINDOW);
    }
}

fn point(lparam: LPARAM, scale: f32) -> (f32, f32) {
    let (px, py) = ((lparam.0 & 0xFFFF) as i16 as f32, ((lparam.0 >> 16) & 0xFFFF) as i16 as f32);
    (px / scale - MARGIN, py / scale - MARGIN)
}

/// Sends an event to the view and carries out what it asks.
fn send(hwnd: HWND, e: Event) {
    let out = STATE.with(|s| s.try_borrow_mut().ok().and_then(|mut g| g.as_mut().map(|st| st.view.on_event(&e))));
    if let Some(out) = out {
        handle(hwnd, out);
    }
}

fn handle(hwnd: HWND, out: Outcome) {
    let mut close = false;
    if let Some(text) = &out.copy {
        crate::input_win::write_clipboard(hwnd, text);
    }
    if !out.changed.is_empty() {
        save(&out.changed);
    }
    for a in &out.acts {
        match a {
            Act::Close => close = true,
            Act::CheckUpdate => crate::updater::check_now(),
            Act::ClearSkipped => {
                crate::settings_io::update(|s| s.skipped_version.clear());
                STATE.with(|s| {
                    if let Ok(mut g) = s.try_borrow_mut() {
                        if let Some(st) = g.as_mut() {
                            st.view.set_skipped("");
                        }
                    }
                });
            }
            Act::OpenData => open_path(&crate::system::data_path("settings.toml").parent().map(|p| p.to_path_buf()).unwrap_or_default()),
            Act::OpenToml => open_path(&crate::system::data_path("settings.toml")),
        }
    }
    if close {
        unsafe {
            let _ = DestroyWindow(hwnd);
        }
        return;
    }
    if out.redraw || !out.changed.is_empty() {
        draw();
    }
}

fn open_path(path: &std::path::Path) {
    let wide: Vec<u16> = path.as_os_str().to_string_lossy().encode_utf16().chain(std::iter::once(0)).collect();
    unsafe {
        ShellExecuteW(None, w!("open"), PCWSTR(wide.as_ptr()), None, None, SW_SHOWNORMAL);
    }
}

/// Writes the edited settings to settings.toml and tells the notch to pick them up.
fn save(changed: &[Key]) {
    let info = STATE.with(|s| s.try_borrow().ok().and_then(|g| g.as_ref().map(|st| (st.owner, st.view.settings.autostart))));
    let applied = STATE.with(|s| {
        s.try_borrow().ok().and_then(|g| {
            g.as_ref().map(|st| {
                let view = &st.view;
                crate::settings_io::update(|stored| view.apply_to(stored));
            })
        })
    });
    if applied.is_none() {
        return;
    }
    if let Some((owner, autostart)) = info {
        if changed.contains(&Key::Autostart) {
            let _ = crate::system::set_autostart(autostart);
        }
        unsafe {
            let _ = PostMessageW(Some(owner), WM_APP_SETTINGS, WPARAM(0), LPARAM(0));
        }
    }
}

extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    let scale = STATE.with(|s| s.try_borrow().ok().and_then(|g| g.as_ref().map(|st| st.scale))).unwrap_or(1.0);
    let wants_text = STATE.with(|s| s.try_borrow().ok().and_then(|g| g.as_ref().map(|st| st.view.wants_text()))).unwrap_or(false);
    match msg {
        WM_NCHITTEST => {
            let mut rc = RECT::default();
            unsafe {
                let _ = GetWindowRect(hwnd, &mut rc);
            }
            let (x, y) = ((lparam.0 & 0xFFFF) as i16 as f32, ((lparam.0 >> 16) & 0xFFFF) as i16 as f32);
            let (cx, cy) = ((x - rc.left as f32) / scale - MARGIN, (y - rc.top as f32) / scale - MARGIN);
            if cx < 0.0 || cy < 0.0 || cx >= W || cy >= H {
                return LRESULT(HTTRANSPARENT as isize);
            }
            if cy < 44.0 && cx < W - 50.0 {
                return LRESULT(HTCAPTION as isize);
            }
            LRESULT(HTCLIENT as isize)
        }
        WM_ERASEBKGND => LRESULT(1),
        WM_PAINT => {
            unsafe {
                let mut ps = PAINTSTRUCT::default();
                BeginPaint(hwnd, &mut ps);
                let _ = EndPaint(hwnd, &ps);
            }
            draw();
            LRESULT(0)
        }
        WM_MOUSEMOVE => {
            let (x, y) = point(lparam, scale);
            let held = wparam.0 & 0x0001 != 0; // MK_LBUTTON
            send(hwnd, if held { Event::PointerDrag(x, y) } else { Event::PointerMove(x, y) });
            LRESULT(0)
        }
        WM_LBUTTONDOWN => {
            unsafe {
                SetCapture(hwnd);
            }
            let (x, y) = point(lparam, scale);
            send(hwnd, Event::PointerDown(x, y));
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            unsafe {
                let _ = ReleaseCapture();
            }
            let (x, y) = point(lparam, scale);
            send(hwnd, Event::PointerUp(x, y));
            LRESULT(0)
        }
        WM_MOUSEWHEEL => {
            send(hwnd, Event::PointerWheel(((wparam.0 >> 16) as i16) as f32 / 120.0));
            LRESULT(0)
        }
        WM_KEYDOWN => {
            if wants_text {
                if let Some(e) = crate::input_win::key_event(hwnd, wparam.0 as u16) {
                    send(hwnd, e);
                }
            } else if wparam.0 as u16 == VK_ESCAPE.0 {
                unsafe {
                    let _ = DestroyWindow(hwnd);
                }
            }
            LRESULT(0)
        }
        WM_CHAR if wants_text => {
            let unit = wparam.0 as u16;
            let text = STATE.with(|s| {
                let mut g = s.try_borrow_mut().ok()?;
                let st = g.as_mut()?;
                if (0xD800..0xDC00).contains(&unit) {
                    st.high_surrogate = unit;
                    None
                } else if (0xDC00..0xE000).contains(&unit) {
                    char::decode_utf16([std::mem::take(&mut st.high_surrogate), unit]).next().and_then(|r| r.ok()).map(String::from)
                } else if unit >= 0x20 && unit != 0x7F {
                    char::from_u32(unit as u32).map(String::from)
                } else {
                    None
                }
            });
            if let Some(t) = text {
                send(hwnd, Event::Text(t));
            }
            LRESULT(0)
        }
        WM_IME_STARTCOMPOSITION if wants_text => {
            place_ime(hwnd, scale);
            LRESULT(0)
        }
        WM_IME_COMPOSITION if wants_text => {
            for e in crate::input_win::ime_events(hwnd, lparam.0 as u32) {
                send(hwnd, e);
            }
            place_ime(hwnd, scale);
            LRESULT(0)
        }
        WM_IME_ENDCOMPOSITION if wants_text => {
            send(hwnd, Event::Compose(None));
            LRESULT(0)
        }
        WM_WINDOWPOSCHANGED | WM_ACTIVATE => {
            sync_glass(hwnd);
            unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
        }
        WM_CLOSE => {
            unsafe {
                let _ = DestroyWindow(hwnd);
            }
            LRESULT(0)
        }
        WM_DESTROY => {
            let st = STATE.with(|s| s.try_borrow_mut().ok().and_then(|mut g| g.take()));
            if let Some(st) = st {
                unsafe {
                    let _ = DestroyWindow(st.glass);
                }
            }
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

fn place_ime(hwnd: HWND, scale: f32) {
    let at = STATE.with(|s| s.try_borrow().ok().and_then(|g| g.as_ref().and_then(|st| st.view.text_anchor())));
    if let Some((x, y)) = at {
        crate::input_win::place_ime(hwnd, ((x + MARGIN) * scale) as i32, ((y + MARGIN) * scale) as i32);
    }
}
