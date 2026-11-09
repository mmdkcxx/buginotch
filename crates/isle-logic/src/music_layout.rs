// SPDX-License-Identifier: GPL-3.0-or-later
//! Geometry of the Music page (DIPs), shared by drawing and hit-testing.

use crate::canvas::Rect;

pub const PLAY_RADIUS: f32 = 17.0;
pub const SKIP_RADIUS: f32 = 14.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MusicLayout {
    pub cover: Rect,
    pub title: Rect,
    pub lyrics_btn: Rect,
    pub artist: Rect,
    pub elapsed: Rect,
    pub bar: Rect,
    pub total: Rect,
    pub prev: (f32, f32),
    pub play: (f32, f32),
    pub next: (f32, f32),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MusicHit {
    Prev,
    PlayPause,
    Next,
    /// A click on the scrub bar, as a fraction 0..=1 of its length.
    Seek(f32),
    LyricsToggle,
    /// One of the three visible lyric rows (0 = previous, 1 = current, 2 = next).
    LyricRow(usize),
}

/// Width of the lyrics column when it is shown, and the gap before it.
pub const LYRICS_W: f32 = 200.0;
const LYRICS_GAP: f32 = 12.0;

pub fn layout(a: Rect) -> MusicLayout {
    layout_for(a, false)
}

/// The player's layout; with `lyrics_on` it gives up the right 212 px to the lyrics.
pub fn layout_for(a: Rect, lyrics_on: bool) -> MusicLayout {
    let left_w = if lyrics_on { a.w - LYRICS_W - LYRICS_GAP } else { a.w };
    let col_x = a.x + 96.0;
    let col_w = left_w - 96.0;
    let cx = col_x + col_w / 2.0;
    let yc = a.y + 69.0;
    MusicLayout {
        cover: Rect { x: a.x, y: a.y + 3.0, w: 80.0, h: 80.0 },
        title: Rect { x: col_x, y: a.y, w: col_w - 52.0, h: 18.0 },
        lyrics_btn: Rect { x: col_x + col_w - 46.0, y: a.y, w: 46.0, h: 18.0 },
        artist: Rect { x: col_x, y: a.y + 18.0, w: col_w - 52.0, h: 16.0 },
        elapsed: Rect { x: col_x, y: a.y + 36.0, w: 36.0, h: 14.0 },
        bar: Rect { x: col_x + 40.0, y: a.y + 41.0, w: col_w - 80.0, h: 4.0 },
        total: Rect { x: col_x + col_w - 36.0, y: a.y + 36.0, w: 36.0, h: 14.0 },
        prev: (cx - 56.0, yc),
        play: (cx, yc),
        next: (cx + 56.0, yc),
    }
}

/// The three lyric rows (previous, current, next) on the right of the page.
pub fn lyric_rows(a: Rect) -> [Rect; 3] {
    let x = a.x + a.w - LYRICS_W;
    [
        Rect { x, y: a.y, w: LYRICS_W, h: 24.0 },
        Rect { x, y: a.y + 28.0, w: LYRICS_W, h: 28.0 },
        Rect { x, y: a.y + 60.0, w: LYRICS_W, h: 24.0 },
    ]
}

pub fn hit(a: Rect, x: f32, y: f32) -> Option<MusicHit> {
    hit_for(a, false, x, y)
}

pub fn hit_for(a: Rect, lyrics_on: bool, x: f32, y: f32) -> Option<MusicHit> {
    let l = layout_for(a, lyrics_on);
    let in_rect = |r: Rect| x >= r.x && x <= r.x + r.w && y >= r.y && y <= r.y + r.h;
    let inside = |c: (f32, f32), r: f32| (x - c.0).powi(2) + (y - c.1).powi(2) <= r * r;
    if in_rect(l.lyrics_btn) {
        return Some(MusicHit::LyricsToggle);
    }
    if lyrics_on {
        if let Some(i) = lyric_rows(a).iter().position(|r| in_rect(*r)) {
            return Some(MusicHit::LyricRow(i));
        }
    }
    if inside(l.play, PLAY_RADIUS) {
        return Some(MusicHit::PlayPause);
    }
    if inside(l.prev, SKIP_RADIUS) {
        return Some(MusicHit::Prev);
    }
    if inside(l.next, SKIP_RADIUS) {
        return Some(MusicHit::Next);
    }
    let b = l.bar;
    if x >= b.x - 2.0 && x <= b.x + b.w + 2.0 && (y - (b.y + b.h / 2.0)).abs() <= 6.0 {
        return Some(MusicHit::Seek(((x - b.x) / b.w).clamp(0.0, 1.0)));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: Rect = Rect { x: 44.0, y: 36.0, w: 452.0, h: 86.0 };

    fn centre(r: Rect) -> (f32, f32) {
        (r.x + r.w / 2.0, r.y + r.h / 2.0)
    }

    #[test]
    fn hit_play_button_centre() {
        let l = layout(A);
        let (x, y) = l.play;
        assert_eq!(hit(A, x, y), Some(MusicHit::PlayPause));
    }

    #[test]
    fn hit_prev_and_next() {
        let l = layout(A);
        assert_eq!(hit(A, l.prev.0, l.prev.1), Some(MusicHit::Prev));
        assert_eq!(hit(A, l.next.0, l.next.1), Some(MusicHit::Next));
    }

    #[test]
    fn seek_fraction_in_the_middle() {
        let l = layout(A);
        let (cx, cy) = centre(l.bar);
        match hit(A, cx, cy) {
            Some(MusicHit::Seek(f)) => assert!((f - 0.5).abs() < 0.01),
            other => panic!("expected seek, got {other:?}"),
        }
    }

    #[test]
    fn seek_fraction_clamped_at_the_ends() {
        let l = layout(A);
        let y = centre(l.bar).1;
        assert_eq!(hit(A, l.bar.x, y), Some(MusicHit::Seek(0.0)));
        assert_eq!(hit(A, l.bar.x + l.bar.w, y), Some(MusicHit::Seek(1.0)));
    }

    #[test]
    fn lyrics_button_is_hit_and_clear_of_the_title() {
        let l = layout_for(A, false);
        let (x, y) = centre(l.lyrics_btn);
        assert_eq!(hit_for(A, false, x, y), Some(MusicHit::LyricsToggle));
        assert!(l.title.x + l.title.w <= l.lyrics_btn.x);
    }

    #[test]
    fn lyrics_on_narrows_the_controls() {
        assert!(layout_for(A, true).play.0 < layout_for(A, false).play.0);
        let rows = lyric_rows(A);
        assert!(layout_for(A, true).play.0 + PLAY_RADIUS < rows[0].x);
    }

    #[test]
    fn lyric_rows_are_hit_only_when_lyrics_are_on() {
        let rows = lyric_rows(A);
        let (x, y) = centre(rows[1]);
        assert_eq!(hit_for(A, true, x, y), Some(MusicHit::LyricRow(1)));
        assert!(!matches!(hit_for(A, false, x, y), Some(MusicHit::LyricRow(_))));
    }

    #[test]
    fn click_on_cover_or_title_hits_nothing() {
        let l = layout(A);
        assert_eq!(hit(A, l.cover.x + 5.0, l.cover.y + 5.0), None);
        assert_eq!(hit(A, l.title.x + 5.0, l.title.y + 5.0), None);
    }
}
