// SPDX-License-Identifier: GPL-3.0-or-later
//! Drawing of the open panel's chrome: header, page arrows and dots.

use isle_logic::canvas::{Align, Canvas, Color, Rect};
use isle_logic::chrome::{ARROW_RADIUS, dot_center, next_center, prev_center};

const WHITE: (u8, u8, u8) = (245, 246, 248);
const DIM: (u8, u8, u8) = (139, 144, 153);
const BUTTON: (u8, u8, u8) = (43, 47, 54);
const DOT: (u8, u8, u8) = (74, 79, 88);

fn rgba(c: (u8, u8, u8), alpha: f32) -> Color {
    Color(c.0, c.1, c.2, (alpha.clamp(0.0, 1.0) * 255.0) as u8)
}

pub struct Header<'a> {
    pub title: &'a str,
    pub date: &'a str,
    pub time: &'a str,
}

/// The rect left for a page's body: below the header, above the dots, between the arrows.
pub fn page_area(area: Rect) -> Rect {
    Rect { x: area.x + 44.0, y: area.y + 36.0, w: area.w - 88.0, h: area.h - 36.0 - 26.0 }
}

pub fn draw_chrome(c: &mut dyn Canvas, area: Rect, header: &Header, pages: usize, current: usize, alpha: f32) {
    c.draw_text(header.title, Rect { x: area.x + 20.0, y: area.y + 10.0, w: 200.0, h: 20.0 }, 14.0, 650, rgba(WHITE, alpha), Align::Left);
    let right = area.x + area.w - 20.0;
    c.draw_text(header.time, Rect { x: right - 52.0, y: area.y + 10.0, w: 52.0, h: 20.0 }, 12.5, 650, rgba(WHITE, alpha), Align::Right);
    c.draw_text(header.date, Rect { x: right - 58.0 - 140.0, y: area.y + 10.0, w: 140.0, h: 20.0 }, 12.5, 400, rgba(DIM, alpha), Align::Right);

    if pages > 0 {
        for (center, glyph) in [(prev_center(area), "‹"), (next_center(area), "›")] {
            c.fill_circle(center.0, center.1, ARROW_RADIUS, rgba(BUTTON, alpha));
            let r = Rect { x: center.0 - ARROW_RADIUS, y: center.1 - ARROW_RADIUS - 1.0, w: ARROW_RADIUS * 2.0, h: ARROW_RADIUS * 2.0 };
            c.draw_text(glyph, r, 17.0, 600, rgba(WHITE, alpha), Align::Center);
        }
        for i in 0..pages {
            let (x, y) = dot_center(area, pages, i);
            if i == current {
                c.fill_round_rect(Rect { x: x - 9.0, y: y - 2.5, w: 18.0, h: 5.0 }, 2.5, rgba(WHITE, alpha));
            } else {
                c.fill_round_rect(Rect { x: x - 2.5, y: y - 2.5, w: 5.0, h: 5.0 }, 2.5, rgba(DOT, alpha));
            }
        }
    }
}
