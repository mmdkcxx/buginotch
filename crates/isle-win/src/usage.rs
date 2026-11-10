// SPDX-License-Identifier: GPL-3.0-or-later
//! Claude usage: reads the Claude Code login on this PC (read only) and asks Anthropic how much
//! of the 5-hour and weekly allowance is used. The login is never refreshed, copied, logged or
//! written anywhere: when it has expired the widget says so and waits for Claude Code to renew it.

use crate::lrclib::{Handle, check as handle};
use crate::system::epoch_ms;
use isle_logic::usage::{Credentials, Usage, parse_credentials, parse_usage, poll_seconds};
use std::ffi::c_void;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::time::Duration;
use windows::{Win32::Networking::WinHttp::*, core::*};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    /// Nothing asked yet.
    Loading,
    Ok,
    /// No Claude Code login was found.
    NoLogin,
    /// A login exists but has run out; Claude Code renews it when used.
    Expired,
    /// The last try failed (offline, or the reply was not understood).
    Offline,
}

#[derive(Clone, Debug)]
pub struct Snapshot {
    pub status: Status,
    pub usage: Option<Usage>,
    /// When `usage` was read.
    pub fetched_ms: i64,
    pub version: u64,
}

struct State {
    status: Status,
    usage: Option<Usage>,
    fetched_ms: i64,
    version: u64,
    started: bool,
}

static REFRESH: AtomicBool = AtomicBool::new(false);

fn state() -> MutexGuard<'static, State> {
    static S: OnceLock<Mutex<State>> = OnceLock::new();
    S.get_or_init(|| Mutex::new(State { status: Status::Loading, usage: None, fetched_ms: 0, version: 0, started: false })).lock().unwrap_or_else(|e| e.into_inner())
}

pub fn snapshot() -> Snapshot {
    let s = state();
    Snapshot { status: s.status, usage: s.usage, fetched_ms: s.fetched_ms, version: s.version }
}

/// Starts the background reader (once).
pub fn start() {
    {
        let mut s = state();
        if s.started {
            return;
        }
        s.started = true;
    }
    std::thread::spawn(run);
}

/// Asks for a reading now instead of waiting.
pub fn refresh() {
    REFRESH.store(true, Ordering::SeqCst);
}

fn set(status: Status, usage: Option<Usage>) {
    let mut s = state();
    s.status = status;
    if let Some(u) = usage {
        s.usage = Some(u);
        s.fetched_ms = epoch_ms();
    }
    s.version += 1;
}

fn run() {
    let mut found: Option<PathBuf> = None;
    let mut last_search = 0i64;
    loop {
        let now = epoch_ms();
        // A path that worked is kept; a search (which may touch WSL) is repeated at most every 10 minutes.
        if found.is_none() && now - last_search > 10 * 60_000 {
            last_search = now;
            found = find_credentials();
        }
        let creds = found.as_ref().and_then(|p| std::fs::read_to_string(p).ok()).and_then(|t| parse_credentials(&t));
        if creds.is_none() {
            found = None;
        }
        match creds {
            None => set(Status::NoLogin, None),
            Some(c) if c.expires_ms > 0 && c.expires_ms < now + 30_000 => set(Status::Expired, None),
            Some(c) => match fetch(&c) {
                Ok(Fetched::Usage(u)) => set(Status::Ok, Some(u)),
                Ok(Fetched::Unauthorized) => set(Status::Expired, None),
                Err(_) => set(Status::Offline, None),
            },
        }
        let nearest = {
            let s = state();
            s.usage.and_then(|u| [u.five, u.week].into_iter().flatten().filter_map(|w| w.resets_at_ms).map(|t| t - epoch_ms()).filter(|r| *r > -180_000).min())
        };
        let wait = Duration::from_secs(poll_seconds(nearest));
        let mut waited = Duration::ZERO;
        while waited < wait && !REFRESH.swap(false, Ordering::SeqCst) {
            std::thread::sleep(Duration::from_millis(500));
            waited += Duration::from_millis(500);
        }
    }
}

