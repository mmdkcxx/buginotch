// SPDX-License-Identifier: GPL-3.0-or-later
//! Geometry of the Timer Digioh page (DIPs), shared by drawing and hit-testing.

use crate::canvas::Rect;
use crate::digioh::PRESETS;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DigiohLayout {
    // Join screen.
    pub prompt: Rect,
    /// The row of code cells as a whole, and each cell.
    pub field: Rect,
    pub cells: [Rect; 8],
    pub join: Rect,
    pub last: Rect,
    // Online screen.
    pub dot: Rect,
    pub code: Rect,
    pub status: Rect,
    pub leave: Rect,
    pub time: Rect,
    pub info: Rect,
    pub ruler: Rect,
    pub reset: Rect,
    pub main: Rect,
    pub plus: Rect,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DigiohHit {
    Field,
    Join,
    Last,
    Leave,
    Reset,
    Main,
    Plus,
    /// The big digits: tap to type a length.
    Time,
    /// One of the duration zones on the ruler (index into `PRESETS`).
    Preset(usize),
}

pub fn layout(a: Rect) -> DigiohLayout {
    let r = |x: f32, y: f32, w: f32, h: f32| Rect { x: a.x + x, y: a.y + y, w, h };
    DigiohLayout {
        prompt: r(8.0, 4.0, 300.0, 18.0),
        field: r(8.0, 28.0, 355.0, 50.0),
        cells: std::array::from_fn(|i| r(8.0 + 45.0 * i as f32, 28.0, 40.0, 50.0)),
        join: r(372.0, 28.0, 72.0, 50.0),
        last: r(122.0, 90.0, 96.0, 26.0),
        dot: r(8.0, 8.0, 8.0, 8.0),
        code: r(22.0, 3.0, 96.0, 18.0),
        status: r(112.0, 3.0, 150.0, 18.0),
        leave: Rect { x: a.x + a.w - 66.0, y: a.y + 1.0, w: 66.0, h: 22.0 },
        time: r(6.0, 22.0, 262.0, 60.0),
        info: r(8.0, 84.0, 262.0, 16.0),
        ruler: r(8.0, 104.0, 262.0, 10.0),
        reset: r(292.0, 46.0, 34.0, 34.0),
        main: r(334.0, 31.0, 64.0, 64.0),
        plus: r(406.0, 46.0, 34.0, 34.0),
    }
}

pub fn hit(a: Rect, online: bool, x: f32, y: f32) -> Option<DigiohHit> {
    let l = layout(a);
    let inside = |r: Rect| x >= r.x && x <= r.x + r.w && y >= r.y && y <= r.y + r.h;
    if !online {
        return [(l.join, DigiohHit::Join), (l.field, DigiohHit::Field), (l.last, DigiohHit::Last)].into_iter().find(|(r, _)| inside(*r)).map(|(_, h)| h);
    }
    let fixed = [(l.leave, DigiohHit::Leave), (l.main, DigiohHit::Main), (l.reset, DigiohHit::Reset), (l.plus, DigiohHit::Plus)];
    if let Some((_, h)) = fixed.into_iter().find(|(r, _)| inside(*r)) {
        return Some(h);
    }
    if inside(l.time) {
        return Some(DigiohHit::Time);
    }
    // Generous vertical target around the thin ruler.
    let zone = Rect { y: l.ruler.y - 8.0, h: l.ruler.h + 14.0, ..l.ruler };
    if inside(zone) {
        let k = ((x - zone.x) / zone.w * PRESETS.len() as f32).floor() as usize;
        return Some(DigiohHit::Preset(k.min(PRESETS.len() - 1)));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: Rect = Rect { x: 44.0, y: 36.0, w: 452.0, h: 122.0 };

    fn mid(r: Rect) -> (f32, f32) {
        (r.x + r.w / 2.0, r.y + r.h / 2.0)
    }

    #[test]
    fn join_screen_targets() {
        let l = layout(A);
        for (r, h) in [(l.field, DigiohHit::Field), (l.join, DigiohHit::Join), (l.last, DigiohHit::Last)] {
            let (x, y) = mid(r);
            assert_eq!(hit(A, false, x, y), Some(h));
        }
        assert!(l.cells.iter().all(|c| c.x >= l.field.x && c.x + c.w <= l.field.x + l.field.w + 0.01));
        let (x, y) = mid(l.main);
        assert_eq!(hit(A, false, x, y), None, "controls are not live before joining");
    }

    #[test]
    fn online_targets_and_ruler_zones() {
        let l = layout(A);
        for (r, h) in [(l.leave, DigiohHit::Leave), (l.main, DigiohHit::Main), (l.reset, DigiohHit::Reset), (l.plus, DigiohHit::Plus)] {
            let (x, y) = mid(r);
            assert_eq!(hit(A, true, x, y), Some(h));
        }
        let y = l.ruler.y + l.ruler.h / 2.0;
        assert_eq!(hit(A, true, l.ruler.x + 2.0, y), Some(DigiohHit::Preset(0)));
        assert_eq!(hit(A, true, l.ruler.x + l.ruler.w - 2.0, y), Some(DigiohHit::Preset(PRESETS.len() - 1)));
        let (x, y) = mid(l.time);
        assert_eq!(hit(A, true, x, y), Some(DigiohHit::Time));
        assert_eq!(hit(A, true, l.plus.x, A.y + 112.0), None);
    }

    #[test]
    fn everything_fits_the_page() {
        let l = layout(A);
        for r in [l.field, l.join, l.last, l.time, l.info, l.ruler, l.reset, l.main, l.plus, l.leave] {
            assert!(r.x >= A.x && r.x + r.w <= A.x + A.w + 0.01 && r.y >= A.y && r.y + r.h <= A.y + A.h + 0.01, "{r:?}");
        }
    }
}
