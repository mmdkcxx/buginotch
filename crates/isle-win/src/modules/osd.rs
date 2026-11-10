// SPDX-License-Identifier: GPL-3.0-or-later
//! Volume and brightness OSD: the closed pill grows a bar when either changes.

use crate::osd_win::OsdWatcher;
use isle_logic::canvas::{Canvas, Rect};
use isle_logic::module::{Event, Module, PillItem, Reaction};
use isle_logic::osd::{Osd, PillOsd};

pub struct OsdModule {
    watcher: OsdWatcher,
    osd: Osd,
    showing: bool,
}

impl OsdModule {
    pub fn new(watcher: OsdWatcher) -> Self {
        Self { watcher, osd: Osd::default(), showing: false }
    }
}

impl Module for OsdModule {
    fn id(&self) -> &'static str {
        "osd"
    }
    fn title(&self) -> &'static str {
        "OSD"
    }
    fn pill_left(&self) -> Option<PillItem> {
        None
    }
    fn pill_osd(&self, now_ms: i64) -> Option<PillOsd> {
        self.osd.view(now_ms)
    }
    fn pill_priority(&self) -> u8 {
        30
    }
    fn has_page(&self) -> bool {
        false
    }
    fn poll(&mut self, now_ms: i64) -> bool {
        self.osd.observe(self.watcher.latest(), now_ms);
        let active = self.osd.view(now_ms).is_some();
        let changed = active || active != self.showing;
        self.showing = active;
        changed
    }
    fn draw_panel(&mut self, _c: &mut dyn Canvas, _area: Rect, _now_ms: i64) {}
    fn on_event(&mut self, _e: &Event, _area: Rect, _now_ms: i64) -> Reaction {
        Reaction::None
    }
    fn next_wake_ms(&self, now_ms: i64) -> Option<i64> {
        self.osd.next_wake_ms(now_ms)
    }
}
