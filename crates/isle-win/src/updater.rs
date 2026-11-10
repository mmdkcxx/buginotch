// SPDX-License-Identifier: GPL-3.0-or-later
//! Self-update: looks for a newer GitHub release now and then, downloads its installer on
//! request, checks its SHA-256 and runs it. All network work happens on one worker thread;
//! the UI only reads `state()` and sends commands. Nothing is installed without a click.

use crate::lrclib::{Handle, check as handle};
use crate::system::epoch_ms;
use isle_logic::update::{Release, Sha256, is_newer, parse_release, parse_sha256_text, split_url};
use std::ffi::c_void;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicU64, Ordering};
use std::sync::mpsc::{Sender, channel};
use std::sync::{Mutex, Once, OnceLock};
use std::time::Duration;
use windows::{
    Win32::{
        Foundation::{LPARAM, WPARAM, HWND},
        Networking::WinHttp::*,
        UI::WindowsAndMessaging::{PostMessageW, WM_COMMAND},
    },
    core::*,
};

/// What the update feature is doing, for the Pembaruan page.
#[derive(Debug, Clone)]
pub enum State {
    /// Nothing checked yet.
    Idle,
    Checking,
    UpToDate { at_ms: i64 },
    Available(Release),
    Downloading { release: Release, done: u64, total: u64 },
    Ready { release: Release, path: PathBuf },
    /// `release` is kept when the failure was in downloading it, so "Coba lagi" can resume.
    Failed { msg: String, release: Option<Release> },
}

enum Cmd {
    Check,
    Download,
}

static STATE: Mutex<State> = Mutex::new(State::Idle);
static CHANGED: AtomicU64 = AtomicU64::new(1);
static CANCEL: AtomicBool = AtomicBool::new(false);
static HWND_RAW: AtomicIsize = AtomicIsize::new(0);
static TX: OnceLock<Mutex<Sender<Cmd>>> = OnceLock::new();
static INSTALL_ON_EXIT: Mutex<Option<PathBuf>> = Mutex::new(None);
static START: Once = Once::new();

/// The version this build is.
pub fn current_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

pub fn state() -> State {
    STATE.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

/// Goes up on every state change or download progress step, so callers know when to redraw.
pub fn changes() -> u64 {
    CHANGED.load(Ordering::Relaxed)
}

fn set(state: State) {
    *STATE.lock().unwrap_or_else(|e| e.into_inner()) = state;
    CHANGED.fetch_add(1, Ordering::Relaxed);
}

pub fn set_hwnd(hwnd: HWND) {
    HWND_RAW.store(hwnd.0 as isize, Ordering::Relaxed);
}

fn send(cmd: Cmd) {
    if let Some(tx) = TX.get() {
        let _ = tx.lock().map(|t| t.send(cmd));
    }
}

/// Look for a newer release now and say so on the page even when there is none.
pub fn check_now() {
    send(Cmd::Check);
}

pub fn download() {
    CANCEL.store(false, Ordering::Relaxed);
    send(Cmd::Download);
}

pub fn cancel_download() {
    CANCEL.store(true, Ordering::Relaxed);
}

/// Leaves an error message: back to the release if there is one to retry, else idle.
pub fn dismiss_failure() {
    let next = match state() {
        State::Failed { release: Some(r), .. } => State::Available(r),
        State::Failed { .. } => State::Idle,
        other => other,
    };
    set(next);
}

/// Never announce this version again (a newer one still is).
pub fn skip(version: &str) {
    let v = version.to_string();
    crate::settings_io::update(|s| s.skipped_version = v);
    set(State::UpToDate { at_ms: epoch_ms() });
}

/// Runs the downloaded installer silently and quits, so the installer can replace the exe
/// and start the new version.
pub fn install_now() {
    let State::Ready { path, .. } = state() else { return };
    if spawn_installer(&path) {
        let raw = HWND_RAW.load(Ordering::Relaxed);
        if raw != 0 {
            unsafe {
                let _ = PostMessageW(Some(HWND(raw as *mut c_void)), WM_COMMAND, WPARAM(crate::tray::ID_QUIT), LPARAM(0));
            }
        }
    } else {
        set(State::Failed { msg: "Installer tidak bisa dijalankan".into(), release: None });
    }
}

/// Install the downloaded update when IsleWin quits next.
pub fn install_on_exit() {
    if let State::Ready { path, .. } = state() {
        *INSTALL_ON_EXIT.lock().unwrap_or_else(|e| e.into_inner()) = Some(path);
    }
}

pub fn install_on_exit_pending() -> bool {
    INSTALL_ON_EXIT.lock().unwrap_or_else(|e| e.into_inner()).is_some()
}

/// Called once after the window closed: runs an installer queued by `install_on_exit`.
pub fn run_pending_install() {
    let path = INSTALL_ON_EXIT.lock().unwrap_or_else(|e| e.into_inner()).take();
    if let Some(path) = path {
        spawn_installer(&path);
    }
}

fn spawn_installer(path: &std::path::Path) -> bool {
    use std::os::windows::process::CommandExt;
    // DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP: the installer must outlive this process.
    match std::process::Command::new(path).arg("/S").creation_flags(0x0000_0008 | 0x0000_0200).spawn() {
        Ok(_) => true,
        Err(e) => {
            crate::log(&format!("update: cannot start installer: {e}"));
            false
        }
    }
}

/// Starts the worker (once): first check shortly after start, then every `update_check_hours`.
pub fn spawn() {
    START.call_once(|| {
        let (tx, rx) = channel();
        let _ = TX.set(Mutex::new(tx));
        std::thread::spawn(move || {
            let mut wait = Duration::from_secs(20);
            loop {
                match rx.recv_timeout(wait) {
                    Ok(Cmd::Check) => check(true),
                    Ok(Cmd::Download) => download_release(),
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                        if crate::settings_io::load().auto_update {
                            check(false);
                        }
                    }
                    Err(_) => break,
                }
                let hours = crate::settings_io::load().update_check_hours.max(1) as u64;
                wait = Duration::from_secs(hours * 3600);
            }
        });
    });
}

