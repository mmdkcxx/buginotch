// SPDX-License-Identifier: GPL-3.0-or-later
//! Geometry of the Focus timer page (DIPs), shared by drawing and hit-testing.

use crate::canvas::Rect;

/// One settings row: label, minus, value, plus.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StepRow {
    pub label: Rect,
    pub minus: Rect,
    pub value: Rect,
    pub plus: Rect,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TimerLayout {
    /// The big countdown, the line under it, and the progress bar.
    pub time: Rect,
    pub label: Rect,
    pub bar: Rect,
    /// The capsule on the right and its three segments.
    pub capsule: Rect,
    pub main: Rect,
    pub second: Rect,
    pub gear: Rect,
    /// Work and break steppers, shown instead of the countdown while settings are open.
    pub rows: [StepRow; 2],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimerHit {
    /// Start / pause / resume (or "Selesai" while settings are open).
    Main,
    /// Reset (or skip, during a break).
    Second,
    Gear,
    WorkMinus,
    WorkPlus,
    BreakMinus,
    BreakPlus,
}

const CAPSULE_W: f32 = 176.0;
const CAPSULE_H: f32 = 38.0;

pub fn layout(a: Rect) -> TimerLayout {
    let capsule = Rect { x: a.x + a.w - CAPSULE_W, y: a.y + (a.h - CAPSULE_H) / 2.0, w: CAPSULE_W, h: CAPSULE_H };
    let left_x = a.x + 8.0;
    let left_w = capsule.x - 20.0 - left_x;
    let row = |y: f32| StepRow {
        label: Rect { x: left_x, y, w: 80.0, h: 24.0 },
        minus: Rect { x: left_x + 96.0, y, w: 24.0, h: 24.0 },
        value: Rect { x: left_x + 124.0, y, w: 84.0, h: 24.0 },
        plus: Rect { x: left_x + 212.0, y, w: 24.0, h: 24.0 },
    };
    TimerLayout {
        time: Rect { x: left_x, y: a.y + 12.0, w: left_w, h: 62.0 },
        label: Rect { x: left_x, y: a.y + 76.0, w: left_w, h: 18.0 },
        bar: Rect { x: left_x, y: a.y + 100.0, w: left_w, h: 4.0 },
        capsule,
        main: Rect { w: 80.0, ..capsule },
        second: Rect { x: capsule.x + 80.0, w: 58.0, ..capsule },
        gear: Rect { x: capsule.x + 138.0, w: 38.0, ..capsule },
        rows: [row(a.y + 22.0), row(a.y + 66.0)],
    }
}

pub fn hit(a: Rect, settings_open: bool, x: f32, y: f32) -> Option<TimerHit> {
    let l = layout(a);
    let inside = |r: Rect| x >= r.x && x <= r.x + r.w && y >= r.y && y <= r.y + r.h;
    let mut targets = vec![(l.main, TimerHit::Main), (l.second, TimerHit::Second), (l.gear, TimerHit::Gear)];
    if settings_open {
        targets.extend([
            (l.rows[0].minus, TimerHit::WorkMinus),
            (l.rows[0].plus, TimerHit::WorkPlus),
            (l.rows[1].minus, TimerHit::BreakMinus),
            (l.rows[1].plus, TimerHit::BreakPlus),
        ]);
    }
    targets.into_iter().find(|(r, _)| inside(*r)).map(|(_, h)| h)
}

/// Work length in minutes, stepped by 5 within 5..=90.
pub fn step_work(minutes: u32, dir: i32) -> u32 {
    (minutes as i32 + dir * 5).clamp(5, 90) as u32
}

/// Break length in minutes, stepped by 1 within 1..=30.
pub fn step_break(minutes: u32, dir: i32) -> u32 {
    (minutes as i32 + dir).clamp(1, 30) as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: Rect = Rect { x: 44.0, y: 36.0, w: 452.0, h: 122.0 };

    fn c(r: Rect) -> (f32, f32) {
        (r.x + r.w / 2.0, r.y + r.h / 2.0)
    }

    #[test]
    fn capsule_segments_hit_main_second_and_gear() {
        let l = layout(A);
        for (rect, expected) in [(l.main, TimerHit::Main), (l.second, TimerHit::Second), (l.gear, TimerHit::Gear)] {
            let (x, y) = c(rect);
            assert_eq!(hit(A, false, x, y), Some(expected));
            assert_eq!(hit(A, true, x, y), Some(expected));
        }
    }

    #[test]
    fn steppers_only_hit_while_settings_are_open() {
        let l = layout(A);
        let (x, y) = c(l.rows[0].plus);
        assert_eq!(hit(A, false, x, y), None);
        assert_eq!(hit(A, true, x, y), Some(TimerHit::WorkPlus));
        let (x, y) = c(l.rows[1].minus);
        assert_eq!(hit(A, true, x, y), Some(TimerHit::BreakMinus));
    }

    #[test]
    fn time_and_progress_bar_hit_nothing() {
        let l = layout(A);
        let (x, y) = c(l.time);
        assert_eq!(hit(A, false, x, y), None);
        let (x, y) = c(l.bar);
        assert_eq!(hit(A, true, x, y), None);
    }

    #[test]
    fn everything_stays_inside_the_area_and_the_left_column_clears_the_capsule() {
        let l = layout(A);
        let mut all = vec![l.time, l.label, l.bar, l.capsule, l.main, l.second, l.gear];
        for r in l.rows {
            all.extend([r.label, r.minus, r.value, r.plus]);
        }
        for r in all {
            assert!(r.x >= A.x && r.x + r.w <= A.x + A.w && r.y >= A.y && r.y + r.h <= A.y + A.h, "{r:?}");
        }
        assert!(l.time.x + l.time.w <= l.capsule.x);
        assert!(l.bar.x + l.bar.w <= l.capsule.x);
    }

    #[test]
    fn stepper_clamps_work_and_break_minutes() {
        assert_eq!(step_work(25, -1), 20);
        assert_eq!(step_work(5, -1), 5);
        assert_eq!(step_work(90, 1), 90);
        assert_eq!(step_break(5, -1), 4);
        assert_eq!(step_break(1, -1), 1);
        assert_eq!(step_break(30, 1), 30);
    }
}
