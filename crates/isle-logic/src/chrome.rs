// SPDX-License-Identifier: GPL-3.0-or-later
//! Geometry of the open panel's chrome: page arrows and the dots row (DIPs).

use crate::canvas::Rect;

pub const ARROW_RADIUS: f32 = 13.0;
const ARROW_INSET: f32 = 8.0;
pub const DOT_SLOT: f32 = 16.0;
/// Width of the white indicator capsule at rest.
pub const INDICATOR_W: f32 = 18.0;
const DOT_BOTTOM: f32 = 12.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChromeAction {
    Prev,
    Next,
    Dot(usize),
}

pub fn prev_center(area: Rect) -> (f32, f32) {
    (area.x + ARROW_INSET + ARROW_RADIUS, area.y + area.h / 2.0)
}

pub fn next_center(area: Rect) -> (f32, f32) {
    (area.x + area.w - ARROW_INSET - ARROW_RADIUS, area.y + area.h / 2.0)
}

/// Centre of page dot `i` of `pages`.
pub fn dot_center(area: Rect, pages: usize, i: usize) -> (f32, f32) {
    let start = area.x + (area.w - pages as f32 * DOT_SLOT) / 2.0;
    (start + DOT_SLOT / 2.0 + DOT_SLOT * i as f32, area.y + area.h - DOT_BOTTOM)
}

/// Left and right edge of the white page indicator. While moving from page `from` to `to`
/// (`elapsed_ms` since the move began) the edge in front runs ahead and the one behind
/// follows a moment later, so the capsule stretches like a drop and then settles.
pub fn indicator_span(area: Rect, pages: usize, from: usize, to: usize, elapsed_ms: i64) -> (f32, f32) {
    let half = INDICATOR_W / 2.0;
    let (a, b) = (dot_center(area, pages, from).0, dot_center(area, pages, to).0);
    let t = (elapsed_ms as f32 / crate::page_transition::DURATION_MS as f32).clamp(0.0, 1.0);
    let lead = 1.0 - (1.0 - t).powi(3);
    let follow = {
        let u = ((t - 0.18) / 0.82).clamp(0.0, 1.0);
        1.0 - (1.0 - u).powi(3)
    };
    let mix = |p: f32, q: f32, k: f32| p + (q - p) * k;
    if b >= a {
        (mix(a - half, b - half, follow), mix(a + half, b + half, lead))
    } else {
        (mix(a - half, b - half, lead), mix(a + half, b + half, follow))
    }
}

pub fn chrome_hit(area: Rect, x: f32, y: f32, pages: usize) -> Option<ChromeAction> {
    if pages == 0 {
        return None;
    }
    let inside = |c: (f32, f32), r: f32| (x - c.0).powi(2) + (y - c.1).powi(2) <= r * r;
    if inside(prev_center(area), ARROW_RADIUS) {
        return Some(ChromeAction::Prev);
    }
    if inside(next_center(area), ARROW_RADIUS) {
        return Some(ChromeAction::Next);
    }
    (0..pages).find(|&i| {
        let c = dot_center(area, pages, i);
        (x - c.0).abs() <= DOT_SLOT / 2.0 && (y - c.1).abs() <= DOT_SLOT / 2.0
    }).map(ChromeAction::Dot)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::canvas::Rect;

    const AREA: Rect = Rect { x: 0.0, y: 0.0, w: 540.0, h: 184.0 };

    #[test]
    fn hit_next_arrow() {
        assert_eq!(chrome_hit(AREA, 519.0, 92.0, 3), Some(ChromeAction::Next));
    }

    #[test]
    fn hit_prev_arrow() {
        assert_eq!(chrome_hit(AREA, 21.0, 92.0, 3), Some(ChromeAction::Prev));
    }

    #[test]
    fn hit_third_dot() {
        assert_eq!(chrome_hit(AREA, 286.0, 172.0, 3), Some(ChromeAction::Dot(2)));
    }

    #[test]
    fn miss_returns_none() {
        assert_eq!(chrome_hit(AREA, 270.0, 92.0, 3), None);
        assert_eq!(chrome_hit(AREA, 270.0, 172.0, 0), None);
    }

    #[test]
    fn dot_centres_are_centred_on_the_panel() {
        let first = dot_center(AREA, 3, 0).0;
        let last = dot_center(AREA, 3, 2).0;
        assert_eq!((first + last) / 2.0, 270.0);
    }

    #[test]
    fn hit_respects_area_offset() {
        let a = Rect { x: 100.0, y: 10.0, w: 540.0, h: 184.0 };
        assert_eq!(chrome_hit(a, 619.0, 102.0, 3), Some(ChromeAction::Next));
    }

    #[test]
    fn the_indicator_rests_on_the_current_dot() {
        let c = dot_center(AREA, 3, 1).0;
        assert_eq!(indicator_span(AREA, 3, 1, 1, 0), (c - INDICATOR_W / 2.0, c + INDICATOR_W / 2.0));
        let (l, r) = indicator_span(AREA, 3, 0, 2, crate::page_transition::DURATION_MS);
        let c2 = dot_center(AREA, 3, 2).0;
        assert_eq!((l, r), (c2 - INDICATOR_W / 2.0, c2 + INDICATOR_W / 2.0));
    }

    #[test]
    fn the_indicator_stretches_towards_the_next_dot_then_catches_up() {
        let start = indicator_span(AREA, 3, 0, 1, 0);
        let mid = indicator_span(AREA, 3, 0, 1, 100);
        assert!(mid.1 - mid.0 > INDICATOR_W, "stretched while travelling right");
        assert!(mid.1 > start.1 && mid.0 >= start.0, "the right edge leads");
        let back = indicator_span(AREA, 3, 2, 1, 100);
        let rest = dot_center(AREA, 3, 2).0;
        assert!(back.0 < rest - INDICATOR_W / 2.0, "the left edge leads when going back");
    }
}
