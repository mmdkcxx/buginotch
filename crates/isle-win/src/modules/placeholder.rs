// SPDX-License-Identifier: GPL-3.0-or-later
//! Temporary empty pages so the paging chrome is visible; replaced by the real modules
//! in later tasks (Music in Task 11, Focus timer in Task 12).

use isle_logic::canvas::{Align, Canvas, Color, Rect};
use isle_logic::module::{Event, Module, PillItem, Reaction};

pub struct Placeholder {
    id: &'static str,
    title: &'static str,
}

impl Placeholder {
    pub fn new(id: &'static str, title: &'static str) -> Self {
        Self { id, title }
    }
}

impl Module for Placeholder {
    fn id(&self) -> &'static str {
        self.id
    }
    fn title(&self) -> &'static str {
        self.title
    }
    fn pill_left(&self) -> Option<PillItem> {
        None
    }
    fn draw_panel(&mut self, c: &mut dyn Canvas, area: Rect, _now_ms: i64) {
        c.draw_text("Segera hadir", area, 13.0, 400, Color(139, 144, 153, 255), Align::Center);
    }
    fn on_event(&mut self, _e: &Event, _area: Rect, _now_ms: i64) -> Reaction {
        Reaction::None
    }
    fn next_wake_ms(&self, _now_ms: i64) -> Option<i64> {
        None
    }
}
