// SPDX-License-Identifier: GPL-3.0-or-later
//! Drawing abstraction. `isle-win` implements it with Direct2D; tests use a recording fake.

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Color(pub u8, pub u8, pub u8, pub u8);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Align {
    Left,
    Center,
    Right,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ImageId(pub u32);

pub trait Canvas {
    fn fill_round_rect(&mut self, r: Rect, radius: f32, c: Color);
    fn fill_circle(&mut self, cx: f32, cy: f32, r: f32, c: Color);
    fn stroke_arc(&mut self, cx: f32, cy: f32, r: f32, start_deg: f32, sweep_deg: f32, width: f32, c: Color);
    fn draw_text(&mut self, text: &str, r: Rect, size: f32, weight: u16, c: Color, align: Align);
    fn measure_text(&mut self, text: &str, size: f32, weight: u16) -> f32;
    fn draw_image(&mut self, id: ImageId, r: Rect, radius: f32);
    fn push_clip(&mut self, r: Rect, radius: f32);
    fn pop_clip(&mut self);
}

/// Returns `text` if it fits in `max_w`, else the longest prefix plus "…" that does.
/// Splits only on char boundaries; returns "" when not even "…" fits.
pub fn fit_text(c: &mut dyn Canvas, text: &str, size: f32, weight: u16, max_w: f32) -> String {
    if text.is_empty() || c.measure_text("…", size, weight) > max_w && c.measure_text(text, size, weight) > max_w {
        return String::new();
    }
    if c.measure_text(text, size, weight) <= max_w {
        return text.to_string();
    }
    let cuts: Vec<usize> = text.char_indices().map(|(i, _)| i).collect();
    let (mut lo, mut hi) = (0usize, cuts.len());
    // Largest k (chars kept) with width(prefix + "…") <= max_w; k = 0 means just "…".
    while lo < hi {
        let mid = (lo + hi).div_ceil(2);
        let candidate = format!("{}…", &text[..cuts[mid]]);
        if c.measure_text(&candidate, size, weight) <= max_w {
            lo = mid;
        } else {
            hi = mid - 1;
        }
    }
    format!("{}…", &text[..cuts[lo]])
}
