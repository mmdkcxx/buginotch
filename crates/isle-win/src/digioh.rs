// SPDX-License-Identifier: GPL-3.0-or-later
//! Timer Digioh link: a WebSocket to timer.digioh.id for one room. The site keeps the timer
//! state and relays it; this module keeps the latest state and sends changed ones. It joins
//! only when asked, reconnects by itself with a growing pause (up to 10 s), and stops on leave.

use crate::lrclib::{Handle, check as handle};
use isle_logic::digioh::{Timer, normalize_room, valid_room, ws_path};
use std::ffi::c_void;
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::time::Duration;
use windows::{
    Win32::Networking::WinHttp::*,
    core::*,
};

const HOST: &str = "timer.digioh.id";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Conn {
    /// Not in a room.
    Off,
    Connecting,
    Online,
    /// The connection dropped; trying again.
    Retrying,
}

#[derive(Clone, Debug)]
pub struct Snapshot {
    pub room: String,
    pub conn: Conn,
    /// The latest timer state; `None` until the room has answered.
    pub timer: Option<Timer>,
    /// Failed attempts since the last good connection.
    pub attempts: u32,
    /// Changes whenever anything above does.
    pub version: u64,
}

struct State {
    room: String,
    generation: u64,
    conn: Conn,
    timer: Option<Timer>,
    attempts: u32,
    version: u64,
    /// The open WebSocket, as a raw handle; whoever takes it closes it.
    ws: usize,
}

fn state() -> MutexGuard<'static, State> {
    static S: OnceLock<Mutex<State>> = OnceLock::new();
    S.get_or_init(|| Mutex::new(State { room: String::new(), generation: 0, conn: Conn::Off, timer: None, attempts: 0, version: 0, ws: 0 })).lock().unwrap_or_else(|e| e.into_inner())
}

fn bump(s: &mut State) {
    s.version += 1;
}

pub fn snapshot() -> Snapshot {
    let s = state();
    Snapshot { room: s.room.clone(), conn: s.conn, timer: s.timer.clone(), attempts: s.attempts, version: s.version }
}

/// The room code saved from last time.
pub fn last_room() -> String {
    std::fs::read_to_string(crate::system::data_path("digioh.json"))
        .ok()
        .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
        .and_then(|v| v["last"].as_str().map(normalize_room))
        .filter(|r| valid_room(r))
        .unwrap_or_default()
}

fn save_last(room: &str) {
    let path = crate::system::data_path("digioh.json");
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(path, serde_json::json!({ "last": room }).to_string());
}

/// Joins a room (leaving the current one). Returns false for a code that is not valid.
pub fn join(room: &str) -> bool {
    let room = normalize_room(room);
    if !valid_room(&room) {
        return false;
    }
    leave();
    let gen_id = {
        let mut s = state();
        s.generation += 1;
        s.room = room.clone();
        s.conn = Conn::Connecting;
        s.timer = None;
        s.attempts = 0;
        bump(&mut s);
        s.generation
    };
    save_last(&room);
    std::thread::spawn(move || run(room, gen_id));
    true
}

pub fn leave() {
    let ws = {
        let mut s = state();
        s.generation += 1;
        s.room.clear();
        s.conn = Conn::Off;
        s.timer = None;
        s.attempts = 0;
        bump(&mut s);
        std::mem::take(&mut s.ws)
    };
    close(ws);
}

fn close(ws: usize) {
    if ws != 0 {
        unsafe {
            let _ = WinHttpCloseHandle(ws as *mut c_void);
        }
    }
}

/// Sends a changed timer to the room and shows it at once. False when not connected.
pub fn send(timer: Timer, message: &str) -> bool {
    let ws = {
        let s = state();
        if s.conn != Conn::Online {
            return false;
        }
        s.ws
    };
    if ws == 0 {
        return false;
    }
    let ok = unsafe { WinHttpWebSocketSend(ws as *mut c_void, WINHTTP_WEB_SOCKET_UTF8_MESSAGE_BUFFER_TYPE, Some(message.as_bytes())) } == 0;
    if ok {
        let mut s = state();
        s.timer = Some(timer);
        bump(&mut s);
    }
    ok
}

fn current(gen_id: u64) -> bool {
    state().generation == gen_id
}

