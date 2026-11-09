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
    fn fill_polygon(&mut self, points: &[(f32, f32)], c: Color);
    fn fill_circle(&mut self, cx: f32, cy: f32, r: f32, c: Color);
    fn stroke_arc(&mut self, cx: f32, cy: f32, r: f32, start_deg: f32, sweep_deg: f32, width: f32, c: Color);
    fn draw_text(&mut self, text: &str, r: Rect, size: f32, weight: u16, c: Color, align: Align);
    fn measure_text(&mut self, text: &str, size: f32, weight: u16) -> f32;
    /// Uploads straight-alpha RGBA8 pixels and returns a handle for `draw_image`.
    fn load_image(&mut self, rgba: &[u8], w: u32, h: u32) -> ImageId;
    fn draw_image(&mut self, id: ImageId, r: Rect, radius: f32);
    /// Like `draw_image` with an opacity 0..=1. The default only shows it when mostly opaque.
    fn draw_image_alpha(&mut self, id: ImageId, r: Rect, radius: f32, alpha: f32) {
        if alpha > 0.5 {
            self.draw_image(id, r, radius);
        }
    }
    /// Draws what follows shifted `dx` sideways at the given opacity, until `pop_shift`.
    /// The default ignores both (tests don't need them).
    fn push_shift(&mut self, _dx: f32, _alpha: f32) {}
    fn pop_shift(&mut self) {}
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
    let (mut lo, mut hi) = (0usize, cuts.len() - 1); // the whole text is known not to fit
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

/// Splits `text` into at most two lines that fit `max_w`: breaks at the last space that fits
/// (or mid-word when there is none, as for CJK), and ellipsises the second line if needed.
pub fn wrap_two_lines(c: &mut dyn Canvas, text: &str, size: f32, weight: u16, max_w: f32) -> Vec<String> {
    if c.measure_text(text, size, weight) <= max_w {
        return vec![text.to_string()];
    }
    let cuts: Vec<usize> = text.char_indices().map(|(i, _)| i).skip(1).collect();
    let mut fit = 0; // byte offset of the longest prefix that fits
    for &i in &cuts {
        if c.measure_text(&text[..i], size, weight) <= max_w {
            fit = i;
        } else {
            break;
        }
    }
    if fit == 0 {
        return vec![fit_text(c, text, size, weight, max_w)];
    }
    let split = text[..fit].rfind(' ').filter(|&i| i > 0).unwrap_or(fit);
    let (a, b) = (text[..split].trim_end(), text[split..].trim_start());
    vec![a.to_string(), fit_text(c, b, size, weight, max_w)]
}

#[cfg(test)]
mod fit_tests {
    use super::*;


    #[test]
    fn fit_text_does_not_panic_when_last_char_is_wider_than_ellipsis() {
        struct W;
        impl Canvas for W {
            fn fill_round_rect(&mut self, _: Rect, _: f32, _: Color) {}
            fn fill_polygon(&mut self, _: &[(f32, f32)], _: Color) {}
            fn fill_circle(&mut self, _: f32, _: f32, _: f32, _: Color) {}
            fn stroke_arc(&mut self, _: f32, _: f32, _: f32, _: f32, _: f32, _: f32, _: Color) {}
            fn draw_text(&mut self, _: &str, _: Rect, _: f32, _: u16, _: Color, _: Align) {}
            fn measure_text(&mut self, t: &str, _: f32, _: u16) -> f32 {
                t.chars().map(|c| if c == '…' { 10.0 } else { 14.0 }).sum()
            }
            fn load_image(&mut self, _: &[u8], _: u32, _: u32) -> ImageId { ImageId(0) }
            fn draw_image(&mut self, _: ImageId, _: Rect, _: f32) {}
            fn push_clip(&mut self, _: Rect, _: f32) {}
            fn pop_clip(&mut self) {}
        }
        // Full text is 42 wide; "日本…" is 38, so max 40 fits the prefix but not the whole text.
        assert_eq!(fit_text(&mut W, "日本語", 14.0, 400, 40.0), "日本…");
    }

    #[test]
    fn wrap_two_lines_keeps_short_text_whole() {
        let mut c = crate::testing::RecordingCanvas::default();
        assert_eq!(wrap_two_lines(&mut c, "short", 12.0, 400, 100.0), vec!["short"]);
    }

    #[test]
    fn wrap_two_lines_breaks_at_a_space_and_ellipsises_the_rest() {
        let mut c = crate::testing::RecordingCanvas::default(); // 7 px per char
        // 70 px = 10 chars per line.
        assert_eq!(wrap_two_lines(&mut c, "aaaa bbbb cccc dddd", 12.0, 400, 70.0), vec!["aaaa bbbb", "cccc dddd"]);
        let out = wrap_two_lines(&mut c, "aaaa bbbb cccc dddd eeee ffff", 12.0, 400, 70.0);
        assert_eq!(out[0], "aaaa bbbb");
        assert!(out[1].ends_with('…') && out.len() == 2);
    }

    #[test]
    fn wrap_two_lines_splits_mid_word_when_there_is_no_space() {
        let mut c = crate::testing::RecordingCanvas::default();
        let out = wrap_two_lines(&mut c, "日本語のタイトルです", 12.0, 400, 35.0);
        assert_eq!(out.len(), 2);
        assert!(!out[0].is_empty());
    }
}
