// SPDX-License-Identifier: GPL-3.0-or-later
//! Geometry and hit-testing of the Shelf page: a header line and a row of file tiles that
//! scrolls sideways.

use crate::canvas::Rect;

pub const TILE_W: f32 = 80.0;
pub const TILE_H: f32 = 92.0;
pub const TILE_GAP: f32 = 8.0;
const HEADER_H: f32 = 22.0;
const CLEAR_W: f32 = 84.0;
/// The little remove button in a tile's top-right corner.
pub const REMOVE: f32 = 18.0;

pub struct ShelfLayout {
    pub caption: Rect,
    pub phone: Rect,
    pub clear: Rect,
    pub tiles: Rect,
}

pub fn layout(a: Rect) -> ShelfLayout {
    ShelfLayout {
        caption: Rect { x: a.x, y: a.y, w: a.w - CLEAR_W * 2.0 - 16.0, h: HEADER_H },
        phone: Rect { x: a.x + a.w - CLEAR_W * 2.0 - 8.0, y: a.y, w: CLEAR_W, h: HEADER_H },
        clear: Rect { x: a.x + a.w - CLEAR_W, y: a.y, w: CLEAR_W, h: HEADER_H },
        tiles: Rect { x: a.x, y: a.y + HEADER_H + 8.0, w: a.w, h: TILE_H },
    }
}

pub fn tile_rect(l: &ShelfLayout, i: usize, scroll: f32) -> Rect {
    Rect { x: l.tiles.x + i as f32 * (TILE_W + TILE_GAP) - scroll, y: l.tiles.y, w: TILE_W, h: TILE_H }
}

pub fn remove_rect(tile: Rect) -> Rect {
    Rect { x: tile.x + tile.w - REMOVE - 3.0, y: tile.y + 3.0, w: REMOVE, h: REMOVE }
}

/// The small "send to phone" button in a tile's top-left corner.
pub fn send_rect(tile: Rect) -> Rect {
    Rect { x: tile.x + 3.0, y: tile.y + 3.0, w: REMOVE, h: REMOVE }
}

/// How far the row can scroll sideways.
pub fn max_scroll(l: &ShelfLayout, count: usize) -> f32 {
    let content = if count == 0 { 0.0 } else { count as f32 * (TILE_W + TILE_GAP) - TILE_GAP };
    (content - l.tiles.w).max(0.0)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShelfHit {
    Clear,
    Phone,
    Tile(usize),
    Remove(usize),
    Send(usize),
}

fn inside(r: Rect, x: f32, y: f32) -> bool {
    x >= r.x && x < r.x + r.w && y >= r.y && y < r.y + r.h
}

pub fn hit(a: Rect, scroll: f32, count: usize, x: f32, y: f32) -> Option<ShelfHit> {
    let l = layout(a);
    if inside(l.clear, x, y) {
        return Some(ShelfHit::Clear);
    }
    if inside(l.phone, x, y) {
        return Some(ShelfHit::Phone);
    }
    if !inside(l.tiles, x, y) {
        return None;
    }
    (0..count).find_map(|i| {
        let t = tile_rect(&l, i, scroll);
        inside(t, x, y).then(|| {
            if inside(remove_rect(t), x, y) {
                ShelfHit::Remove(i)
            } else if inside(send_rect(t), x, y) {
                ShelfHit::Send(i)
            } else {
                ShelfHit::Tile(i)
            }
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: Rect = Rect { x: 44.0, y: 36.0, w: 452.0, h: 122.0 };

    #[test]
    fn everything_fits_inside_the_page_area() {
        let l = layout(A);
        for r in [l.caption, l.phone, l.clear, l.tiles] {
            assert!(r.x >= A.x && r.x + r.w <= A.x + A.w + 0.01 && r.y >= A.y && r.y + r.h <= A.y + A.h + 0.01, "{r:?}");
        }
    }

    #[test]
    fn five_tiles_fit_without_scrolling_and_a_sixth_makes_it_scroll() {
        let l = layout(A);
        assert_eq!(max_scroll(&l, 5), 0.0);
        assert!(max_scroll(&l, 6) > 0.0);
        assert_eq!(max_scroll(&l, 0), 0.0);
    }

    #[test]
    fn hits_find_the_tile_its_remove_button_and_the_clear_button() {
        let l = layout(A);
        let t1 = tile_rect(&l, 1, 0.0);
        assert_eq!(hit(A, 0.0, 3, t1.x + 10.0, t1.y + 50.0), Some(ShelfHit::Tile(1)));
        let r = remove_rect(t1);
        assert_eq!(hit(A, 0.0, 3, r.x + 2.0, r.y + 2.0), Some(ShelfHit::Remove(1)));
        assert_eq!(hit(A, 0.0, 3, l.clear.x + 5.0, l.clear.y + 5.0), Some(ShelfHit::Clear));
        assert_eq!(hit(A, 0.0, 3, l.phone.x + 5.0, l.phone.y + 5.0), Some(ShelfHit::Phone));
        let sd = send_rect(t1);
        assert_eq!(hit(A, 0.0, 3, sd.x + 2.0, sd.y + 2.0), Some(ShelfHit::Send(1)));
        assert_eq!(hit(A, 0.0, 3, 300.0, 40.0), None);
        assert_eq!(hit(A, 0.0, 1, t1.x + 10.0, t1.y + 50.0), None, "no tile there");
    }

    #[test]
    fn scrolling_moves_the_tiles_under_the_pointer() {
        let l = layout(A);
        let shifted = tile_rect(&l, 2, 100.0);
        assert_eq!(hit(A, 100.0, 6, shifted.x + 10.0, shifted.y + 50.0), Some(ShelfHit::Tile(2)));
    }
}
