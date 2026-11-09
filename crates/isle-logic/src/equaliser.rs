// SPDX-License-Identifier: GPL-3.0-or-later
//! The little equaliser bars in the pill. Generated motion that does not follow the music:
//! each bar mixes a few sine waves of unrelated speeds, so they look lively and uneven.

pub const BARS: usize = 3;

/// Height of bar `i` at `t` seconds as a fraction 0..=1.
pub fn bar(t: f32, i: usize) -> f32 {
    let k = i as f32;
    let a = (t * (5.3 + 1.7 * k) + k * 2.1).sin();
    let b = (t * (8.9 + 0.9 * k) + k * 4.7).sin();
    let c = (t * (2.3 + 0.6 * k) + k * 1.3).sin();
    (0.5 + 0.28 * a + 0.14 * b + 0.08 * c).clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bars_stay_between_zero_and_one() {
        for step in 0..2000 {
            for i in 0..BARS {
                let v = bar(step as f32 * 0.013, i);
                assert!((0.0..=1.0).contains(&v));
            }
        }
    }

    #[test]
    fn bars_move_and_do_not_move_in_lockstep() {
        let at = |t| (0..BARS).map(|i| bar(t, i)).collect::<Vec<_>>();
        assert_ne!(at(0.3), at(0.4));
        let b = at(1.7);
        assert!(b[0] != b[1] && b[1] != b[2]);
    }

    #[test]
    fn each_bar_travels_a_wide_range() {
        for i in 0..BARS {
            let vals: Vec<f32> = (0..3000).map(|s| bar(s as f32 * 0.01, i)).collect();
            let (lo, hi) = (vals.iter().cloned().fold(1.0, f32::min), vals.iter().cloned().fold(0.0, f32::max));
            assert!(hi - lo > 0.6, "bar {i}: {lo}..{hi}");
        }
    }
}
