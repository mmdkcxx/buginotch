// SPDX-License-Identifier: GPL-3.0-or-later
//! Page-to-page slide with a faked motion blur: each page is drawn several times along its
//! path, every copy faint enough that together they add up to the page's own opacity.

pub const DURATION_MS: i64 = 420;
/// How far (DIPs) a page travels while it fades in or out.
pub const TRAVEL: f32 = 90.0;
/// Copies are at most this far apart (DIPs), up to `MAX_SAMPLES`, so the blur looks smooth.
const SPACING: f32 = 4.0;
const MAX_SAMPLES: usize = 14;
/// Blur length as a share of the distance covered per unit of normalised time.
const BLUR_GAIN: f32 = 0.2;

/// One drawing of a page: horizontal shift and layer opacity.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sample {
    pub dx: f32,
    pub alpha: f32,
}

fn in_out_cubic(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    if t < 0.5 { 4.0 * t * t * t } else { 1.0 - (-2.0 * t + 2.0).powi(3) / 2.0 }
}

/// Eased progress 0..=1 after `elapsed_ms`.
pub fn progress(elapsed_ms: i64) -> f32 {
    in_out_cubic(elapsed_ms as f32 / DURATION_MS as f32)
}

/// Speed of `progress` per unit of normalised time (0 at both ends, 3 at the middle).
fn speed(elapsed_ms: i64) -> f32 {
    let t = elapsed_ms as f32 / DURATION_MS as f32;
    if !(0.0..=1.0).contains(&t) {
        return 0.0;
    }
    if t < 0.5 { 12.0 * t * t } else { 3.0 * (-2.0 * t + 2.0).powi(2) }
}

pub fn finished(elapsed_ms: i64) -> bool {
    elapsed_ms >= DURATION_MS
}

/// The copies to draw for a page centred `dx` away from home with opacity `alpha`.
pub fn samples(elapsed_ms: i64, dx: f32, alpha: f32) -> Vec<Sample> {
    let alpha = alpha.clamp(0.0, 1.0);
    if alpha <= 0.0 {
        return Vec::new();
    }
    let length = speed(elapsed_ms) * TRAVEL * BLUR_GAIN;
    if length < 1.0 {
        return vec![Sample { dx, alpha }];
    }
    let n = ((length / SPACING).ceil() as usize + 1).clamp(2, MAX_SAMPLES);
    let each = 1.0 - (1.0 - alpha).powf(1.0 / n as f32);
    (0..n)
        .map(|i| {
            let k = i as f32 / (n - 1) as f32 - 0.5;
            Sample { dx: dx + k * length, alpha: each }
        })
        .collect()
}

/// (outgoing, incoming) copies when moving in `dir` (+1 = towards the next page).
pub fn frame(elapsed_ms: i64, dir: f32) -> (Vec<Sample>, Vec<Sample>) {
    let e = progress(elapsed_ms);
    (samples(elapsed_ms, -dir * e * TRAVEL, 1.0 - e), samples(elapsed_ms, dir * (1.0 - e) * TRAVEL, e))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn progress_runs_from_zero_to_one_monotonically() {
        assert_eq!(progress(0), 0.0);
        assert_eq!(progress(DURATION_MS), 1.0);
        let mut last = 0.0;
        for ms in (0..=DURATION_MS).step_by(10) {
            let p = progress(ms);
            assert!(p >= last);
            last = p;
        }
    }

    #[test]
    fn the_copies_add_up_to_the_page_opacity() {
        for ms in [0, 80, 210, 300] {
            let s = samples(ms, 0.0, 0.8);
            let covered = 1.0 - s.iter().fold(1.0, |acc, x| acc * (1.0 - x.alpha));
            assert!((covered - 0.8).abs() < 1e-4, "{ms}: {covered}");
        }
    }

    #[test]
    fn the_blur_is_widest_mid_slide_and_gone_at_the_ends() {
        let spread = |ms| {
            let s = samples(ms, 0.0, 1.0);
            s.iter().map(|x| x.dx).fold(f32::MIN, f32::max) - s.iter().map(|x| x.dx).fold(f32::MAX, f32::min)
        };
        assert_eq!(spread(0), 0.0);
        assert_eq!(spread(DURATION_MS), 0.0);
        assert!(spread(DURATION_MS / 2) > 40.0);
        assert!(spread(DURATION_MS / 2) > spread(DURATION_MS / 8));
    }

    #[test]
    fn pages_move_opposite_ways_and_are_centred_on_their_own_path() {
        let (out, inc) = frame(DURATION_MS / 4, 1.0);
        let mid = |s: &[Sample]| s.iter().map(|x| x.dx).sum::<f32>() / s.len() as f32;
        assert!(mid(&out) < 0.0 && mid(&inc) > 0.0);
        let (out, inc) = frame(DURATION_MS / 4, -1.0);
        assert!(mid(&out) > 0.0 && mid(&inc) < 0.0);
    }

    #[test]
    fn a_fully_faded_page_draws_nothing() {
        assert!(samples(0, 0.0, 0.0).is_empty());
        assert!(frame(DURATION_MS, 1.0).0.is_empty());
    }
}