/// The Claude Code login: `ISLE_CLAUDE_CREDENTIALS`, this user's own profile, then any WSL
/// distribution's home folders (Claude Code is often run there).
fn find_credentials() -> Option<PathBuf> {
    let has_login = |p: &PathBuf| std::fs::read_to_string(p).ok().and_then(|t| parse_credentials(&t)).is_some();
    if let Some(p) = std::env::var_os("ISLE_CLAUDE_CREDENTIALS").map(PathBuf::from) {
        if has_login(&p) {
            return Some(p);
        }
    }
    if let Some(home) = std::env::var_os("USERPROFILE").map(PathBuf::from) {
        let p = home.join(".claude").join(".credentials.json");
        if has_login(&p) {
            return Some(p);
        }
    }
    for distro in wsl_distributions() {
        let homes = PathBuf::from(format!("\\\\wsl.localhost\\{distro}\\home"));
        let Ok(rd) = std::fs::read_dir(&homes) else { continue };
        for entry in rd.flatten() {
            let p = entry.path().join(".claude").join(".credentials.json");
            if has_login(&p) {
                return Some(p);
            }
        }
    }
    None
}

/// Names of installed WSL distributions, from the registry (so WSL is not started to ask).
fn wsl_distributions() -> Vec<String> {
    use std::os::windows::process::CommandExt;
    let out = std::process::Command::new("reg.exe")
        .args(["query", "HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Lxss", "/s", "/v", "DistributionName"])
        .creation_flags(0x0800_0000)
        .output();
    let Ok(out) = out else { return Vec::new() };
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|l| l.trim().strip_prefix("DistributionName"))
        .filter_map(|rest| rest.trim().strip_prefix("REG_SZ"))
        .map(|n| n.trim().to_string())
        .filter(|n| !n.is_empty() && !n.contains(['\\', '/']))
        .collect()
}

enum Fetched {
    Usage(Usage),
    Unauthorized,
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn fetch(c: &Credentials) -> Result<Fetched> {
    unsafe {
        let session = handle(WinHttpOpen(w!("Buginotch-Usage"), WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, PCWSTR::null(), PCWSTR::null(), 0))?;
        WinHttpSetTimeouts(session.0, 10_000, 10_000, 15_000, 20_000)?;
        let conn = handle(WinHttpConnect(session.0, w!("api.anthropic.com"), 443, 0))?;
        let path = wide("/api/oauth/usage");
        let req = handle(WinHttpOpenRequest(conn.0, w!("GET"), PCWSTR(path.as_ptr()), PCWSTR::null(), PCWSTR::null(), std::ptr::null(), WINHTTP_FLAG_SECURE))?;
        let headers = wide(&format!("Authorization: Bearer {}\r\nanthropic-beta: oauth-2025-04-20\r\nAccept: application/json\r\n", c.token));
        WinHttpAddRequestHeaders(req.0, &headers[..headers.len() - 1], WINHTTP_ADDREQ_FLAG_ADD)?;
        WinHttpSendRequest(req.0, None, None, 0, 0, 0)?;
        WinHttpReceiveResponse(req.0, std::ptr::null_mut())?;
        let mut status = 0u32;
        let mut len = std::mem::size_of::<u32>() as u32;
        WinHttpQueryHeaders(req.0, WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER, PCWSTR::null(), Some(&mut status as *mut u32 as *mut c_void), &mut len, std::ptr::null_mut())?;
        crate::log(&format!("usage GET -> {status}"));
        if status == 401 || status == 403 {
            return Ok(Fetched::Unauthorized);
        }
        if status != 200 {
            return Err(Error::from(windows::Win32::Foundation::E_FAIL));
        }
        let mut body = Vec::new();
        loop {
            let mut available = 0u32;
            WinHttpQueryDataAvailable(req.0, &mut available)?;
            if available == 0 || body.len() > 1_000_000 {
                break;
            }
            let mut chunk = vec![0u8; available as usize];
            let mut read = 0u32;
            WinHttpReadData(req.0, chunk.as_mut_ptr() as *mut c_void, available, &mut read)?;
            body.extend_from_slice(&chunk[..read as usize]);
        }
        match parse_usage(&String::from_utf8_lossy(&body)) {
            Some(u) => Ok(Fetched::Usage(u)),
            None => Err(Error::from(windows::Win32::Foundation::E_FAIL)),
        }
    }
}

#[allow(dead_code)]
fn _keep(_: &Handle) {}
