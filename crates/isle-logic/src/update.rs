// SPDX-License-Identifier: GPL-3.0-or-later
//! Self-update logic that needs no Windows: reading a GitHub release, comparing versions,
//! checking the installer's SHA-256, and splitting URLs.

use serde_json::Value;

/// A release worth offering: newer than the running version is decided by `is_newer`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Release {
    /// "0.3.0" (no leading "v").
    pub version: String,
    /// Bullet lines from the release notes, for the Pembaruan page.
    pub notes: Vec<String>,
    pub asset_name: String,
    /// The GitHub API URL of the installer asset (works for private repositories with a token).
    pub asset_url: String,
    pub size: u64,
    /// Expected SHA-256 of the installer, lowercase hex, when the release says it directly.
    pub sha256: Option<String>,
    /// API URL of a `<installer>.sha256` asset, when the release carries one instead.
    pub sha_asset_url: Option<String>,
}

/// "v0.3.1" or "0.3.1" -> [0, 3, 1]. Anything that is not three plain numbers is `None`
/// (so a tag like "v0.3.0-beta" is never offered).
pub fn parse_version(text: &str) -> Option<[u32; 3]> {
    let mut parts = text.trim().trim_start_matches(['v', 'V']).split('.');
    let mut out = [0u32; 3];
    for slot in &mut out {
        *slot = parts.next()?.parse().ok()?;
    }
    parts.next().is_none().then_some(out)
}

/// True when `latest` is a higher version than `current`. Unreadable versions are never newer.
pub fn is_newer(current: &str, latest: &str) -> bool {
    match (parse_version(current), parse_version(latest)) {
        (Some(c), Some(l)) => l > c,
        _ => false,
    }
}

/// Reads the body of `GET /repos/{owner}/{repo}/releases/latest`.
/// Drafts, pre-releases and releases without an installer give `None`.
pub fn parse_release(json: &str) -> Option<Release> {
    let v: Value = serde_json::from_str(json).ok()?;
    if v["draft"].as_bool().unwrap_or(false) || v["prerelease"].as_bool().unwrap_or(false) {
        return None;
    }
    let tag = v["tag_name"].as_str()?;
    parse_version(tag)?;
    let version = tag.trim().trim_start_matches(['v', 'V']).to_string();
    let assets = v["assets"].as_array()?;
    let installer = assets.iter().find(|a| {
        let n = a["name"].as_str().unwrap_or("");
        n.starts_with("IsleWin-Setup-") && n.ends_with(".exe")
    })?;
    let asset_name = installer["name"].as_str()?.to_string();
    let sha256 = installer["digest"].as_str().and_then(|d| d.strip_prefix("sha256:")).filter(|h| is_hex64(h)).map(str::to_ascii_lowercase);
    let sha_asset_url = assets.iter().find(|a| a["name"].as_str() == Some(&format!("{asset_name}.sha256"))).and_then(|a| a["url"].as_str()).map(str::to_string);
    Some(Release {
        version,
        notes: notes_lines(v["body"].as_str().unwrap_or("")),
        asset_url: installer["url"].as_str()?.to_string(),
        asset_name,
        size: installer["size"].as_u64().unwrap_or(0),
        sha256,
        sha_asset_url,
    })
}

fn is_hex64(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit())
}

/// The hash in a checksum file: `<hash>  <name>` lines (sha256sum style) or a bare hash.
pub fn parse_sha256_text(text: &str, file_name: &str) -> Option<String> {
    let mut bare = None;
    for line in text.lines() {
        let mut it = line.split_whitespace();
        let Some(hash) = it.next().filter(|h| is_hex64(h)) else { continue };
        match it.next().map(|n| n.trim_start_matches('*')) {
            Some(name) if name == file_name => return Some(hash.to_ascii_lowercase()),
            None => bare = Some(hash.to_ascii_lowercase()),
            Some(_) => {}
        }
    }
    bare
}

