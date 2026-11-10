// SPDX-License-Identifier: GPL-3.0-or-later
//! Glass mode colour rules. The pages were drawn for a solid black notch; on see-through glass
//! their dark tracks and chips, grey text and white text need other colours. The canvas applies
//! these to every colour it is given while glass is on, so pages need no glass code of their own.

use crate::canvas::Color;

fn spread(c: Color) -> (u8, u8) {
    (c.0.max(c.1).max(c.2), c.0.min(c.1).min(c.2))
}

/// Shapes (rounded rectangles, circles, arcs). Dark, nearly grey colours become translucent
/// light ones on dark glass, and faint dark tints on light glass. Pure black, saturated colours
/// and anything bright are left as they are.
pub fn fill(c: Color, light: bool) -> Color {
    let (mx, mn) = spread(c);
    if mx == 0 || mx > 90 || mx - mn > 24 {
        return c;
    }
    let keep = c.3 as f32 / 255.0;
    if light {
        let alpha = if mx <= 22 { 0.16 } else { 0.07 + mx as f32 / 90.0 * 0.07 };
        return Color(20, 22, 26, (alpha * 255.0 * keep) as u8);
    }
    // The very darkest fills are wells (empty input cells): they stay dark so they read as recesses.
    if mx <= 22 {
        return Color(0, 0, 0, (70.0 * keep) as u8);
    }
    Color(255, 255, 255, ((0.10 + mx as f32 / 90.0 * 0.12) * 255.0 * keep) as u8)
}

/// `fill`, plus: on light glass, small white shapes (the active page dot, a progress knob) turn
/// dark so they stay visible. Big white shapes (buttons, QR codes) are left alone.
pub fn fill_sized(c: Color, light: bool, smallest_side: f32) -> Color {
    let (mx, mn) = spread(c);
    if light && mn >= 235 && mx - mn <= 24 && smallest_side <= 8.0 {
        return Color(24, 26, 32, c.3);
    }
    fill(c, light)
}

/// Text (and display text). Dark glass: mid grey is lifted toward white. Light glass: white text
/// turns to dark ink, grey to a softer dark grey, and bright accents are darkened.
pub fn text(c: Color, light: bool) -> Color {
    let (mx, mn) = spread(c);
    if light {
        let a = c.3;
        if mx - mn <= 24 {
            return match mx {
                200..=255 => Color(20, 22, 26, a),
                85..=199 => Color(75, 81, 92, a),
                _ => c,
            };
        }
        if mx > 140 {
            let k = |v: u8| (v as f32 * 0.62) as u8;
            return Color(k(c.0), k(c.1), k(c.2), a);
        }
        return c;
    }
    if !(85..=200).contains(&mx) || mx - mn > 24 {
        return c;
    }
    let alpha = if mx < 115 { 150.0 } else { 185.0 };
    Color(244, 245, 247, (alpha * c.3 as f32 / 255.0) as u8)
}

/// Icon shapes (polygons): on light glass white ones turn to dark ink; otherwise unchanged.
pub fn polygon(c: Color, light: bool) -> Color {
    let (mx, mn) = spread(c);
    if light && mn >= 200 && mx - mn <= 24 {
        return Color(20, 22, 26, c.3);
    }
    c
}

#[cfg(test)]
mod tests {
    use super::*;

    const WHITE: Color = Color(245, 246, 248, 255);
    const DIM: Color = Color(139, 144, 153, 255);
    const TRACK: Color = Color(43, 47, 54, 255);

    fn luma(c: Color) -> u32 {
        (c.0 as u32 * 3 + c.1 as u32 * 6 + c.2 as u32) / 10
    }

    #[test]
    fn white_text_is_dark_on_light_glass_and_unchanged_on_dark() {
        assert!(luma(text(WHITE, true)) < 60);
        assert_eq!(text(WHITE, false), WHITE);
    }

    #[test]
    fn dim_text_is_lifted_on_dark_glass_and_darkened_on_light() {
        assert!(luma(text(DIM, false)) > luma(DIM));
        assert!(luma(text(DIM, true)) < 110);
    }

    #[test]
    fn display_text_keeps_its_fade() {
        assert_eq!(text(Color(245, 246, 248, 100), true).3, 100);
    }

    #[test]
    fn bright_accents_are_darkened_only_on_light_glass() {
        let green = Color(52, 211, 153, 255);
        assert_eq!(text(green, false), green);
        assert!(text(green, true).1 < green.1);
    }

    #[test]
    fn tracks_become_translucent_not_solid() {
        let dark = fill(TRACK, false);
        assert_eq!((dark.0, dark.1, dark.2), (255, 255, 255));
        assert!(dark.3 < 80);
        let light = fill(TRACK, true);
        assert_eq!((light.0, light.1, light.2), (20, 22, 26));
        assert!(light.3 < 60);
    }

    #[test]
    fn black_and_big_white_shapes_stay() {
        assert_eq!(fill(Color(0, 0, 0, 255), true), Color(0, 0, 0, 255));
        assert_eq!(fill_sized(WHITE, true, 34.0), WHITE); // play button, QR background
        assert_eq!(fill_sized(WHITE, true, 5.0), Color(24, 26, 32, 255)); // active page dot
    }

    #[test]
    fn white_icons_turn_dark_on_light_glass_only() {
        assert_eq!(polygon(WHITE, false), WHITE);
        assert_eq!(polygon(WHITE, true), Color(20, 22, 26, 255));
        assert_eq!(polygon(Color(0, 0, 0, 255), true), Color(0, 0, 0, 255));
    }
}
