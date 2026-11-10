// SPDX-License-Identifier: GPL-3.0-or-later
//! Fetches a Bible chapter from SABDA's online Bible (Terjemahan Baru) for the Ayat page. One
//! small request per half hour; nothing is stored on disk.

use crate::lrclib::check as handle;
use std::ffi::c_void;
use windows::{Win32::Networking::WinHttp::*, core::*};

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// The XML for a whole chapter, e.g. `passage` = "1 Samuel 3".
pub fn fetch_chapter(passage: &str) -> Result<String> {
    unsafe {
        let session = handle(WinHttpOpen(w!("Buginotch-Ayat"), WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, PCWSTR::null(), PCWSTR::null(), 0))?;
        WinHttpSetTimeouts(session.0, 10_000, 10_000, 15_000, 20_000)?;
        let conn = handle(WinHttpConnect(session.0, w!("alkitab.sabda.org"), 443, 0))?;
        let query: String = passage.chars().map(|c| if c == ' ' { "+".to_string() } else { c.to_string() }).collect();
        let path = wide(&format!("/api/passage.php?passage={query}"));
        let req = handle(WinHttpOpenRequest(conn.0, w!("GET"), PCWSTR(path.as_ptr()), PCWSTR::null(), PCWSTR::null(), std::ptr::null(), WINHTTP_FLAG_SECURE))?;
        WinHttpSendRequest(req.0, None, None, 0, 0, 0)?;
        WinHttpReceiveResponse(req.0, std::ptr::null_mut())?;
        let mut status = 0u32;
        let mut len = std::mem::size_of::<u32>() as u32;
        WinHttpQueryHeaders(req.0, WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER, PCWSTR::null(), Some(&mut status as *mut u32 as *mut c_void), &mut len, std::ptr::null_mut())?;
        crate::log(&format!("ayat GET {passage} -> {status}"));
        if status != 200 {
            return Err(Error::from(windows::Win32::Foundation::E_FAIL));
        }
        let mut body = Vec::new();
        loop {
            let mut available = 0u32;
            WinHttpQueryDataAvailable(req.0, &mut available)?;
            if available == 0 || body.len() > 500_000 {
                break;
            }
            let mut chunk = vec![0u8; available as usize];
            let mut read = 0u32;
            WinHttpReadData(req.0, chunk.as_mut_ptr() as *mut c_void, available, &mut read)?;
            body.extend_from_slice(&chunk[..read as usize]);
        }
        Ok(String::from_utf8_lossy(&body).into_owned())
    }
}