/// Bullet points from release notes ("- fixed x", "* added y"), cut at GitHub's " by @user"
/// tail. Up to 12 lines.
pub fn notes_lines(body: &str) -> Vec<String> {
    body.lines()
        .map(str::trim)
        .filter_map(|l| l.strip_prefix("- ").or_else(|| l.strip_prefix("* ")).or_else(|| l.strip_prefix("• ")))
        .map(|l| l.split(" by @").next().unwrap_or(l).trim())
        .filter(|l| !l.is_empty() && !l.contains("Full Changelog"))
        .map(|l| format!("• {l}"))
        .take(12)
        .collect()
}

/// "https://host/path?q" -> ("host", "/path?q"). Only https is accepted.
pub fn split_url(url: &str) -> Option<(String, String)> {
    let rest = url.strip_prefix("https://")?;
    let (host, path) = rest.split_once('/').map_or((rest, String::from("/")), |(h, p)| (h, format!("/{p}")));
    (!host.is_empty() && !host.contains('@')).then(|| (host.to_string(), path))
}

/// "12 MB", "7,4 MB", "820 KB" (comma as the decimal mark).
pub fn format_bytes(n: u64) -> String {
    const MB: f64 = 1024.0 * 1024.0;
    let n = n as f64;
    if n >= MB {
        let mb = n / MB;
        if mb >= 10.0 { format!("{:.0} MB", mb) } else { format!("{:.1} MB", mb).replace('.', ",") }
    } else {
        format!("{:.0} KB", (n / 1024.0).max(1.0))
    }
}

/// Streaming SHA-256.
pub struct Sha256 {
    state: [u32; 8],
    buf: [u8; 64],
    buf_len: usize,
    total: u64,
}

const K: [u32; 64] = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786,
    0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13,
    0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a,
    0x5b9cca4f, 0x682e6ff3, 0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
];

impl Default for Sha256 {
    fn default() -> Self {
        Self { state: [0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19], buf: [0; 64], buf_len: 0, total: 0 }
    }
}

impl Sha256 {
    pub fn update(&mut self, mut data: &[u8]) {
        self.total += data.len() as u64;
        if self.buf_len > 0 {
            let take = (64 - self.buf_len).min(data.len());
            self.buf[self.buf_len..self.buf_len + take].copy_from_slice(&data[..take]);
            self.buf_len += take;
            data = &data[take..];
            if self.buf_len == 64 {
                let block = self.buf;
                self.compress(&block);
                self.buf_len = 0;
            }
        }
        while data.len() >= 64 {
            let (block, rest) = data.split_at(64);
            self.compress(block.try_into().unwrap());
            data = rest;
        }
        if !data.is_empty() {
            self.buf[..data.len()].copy_from_slice(data);
            self.buf_len = data.len();
        }
    }

    /// The digest as lowercase hex.
    pub fn finish_hex(mut self) -> String {
        let bits = self.total * 8;
        let mut pad = vec![0x80u8];
        while (self.buf_len + pad.len()) % 64 != 56 {
            pad.push(0);
        }
        pad.extend_from_slice(&bits.to_be_bytes());
        let total = self.total;
        self.update(&pad);
        self.total = total;
        self.state.iter().map(|w| format!("{w:08x}")).collect()
    }

