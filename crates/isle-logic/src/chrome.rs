// SPDX-License-Identifier: GPL-3.0-or-later
//! Geometry of the open panel's chrome: page arrows and the dots row (DIPs).

use crate::canvas::Rect;

pub const ARROW_RADIUS: f32 = 13.0;
const ARROW_INSET: f32 = 8.0;
pub const DOT_SLOT: f32 = 16.0;
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
}
