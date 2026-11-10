// SPDX-License-Identifier: GPL-3.0-or-later
//! Lyrics from LRCLIB (https://lrclib.net) over WinHTTP, with an on-disk cache.
//! Blocking: call from a worker thread only.

use crate::system::data_path;
use isle_logic::lyrics::{Lyrics, cache_key, parse_lrclib, request_path};
use std::ffi::c_void;
use std::time::{Duration, SystemTime};
use windows::{Win32::Networking::WinHttp::*, core::*};

const MISS_TTL: Duration = Duration::from_secs(24 * 3600);

pub(crate) struct Handle(pub(crate) *mut c_void);

impl Drop for Handle {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe {
                let _ = WinHttpCloseHandle(self.0);
            }
        }
    }
}

pub(crate) fn check(h: *mut c_void) -> Result<Handle> {
    if h.is_null() { Err(Error::from_thread()) } else { Ok(Handle(h)) }
}

/// GET `path` on lrclib.net. `Ok(None)` on 404; an error for any other failure.
fn get(path: &str) -> Result<Option<String>> {
    unsafe {
        let session = check(WinHttpOpen(w!("Buginotch/0.1"), WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, PCWSTR::null(), PCWSTR::null(), 0))?;
        WinHttpSetTimeouts(session.0, 5000, 5000, 5000, 5000)?;
        let connect = check(WinHttpConnect(session.0, w!("lrclib.net"), 443, 0))?;
        let wide: Vec<u16> = path.encode_utf16().chain(std::iter::once(0)).collect();
        let req = check(WinHttpOpenRequest(connect.0, w!("GET"), PCWSTR(wide.as_ptr()), PCWSTR::null(), PCWSTR::null(), std::ptr::null(), WINHTTP_FLAG_SECURE))?;
        WinHttpSendRequest(req.0, None, None, 0, 0, 0)?;
        WinHttpReceiveResponse(req.0, std::ptr::null_mut())?;

        let mut status = 0u32;
        let mut len = std::mem::size_of::<u32>() as u32;
        WinHttpQueryHeaders(req.0, WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER, PCWSTR::null(), Some(&mut status as *mut u32 as *mut c_void), &mut len, std::ptr::null_mut())?;
        crate::log(&format!("lrclib GET {path} -> {status}"));
        if status == 404 {
            return Ok(None);
        }
        if status != 200 {
            return Err(Error::from(windows::Win32::Foundation::E_FAIL));
        }
        let mut body = Vec::new();
        loop {
            let mut available = 0u32;
            WinHttpQueryDataAvailable(req.0, &mut available)?;
            if available == 0 || body.len() > 4_000_000 {
                break;
            }
            let mut chunk = vec![0u8; available as usize];
            let mut read = 0u32;
            WinHttpReadData(req.0, chunk.as_mut_ptr() as *mut c_void, available, &mut read)?;
            body.extend_from_slice(&chunk[..read as usize]);
        }
        Ok(Some(String::from_utf8_lossy(&body).into_owned()))
    }
}

/// Lyrics for a track: from the cache when possible, else from LRCLIB (and cached).
/// Network failures give `Lyrics::None` without caching, so the next track change retries.
pub fn lookup(title: &str, artist: &str, album: &str, duration_ms: i64) -> Lyrics {
    let file = data_path(&format!("lyrics\\{}.json", cache_key(title, artist, duration_ms)));
    if let Ok(text) = std::fs::read_to_string(&file) {
        let parsed = parse_lrclib(&text);
        let fresh_miss = std::fs::metadata(&file)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| SystemTime::now().duration_since(t).ok())
            .is_some_and(|age| age < MISS_TTL);
        if parsed != Lyrics::None || fresh_miss {
            return parsed;
        }
    }
    let duration_s = ((duration_ms + 500) / 1000).max(0) as u32;
    // LRCLIB matches the album exactly when given, so a spelling difference gives a
    // 404; retry once without it.
    let mut result = get(&request_path(title, artist, album, duration_s));
    if matches!(result, Ok(None)) && !album.trim().is_empty() {
        result = get(&request_path(title, artist, "", duration_s));
    }
    match result {
        Ok(Some(body)) => {
            store(&file, &body);
            parse_lrclib(&body)
        }
        Ok(None) => {
            store(&file, "{}");
            Lyrics::None
        }
        Err(e) => {
            crate::log(&format!("lrclib error: {e}"));
            Lyrics::None
        }
    }
}

fn store(file: &std::path::Path, body: &str) {
    if let Some(dir) = file.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(file, body);
}