    fn compress(&mut self, block: &[u8; 64]) {
        let mut w = [0u32; 64];
        for (i, c) in block.chunks_exact(4).enumerate() {
            w[i] = u32::from_be_bytes([c[0], c[1], c[2], c[3]]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16].wrapping_add(s0).wrapping_add(w[i - 7]).wrapping_add(s1);
        }
        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut h] = self.state;
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ (!e & g);
            let t1 = h.wrapping_add(s1).wrapping_add(ch).wrapping_add(K[i]).wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let t2 = s0.wrapping_add(maj);
            h = g;
            g = f;
            f = e;
            e = d.wrapping_add(t1);
            d = c;
            c = b;
            b = a;
            a = t1.wrapping_add(t2);
        }
        for (s, v) in self.state.iter_mut().zip([a, b, c, d, e, f, g, h]) {
            *s = s.wrapping_add(v);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RELEASE: &str = r###"{
        "tag_name": "v0.3.0", "draft": false, "prerelease": false,
        "body": "## Baru\n- Notifikasi bergulir by @mmdkcxx in https://x/1\n* Ikon app di pill\n\n**Full Changelog**: https://x",
        "assets": [
          {"name": "IsleWin-Setup-0.3.0.exe", "url": "https://api.github.com/repos/o/r/releases/assets/11", "size": 659701,
           "digest": "sha256:BA7816BF8F01CFEA414140DE5DAE2223B00361A396177A9CB410FF61F20015AD"},
          {"name": "IsleWin-Setup-0.3.0.exe.sha256", "url": "https://api.github.com/repos/o/r/releases/assets/12", "size": 90}
        ]
    }"###;

    #[test]
    fn versions_compare_numerically_not_as_text() {
        assert_eq!(parse_version("v0.10.2"), Some([0, 10, 2]));
        assert!(is_newer("0.2.9", "0.2.10"));
        assert!(is_newer("0.2.4", "v0.3.0"));
        assert!(!is_newer("0.3.0", "0.3.0"));
        assert!(!is_newer("0.3.0", "0.2.9"));
        assert!(!is_newer("0.3.0", "0.4.0-beta"));
        assert_eq!(parse_version("1.2"), None);
        assert_eq!(parse_version("1.2.3.4"), None);
    }

    #[test]
    fn a_release_is_read_with_its_installer_digest_and_notes() {
        let r = parse_release(RELEASE).unwrap();
        assert_eq!(r.version, "0.3.0");
        assert_eq!(r.asset_name, "IsleWin-Setup-0.3.0.exe");
        assert_eq!(r.size, 659701);
        assert_eq!(r.sha256.as_deref(), Some("ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"));
        assert!(r.sha_asset_url.unwrap().ends_with("/12"));
        assert_eq!(r.notes, vec!["• Notifikasi bergulir", "• Ikon app di pill"]);
    }

    #[test]
    fn drafts_prereleases_and_releases_without_an_installer_are_ignored() {
        assert!(parse_release(&RELEASE.replace("\"draft\": false", "\"draft\": true")).is_none());
        assert!(parse_release(&RELEASE.replace("\"prerelease\": false", "\"prerelease\": true")).is_none());
        assert!(parse_release(&RELEASE.replace("IsleWin-Setup-0.3.0.exe\"", "other.zip\"").replace("IsleWin-Setup-0.3.0.exe.sha256", "other.sha")).is_none());
        assert!(parse_release("not json").is_none());
    }

    #[test]
    fn checksum_files_are_read_by_file_name_or_bare() {
        let h = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
        let other = "0".repeat(64);
        let text = format!("{other}  other.exe\n{h} *IsleWin-Setup-0.3.0.exe\n");
        assert_eq!(parse_sha256_text(&text, "IsleWin-Setup-0.3.0.exe").as_deref(), Some(h));
        assert_eq!(parse_sha256_text(&format!("{h}\n"), "x.exe").as_deref(), Some(h));
        assert_eq!(parse_sha256_text("nothing here", "x.exe"), None);
    }

    #[test]
    fn urls_split_into_host_and_path_and_only_https_is_accepted() {
        assert_eq!(split_url("https://objects.example.com/a/b?x=1"), Some(("objects.example.com".into(), "/a/b?x=1".into())));
        assert_eq!(split_url("https://example.com"), Some(("example.com".into(), "/".into())));
        assert_eq!(split_url("http://example.com/a"), None);
        assert_eq!(split_url("https://user@example.com/a"), None);
    }

    #[test]
    fn sizes_use_a_decimal_comma() {
        assert_eq!(format_bytes(12 * 1024 * 1024), "12 MB");
        assert_eq!(format_bytes(7_759_462), "7,4 MB");
        assert_eq!(format_bytes(2048), "2 KB");
    }

    #[test]
    fn sha256_matches_the_standard_vectors_in_any_chunking() {
        let hex = |data: &[&[u8]]| {
            let mut h = Sha256::default();
            for d in data {
                h.update(d);
            }
            h.finish_hex()
        };
        assert_eq!(hex(&[b"abc"]), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
        assert_eq!(hex(&[b""]), "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
        let long = vec![b'a'; 1_000_000];
        let whole = hex(&[&long]);
        assert_eq!(whole, "cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0");
        assert_eq!(hex(&[&long[..1], &long[1..63], &long[63..64], &long[64..]]), whole);
    }
}
