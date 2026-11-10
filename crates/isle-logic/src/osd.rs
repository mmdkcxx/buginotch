// SPDX-License-Identifier: GPL-3.0-or-later
//! Volume and brightness OSD shown inside the closed pill: what changed, how far, and for
//! how long. Pure state; the Windows side only supplies readings.

/// How long the OSD stays after the last change.
pub const HOLD_MS: i64 = 1600;
/// The bar glides to a new value over this long.
pub const EASE_MS: i64 = 140;
/// Fade in when it appears and fade out before it goes.
pub const FADE_IN_MS: i64 = 120;
pub const FADE_OUT_MS: i64 = 160;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OsdKind {
    Volume,
    Brightness,
}

/// What the system reports right now. `None` means that control does not exist (a desktop
/// without a built-in panel has no brightness).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Reading {
    /// 0..=100 and whether the output is muted.
    pub volume: Option<(u8, bool)>,
    /// 0..=100.
    pub brightness: Option<u8>,
}

/// What the pill draws.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PillOsd {
    pub kind: OsdKind,
    /// Bar fill, 0..=1, already eased.
    pub level: f32,
    pub muted: bool,
    /// 0..=1 for fading the content.
    pub alpha: f32,
}

#[derive(Debug, Default)]
pub struct Osd {
    last: Reading,
    seen: bool,
    kind: Option<OsdKind>,
    muted: bool,
    from: f32,
    to: f32,
    changed_ms: i64,
    appeared_ms: i64,
    until_ms: i64,
}

fn ease_out(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    1.0 - (1.0 - t) * (1.0 - t)
}

impl Osd {
    /// Feeds a new reading. The first one only sets the baseline.
    pub fn observe(&mut self, r: Reading, now_ms: i64) {
        if !self.seen {
            self.seen = true;
            self.last = r;
            return;
        }
        let shown_now = self.level_at(now_ms);
        let vol_changed = r.volume.is_some() && r.volume != self.last.volume;
        let bri_changed = r.brightness.is_some() && r.brightness != self.last.brightness;
        // If both moved in one step (rare), volume wins: it is the one with keys everywhere.
        let kind = if vol_changed {
            Some(OsdKind::Volume)
        } else if bri_changed {
            Some(OsdKind::Brightness)
        } else {
            None
        };
        if let Some(kind) = kind {
            let (to, muted) = match kind {
                OsdKind::Volume => {
                    let (v, m) = r.volume.unwrap_or((0, false));
                    (v as f32 / 100.0, m)
                }
                OsdKind::Brightness => (r.brightness.unwrap_or(0) as f32 / 100.0, false),
            };
            let was_visible = self.active(now_ms);
            // Switching between volume and brightness starts from the new bar's old value.
            self.from = if was_visible && self.kind == Some(kind) {
                shown_now
            } else {
                match kind {
                    OsdKind::Volume => self.last.volume.map_or(to, |(v, _)| v as f32 / 100.0),
                    OsdKind::Brightness => self.last.brightness.map_or(to, |b| b as f32 / 100.0),
                }
            };
            self.to = to;
            self.muted = muted;
            self.kind = Some(kind);
            self.changed_ms = now_ms;
            if !was_visible {
                self.appeared_ms = now_ms;
            }
            self.until_ms = now_ms + HOLD_MS;
        }
        self.last = r;
    }

    fn active(&self, now_ms: i64) -> bool {
        self.kind.is_some() && now_ms < self.until_ms
    }

    fn level_at(&self, now_ms: i64) -> f32 {
        if self.kind.is_none() {
            return 0.0;
        }
        let t = (now_ms - self.changed_ms) as f32 / EASE_MS as f32;
        self.from + (self.to - self.from) * ease_out(t)
    }

    pub fn view(&self, now_ms: i64) -> Option<PillOsd> {
        if !self.active(now_ms) {
            return None;
        }
        let fade_in = ((now_ms - self.appeared_ms) as f32 / FADE_IN_MS as f32).clamp(0.0, 1.0);
        let fade_out = ((self.until_ms - now_ms) as f32 / FADE_OUT_MS as f32).clamp(0.0, 1.0);
        Some(PillOsd { kind: self.kind?, level: self.level_at(now_ms).clamp(0.0, 1.0), muted: self.muted, alpha: fade_in.min(fade_out) })
    }