/// The GitHub token: the `ISLE_UPDATE_TOKEN` environment variable if set (nothing on disk),
/// else `update_token` from settings.toml. Empty for a public repository.
fn token_of(settings: &isle_logic::settings::Settings) -> String {
    std::env::var("ISLE_UPDATE_TOKEN").ok().filter(|t| !t.is_empty()).unwrap_or_else(|| settings.update_token.clone())
}

fn busy_with_a_release() -> bool {
    matches!(state(), State::Downloading { .. } | State::Ready { .. })
}

/// `manual`: the user asked, so say "up to date" or the error; an automatic check stays quiet.
fn check(manual: bool) {
    if busy_with_a_release() {
        return;
    }
    let before = state();
    if manual {
        set(State::Checking);
    }
    let settings = crate::settings_io::load();
    let path = format!("/repos/{}/releases/latest", settings.update_repo);
    let token = token_of(&settings);
    let result = api_get(&path, &token);
    let failed = |msg: String| {
        crate::log(&format!("update check failed: {msg}"));
        if manual {
            set(State::Failed { msg, release: None });
        } else if matches!(state(), State::Checking) {
            set(before.clone());
        }
    };
    match result {
        Ok((200, body)) => match parse_release(&body) {
            Some(r) if is_newer(current_version(), &r.version) && (manual || r.version != settings.skipped_version) => {
                if !matches!(state(), State::Available(ref a) if a.version == r.version) {
                    set(State::Available(r));
                }
            }
            _ => set(State::UpToDate { at_ms: epoch_ms() }),
        },
        Ok((404, _)) => failed(if token.is_empty() { "Rilis tidak ditemukan. Repo privat? Isi update_token di settings.toml".into() } else { "Rilis tidak ditemukan atau token tidak berhak".into() }),
        Ok((401 | 403, _)) => failed("Token ditolak GitHub".into()),
        Ok((code, _)) => failed(format!("GitHub membalas {code}")),
        Err(_) => failed("Tidak ada koneksi".into()),
    }
}

