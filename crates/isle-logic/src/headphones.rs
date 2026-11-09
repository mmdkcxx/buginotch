// SPDX-License-Identifier: GPL-3.0-or-later
//! Headphones peek: which audio outputs are personal ones (headphones, headsets), and
//! turning the list of present outputs into "connected" / "disconnected" events.

use std::collections::HashMap;

/// `PKEY_AudioEndpoint_FormFactor` values for headphones and headsets.
const FORM_HEADPHONES: u32 = 3;
const FORM_HEADSET: u32 = 5;
/// Longest device name shown in the pill, in characters.
const NAME_MAX: usize = 14;

pub fn is_personal_audio(form_factor: Option<u32>) -> bool {
    matches!(form_factor, Some(FORM_HEADPHONES | FORM_HEADSET))
}

/// "Headphones (WH-1000XM4 Stereo)" -> "WH-1000XM4": the name inside the parentheses,
/// without Bluetooth profile suffixes.
pub fn clean_name(raw: &str) -> String {
    let raw = raw.trim();
    let inner = match (raw.find('('), raw.ends_with(')')) {
        (Some(open), true) => &raw[open + 1..raw.len() - 1],
        _ => raw,
    };
    let mut name = inner.trim();
    for suffix in [" Hands-Free AG Audio", " Hands-Free", " Stereo"] {
        if let Some(stripped) = name.strip_suffix(suffix) {
            name = stripped.trim_end();
        }
    }
    // "Aziz's AirPods" -> "AirPods": the owner's name only takes up room.
    for sep in ["’s ", "'s "] {
        if let Some(i) = name.find(sep) {
            name = name[i + sep.len()..].trim_start();
        }
    }
    name.to_string()
}

/// Whether the device is an AirPods / Beats product, which broadcast their battery levels.
pub fn is_apple_audio(name: &str) -> bool {
    let n = name.to_lowercase();
    n.contains("airpods") || n.contains("beats")
}

/// How long the battery icon takes to fill and the number to count up.
pub const BATTERY_ANIM_MS: i64 = 1200;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Good,
    Warn,
    Low,
}

/// Colour band of a battery percentage.
pub fn level_of(percent: u8) -> Level {
    match percent {
        40..=u8::MAX => Level::Good,
        20..=39 => Level::Warn,
        _ => Level::Low,
    }
}

/// (number to show, fill fraction 0..=1) `elapsed_ms` after the battery appeared.
pub fn battery_anim(percent: u8, elapsed_ms: i64) -> (u8, f32) {
    let t = (elapsed_ms as f32 / BATTERY_ANIM_MS as f32).clamp(0.0, 1.0);
    let k = 1.0 - (1.0 - t).powi(3);
    ((percent as f32 * k).round() as u8, percent as f32 / 100.0 * k)
}

