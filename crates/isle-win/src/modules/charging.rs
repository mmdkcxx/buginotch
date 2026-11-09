// SPDX-License-Identifier: GPL-3.0-or-later
//! Charging peek: when the charger is plugged in, the pill shows "⚡ 82%" for ~3 s.

use crate::power;
use crate::system::epoch_ms;
use isle_logic::canvas::{Canvas, Rect};
use isle_logic::module::{Event, Module, PillItem, Reaction};
use isle_logic::peek::{Peek, PowerState, charge_event};

const PEEK_MS: i64 = 3200;

pub struct ChargingModule {
    prev: Option<PowerState>,
    peek: Peek,
    showing: bool,
}

impl ChargingModule {
    pub fn new() -> Self {
        Self { prev: None, peek: Peek::default(), showing: false }
    }

    fn active(&self) -> bool {
        self.peek.current(epoch_ms()).is_some()
    }
}

impl Module for ChargingModule {
    fn id(&self) -> &'static str {
        "charging"
    }
    fn title(&self) -> &'static str {
        "Charging"
    }
    fn pill_left(&self) -> Option<PillItem> {
        None
    }
    fn pill_center(&self) -> Option<String> {
        self.peek.current(epoch_ms()).map(str::to_string)
    }
    fn pill_priority(&self) -> u8 {
        20
    }
    fn pill_accent(&self) -> bool {
        true
    }
    fn has_page(&self) -> bool {
        false
    }
    fn wide_pill(&self) -> bool {
        self.active()
    }
    fn poll(&mut self, now_ms: i64) -> bool {
        let now = power::read();
        if let Some(text) = charge_event(self.prev, now) {
            self.peek.show(text, now_ms, PEEK_MS);
        }
        self.prev = Some(now);
        let active = self.peek.current(now_ms).is_some();
        let changed = active != self.showing || (active && self.peek.next_wake_ms().is_some());
        self.showing = active;
        changed
    }
    fn draw_panel(&mut self, _c: &mut dyn Canvas, _area: Rect, _now_ms: i64) {}
    fn on_event(&mut self, _e: &Event, _area: Rect, _now_ms: i64) -> Reaction {
        Reaction::None
    }
    fn next_wake_ms(&self, now_ms: i64) -> Option<i64> {
        self.peek.next_wake_ms().filter(|&t| t > now_ms)
    }
}
