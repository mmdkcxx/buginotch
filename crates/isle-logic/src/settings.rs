// SPDX-License-Identifier: GPL-3.0-or-later
//! User settings, stored as `%APPDATA%\IsleWin\settings.toml`.

use crate::pomodoro::PomodoroConfig;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub autostart: bool,
    pub monitor: u32,
    pub pill_width: f32,
    pub pill_height: f32,
    pub pomodoro: PomodoroConfig,
    /// Lyrics are shown this many ms ahead of the reported playback position (players report
    /// it late, so lyrics otherwise trail the audio). Raise it if lyrics are late.
    pub lyrics_lead_ms: i64,
}

impl Default for Settings {
    fn default() -> Self {
        Self { autostart: false, monitor: 0, pill_width: 96.0, pill_height: 26.0, pomodoro: PomodoroConfig::default(), lyrics_lead_ms: 500 }
    }
}

impl Settings {
    /// Never fails: unreadable input gives the defaults.
    pub fn from_toml(source: &str) -> Self {
        toml::from_str(source).unwrap_or_default()
    }

    pub fn to_toml(&self) -> String {
        toml::to_string(self).unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lyrics_lead_defaults_to_500_ms_and_can_be_set() {
        assert_eq!(Settings::default().lyrics_lead_ms, 500);
        assert_eq!(Settings::from_toml("lyrics_lead_ms = 250").lyrics_lead_ms, 250);
    }

    #[test]
    fn empty_file_gives_defaults() {
        assert_eq!(Settings::from_toml(""), Settings::default());
    }

    #[test]
    fn partial_file_keeps_other_defaults() {
        let s = Settings::from_toml("pill_width = 80.0");
        assert_eq!(s.pill_width, 80.0);
        assert_eq!(s.pill_height, 26.0);
        assert!(!s.autostart);
    }

    #[test]
    fn garbage_gives_defaults() {
        assert_eq!(Settings::from_toml("this is = = not toml [["), Settings::default());
    }

    #[test]
    fn round_trips() {
        let mut s = Settings::default();
        s.autostart = true;
        s.pomodoro.focus_minutes = 50;
        assert_eq!(Settings::from_toml(&s.to_toml()), s);
    }
}
