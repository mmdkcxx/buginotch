// SPDX-License-Identifier: GPL-3.0-or-later
//! A `Canvas` that records calls and measures text at 7 px per char, for tests.

use crate::canvas::{Align, Canvas, Color, ImageId, Rect};

#[derive(Default)]
pub struct RecordingCanvas {
    pub calls: Vec<String>,
}

impl Canvas for RecordingCanvas {
    fn fill_round_rect(&mut self, r: Rect, radius: f32, _c: Color) {
        self.calls.push(format!("rrect {} {} {} {} r{}", r.x, r.y, r.w, r.h, radius));
    }
    fn fill_circle(&mut self, cx: f32, cy: f32, r: f32, _c: Color) {
        self.calls.push(format!("circle {cx} {cy} {r}"));
    }
    fn stroke_arc(&mut self, cx: f32, cy: f32, r: f32, start_deg: f32, sweep_deg: f32, _w: f32, _c: Color) {
        self.calls.push(format!("arc {cx} {cy} {r} {start_deg} {sweep_deg}"));
    }
    fn draw_text(&mut self, text: &str, _r: Rect, _size: f32, _weight: u16, _c: Color, _a: Align) {
        self.calls.push(format!("text {text}"));
    }
    fn measure_text(&mut self, text: &str, _size: f32, _weight: u16) -> f32 {
        7.0 * text.chars().count() as f32
    }
    fn draw_image(&mut self, id: ImageId, _r: Rect, _radius: f32) {
        self.calls.push(format!("image {}", id.0));
    }
    fn push_clip(&mut self, _r: Rect, _radius: f32) {
        self.calls.push("clip+".into());
    }
    fn pop_clip(&mut self) {
        self.calls.push("clip-".into());
    }
}
