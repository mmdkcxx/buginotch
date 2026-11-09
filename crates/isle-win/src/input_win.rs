// SPDX-License-Identifier: GPL-3.0-or-later
//! Keyboard text input for the notch: key mapping, the clipboard and IME composition.

use isle_logic::module::{Composition, EditKey, Event};
use windows::core::{PCWSTR, w};
use windows::Win32::{
    Foundation::*,
    System::DataExchange::*,
    System::Memory::*,
    UI::{
        Input::Ime::*,
        Input::KeyboardAndMouse::*,
    },
};

const CF_UNICODETEXT: u32 = 13;

fn down(vk: VIRTUAL_KEY) -> bool {
    unsafe { GetKeyState(vk.0 as i32) < 0 }
}

/// Maps a WM_KEYDOWN to a text-box event. Paste needs the clipboard, so it is read here.
pub fn key_event(hwnd: HWND, vk: u16) -> Option<Event> {
    let (shift, ctrl) = (down(VK_SHIFT), down(VK_CONTROL));
    let key = match VIRTUAL_KEY(vk) {
        VK_BACK => EditKey::Backspace,
        VK_DELETE => EditKey::Delete,
        VK_LEFT => EditKey::Left,
        VK_RIGHT => EditKey::Right,
        VK_HOME => EditKey::Home,
        VK_END => EditKey::End,
        VK_RETURN => EditKey::Enter,
        VK_ESCAPE => EditKey::Escape,
        VK_A if ctrl => return Some(Event::Shortcut('a')),
        VK_C if ctrl => return Some(Event::Shortcut('c')),
        VK_X if ctrl => return Some(Event::Shortcut('x')),
        VK_V if ctrl => return read_clipboard(hwnd).map(Event::Text),
        _ => return None,
    };
    Some(Event::Edit { key, shift })
}

/// Reads the clipboard as text, if it holds any.
pub fn read_clipboard(hwnd: HWND) -> Option<String> {
    unsafe {
        OpenClipboard(Some(hwnd)).ok()?;
        let text = (|| {
            let handle = GetClipboardData(CF_UNICODETEXT).ok()?;
            let global = HGLOBAL(handle.0);
            let ptr = GlobalLock(global) as *const u16;
            if ptr.is_null() {
                return None;
            }
            let mut len = 0;
            while *ptr.add(len) != 0 && len < 100_000 {
                len += 1;
            }
            let s = String::from_utf16_lossy(std::slice::from_raw_parts(ptr, len));
            let _ = GlobalUnlock(global);
            Some(s)
        })();
        let _ = CloseClipboard();
        text
    }
}

/// The clipboard's text for the history, or None when it holds none or the app that copied
/// it asked not to be recorded (password managers do). Another app may hold the clipboard
/// for a moment, so opening is retried briefly.
pub fn clipboard_for_history(hwnd: HWND) -> Option<String> {
    unsafe {
        let mut opened = false;
        for _ in 0..8 {
            if OpenClipboard(Some(hwnd)).is_ok() {
                opened = true;
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(15));
        }
        if !opened {
            return None;
        }
        let excluded = |name: PCWSTR| {
            let f = RegisterClipboardFormatW(name);
            f != 0 && IsClipboardFormatAvailable(f).is_ok()
        };
        let private = excluded(w!("ExcludeClipboardContentFromMonitorProcessing"))
            || (|| {
                // "CanIncludeInClipboardHistory" holds a DWORD; 0 means keep it out.
                let f = RegisterClipboardFormatW(w!("CanIncludeInClipboardHistory"));
                if f == 0 || IsClipboardFormatAvailable(f).is_err() {
                    return false;
                }
                let Ok(h) = GetClipboardData(f) else { return false };
                let g = HGLOBAL(h.0);
                let p = GlobalLock(g) as *const u32;
                let zero = !p.is_null() && *p == 0;
                let _ = GlobalUnlock(g);
                zero
            })();
        let text = if private || IsClipboardFormatAvailable(CF_UNICODETEXT).is_err() {
            None
        } else {
            (|| {
                let handle = GetClipboardData(CF_UNICODETEXT).ok()?;
                let global = HGLOBAL(handle.0);
                let ptr = GlobalLock(global) as *const u16;
                if ptr.is_null() {
                    return None;
                }
                let mut len = 0;
                while *ptr.add(len) != 0 && len < 100_000 {
                    len += 1;
                }
                let s = String::from_utf16_lossy(std::slice::from_raw_parts(ptr, len));
                let _ = GlobalUnlock(global);
                Some(s)
            })()
        };
        let _ = CloseClipboard();
        text
    }
}

