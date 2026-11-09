// SPDX-License-Identifier: GPL-3.0-or-later
//! Synced lyrics: which line is current, cache keys, and the LRCLIB request/response.

use crate::lrc::{self, LyricLine};

#[derive(Debug, Clone, PartialEq)]
pub enum Lyrics {
    None,
    Synced(Vec<LyricLine>),
    Plain(Vec<String>),
}

/// Index of the line being sung at `position_ms`: the last line that started at or
/// before it; `None` before the first line or when there are no lines.
pub fn current_index(lines: &[LyricLine], position_ms: i64) -> Option<usize> {
    lines.iter().rposition(|l| l.time_ms <= position_ms)
}

/// The previous, current and next line indices around `index` (of `len` lines).
pub fn window(len: usize, index: Option<usize>) -> [Option<usize>; 3] {
    match index {
        Some(i) => [i.checked_sub(1), Some(i), (i + 1 < len).then_some(i + 1)],
        None => [None, None, (len > 0).then_some(0)],
    }
}

/// A file-system-safe, stable key for a track (durations within a second share a key).
pub fn cache_key(title: &str, artist: &str, duration_ms: i64) -> String {
    let clean = |s: &str| -> String {
        s.trim()
            .to_lowercase()
            .chars()
            .map(|c| if "\\/:*?\"<>|".contains(c) || c.is_control() { '_' } else { c })
            .take(60)
            .collect()
    };
    format!("{} - {} - {}", clean(artist), clean(title), (duration_ms + 500) / 1000)
}

/// Percent-encodes everything except RFC 3986 unreserved characters.
pub fn url_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// The LRCLIB `GET /api/get` path and query; the album is left out when empty.
pub fn request_path(title: &str, artist: &str, album: &str, duration_s: u32) -> String {
    let mut p = format!("/api/get?track_name={}&artist_name={}", url_encode(title), url_encode(artist));
    if !album.trim().is_empty() {
        p.push_str(&format!("&album_name={}", url_encode(album)));
    }
    p.push_str(&format!("&duration={duration_s}"));
    p
}

/// Reads an LRCLIB response: synced lyrics if present, else plain, else none.
pub fn parse_lrclib(json: &str) -> Lyrics {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(json) else { return Lyrics::None };
    if let Some(text) = v.get("syncedLyrics").and_then(|t| t.as_str()) {
        let lines = lrc::parse(text);
        if !lines.is_empty() {
            return Lyrics::Synced(lines);
        }
    }
    if let Some(text) = v.get("plainLyrics").and_then(|t| t.as_str()) {
        let lines: Vec<String> = text.lines().map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).collect();
        if !lines.is_empty() {
            return Lyrics::Plain(lines);
        }
    }
    Lyrics::None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lrc::LyricLine;

    fn lines() -> Vec<LyricLine> {
        [(1000, "a"), (5000, "b"), (9000, "c")].iter().map(|&(t, s)| LyricLine { time_ms: t, text: s.to_string() }).collect()
    }

    #[test]
    fn index_before_first_line_is_none() {
        assert_eq!(current_index(&lines(), 999), None);
    }

    #[test]
    fn index_at_exact_timestamp() {
        assert_eq!(current_index(&lines(), 5000), Some(1));
    }

    #[test]
    fn index_after_last_line() {
        assert_eq!(current_index(&lines(), 999_999), Some(2));
    }

    #[test]
    fn index_empty_lines_is_none() {
        assert_eq!(current_index(&[], 5000), None);
    }

    #[test]
    fn window_at_start_has_no_previous() {
        assert_eq!(window(3, Some(0)), [None, Some(0), Some(1)]);
        assert_eq!(window(3, None), [None, None, Some(0)]);
    }

    #[test]
    fn window_at_end_has_no_next() {
        assert_eq!(window(3, Some(2)), [Some(1), Some(2), None]);
    }

    #[test]
    fn cache_key_strips_illegal_chars() {
        let k = cache_key("AC/DC: Back?", "Who*", 230_000);
        assert!(!k.contains(['\\', '/', ':', '*', '?', '"', '<', '>', '|']), "{k}");
    }

    #[test]
    fn cache_key_is_stable_and_case_insensitive() {
        assert_eq!(cache_key("Song", "Artist", 200_000), cache_key("  song ", "ARTIST", 200_400));
        assert_ne!(cache_key("Song", "Artist", 200_000), cache_key("Song", "Other", 200_000));
    }

    #[test]
    fn url_encodes_reserved_and_non_ascii() {
        assert_eq!(url_encode("a b&c"), "a%20b%26c");
        assert_eq!(url_encode("é"), "%C3%A9");
        assert_eq!(url_encode("AZaz09-_.~"), "AZaz09-_.~");
    }

    #[test]
    fn request_path_omits_empty_album() {
        let p = request_path("Midnight City", "M83", "", 243);
        assert_eq!(p, "/api/get?track_name=Midnight%20City&artist_name=M83&duration=243");
        let p = request_path("A", "B", "C D", 10);
        assert!(p.contains("&album_name=C%20D"));
    }

    #[test]
    fn parse_prefers_synced() {
        let json = r#"{"syncedLyrics":"[00:01.00] hi\n[00:05.00] there","plainLyrics":"hi\nthere"}"#;
        match parse_lrclib(json) {
            Lyrics::Synced(l) => {
                assert_eq!(l.len(), 2);
                assert_eq!(l[0].time_ms, 1000);
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn parse_falls_back_to_plain_then_none() {
        match parse_lrclib(r#"{"syncedLyrics":null,"plainLyrics":"one\ntwo"}"#) {
            Lyrics::Plain(l) => assert_eq!(l, vec!["one".to_string(), "two".to_string()]),
            other => panic!("{other:?}"),
        }
        assert!(matches!(parse_lrclib(r#"{"syncedLyrics":null,"plainLyrics":null}"#), Lyrics::None));
        assert!(matches!(parse_lrclib("{}"), Lyrics::None));
        assert!(matches!(parse_lrclib("garbage"), Lyrics::None));
    }
}
