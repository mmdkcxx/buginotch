// SPDX-License-Identifier: GPL-3.0-or-later
//! Phone link: a tiny HTTP server that lets a phone on the same Wi-Fi send files to the Shelf
//! and fetch the files marked for it. Off until the user turns it on, and it stops by itself
//! after 30 minutes without a request. Every request needs the pairing token from the QR code.

use crate::system::{data_path, epoch_ms};
use isle_logic::phone::{self, MAX_FILE_BYTES};
use std::collections::hash_map::RandomState;
use std::hash::{BuildHasher, Hasher};
use std::io::{Read, Write};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener, TcpStream, UdpSocket};
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::time::Duration;

const PAGE: &str = include_str!("phone.html");

#[derive(Clone, Debug)]
pub struct Transfer {
    pub id: u64,
    pub name: String,
    pub total: u64,
    pub done: u64,
}

#[derive(Default)]
struct State {
    running: bool,
    /// Bumped on every start, so an old server thread knows to quit.
    generation: u64,
    port: u16,
    token: String,
    ip: String,
    last_ms: i64,
    active: Vec<Transfer>,
    finished: Vec<String>,
    outgoing: Vec<(u64, String)>,
    next_id: u64,
    loaded: bool,
}

fn state() -> MutexGuard<'static, State> {
    static S: OnceLock<Mutex<State>> = OnceLock::new();
    S.get_or_init(|| Mutex::new(State::default())).lock().unwrap_or_else(|e| e.into_inner())
}

fn random_bytes() -> [u8; 16] {
    let mut out = [0u8; 16];
    for chunk in out.chunks_mut(8) {
        let mut h = RandomState::new().build_hasher();
        h.write_i64(epoch_ms());
        chunk.copy_from_slice(&h.finish().to_le_bytes());
    }
    out
}

fn config_path() -> std::path::PathBuf {
    data_path("phone.json")
}

fn save_config(s: &State) {
    let path = config_path();
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(path, serde_json::json!({ "token": s.token, "port": s.port }).to_string());
}

fn load_config(s: &mut State) {
    if s.loaded {
        return;
    }
    s.loaded = true;
    if let Some(v) = std::fs::read_to_string(config_path()).ok().and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok()) {
        s.token = v["token"].as_str().unwrap_or("").to_string();
        s.port = v["port"].as_u64().unwrap_or(0) as u16;
    }
    if s.token.len() != 16 {
        s.token = phone::token_from(&random_bytes());
    }
}

/// Where files from the phone are kept.
pub fn inbox() -> std::path::PathBuf {
    let base = std::env::var_os("LOCALAPPDATA").map(std::path::PathBuf::from).unwrap_or_else(std::env::temp_dir);
    base.join("IsleWin").join("Shelf").join("Ponsel")
}

/// True for a file that came from the phone.
pub fn is_from_phone(path: &str) -> bool {
    path.to_ascii_lowercase().contains("\\shelf\\ponsel\\")
}

/// This PC's address on the local network (the one the default route uses).
fn local_ip() -> Option<Ipv4Addr> {
    let sock = UdpSocket::bind("0.0.0.0:0").ok()?;
    sock.connect("192.0.2.1:9").ok()?;
    match sock.local_addr().ok()?.ip() {
        IpAddr::V4(v) if !v.is_unspecified() && !v.is_loopback() => Some(v),
        _ => None,
    }
}

#[derive(Clone, Debug)]
pub struct Status {
    pub running: bool,
    pub url: String,
    pub address: String,
    pub minutes_left: i64,
    pub active: Vec<Transfer>,
}

pub fn status() -> Status {
    let s = state();
    Status {
        running: s.running,
        url: phone::page_url(&s.ip, s.port, &s.token),
        address: format!("{}:{}", s.ip, s.port),
        minutes_left: phone::minutes_left(s.last_ms, epoch_ms()),
        active: s.active.clone(),
    }
}

/// Paths of files that finished arriving since the last call.
pub fn take_finished() -> Vec<String> {
    std::mem::take(&mut state().finished)
}

/// The files the phone may download (Shelf tiles marked "send to phone").
pub fn set_outgoing(files: Vec<(u64, String)>) {
    state().outgoing = files;
}

