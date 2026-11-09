// SPDX-License-Identifier: GPL-3.0-or-later
//! The "new song" card: the closed pill briefly grows into a small card with the cover,
//! title, artist, equaliser and a progress bar.

use crate::canvas::Rect;

pub const CARD_W: f32 = 270.0;
pub const CARD_H: f32 = 66.0;
pub const CARD_RADIUS: f32 = 22.0;
/// How long the card stays before the pill shrinks back.
pub const CARD_MS: i64 = 3500;

/// Whether moving from `prev` to `now` (title, artist) is a song change worth announcing:
/// something was playing before and the new song is a different, non-empty one.
pub fn is_track_change(prev: Option<(&str, &str)>, now: (&str, &str)) -> bool {
    let named = |t: (&str, &str)| !t.0.is_empty() || !t.1.is_empty();
    prev.is_some_and(|p| named(p) && p != now) && named(now)
}

pub struct CardLayout {
    pub cover: Rect,
    pub title: Rect,
    pub sub: Rect,
    pub eq: Rect,
    pub bar: Rect,
}

/// Positions inside a card whose left edge is `x` and width `w` (top at y = 0).
pub fn layout(x: f32, w: f32) -> CardLayout {
    let text_x = x + 62.0;
    let eq_x = x + w - 24.0;
    CardLayout {
        cover: Rect { x: x + 10.0, y: 10.0, w: 42.0, h: 42.0 },
        title: Rect { x: text_x, y: 11.0, w: eq_x - 8.0 - text_x, h: 18.0 },
        sub: Rect { x: text_x, y: 29.0, w: eq_x - 8.0 - text_x, h: 16.0 },
        eq: Rect { x: eq_x, y: 16.0, w: 10.0, h: 12.0 },
        bar: Rect { x: x + 10.0, y: 56.0, w: w - 20.0, h: 3.0 },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_different_song_after_another_is_a_change() {
        assert!(is_track_change(Some(("A", "X")), ("B", "X")));
        assert!(is_track_change(Some(("A", "X")), ("A", "Y")));
    }

    #[test]
    fn the_first_song_a_repeat_and_an_empty_title_are_not() {
        assert!(!is_track_change(None, ("A", "X")));
        assert!(!is_track_change(Some(("A", "X")), ("A", "X")));
        assert!(!is_track_change(Some(("A", "X")), ("", "")));
        assert!(!is_track_change(Some(("", "")), ("B", "X")));
    }

    #[test]
    fn the_card_content_stays_inside_the_card_and_the_text_clears_the_cover_and_equaliser() {
        let l = layout(100.0, CARD_W);
        for r in [l.cover, l.title, l.sub, l.eq, l.bar] {
            assert!(r.x >= 100.0 && r.x + r.w <= 100.0 + CARD_W, "{r:?}");
            assert!(r.y >= 0.0 && r.y + r.h <= CARD_H, "{r:?}");
        }
        assert!(l.title.x >= l.cover.x + l.cover.w);
        assert!(l.title.x + l.title.w <= l.eq.x);
        assert!(l.bar.y >= l.cover.y + l.cover.h);
    }
}