fn download_release() {
    let (State::Available(release) | State::Failed { release: Some(release), .. }) = state() else { return };
    let fail = |msg: &str, release: &Release| set(State::Failed { msg: msg.to_string(), release: Some(release.clone()) });
    set(State::Downloading { release: release.clone(), done: 0, total: release.size });
    let settings = crate::settings_io::load();
    let token = token_of(&settings);
    let token = token.as_str();

    // The expected hash comes from the release itself, so a tampered download cannot match.
    let expected = match (&release.sha256, &release.sha_asset_url) {
        (Some(h), _) => Some(h.clone()),
        (None, Some(url)) => fetch_asset_text(url, token).and_then(|t| parse_sha256_text(&t, &release.asset_name)),
        _ => None,
    };
    let Some(expected) = expected else {
        return fail("Rilis ini tidak menyertakan checksum, jadi tidak dipasang", &release);
    };

    let dir = std::env::temp_dir().join("IsleWin-Update");
    let _ = std::fs::remove_dir_all(&dir);
    if std::fs::create_dir_all(&dir).is_err() {
        return fail("Folder sementara tidak bisa dibuat", &release);
    }
    let part = dir.join(format!("{}.part", release.asset_name));
    let final_path = dir.join(&release.asset_name);
    match stream_asset(&release, token, &part) {
        Ok(hash) if hash == expected => {
            if std::fs::rename(&part, &final_path).is_ok() {
                set(State::Ready { release, path: final_path });
            } else {
                fail("Berkas unduhan tidak bisa disimpan", &release);
            }
        }
        Ok(_) => {
            let _ = std::fs::remove_file(&part);
            fail("Checksum tidak cocok, berkas dibuang", &release);
        }
        Err(msg) => {
            let _ = std::fs::remove_file(&part);
            if msg == "dibatalkan" {
                set(State::Available(release));
            } else {
                fail(&msg, &release);
            }
        }
    }
}

// ---- HTTP ----

struct Response {
    status: u32,
    location: Option<String>,
    length: u64,
    req: Handle,
    _conn: Handle,
    _session: Handle,
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// GET https://host/path. `headers` are `(name, value)`; redirects are followed only when
/// `follow` (a download redirect must be taken by hand so the token is not sent along).
fn request(host: &str, path: &str, headers: &[(&str, &str)], follow: bool) -> Result<Response> {
    unsafe {
        let session = handle(WinHttpOpen(w!("IsleWin-Updater"), WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, PCWSTR::null(), PCWSTR::null(), 0))?;
        WinHttpSetTimeouts(session.0, 10_000, 10_000, 15_000, 30_000)?;
        let h = wide(host);
        let conn = handle(WinHttpConnect(session.0, PCWSTR(h.as_ptr()), 443, 0))?;
        let p = wide(path);
        let req = handle(WinHttpOpenRequest(conn.0, w!("GET"), PCWSTR(p.as_ptr()), PCWSTR::null(), PCWSTR::null(), std::ptr::null(), WINHTTP_FLAG_SECURE))?;
        if !follow {
            let never = WINHTTP_OPTION_REDIRECT_POLICY_NEVER.to_ne_bytes();
            WinHttpSetOption(Some(req.0 as *const c_void), WINHTTP_OPTION_REDIRECT_POLICY, Some(&never))?;
        }
        let block: String = headers.iter().map(|(n, v)| format!("{n}: {v}\r\n")).collect();
        if !block.is_empty() {
            let w = wide(&block);
            WinHttpAddRequestHeaders(req.0, &w[..w.len() - 1], WINHTTP_ADDREQ_FLAG_ADD)?;
        }
        WinHttpSendRequest(req.0, None, None, 0, 0, 0)?;
        WinHttpReceiveResponse(req.0, std::ptr::null_mut())?;

        let mut status = 0u32;
        let mut len = std::mem::size_of::<u32>() as u32;
        WinHttpQueryHeaders(req.0, WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER, PCWSTR::null(), Some(&mut status as *mut u32 as *mut c_void), &mut len, std::ptr::null_mut())?;
        let mut length = 0u32;
        let mut len = std::mem::size_of::<u32>() as u32;
        if WinHttpQueryHeaders(req.0, WINHTTP_QUERY_CONTENT_LENGTH | WINHTTP_QUERY_FLAG_NUMBER, PCWSTR::null(), Some(&mut length as *mut u32 as *mut c_void), &mut len, std::ptr::null_mut()).is_err() {
            length = 0;
        }
        let location = {
            let mut buf = vec![0u16; 2048];
            let mut bytes = (buf.len() * 2) as u32;
            WinHttpQueryHeaders(req.0, WINHTTP_QUERY_LOCATION, PCWSTR::null(), Some(buf.as_mut_ptr() as *mut c_void), &mut bytes, std::ptr::null_mut())
                .ok()
                .map(|_| String::from_utf16_lossy(&buf[..(bytes / 2) as usize]))
        };
        Ok(Response { status, location, length: length as u64, req, _conn: conn, _session: session })
    }
}

/// Reads the whole body in chunks; `sink` returns false to stop early.
fn read_body(resp: &Response, mut sink: impl FnMut(&[u8]) -> bool) -> Result<()> {
    unsafe {
        loop {
            let mut available = 0u32;
            WinHttpQueryDataAvailable(resp.req.0, &mut available)?;
            if available == 0 {
                return Ok(());
            }
            let mut chunk = vec![0u8; available.min(256 * 1024) as usize];
            let mut read = 0u32;
            WinHttpReadData(resp.req.0, chunk.as_mut_ptr() as *mut c_void, chunk.len() as u32, &mut read)?;
            if read == 0 || !sink(&chunk[..read as usize]) {
                return Ok(());
            }
        }
    }
}

fn auth_headers<'a>(token: &'a str, accept: &'a str, store: &'a mut String) -> Vec<(&'a str, &'a str)> {
    let mut h = vec![("Accept", accept), ("X-GitHub-Api-Version", "2022-11-28")];
    if !token.is_empty() {
        *store = format!("Bearer {token}");
        h.push(("Authorization", store.as_str()));
    }
    h
}