/// Starts the server. Returns an error text when it cannot.
pub fn start() -> Result<(), String> {
    let mut s = state();
    if s.running {
        s.last_ms = epoch_ms();
        return Ok(());
    }
    load_config(&mut s);
    let ip = local_ip().ok_or("Tidak ada jaringan Wi-Fi")?;
    // Reuse the saved port so a saved Shortcut keeps working; fall back to a fresh one.
    let ports: Vec<u16> = if s.port != 0 { vec![s.port, 0] } else { vec![0] };
    let listener = ports.iter().find_map(|&p| TcpListener::bind(SocketAddr::from(([0, 0, 0, 0], p))).ok()).ok_or("Port tidak bisa dibuka")?;
    let _ = listener.set_nonblocking(true);
    s.port = listener.local_addr().map_err(|e| e.to_string())?.port();
    s.ip = ip.to_string();
    s.running = true;
    s.last_ms = epoch_ms();
    s.generation += 1;
    let gen_id = s.generation;
    save_config(&s);
    drop(s);
    let _ = std::fs::create_dir_all(inbox());
    std::thread::spawn(move || accept_loop(listener, gen_id));
    Ok(())
}

pub fn stop() {
    let mut s = state();
    s.running = false;
    s.active.clear();
}

/// Revokes the paired phone: a new token, so it has to scan the code again.
pub fn forget() {
    let mut s = state();
    s.token = phone::token_from(&random_bytes());
    save_config(&s);
}

fn accept_loop(listener: TcpListener, gen_id: u64) {
    loop {
        {
            let mut s = state();
            if !s.running || s.generation != gen_id {
                return;
            }
            // Idle only counts when nothing is transferring.
            if s.active.is_empty() && phone::minutes_left(s.last_ms, epoch_ms()) == 0 {
                s.running = false;
                return;
            }
        }
        match listener.accept() {
            Ok((stream, peer)) => {
                let _ = stream.set_nonblocking(false);
                std::thread::spawn(move || {
                    let _ = handle(stream, peer, gen_id);
                });
            }
            Err(_) => std::thread::sleep(Duration::from_millis(60)),
        }
    }
}

fn respond(s: &mut TcpStream, code: u16, kind: &str, body: &[u8]) -> std::io::Result<()> {
    let reason = match code {
        200 => "OK",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        411 => "Length Required",
        413 => "Payload Too Large",
        _ => "Error",
    };
    write!(s, "HTTP/1.1 {code} {reason}\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n", body.len())?;
    s.write_all(body)
}

fn read_head(s: &mut TcpStream) -> Option<(String, Vec<u8>)> {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 2048];
    loop {
        if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            let rest = buf.split_off(i + 4);
            buf.truncate(i + 2);
            return Some((String::from_utf8_lossy(&buf).into_owned(), rest));
        }
        if buf.len() > 16 * 1024 {
            return None;
        }
        let n = s.read(&mut chunk).ok()?;
        if n == 0 {
            return None;
        }
        buf.extend_from_slice(&chunk[..n]);
    }
}

fn touch() {
    state().last_ms = epoch_ms();
}

