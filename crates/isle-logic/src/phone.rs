// SPDX-License-Identifier: GPL-3.0-or-later
//! Phone link: the platform independent parts of the little web server that lets a phone
//! send files to the Shelf and fetch files from it over the local network.

use std::net::IpAddr;

/// The server stops by itself after this long without any request.
pub const IDLE_MS: i64 = 30 * 60 * 1000;
/// Largest file accepted in one upload.
pub const MAX_FILE_BYTES: u64 = 2 * 1024 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    pub method: String,
    pub path: String,
    pub query: Vec<(String, String)>,
    pub headers: Vec<(String, String)>,
}

impl Request {
    /// A header by name (any case).
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
    }

    /// A query parameter, already percent-decoded.
    pub fn param(&self, name: &str) -> Option<&str> {
        self.query.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str())
    }

    /// The pairing token: `?t=` in the address or an `X-Token` header.
    pub fn token(&self) -> Option<&str> {
        self.param("t").or_else(|| self.header("x-token"))
    }
}

/// Parses the request line and headers (everything before the blank line).
pub fn parse_head(head: &str) -> Option<Request> {
    let mut lines = head.split("\r\n");
    let mut first = lines.next()?.split(' ');
    let method = first.next()?.to_ascii_uppercase();
    let target = first.next()?;
    if !first.next()?.starts_with("HTTP/") || method.is_empty() {
        return None;
    }
    let (path, q) = target.split_once('?').unwrap_or((target, ""));
    let query = q
        .split('&')
        .filter(|p| !p.is_empty())
        .map(|p| {
            let (k, v) = p.split_once('=').unwrap_or((p, ""));
            (percent_decode(k), percent_decode(v))
        })
        .collect();
    let headers = lines.take_while(|l| !l.is_empty()).filter_map(|l| l.split_once(':')).map(|(k, v)| (k.trim().to_string(), v.trim().to_string())).collect();
    Some(Request { method, path: percent_decode(path), query, headers })
}

pub fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let (Some(h), Some(l)) = ((b[i + 1] as char).to_digit(16), (b[i + 2] as char).to_digit(16)) {
                out.push((h * 16 + l) as u8);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

pub fn percent_encode(s: &str) -> String {
    let mut out = String::new();
    for &b in s.as_bytes() {
        if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

const RESERVED: [&str; 22] = ["CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8", "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9"];

/// A file name that is safe to create on Windows from whatever the phone sent: no folders,
/// no forbidden characters, no reserved device names, not too long.
pub fn safe_file_name(raw: &str) -> String {
    let base = raw.rsplit(['/', '\\']).next().unwrap_or(raw);
    let cleaned: String = base.chars().map(|c| if c.is_control() || "<>:\"|?*".contains(c) { '_' } else { c }).collect();
    let cleaned = cleaned.trim_matches(|c: char| c == ' ' || c == '.').to_string();
    let (stem, ext) = match cleaned.rsplit_once('.') {
        Some((s, e)) if !s.is_empty() => (s.to_string(), format!(".{e}")),
        _ => (cleaned.clone(), String::new()),
    };
    let ext: String = ext.chars().take(12).collect();
    let mut stem: String = stem.chars().take(120).collect();
    if stem.is_empty() || RESERVED.contains(&stem.to_ascii_uppercase().as_str()) {
        stem = format!("berkas{}", if stem.is_empty() { String::new() } else { format!("_{stem}") });
    }
    format!("{stem}{ext}")
}

/// `name`, or `name (1)`, `name (2)` ... until `exists` says the name is free.
pub fn unique_name(name: &str, exists: impl Fn(&str) -> bool) -> String {
    if !exists(name) {
        return name.to_string();
    }
    let (stem, ext) = match name.rsplit_once('.') {
        Some((s, e)) if !s.is_empty() => (s, format!(".{e}")),
        _ => (name, String::new()),
    };
    (1..).map(|n| format!("{stem} ({n}){ext}")).find(|c| !exists(c)).unwrap_or_default()
}

/// Only machines on the same local network may connect.
pub fn allowed_peer(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v) => v.is_loopback() || v.is_private() || v.is_link_local(),
        IpAddr::V6(v) => v.is_loopback() || (v.segments()[0] & 0xfe00) == 0xfc00 || (v.segments()[0] & 0xffc0) == 0xfe80,
    }
}

/// A pairing token from random bytes (16 hex digits from the first 8 bytes).
pub fn token_from(bytes: &[u8]) -> String {
    bytes.iter().take(8).map(|b| format!("{b:02x}")).collect()
}

/// Constant-time comparison, so the token cannot be guessed by timing.
pub fn token_matches(given: Option<&str>, token: &str) -> bool {
    match given {
        Some(g) if g.len() == token.len() && !token.is_empty() => g.bytes().zip(token.bytes()).fold(0u8, |a, (x, y)| a | (x ^ y)) == 0,
        _ => false,
    }
}

/// Whole minutes until the idle stop, rounded up; 0 once it is due.
pub fn minutes_left(last_ms: i64, now_ms: i64) -> i64 {
    let left = last_ms + IDLE_MS - now_ms;
    if left <= 0 { 0 } else { (left + 59_999) / 60_000 }
}

pub fn content_type(name: &str) -> &'static str {
    match name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).as_deref() {
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("png") => "image/png",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        Some("pdf") => "application/pdf",
        Some("txt") => "text/plain; charset=utf-8",
        Some("mp4") => "video/mp4",
        Some("mov") => "video/quicktime",
        Some("mp3") => "audio/mpeg",
        Some("zip") => "application/zip",
        _ => "application/octet-stream",
    }
}

