// SPDX-License-Identifier: GPL-3.0-or-later
//! Page-to-page transition: both pages drift a little in the direction of travel while the
//! old one fades out and the new one fades in.

pub const DURATION_MS: i64 = 320;
/// How far (DIPs) a page moves while it fades.
pub const TRAVEL: f32 = 24.0;

/// Where one page is drawn: horizontal shift and opacity.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pose {
    pub dx: f32,
    pub alpha: f32,
}

fn ease_out(t: f32) -> f32 {
    1.0 - (1.0 - t.clamp(0.0, 1.0)).powi(3)
}

/// Eased progress 0..=1 after `elapsed_ms`.
pub fn progress(elapsed_ms: i64) -> f32 {
    ease_out(elapsed_ms as f32 / DURATION_MS as f32)
}

pub fn finished(elapsed_ms: i64) -> bool {
    elapsed_ms >= DURATION_MS
}

/// (page being left, page arriving) when moving in `dir` (+1 towards the next page).
pub fn frame(elapsed_ms: i64, dir: f32) -> (Pose, Pose) {
    let e = progress(elapsed_ms);
    (Pose { dx: -dir * TRAVEL * e, alpha: 1.0 - e }, Pose { dx: dir * TRAVEL * (1.0 - e), alpha: e })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn starts_with_the_old_page_in_place_and_ends_with_the_new_one() {
        let (out, inc) = frame(0, 1.0);
        assert_eq!((out.dx, out.alpha, inc.alpha), (0.0, 1.0, 0.0));
        assert_eq!(inc.dx, TRAVEL);
        let (out, inc) = frame(DURATION_MS, 1.0);
        assert_eq!((out.alpha, inc.dx, inc.alpha), (0.0, 0.0, 1.0));
        assert_eq!(out.dx, -TRAVEL);
    }

    #[test]
    fn pages_drift_the_way_the_arrow_points() {
        let (out, inc) = frame(DURATION_MS / 3, 1.0);
        assert!(out.dx < 0.0 && inc.dx > 0.0);
        let (out, inc) = frame(DURATION_MS / 3, -1.0);
        assert!(out.dx > 0.0 && inc.dx < 0.0);
    }

    #[test]
    fn progress_is_monotonic_and_fast_at_first() {
        let mut last = 0.0;
        for ms in (0..=DURATION_MS).step_by(10) {
            let p = progress(ms);
            assert!(p >= last);
            last = p;
        }
        assert!(progress(DURATION_MS / 4) > 0.5);
    }

    #[test]
    fn finished_only_after_the_duration() {
        assert!(!finished(DURATION_MS - 1));
        assert!(finished(DURATION_MS));
    }
}