fn handle(mut s: TcpStream, peer: SocketAddr, gen_id: u64) -> std::io::Result<()> {
    s.set_read_timeout(Some(Duration::from_secs(30)))?;
    s.set_write_timeout(Some(Duration::from_secs(30)))?;
    if !phone::allowed_peer(peer.ip()) {
        return respond(&mut s, 403, "text/plain; charset=utf-8", b"Hanya jaringan lokal");
    }
    let Some((head, rest)) = read_head(&mut s) else { return respond(&mut s, 400, "text/plain", b"?") };
    let Some(req) = phone::parse_head(&head) else { return respond(&mut s, 400, "text/plain", b"?") };
    let (token, running, same_gen) = {
        let st = state();
        (st.token.clone(), st.running, st.generation == gen_id)
    };
    if !running || !same_gen {
        return respond(&mut s, 403, "text/plain; charset=utf-8", b"Server mati");
    }
    if !phone::token_matches(req.token(), &token) {
        return respond(&mut s, 401, "text/plain; charset=utf-8", "Pindai ulang kode QR di Buginotch.".as_bytes());
    }
    touch();
    match (req.method.as_str(), req.path.as_str()) {
        ("GET", "/") => respond(&mut s, 200, "text/html; charset=utf-8", PAGE.as_bytes()),
        ("GET", "/api/list") => {
            let items: Vec<String> = state()
                .outgoing
                .iter()
                .filter_map(|(id, p)| {
                    let meta = std::fs::metadata(p).ok().filter(|m| m.is_file())?;
                    let name = isle_logic::shelf::file_name(p);
                    Some(format!("{{\"id\":{id},\"name\":\"{}\",\"size\":{}}}", phone::json_escape(name), meta.len()))
                })
                .collect();
            respond(&mut s, 200, "application/json", format!("[{}]", items.join(",")).as_bytes())
        }
        ("GET", p) if p.starts_with("/f/") => {
            let id: u64 = p[3..].parse().unwrap_or(0);
            let path = state().outgoing.iter().find(|(i, _)| *i == id).map(|(_, p)| p.clone());
            match path.and_then(|p| std::fs::File::open(&p).ok().map(|f| (p, f))) {
                Some((p, mut f)) => {
                    let len = f.metadata().map(|m| m.len()).unwrap_or(0);
                    let name = isle_logic::shelf::file_name(&p).to_string();
                    write!(
                        s,
                        "HTTP/1.1 200 OK\r\nContent-Type: {}\r\nContent-Length: {len}\r\nContent-Disposition: attachment; filename*=UTF-8''{}\r\nConnection: close\r\n\r\n",
                        phone::content_type(&name),
                        phone::percent_encode(&name)
                    )?;
                    let mut buf = vec![0u8; 256 * 1024];
                    loop {
                        let n = f.read(&mut buf)?;
                        if n == 0 {
                            break;
                        }
                        s.write_all(&buf[..n])?;
                        touch();
                    }
                    Ok(())
                }
                None => respond(&mut s, 404, "text/plain; charset=utf-8", b"Tidak ada"),
            }
        }
        ("POST" | "PUT", "/api/upload") => upload(&mut s, &req, rest),
        _ => respond(&mut s, 404, "text/plain; charset=utf-8", b"Tidak ada"),
    }
}

fn upload(s: &mut TcpStream, req: &phone::Request, mut leftover: Vec<u8>) -> std::io::Result<()> {
    let Some(total) = req.header("content-length").and_then(|v| v.parse::<u64>().ok()) else {
        return respond(s, 411, "text/plain", b"Content-Length diperlukan");
    };
    if total > MAX_FILE_BYTES {
        return respond(s, 413, "text/plain", b"Maks 2 GB");
    }
    let raw_name = req.param("name").or_else(|| req.header("x-filename")).unwrap_or("berkas");
    let dir = inbox();
    std::fs::create_dir_all(&dir)?;
    let name = phone::unique_name(&phone::safe_file_name(raw_name), |n| dir.join(n).exists() || dir.join(format!("{n}.part")).exists());
    let part = dir.join(format!("{name}.part"));
    let final_path = dir.join(&name);
    if req.header("expect").is_some_and(|e| e.eq_ignore_ascii_case("100-continue")) {
        s.write_all(b"HTTP/1.1 100 Continue\r\n\r\n")?;
    }
    let id = {
        let mut st = state();
        st.next_id += 1;
        let id = st.next_id;
        st.active.push(Transfer { id, name: name.clone(), total, done: 0 });
        id
    };
    let result = (|| -> std::io::Result<()> {
        let mut out = std::fs::File::create(&part)?;
        let mut done = 0u64;
        let mut buf = vec![0u8; 256 * 1024];
        let write = |out: &mut std::fs::File, data: &[u8], done: &mut u64| -> std::io::Result<()> {
            out.write_all(data)?;
            *done += data.len() as u64;
            if let Some(t) = state().active.iter_mut().find(|t| t.id == id) {
                t.done = *done;
            }
            Ok(())
        };
        leftover.truncate(total as usize);
        write(&mut out, &leftover, &mut done)?;
        while done < total {
            let want = ((total - done) as usize).min(buf.len());
            let n = s.read(&mut buf[..want])?;
            if n == 0 {
                return Err(std::io::ErrorKind::UnexpectedEof.into());
            }
            write(&mut out, &buf[..n], &mut done)?;
            touch();
        }
        out.flush()
    })();
    let mut st = state();
    st.active.retain(|t| t.id != id);
    match result {
        Ok(()) => {
            std::fs::rename(&part, &final_path)?;
            st.finished.push(final_path.to_string_lossy().into_owned());
            st.last_ms = epoch_ms();
            drop(st);
            respond(s, 200, "application/json", format!("{{\"name\":\"{}\"}}", phone::json_escape(&name)).as_bytes())
        }
        Err(e) => {
            drop(st);
            let _ = std::fs::remove_file(&part);
            Err(e)
        }
    }
}
