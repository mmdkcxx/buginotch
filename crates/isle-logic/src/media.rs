// SPDX-License-Identifier: GPL-3.0-or-later
//! What the system says is playing.

/// A readable app name from an SMTC source id ("X!Spotify" -> "Spotify", "chrome.exe" -> "Chrome").
pub fn friendly_app(id: &str) -> String {
    let name = match id.split_once('!') {
        Some((_, after)) => after,
        None => id.strip_suffix(".exe").unwrap_or(id),
    };
    let mut chars = name.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

/// A decoded cover image, straight-alpha RGBA8.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoverImage {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MediaInfo {
    pub title: String,
    pub artist: String,
    pub album: String,
    pub app: String,
    pub playing: bool,
    pub position_ms: i64,
    pub duration_ms: i64,
    /// Epoch ms at which `position_ms` was read.
    pub updated_at_ms: i64,
    pub cover: Option<CoverImage>,
}

impl MediaInfo {
    /// Playback position at `now_ms`: interpolated while playing, clamped to the
    /// duration (when known) and never before the reported position.
    pub fn position_at(&self, now_ms: i64) -> i64 {
        if !self.playing {
            return self.position_ms;
        }
        let pos = self.position_ms + (now_ms - self.updated_at_ms).max(0);
        if self.duration_ms > 0 { pos.min(self.duration_ms) } else { pos }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info(playing: bool) -> MediaInfo {
        MediaInfo {
            title: "t".into(),
            artist: "a".into(),
            album: String::new(),
            app: "Spotify".into(),
            playing,
            position_ms: 10_000,
            duration_ms: 243_000,
            updated_at_ms: 0,
            cover: None,
        }
    }

    #[test]
    fn friendly_app_names() {
        assert_eq!(friendly_app("SpotifyAB.SpotifyMusic_zpdnekdrzrea0!Spotify"), "Spotify");
        assert_eq!(friendly_app("chrome.exe"), "Chrome");
        assert_eq!(friendly_app("Microsoft.ZuneMusic_8wekyb3d8bbwe!Microsoft.ZuneMusic"), "Microsoft.ZuneMusic");
        assert_eq!(friendly_app(""), "");
    }

    #[test]
    fn position_interpolates_while_playing() {
        assert_eq!(info(true).position_at(2_500), 12_500);
    }

    #[test]
    fn position_frozen_when_paused() {
        assert_eq!(info(false).position_at(2_500), 10_000);
    }

    #[test]
    fn position_clamped_to_duration() {
        assert_eq!(info(true).position_at(10_000_000), 243_000);
    }

    #[test]
    fn position_never_negative_if_clock_goes_back() {
        assert_eq!(info(true).position_at(-5_000), 10_000);
    }

    #[test]
    fn unknown_duration_is_not_clamped_to_zero() {
        let mut i = info(true);
        i.duration_ms = 0;
        assert_eq!(i.position_at(2_500), 12_500);
    }
}
