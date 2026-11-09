// SPDX-License-Identifier: GPL-3.0-or-later
//! Notch geometry (in DIPs) and the hover dwell state machine.

pub const OPEN_DWELL_MS: i64 = 150;
pub const CLOSE_DELAY_MS: i64 = 220;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Size {
    pub w: f32,
    pub h: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NotchTarget {
    pub size: Size,
    pub radius: f32,
}

/// Where the notch is heading, in DIPs. `wide_pill` is for music/charging in the closed
/// pill; `lyrics` only widens the open panel.
pub fn target(open: bool, wide_pill: bool, lyrics: bool) -> NotchTarget {
    if open {
        let w = if lyrics { 660.0 } else { 540.0 };
        NotchTarget { size: Size { w, h: 184.0 }, radius: 30.0 }
    } else {
        let w = if wide_pill { 140.0 } else { 96.0 };
        NotchTarget { size: Size { w, h: 26.0 }, radius: 13.0 }
    }
}

/// Left edge (px) that centres a notch on a monitor.
pub fn notch_x(monitor_width_px: i32, notch_width_px: i32) -> i32 {
    (monitor_width_px - notch_width_px) / 2
}

/// Opens after the pointer rests inside for 150 ms; closes 220 ms after it leaves.
#[derive(Debug, Clone, Copy)]
pub struct Hover {
    inside: bool,
    since_ms: i64,
    open: bool,
}

impl Hover {
    pub fn new() -> Self {
        Self { inside: false, since_ms: 0, open: false }
    }

    pub fn pointer_inside(&mut self, inside: bool, now_ms: i64) {
        if inside != self.inside {
            self.inside = inside;
            self.since_ms = now_ms;
        }
    }

    /// Returns whether the notch should be open at `now_ms`.
    pub fn update(&mut self, now_ms: i64) -> bool {
        let held = now_ms - self.since_ms;
        if !self.open && self.inside && held >= OPEN_DWELL_MS {
            self.open = true;
        } else if self.open && !self.inside && held >= CLOSE_DELAY_MS {
            self.open = false;
        }
        self.open
    }
}

impl Default for Hover {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn target_sizes_match_spec() {
        let t = target(false, false, false);
        assert_eq!((t.size.w, t.size.h, t.radius), (96.0, 26.0, 13.0));
        let t = target(false, true, false);
        assert_eq!((t.size.w, t.size.h, t.radius), (140.0, 26.0, 13.0));
        let t = target(true, false, false);
        assert_eq!((t.size.w, t.size.h, t.radius), (540.0, 184.0, 30.0));
        let t = target(true, true, true);
        assert_eq!((t.size.w, t.size.h, t.radius), (660.0, 184.0, 30.0));
    }

    #[test]
    fn lyrics_only_widen_when_open() {
        assert_eq!(target(false, false, true).size.w, 96.0);
    }

    #[test]
    fn notch_is_centred() {
        assert_eq!(notch_x(1920, 540), 690);
    }

    #[test]
    fn hover_opens_after_dwell() {
        let mut h = Hover::new();
        h.pointer_inside(true, 0);
        assert!(!h.update(100));
        assert!(h.update(150));
    }

    #[test]
    fn passing_over_does_not_open() {
        let mut h = Hover::new();
        h.pointer_inside(true, 0);
        h.pointer_inside(false, 100);
        assert!(!h.update(300));
    }

    #[test]
    fn closes_after_delay() {
        let mut h = Hover::new();
        h.pointer_inside(true, 0);
        assert!(h.update(150));
        h.pointer_inside(false, 1000);
        assert!(h.update(1100));
        assert!(!h.update(1220));
    }

    #[test]
    fn reentering_cancels_close() {
        let mut h = Hover::new();
        h.pointer_inside(true, 0);
        assert!(h.update(150));
        h.pointer_inside(false, 1000);
        h.pointer_inside(true, 1100);
        assert!(h.update(1500));
    }
}