/// Puts `text` on the clipboard.
pub fn write_clipboard(hwnd: HWND, text: &str) {
    unsafe {
        if OpenClipboard(Some(hwnd)).is_err() {
            return;
        }
        let _ = EmptyClipboard();
        let wide: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
        if let Ok(global) = GlobalAlloc(GMEM_MOVEABLE, wide.len() * 2) {
            let ptr = GlobalLock(global) as *mut u16;
            if !ptr.is_null() {
                std::ptr::copy_nonoverlapping(wide.as_ptr(), ptr, wide.len());
                let _ = GlobalUnlock(global);
                if SetClipboardData(CF_UNICODETEXT, Some(HANDLE(global.0))).is_err() {
                    let _ = GlobalFree(Some(global));
                }
            }
        }
        let _ = CloseClipboard();
    }
}

/// The text of an IME string (`GCS_*` selects which one).
fn ime_string(himc: HIMC, kind: IME_COMPOSITION_STRING) -> String {
    unsafe {
        let bytes = ImmGetCompositionStringW(himc, kind, None, 0);
        if bytes <= 0 {
            return String::new();
        }
        let mut buf = vec![0u16; bytes as usize / 2];
        ImmGetCompositionStringW(himc, kind, Some(buf.as_mut_ptr() as *mut _), bytes as u32);
        String::from_utf16_lossy(&buf)
    }
}

/// Events for a WM_IME_COMPOSITION with the given flags: a finished string and/or an
/// updated composition.
pub fn ime_events(hwnd: HWND, flags: u32) -> Vec<Event> {
    let mut out = Vec::new();
    unsafe {
        let himc = ImmGetContext(hwnd);
        if himc.0.is_null() {
            return out;
        }
        if flags & GCS_RESULTSTR.0 != 0 {
            let done = ime_string(himc, GCS_RESULTSTR);
            if !done.is_empty() {
                out.push(Event::Text(done));
            }
        }
        if flags & GCS_COMPSTR.0 != 0 {
            let text = ime_string(himc, GCS_COMPSTR);
            let cursor = ImmGetCompositionStringW(himc, GCS_CURSORPOS, None, 0).max(0) as usize;
            out.push(Event::Compose((!text.is_empty()).then_some(Composition { text, cursor })));
        }
        let _ = ImmReleaseContext(hwnd, himc);
    }
    out
}

/// Places the IME's candidate window at the caret (client coordinates, pixels).
pub fn place_ime(hwnd: HWND, x: i32, y: i32) {
    unsafe {
        let himc = ImmGetContext(hwnd);
        if himc.0.is_null() {
            return;
        }
        let form = CANDIDATEFORM { dwIndex: 0, dwStyle: CFS_CANDIDATEPOS, ptCurrentPos: POINT { x, y }, rcArea: RECT::default() };
        let _ = ImmSetCandidateWindow(himc, &form);
        let comp = COMPOSITIONFORM { dwStyle: CFS_POINT, ptCurrentPos: POINT { x, y }, rcArea: RECT::default() };
        let _ = ImmSetCompositionWindow(himc, &comp);
        let _ = ImmReleaseContext(hwnd, himc);
    }
}