/// `name` cut to fit the pill, with an ellipsis when shortened.
pub fn short_name(name: &str) -> String {
    if name.chars().count() <= NAME_MAX {
        return name.to_string();
    }
    let cut: String = name.chars().take(NAME_MAX - 1).collect();
    format!("{}…", cut.trim_end())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Endpoint {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Change {
    Connected(String),
    Disconnected(String),
}

/// Pill text for a change.
pub fn peek_text(change: &Change) -> String {
    match change {
        Change::Connected(name) => format!("🎧 {}", short_name(name)),
        Change::Disconnected(name) => format!("✕ {}", short_name(name)),
    }
}

/// Compares successive lists of present personal outputs. The first list only sets the
/// baseline, so devices already connected at start-up never announce themselves.
#[derive(Debug, Default)]
pub struct Tracker {
    known: HashMap<String, String>,
    primed: bool,
}

impl Tracker {
    pub fn update(&mut self, current: &[Endpoint]) -> Vec<Change> {
        let now: HashMap<String, String> = current.iter().map(|e| (e.id.clone(), clean_name(&e.name))).collect();
        let mut changes = Vec::new();
        if self.primed {
            let mut seen = Vec::new();
            for name in now.values() {
                let already = self.known.values().any(|n| n == name);
                if !already && !seen.contains(name) {
                    seen.push(name.clone());
                    changes.push(Change::Connected(name.clone()));
                }
            }
            let mut gone = Vec::new();
            for name in self.known.values() {
                if !now.values().any(|n| n == name) && !gone.contains(name) {
                    gone.push(name.clone());
                    changes.push(Change::Disconnected(name.clone()));
                }
            }
        }
        self.known = now;
        self.primed = true;
        changes
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ep(id: &str, name: &str) -> Endpoint {
        Endpoint { id: id.into(), name: name.into() }
    }

    #[test]
    fn only_headphones_and_headsets_are_personal() {
        assert!(is_personal_audio(Some(3)) && is_personal_audio(Some(5)));
        assert!(!is_personal_audio(Some(1)) && !is_personal_audio(Some(9)) && !is_personal_audio(None));
    }

    #[test]
    fn clean_name_takes_the_device_out_of_the_parentheses() {
        assert_eq!(clean_name("Headphones (WH-1000XM4 Stereo)"), "WH-1000XM4");
        assert_eq!(clean_name("Headset (AirPods Pro Hands-Free AG Audio)"), "AirPods Pro");
        assert_eq!(clean_name("Speakers"), "Speakers");
        assert_eq!(clean_name("  Headphones (Realtek(R) Audio)  "), "Realtek(R) Audio");
    }

    #[test]
    fn the_owner_prefix_is_dropped() {
        assert_eq!(clean_name("Headphones (Aziz’s AirPods)"), "AirPods");
        assert_eq!(clean_name("Headphones (Aziz's AirPods Pro Stereo)"), "AirPods Pro");
        assert_eq!(clean_name("Headphones (Sony WH-1000XM4)"), "Sony WH-1000XM4");
    }

    #[test]
    fn apple_audio_is_recognised_by_name() {
        assert!(is_apple_audio("AirPods Pro") && is_apple_audio("Beats Studio Buds") && is_apple_audio("airpods"));
        assert!(!is_apple_audio("Sony WH-1000XM4") && !is_apple_audio("soundcore R50i NC"));
    }

    #[test]
    fn battery_levels_are_banded() {
        assert_eq!((level_of(100), level_of(40), level_of(39), level_of(20), level_of(19), level_of(0)), (Level::Good, Level::Good, Level::Warn, Level::Warn, Level::Low, Level::Low));
    }

    #[test]
    fn the_battery_counts_up_to_the_real_value() {
        assert_eq!(battery_anim(82, 0), (0, 0.0));
        assert_eq!(battery_anim(82, BATTERY_ANIM_MS), (82, 0.82));
        assert_eq!(battery_anim(82, BATTERY_ANIM_MS * 3), (82, 0.82));
        let (mid, fill) = battery_anim(82, BATTERY_ANIM_MS / 3);
        assert!(mid > 20 && mid < 82 && fill > 0.2 && fill < 0.82);
        assert_eq!(battery_anim(82, -50), (0, 0.0));
    }

    #[test]
    fn short_name_fits_the_pill() {
        assert_eq!(short_name("AirPods Pro"), "AirPods Pro");
        assert_eq!(short_name("Sony WH-1000XM4 Wireless"), "Sony WH-1000X…");
        assert_eq!(short_name("Sony WH-1000XM4 Wireless").chars().count(), 14);
    }

    #[test]
    fn peek_text_names_the_device() {
        assert_eq!(peek_text(&Change::Connected("AirPods Pro".into())), "🎧 AirPods Pro");
        assert_eq!(peek_text(&Change::Disconnected("AirPods Pro".into())), "✕ AirPods Pro");
    }

    #[test]
    fn the_first_list_is_a_baseline_and_announces_nothing() {
        let mut t = Tracker::default();
        assert!(t.update(&[ep("1", "Headphones (AirPods Pro Stereo)")]).is_empty());
        assert!(t.update(&[ep("1", "Headphones (AirPods Pro Stereo)")]).is_empty());
    }

    #[test]
    fn a_new_device_connects_and_a_missing_one_disconnects() {
        let mut t = Tracker::default();
        t.update(&[]);
        assert_eq!(t.update(&[ep("1", "Headphones (AirPods Pro Stereo)")]), vec![Change::Connected("AirPods Pro".into())]);
        assert_eq!(t.update(&[]), vec![Change::Disconnected("AirPods Pro".into())]);
    }

    #[test]
    fn two_profiles_of_one_device_count_once() {
        let mut t = Tracker::default();
        t.update(&[]);
        let both = [ep("1", "Headphones (AirPods Pro Stereo)"), ep("2", "Headset (AirPods Pro Hands-Free AG Audio)")];
        assert_eq!(t.update(&both), vec![Change::Connected("AirPods Pro".into())]);
        assert!(t.update(&[both[0].clone()]).is_empty());
        assert_eq!(t.update(&[]), vec![Change::Disconnected("AirPods Pro".into())]);
    }
}