fn run(room: String, gen_id: u64) {
    let mut attempt = 0u32;
    while current(gen_id) {
        match connect(&room) {
            Ok(ws) => {
                {
                    let mut s = state();
                    if s.generation != gen_id {
                        drop(s);
                        close(ws.0 as usize);
                        std::mem::forget(ws);
                        return;
                    }
                    s.ws = ws.0 as usize;
                    s.conn = Conn::Online;
                    s.attempts = 0;
                    bump(&mut s);
                }
                attempt = 0;
                let raw = ws.0;
                std::mem::forget(ws); // the shared state owns it now
                let ask = "{\"type\":\"REQUEST_STATE\"}";
                unsafe {
                    WinHttpWebSocketSend(raw, WINHTTP_WEB_SOCKET_UTF8_MESSAGE_BUFFER_TYPE, Some(ask.as_bytes()));
                }
                listen(raw, gen_id);
                // Closed or broken: hand the handle back (once) and try again.
                let mut s = state();
                if s.generation == gen_id {
                    let owned = std::mem::take(&mut s.ws);
                    s.conn = Conn::Retrying;
                    bump(&mut s);
                    drop(s);
                    close(owned);
                } else {
                    return;
                }
            }
            Err(_) => {
                let mut s = state();
                if s.generation != gen_id {
                    return;
                }
                s.conn = Conn::Retrying;
                s.attempts += 1;
                bump(&mut s);
            }
        }
        attempt += 1;
        let pause = Duration::from_millis((1000u64 << attempt.min(4)).min(10_000));
        let mut waited = Duration::ZERO;
        while waited < pause && current(gen_id) {
            std::thread::sleep(Duration::from_millis(100));
            waited += Duration::from_millis(100);
        }
    }
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn connect(room: &str) -> Result<Handle> {
    unsafe {
        let session = handle(WinHttpOpen(w!("IsleWin-Timer"), WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, PCWSTR::null(), PCWSTR::null(), 0))?;
        WinHttpSetTimeouts(session.0, 10_000, 10_000, 15_000, 15_000)?;
        let h = wide(HOST);
        let conn = handle(WinHttpConnect(session.0, PCWSTR(h.as_ptr()), 443, 0))?;
        let p = wide(&ws_path(room));
        let req = handle(WinHttpOpenRequest(conn.0, w!("GET"), PCWSTR(p.as_ptr()), PCWSTR::null(), PCWSTR::null(), std::ptr::null(), WINHTTP_FLAG_SECURE))?;
        WinHttpSetOption(Some(req.0 as *const c_void), WINHTTP_OPTION_UPGRADE_TO_WEB_SOCKET, None)?;
        let origin = wide("Origin: https://timer.digioh.id\r\n");
        WinHttpAddRequestHeaders(req.0, &origin[..origin.len() - 1], WINHTTP_ADDREQ_FLAG_ADD)?;
        WinHttpSendRequest(req.0, None, None, 0, 0, 0)?;
        WinHttpReceiveResponse(req.0, std::ptr::null_mut())?;
        let ws = handle(WinHttpWebSocketCompleteUpgrade(req.0, None))?;
        // The WebSocket does not need the request, connection or session handles open: dropping
        // them here only releases our references.
        drop(req);
        drop(conn);
        std::mem::forget(session); // closing the session would end the socket with it
        Ok(ws)
    }
}

/// Reads messages until the socket closes, errors out, or this join is replaced.
fn listen(ws: *mut c_void, gen_id: u64) {
    let mut buf = vec![0u8; 64 * 1024];
    let mut message: Vec<u8> = Vec::new();
    while current(gen_id) {
        let mut read = 0u32;
        let mut kind = WINHTTP_WEB_SOCKET_BUFFER_TYPE(0);
        let rc = unsafe { WinHttpWebSocketReceive(ws, buf.as_mut_ptr() as *mut c_void, buf.len() as u32, &mut read, &mut kind) };
        if rc != 0 {
            return;
        }
        if kind == WINHTTP_WEB_SOCKET_CLOSE_BUFFER_TYPE {
            return;
        }
        message.extend_from_slice(&buf[..read as usize]);
        if message.len() > 4 * 1024 * 1024 {
            return;
        }
        if kind == WINHTTP_WEB_SOCKET_UTF8_FRAGMENT_BUFFER_TYPE || kind == WINHTTP_WEB_SOCKET_BINARY_FRAGMENT_BUFFER_TYPE {
            continue;
        }
        let text = String::from_utf8_lossy(&message).into_owned();
        message.clear();
        if let Some(timer) = Timer::from_message(&text) {
            let mut s = state();
            if s.generation != gen_id {
                return;
            }
            s.timer = Some(timer);
            bump(&mut s);
        }
    }
}
