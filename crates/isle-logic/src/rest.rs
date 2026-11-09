// SPDX-License-Identifier: GPL-3.0-or-later
//! The pill's resting state: after a while with nothing to show, the clock gives way to a pair of
//! eyes that blink and glance around.

/// Idle time before the eyes start to appear.
pub const FADE_MS: i64 = 700;
const BLINK_PERIOD_MS: i64 = 3800;
const BLINK_MS: i64 = 180;
const LOOK_PERIOD_MS: i64 = 7000;

/// Where the eyes are at a moment: how open they are (1 open, 0 shut) and where the pupils look
/// (-1..1 in each direction).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Eyes {
    pub open: f32,
    pub look_x: f32,
    pub look_y: f32,
}

/// 0 while the pill is still active, rising to 1 over `FADE_MS` once `idle_ms` passes `after_ms`.
/// An `after_ms` of 0 or less turns the resting state off.
pub fn visibility(idle_ms: i64, after_ms: i64) -> f32 {
    if after_ms <= 0 || idle_ms < after_ms {
        return 0.0;
    }
    ((idle_ms - after_ms) as f32 / FADE_MS as f32).clamp(0.0, 1.0)
}

/// The eyes `t_ms` milliseconds into the animation.
pub fn eyes(t_ms: i64) -> Eyes {
    // Blink: shut and open again in the last BLINK_MS of every period.
    let into = t_ms.rem_euclid(BLINK_PERIOD_MS) - (BLINK_PERIOD_MS - BLINK_MS);
    let open = if into < 0 {
        1.0
    } else {
        let p = into as f32 / BLINK_MS as f32;
        (2.0 * p - 1.0).abs()
    };
    // Glance: hold, look left, look right (a touch down), return, with smooth moves between.
    const KEYS: [(i64, f32, f32); 8] = [
        (0, 0.0, 0.0),
        (1200, 0.0, 0.0),
        (1700, -1.0, 0.0),
        (3000, -1.0, 0.0),
        (3500, 1.0, 0.3),
        (4800, 1.0, 0.3),
        (5400, 0.0, 0.0),
        (LOOK_PERIOD_MS, 0.0, 0.0),
    ];
    let t = t_ms.rem_euclid(LOOK_PERIOD_MS);
    let (mut x, mut y) = (0.0, 0.0);
    for w in KEYS.windows(2) {
        let ((t0, x0, y0), (t1, x1, y1)) = (w[0], w[1]);
        if t >= t0 && t <= t1 {
            let u = (t - t0) as f32 / (t1 - t0) as f32;
            let k = u * u * (3.0 - 2.0 * u);
            x = x0 + (x1 - x0) * k;
            y = y0 + (y1 - y0) * k;
            break;
        }
    }
    Eyes { open, look_x: x, look_y: y }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hidden_until_the_idle_time_has_passed() {
        assert_eq!(visibility(0, 120_000), 0.0);
        assert_eq!(visibility(119_999, 120_000), 0.0);
    }

    #[test]
    fn fades_in_then_stays_fully_visible() {
        let mid = visibility(120_000 + FADE_MS / 2, 120_000);
        assert!(mid > 0.1 && mid < 0.9, "mid = {mid}");
        assert_eq!(visibility(120_000 + FADE_MS, 120_000), 1.0);
        assert_eq!(visibility(10_000_000, 120_000), 1.0);
    }

    #[test]
    fn zero_idle_time_switches_it_off() {
        assert_eq!(visibility(10_000_000, 0), 0.0);
    }

    #[test]
    fn eyes_start_open_and_centred() {
        assert_eq!(eyes(0), Eyes { open: 1.0, look_x: 0.0, look_y: 0.0 });
    }

    #[test]
    fn eyes_blink_shut_and_reopen() {
        let shut = eyes(BLINK_PERIOD_MS - BLINK_MS / 2).open;
        assert!(shut < 0.2, "shut = {shut}");
        assert_eq!(eyes(BLINK_PERIOD_MS + 500).open, 1.0);
    }

    #[test]
    fn eyes_glance_left_then_right_and_stay_in_range() {
        let (mut min_x, mut max_x) = (0.0f32, 0.0f32);
        for t in (0..LOOK_PERIOD_MS * 2).step_by(20) {
            let e = eyes(t);
            assert!((-1.0..=1.0).contains(&e.look_x) && (-1.0..=1.0).contains(&e.look_y));
            min_x = min_x.min(e.look_x);
            max_x = max_x.max(e.look_x);
        }
        assert!(min_x < -0.9 && max_x > 0.9);
    }

    #[test]
    fn the_look_loops() {
        assert_eq!(eyes(2500).look_x, eyes(2500 + LOOK_PERIOD_MS).look_x);
    }
}
