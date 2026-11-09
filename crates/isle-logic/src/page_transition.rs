// SPDX-License-Identifier: GPL-3.0-or-later
//! Page-to-page transition: the old page fades out while the new one fades in.

pub const DURATION_MS: i64 = 260;

fn ease_in_out(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Eased progress 0..=1 after `elapsed_ms`.
pub fn progress(elapsed_ms: i64) -> f32 {
    ease_in_out(elapsed_ms as f32 / DURATION_MS as f32)
}

pub fn finished(elapsed_ms: i64) -> bool {
    elapsed_ms >= DURATION_MS
}

/// (opacity of the page being left, opacity of the page arriving).
pub fn opacities(elapsed_ms: i64) -> (f32, f32) {
    let e = progress(elapsed_ms);
    (1.0 - e, e)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fades_swap_from_the_old_page_to_the_new_one() {
        assert_eq!(opacities(0), (1.0, 0.0));
        assert_eq!(opacities(DURATION_MS), (0.0, 1.0));
    }

    #[test]
    fn progress_is_monotonic_and_the_two_opacities_sum_to_one() {
        let mut last = 0.0;
        for ms in (0..=DURATION_MS).step_by(10) {
            let (out, inc) = opacities(ms);
            assert!(inc >= last);
            assert!((out + inc - 1.0).abs() < 1e-6);
            last = inc;
        }
    }

    #[test]
    fn finished_only_after_the_duration() {
        assert!(!finished(DURATION_MS - 1));
        assert!(finished(DURATION_MS));
    }
}
