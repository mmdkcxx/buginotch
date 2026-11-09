// SPDX-License-Identifier: GPL-3.0-or-later
//! Headphones peek: when headphones or a headset connect or disconnect, the closed pill
//! briefly shows "🎧 name" or "✕ name".

use crate::headphones_win::HeadphoneWatcher;
use crate::system::epoch_ms;
use isle_logic::canvas::{Canvas, Rect};
use isle_logic::headphones::peek_text;
use isle_logic::module::{Event, Module, PillItem, Reaction};
use isle_logic::peek::Peek;

const PEEK_MS: i64 = 3200;

pub struct HeadphonesModule {
    watcher: HeadphoneWatcher,
    peek: Peek,
    showing: bool,
}

impl HeadphonesModule {
    pub fn new(watcher: HeadphoneWatcher) -> Self {
        Self { watcher, peek: Peek::default(), showing: false }
    }

    fn active(&self) -> bool {
        self.peek.current(epoch_ms()).is_some()
    }
}

impl Module for HeadphonesModule {
    fn id(&self) -> &'static str {
        "headphones"
    }
    fn title(&self) -> &'static str {
        "Headphones"
    }
    fn pill_left(&self) -> Option<PillItem> {
        None
    }
    fn pill_center(&self) -> Option<String> {
        self.peek.current(epoch_ms()).map(str::to_string)
    }
    fn pill_priority(&self) -> u8 {
        25
    }
    fn has_page(&self) -> bool {
        false
    }
    fn wide_pill(&self) -> bool {
        self.active()
    }
    fn poll(&mut self, now_ms: i64) -> bool {
        // Several changes at once: the last one wins.
        if let Some(change) = self.watcher.take().last() {
            self.peek.show(peek_text(change), now_ms, PEEK_MS);
        }
        let active = self.peek.current(now_ms).is_some();
        let changed = active != self.showing;
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
