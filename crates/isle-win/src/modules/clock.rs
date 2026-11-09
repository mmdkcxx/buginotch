// SPDX-License-Identifier: GPL-3.0-or-later
//! The clock: supplies the closed pill's centre text. It has no page of its own.

use crate::system::{epoch_ms, local_time};
use isle_logic::canvas::{Canvas, Rect};
use isle_logic::module::{Event, Module, PillItem, Reaction};
use isle_logic::timefmt::format_clock;

pub struct ClockModule;

impl Module for ClockModule {
    fn id(&self) -> &'static str {
        "clock"
    }
    fn title(&self) -> &'static str {
        "Jam"
    }
    fn pill_left(&self) -> Option<PillItem> {
        None
    }
    fn pill_center(&self) -> Option<String> {
        let t = local_time();
        Some(format_clock(t.hour, t.minute))
    }
    fn has_page(&self) -> bool {
        false
    }
    fn draw_panel(&mut self, _c: &mut dyn Canvas, _area: Rect, _now_ms: i64) {}
    fn on_event(&mut self, _e: &Event, _area: Rect, _now_ms: i64) -> Reaction {
        Reaction::None
    }
    /// Wakes at the next minute boundary.
    fn next_wake_ms(&self, _now_ms: i64) -> Option<i64> {
        let t = local_time();
        Some(epoch_ms() + (60 - t.second as i64) * 1000)
    }
}