pub fn json_escape(s: &str) -> String {
    let mut o = String::new();
    for c in s.chars() {
        match c {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            c if (c as u32) < 0x20 => o.push_str(&format!("\\u{:04x}", c as u32)),
            c => o.push(c),
        }
    }
    o
}

/// The address a phone opens (and the QR code holds).
pub fn page_url(ip: &str, port: u16, token: &str) -> String {
    format!("http://{ip}:{port}/?t={token}")
}

/// A QR code: its side in modules and the dark modules row by row.
pub fn qr(text: &str) -> Option<(usize, Vec<bool>)> {
    let code = qrcodegen::QrCode::encode_text(text, qrcodegen::QrCodeEcc::Low).ok()?;
    let n = code.size() as usize;
    let mut cells = Vec::with_capacity(n * n);
    for y in 0..n as i32 {
        for x in 0..n as i32 {
            cells.push(code.get_module(x, y));
        }
    }
    Some((n, cells))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_request_is_read_with_query_and_headers() {
        let r = parse_head("POST /api/upload?name=Foto%20Liburan.jpg&t=abc HTTP/1.1\r\nHost: x\r\nContent-Length: 12\r\nX-Token: zzz\r\n\r\n").unwrap();
        assert_eq!(r.method, "POST");
        assert_eq!(r.path, "/api/upload");
        assert_eq!(r.param("name"), Some("Foto Liburan.jpg"));
        assert_eq!(r.header("content-length"), Some("12"));
        assert_eq!(r.token(), Some("abc"), "the address wins over the header");
        assert!(parse_head("garbage").is_none());
        assert!(parse_head("GET / FTP/1.0\r\n\r\n").is_none());
    }

    #[test]
    fn percent_coding_round_trips_including_non_ascii() {
        assert_eq!(percent_decode("a%20b%2Fc%E2%9C%93"), "a b/c✓");
        assert_eq!(percent_decode("100%"), "100%");
        assert_eq!(percent_decode("%zz"), "%zz");
        assert_eq!(percent_decode(&percent_encode("Laporan Q3 (final) ✓.pdf")), "Laporan Q3 (final) ✓.pdf");
    }

    #[test]
    fn file_names_cannot_escape_or_break_windows() {
        assert_eq!(safe_file_name("../../Windows/evil.exe"), "evil.exe");
        assert_eq!(safe_file_name("C:\\x\\a.txt"), "a.txt");
        assert_eq!(safe_file_name("a<b>:c?.png"), "a_b__c_.png");
        assert_eq!(safe_file_name("  ..bad.. "), "bad");
        assert_eq!(safe_file_name("CON"), "berkas_CON");
        assert_eq!(safe_file_name("nul.txt"), "berkas_nul.txt");
        assert_eq!(safe_file_name(""), "berkas");
        assert_eq!(safe_file_name("IMG_2041.HEIC"), "IMG_2041.HEIC");
        assert!(safe_file_name(&"x".repeat(500)).len() <= 120);
    }

    #[test]
    fn a_taken_name_gets_a_number() {
        let taken = ["a.jpg", "a (1).jpg"];
        assert_eq!(unique_name("a.jpg", |n| taken.contains(&n)), "a (2).jpg");
        assert_eq!(unique_name("b.jpg", |n| taken.contains(&n)), "b.jpg");
        assert_eq!(unique_name("noext", |n| n == "noext"), "noext (1)");
    }

    #[test]
    fn only_local_network_peers_are_allowed() {
        for ok in ["192.168.1.9", "10.0.0.2", "172.16.5.5", "127.0.0.1", "169.254.1.1", "::1", "fe80::1", "fd00::1"] {
            assert!(allowed_peer(ok.parse().unwrap()), "{ok}");
        }
        for no in ["8.8.8.8", "172.32.0.1", "1.1.1.1", "2001:db8::1"] {
            assert!(!allowed_peer(no.parse().unwrap()), "{no}");
        }
    }

    #[test]
    fn tokens_compare_exactly() {
        let t = token_from(&[0xab, 0xcd, 1, 2, 3, 4, 5, 6, 7]);
        assert_eq!(t, "abcd010203040506");
        assert!(token_matches(Some("abcd010203040506"), &t));
        assert!(!token_matches(Some("abcd01020304050"), &t));
        assert!(!token_matches(Some("abcd010203040507"), &t));
        assert!(!token_matches(None, &t));
        assert!(!token_matches(Some(""), ""));
    }

    #[test]
    fn idle_countdown_rounds_up_and_ends() {
        assert_eq!(minutes_left(0, 0), 30);
        assert_eq!(minutes_left(0, 60_001), 29);
        assert_eq!(minutes_left(0, IDLE_MS - 1), 1);
        assert_eq!(minutes_left(0, IDLE_MS), 0);
        assert_eq!(minutes_left(0, IDLE_MS + 5), 0);
    }

    #[test]
    fn the_qr_for_a_page_address_is_a_small_square() {
        let url = page_url("192.168.100.123", 49152, "abcd010203040506");
        let (n, cells) = qr(&url).unwrap();
        assert!((21..=41).contains(&n), "{n}");
        assert_eq!(cells.len(), n * n);
        assert!(cells[0], "the top-left finder pattern is dark");
    }

    #[test]
    fn json_text_is_escaped() {
        assert_eq!(json_escape("a\"b\\c\nd"), "a\\\"b\\\\c\\u000ad");
        assert_eq!(content_type("X.JPG"), "image/jpeg");
        assert_eq!(content_type("x.bin"), "application/octet-stream");
    }
}
