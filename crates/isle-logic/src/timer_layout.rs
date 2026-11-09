// SPDX-License-Identifier: GPL-3.0-or-later
//! Geometry of the Focus timer page (DIPs), shared by drawing and hit-testing.

use crate::canvas::Rect;

pub const RING_RADIUS: f32 = 36.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TimerLayout {
    pub ring: (f32, f32),
    pub work_label: Rect,
    pub work_minus: Rect,
    pub work_value: Rect,
    pub work_plus: Rect,
    pub break_label: Rect,
    pub break_minus: Rect,
    pub break_value: Rect,
    pub break_plus: Rect,
    pub start: Rect,
    pub reset: Rect,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimerHit {
    StartPause,
    Reset,
    WorkMinus,
    WorkPlus,
    BreakMinus,
    BreakPlus,
}

pub fn layout(a: Rect) -> TimerLayout {
    let x0 = a.x + 170.0;
    let row = |y: f32| {
        (
            Rect { x: x0, y, w: 72.0, h: 22.0 },
            Rect { x: x0 + 76.0, y, w: 22.0, h: 22.0 },
            Rect { x: x0 + 100.0, y, w: 62.0, h: 22.0 },
            Rect { x: x0 + 164.0, y, w: 22.0, h: 22.0 },
        )
    };
    let (work_label, work_minus, work_value, work_plus) = row(a.y + 2.0);
    let (break_label, break_minus, break_value, break_plus) = row(a.y + 30.0);
    TimerLayout {
        ring: (a.x + 70.0, a.y + 43.0),
        work_label,
        work_minus,
        work_value,
        work_plus,
        break_label,
        break_minus,
        break_value,
        break_plus,
        start: Rect { x: x0, y: a.y + 58.0, w: 90.0, h: 26.0 },
        reset: Rect { x: x0 + 98.0, y: a.y + 58.0, w: 70.0, h: 26.0 },
    }
}

pub fn hit(a: Rect, x: f32, y: f32) -> Option<TimerHit> {
    let l = layout(a);
    let inside = |r: Rect| x >= r.x && x <= r.x + r.w && y >= r.y && y <= r.y + r.h;
    [
        (l.start, TimerHit::StartPause),
        (l.reset, TimerHit::Reset),
        (l.work_minus, TimerHit::WorkMinus),
        (l.work_plus, TimerHit::WorkPlus),
        (l.break_minus, TimerHit::BreakMinus),
        (l.break_plus, TimerHit::BreakPlus),
    ]
    .into_iter()
    .find(|(r, _)| inside(*r))
    .map(|(_, h)| h)
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

    const A: Rect = Rect { x: 44.0, y: 36.0, w: 452.0, h: 86.0 };

    #[test]
    fn hit_each_button_at_its_centre() {
        let l = layout(A);
        let c = |r: Rect| (r.x + r.w / 2.0, r.y + r.h / 2.0);
        for (rect, expected) in [
            (l.start, TimerHit::StartPause),
            (l.reset, TimerHit::Reset),
            (l.work_minus, TimerHit::WorkMinus),
            (l.work_plus, TimerHit::WorkPlus),
            (l.break_minus, TimerHit::BreakMinus),
            (l.break_plus, TimerHit::BreakPlus),
        ] {
            let (x, y) = c(rect);
            assert_eq!(hit(A, x, y), Some(expected));
        }
    }

    #[test]
    fn click_on_the_ring_hits_nothing() {
        let l = layout(A);
        assert_eq!(hit(A, l.ring.0, l.ring.1), None);
    }

    #[test]
    fn buttons_stay_inside_the_area() {
        let l = layout(A);
        for r in [l.start, l.reset, l.work_minus, l.work_plus, l.break_minus, l.break_plus] {
            assert!(r.x >= A.x && r.x + r.w <= A.x + A.w && r.y >= A.y && r.y + r.h <= A.y + A.h, "{r:?}");
        }
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