    /// When the next redraw is needed (epoch ms), if the OSD is on screen.
    pub fn next_wake_ms(&self, now_ms: i64) -> Option<i64> {
        if !self.active(now_ms) {
            return None;
        }
        let animating = now_ms - self.changed_ms < EASE_MS || now_ms - self.appeared_ms < FADE_IN_MS || self.until_ms - now_ms <= FADE_OUT_MS;
        Some(if animating { now_ms + 16 } else { (self.until_ms - FADE_OUT_MS).min(self.changed_ms + HOLD_MS) })
    }
}

/// Icon wave count for a volume level: 0 (none) to 3.
pub fn waves(level: f32, muted: bool) -> u8 {
    if muted || level <= 0.0 {
        0
    } else if level <= 0.33 {
        1
    } else if level <= 0.66 {
        2
    } else {
        3
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vol(v: u8, m: bool) -> Reading {
        Reading { volume: Some((v, m)), brightness: Some(70) }
    }

    #[test]
    fn first_reading_is_a_baseline() {
        let mut o = Osd::default();
        o.observe(vol(50, false), 0);
        assert!(o.view(1).is_none());
    }

    #[test]
    fn volume_change_shows_and_expires() {
        let mut o = Osd::default();
        o.observe(vol(50, false), 0);
        o.observe(vol(60, false), 1000);
        let v = o.view(1000 + EASE_MS + 10).unwrap();
        assert_eq!(v.kind, OsdKind::Volume);
        assert!((v.level - 0.6).abs() < 1e-4);
        assert!(o.view(1000 + HOLD_MS).is_none());
    }

    #[test]
    fn bar_glides_from_the_old_value() {
        let mut o = Osd::default();
        o.observe(vol(50, false), 0);
        o.observe(vol(80, false), 1000);
        let start = o.view(1000).unwrap().level;
        assert!((start - 0.5).abs() < 1e-4);
        let mid = o.view(1000 + EASE_MS / 2).unwrap().level;
        assert!(mid > 0.5 && mid < 0.8);
    }

    #[test]
    fn brightness_change_uses_its_own_kind() {
        let mut o = Osd::default();
        o.observe(vol(50, false), 0);
        o.observe(Reading { volume: Some((50, false)), brightness: Some(40) }, 500);
        let v = o.view(900).unwrap();
        assert_eq!(v.kind, OsdKind::Brightness);
        assert!((v.level - 0.4).abs() < 1e-4);
    }

    #[test]
    fn mute_toggle_alone_counts_as_a_change() {
        let mut o = Osd::default();
        o.observe(vol(50, false), 0);
        o.observe(vol(50, true), 500);
        assert!(o.view(900).unwrap().muted);
    }

    #[test]
    fn identical_reading_does_not_extend_the_hold() {
        let mut o = Osd::default();
        o.observe(vol(50, false), 0);
        o.observe(vol(55, false), 1000);
        o.observe(vol(55, false), 2000);
        assert!(o.view(1000 + HOLD_MS).is_none());
    }

    #[test]
    fn missing_control_never_fires() {
        let mut o = Osd::default();
        o.observe(Reading { volume: None, brightness: None }, 0);
        o.observe(Reading { volume: None, brightness: None }, 100);
        assert!(o.view(150).is_none());
    }

    #[test]
    fn fades_in_and_out() {
        let mut o = Osd::default();
        o.observe(vol(50, false), 0);
        o.observe(vol(60, false), 1000);
        assert_eq!(o.view(1000).unwrap().alpha, 0.0);
        assert_eq!(o.view(1000 + 500).unwrap().alpha, 1.0);
        assert!(o.view(1000 + HOLD_MS - 10).unwrap().alpha < 0.1);
    }

    #[test]
    fn wake_stops_when_gone() {
        let mut o = Osd::default();
        o.observe(vol(50, false), 0);
        assert_eq!(o.next_wake_ms(10), None);
        o.observe(vol(60, false), 1000);
        assert_eq!(o.next_wake_ms(1010), Some(1026));
        assert_eq!(o.next_wake_ms(1000 + HOLD_MS), None);
    }

    #[test]
    fn wave_count_follows_level() {
        assert_eq!(waves(0.0, false), 0);
        assert_eq!(waves(0.2, false), 1);
        assert_eq!(waves(0.5, false), 2);
        assert_eq!(waves(0.9, false), 3);
        assert_eq!(waves(0.9, true), 0);
    }
}