fn api_get(path: &str, token: &str) -> Result<(u32, String)> {
    let mut bearer = String::new();
    let headers = auth_headers(token, "application/vnd.github+json", &mut bearer);
    let resp = request("api.github.com", path, &headers, true)?;
    let mut body = Vec::new();
    read_body(&resp, |c| {
        body.extend_from_slice(c);
        body.len() < 2_000_000
    })?;
    Ok((resp.status, String::from_utf8_lossy(&body).into_owned()))
}

/// Opens an asset's download. For a private repository the API URL answers with a redirect to
/// a signed address; that second request must not carry the token.
fn open_asset(api_url: &str, token: &str) -> std::result::Result<Response, String> {
    let (host, path) = split_url(api_url).ok_or("Alamat unduhan tidak valid")?;
    let mut bearer = String::new();
    let headers = auth_headers(token, "application/octet-stream", &mut bearer);
    let first = request(&host, &path, &headers, false).map_err(|_| "Tidak ada koneksi".to_string())?;
    match first.status {
        200 => Ok(first),
        301 | 302 | 303 | 307 | 308 => {
            let (h2, p2) = first.location.as_deref().and_then(split_url).ok_or("Pengalihan unduhan tidak valid")?;
            let second = request(&h2, &p2, &[], true).map_err(|_| "Unduhan terputus".to_string())?;
            if second.status == 200 { Ok(second) } else { Err(format!("Unduhan ditolak ({})", second.status)) }
        }
        404 => Err("Berkas rilis tidak ditemukan (token?)".into()),
        401 | 403 => Err("Token ditolak GitHub".into()),
        code => Err(format!("GitHub membalas {code}")),
    }
}

fn fetch_asset_text(api_url: &str, token: &str) -> Option<String> {
    let resp = open_asset(api_url, token).ok()?;
    let mut body = Vec::new();
    read_body(&resp, |c| {
        body.extend_from_slice(c);
        body.len() < 100_000
    })
    .ok()?;
    Some(String::from_utf8_lossy(&body).into_owned())
}

/// Downloads the installer to `part`, reporting progress; returns its SHA-256.
fn stream_asset(release: &Release, token: &str, part: &std::path::Path) -> std::result::Result<String, String> {
    use std::io::Write;
    let resp = open_asset(&release.asset_url, token)?;
    let total = if resp.length > 0 { resp.length } else { release.size };
    let mut file = std::fs::File::create(part).map_err(|_| "Berkas unduhan tidak bisa dibuat".to_string())?;
    let mut hasher = Sha256::default();
    let (mut done, mut last_report) = (0u64, 0u64);
    let mut error = None;
    let read = read_body(&resp, |chunk| {
        if CANCEL.load(Ordering::Relaxed) {
            error = Some("dibatalkan".to_string());
            return false;
        }
        if file.write_all(chunk).is_err() {
            error = Some("Disk penuh atau berkas terkunci".to_string());
            return false;
        }
        hasher.update(chunk);
        done += chunk.len() as u64;
        if done - last_report >= 64 * 1024 {
            last_report = done;
            set(State::Downloading { release: release.clone(), done, total });
        }
        true
    });
    if let Some(e) = error {
        return Err(e);
    }
    read.map_err(|_| "Unduhan terputus".to_string())?;
    drop(file);
    if total > 0 && done != total {
        return Err("Unduhan tidak lengkap".into());
    }
    Ok(hasher.finish_hex())
}
