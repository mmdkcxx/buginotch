// SPDX-License-Identifier: GPL-3.0-or-later
//! An animated pill button for pages: it dips and brightens when pressed, its fill glides
//! between off and on, and a changed label slides and cross-fades.

use isle_logic::canvas::{Align, Canvas, Color, Rect};

const WHITE: Color = Color(245, 246, 248, 255);
const OFF: Color = Color(28, 31, 36, 255);
const ON_TEXT: Color = Color(4, 19, 13, 255);
const PRESS_MS: f32 = 260.0;
const FILL_MS: f32 = 240.0;
const LABEL_MS: f32 = 200.0;

fn ease(t: f32) -> f32 {
    1.0 - (1.0 - t.clamp(0.0, 1.0)).powi(3)
}

fn mix(a: Color, b: Color, k: f32) -> Color {
    let f = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * k).round() as u8;
    Color(f(a.0, b.0), f(a.1, b.1), f(a.2, b.2), 255)
}

pub struct Button {
    pressed_at: i64,
    on: bool,
    /// Fill amount (0 off .. 1 on) when the last change began, and when.
    fill_from: f32,
    fill_at: i64,
    label: String,
    prev_label: String,
    label_at: i64,
    /// Colour of the "on" fill.
    on_color: Color,
}

impl Button {
    pub fn new(on_color: Color) -> Self {
        Self { pressed_at: i64::MIN / 2, on: false, fill_from: 0.0, fill_at: i64::MIN / 2, label: String::new(), prev_label: String::new(), label_at: i64::MIN / 2, on_color }
    }

    pub fn press(&mut self, now_ms: i64) {
        self.pressed_at = now_ms;
    }

    fn fill(&self, now_ms: i64) -> f32 {
        let target = f32::from(self.on);
        self.fill_from + (target - self.fill_from) * ease((now_ms - self.fill_at) as f32 / FILL_MS)
    }

    /// True while any of its animations is still moving.
    pub fn busy(&self, now_ms: i64) -> bool {
        let since = |t: i64| (now_ms - t) as f32;
        since(self.pressed_at) < PRESS_MS || since(self.fill_at) < FILL_MS || since(self.label_at) < LABEL_MS
    }

    pub fn draw(&mut self, c: &mut dyn Canvas, r: Rect, label: &str, on: bool, now_ms: i64) {
        if self.label.is_empty() {
            self.label = label.to_string();
        }
        if on != self.on {
            self.fill_from = self.fill(now_ms);
            self.fill_at = now_ms;
            self.on = on;
        }
        if label != self.label {
            self.prev_label = std::mem::replace(&mut self.label, label.to_string());
            self.label_at = now_ms;
        }
        let k = self.fill(now_ms);
        let t = (now_ms - self.pressed_at) as f32 / PRESS_MS;
        let pulse = if (0.0..1.0).contains(&t) { (std::f32::consts::PI * t).sin() } else { 0.0 };
        let d = 3.0 * pulse;
        let rect = Rect { x: r.x + d, y: r.y + d * 0.5, w: r.w - 2.0 * d, h: r.h - d };
        let fill = mix(mix(OFF, self.on_color, k), WHITE, 0.16 * pulse);
        c.fill_round_rect(rect, rect.h / 2.0, fill);
        let text = mix(WHITE, ON_TEXT, k);
        let lt = ease((now_ms - self.label_at) as f32 / LABEL_MS);
        if lt < 1.0 {
            let fade = |col: Color, a: f32| Color(col.0, col.1, col.2, (a.clamp(0.0, 1.0) * 255.0) as u8);
            c.draw_text(&self.prev_label, Rect { y: rect.y - 5.0 * lt, ..rect }, 12.5, 650, fade(text, 1.0 - lt), Align::Center);
            c.draw_text(&self.label, Rect { y: rect.y + 5.0 * (1.0 - lt), ..rect }, 12.5, 650, fade(text, lt), Align::Center);
        } else {
            c.draw_text(&self.label, rect, 12.5, 650, text, Align::Center);
        }
    }
}
