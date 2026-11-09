// SPDX-License-Identifier: GPL-3.0-or-later
//! The feature-module trait and the registry that pages through modules.

use crate::canvas::{Canvas, ImageId, Rect};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    Char(char),
    Backspace,
    Enter,
    Escape,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    PointerMove(f32, f32),
    PointerDown(f32, f32),
    /// Mouse moved with the left button held (after a `PointerDown` on this notch).
    PointerDrag(f32, f32),
    PointerUp(f32, f32),
    PointerWheel(f32),
    Key(Key),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PillItem {
    MiniCover(ImageId),
    Equaliser { active: bool },
}

/// A short message shown in the closed pill (e.g. "⚡ 82%").
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeekRequest {
    pub text: String,
    pub duration_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reaction {
    None,
    Redraw,
    Peek(PeekRequest),
}

/// What the closed pill shows while it is grown into a "new song" card.
#[derive(Debug, Clone, PartialEq)]
pub struct PillCard {
    pub title: String,
    pub sub: String,
    pub cover: Option<ImageId>,
    /// Playback position as a fraction 0..=1.
    pub progress: f32,
    pub playing: bool,
}

/// A battery readout shown next to the pill's centre text, animating in from `since_ms`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PillBattery {
    pub percent: u8,
    pub since_ms: i64,
}

/// One feature: what it shows in the closed pill and its page in the open panel.
pub trait Module {
    fn id(&self) -> &'static str;
    fn title(&self) -> &'static str;
    fn pill_left(&self) -> Option<PillItem>;
    /// Everything this module shows at the left of the closed pill.
    fn pill_items(&self) -> Vec<PillItem> {
        self.pill_left().into_iter().collect()
    }
    fn pill_center(&self) -> Option<String> {
        None
    }
    /// A card the pill should grow into right now, if any (a song just changed).
    fn pill_card(&self, _now_ms: i64) -> Option<PillCard> {
        None
    }
    /// A battery icon and percentage to show after the centre text.
    fn pill_battery(&self, _now_ms: i64) -> Option<PillBattery> {
        None
    }
    fn pill_priority(&self) -> u8 {
        0
    }
    /// Draw the pill's centre text in the accent colour (running timer, charging).
    fn pill_accent(&self) -> bool {
        false
    }
    /// False for modules that only contribute to the pill (the clock).
    fn has_page(&self) -> bool {
        true
    }
    fn draw_panel(&mut self, c: &mut dyn Canvas, area: Rect, now_ms: i64);
    fn on_event(&mut self, e: &Event, area: Rect, now_ms: i64) -> Reaction;
    /// Next epoch-ms at which this module needs a redraw; `None` means never (idle).
    fn next_wake_ms(&self, now_ms: i64) -> Option<i64>;
    fn wants_wide_panel(&self) -> bool {
        false
    }
    /// True when the closed pill should be wide (music, a peek, ...).
    fn wide_pill(&self) -> bool {
        false
    }
    /// Called once a second and on wake-ups; returns whether anything visible changed.
    fn poll(&mut self, _now_ms: i64) -> bool {
        false
    }
    /// Called before each frame, to upload images and other resources.
    fn prepare(&mut self, _c: &mut dyn Canvas) {}
}

#[derive(Default)]
pub struct Registry {
    modules: Vec<Box<dyn Module>>,
    current: usize,
}

impl Registry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add(&mut self, m: Box<dyn Module>) {
        self.modules.push(m);
    }

    /// Inserts a module at `index` in the module order (page order follows it).
    pub fn insert_page(&mut self, index: usize, m: Box<dyn Module>) {
        let index = index.min(self.modules.len());
        self.modules.insert(index, m);
    }

    fn page_indices(&self) -> Vec<usize> {
        (0..self.modules.len()).filter(|&i| self.modules[i].has_page()).collect()
    }

    pub fn page_count(&self) -> usize {
        self.modules.iter().filter(|m| m.has_page()).count()
    }

    pub fn current(&self) -> usize {
        self.current
    }

    /// Moves `delta` pages, wrapping around.
    pub fn go(&mut self, delta: i32) {
        let n = self.page_count() as i32;
        if n > 0 {
            self.current = (self.current as i32 + delta).rem_euclid(n) as usize;
        }
    }

    pub fn set_page(&mut self, index: usize) {
        if index < self.page_count() {
            self.current = index;
        }
    }

    pub fn current_page_id(&self) -> Option<&'static str> {
        self.current_page().map(|m| m.id())
    }

    pub fn current_page(&self) -> Option<&dyn Module> {
        let i = *self.page_indices().get(self.current)?;
        Some(self.modules[i].as_ref())
    }

    pub fn current_page_mut(&mut self) -> Option<&mut (dyn Module + 'static)> {
        let i = *self.page_indices().get(self.current)?;
        Some(self.modules[i].as_mut())
    }

    /// The page at position `i` (not necessarily the current one).
    pub fn page_at_mut(&mut self, i: usize) -> Option<&mut (dyn Module + 'static)> {
        let m = *self.page_indices().get(i)?;
        Some(self.modules[m].as_mut())
    }

    pub fn modules_mut(&mut self) -> &mut [Box<dyn Module>] {
        &mut self.modules
    }

    /// Text for the closed pill's centre and whether it is accented: the highest-priority
    /// module's, else `fallback`.
    pub fn pill_text(&self, fallback: &str) -> (String, bool) {
        let mut best: Option<(u8, String, bool)> = None;
        for m in &self.modules {
            if let Some(t) = m.pill_center() {
                let p = m.pill_priority();
                if best.as_ref().is_none_or(|(bp, _, _)| p > *bp) {
                    best = Some((p, t, m.pill_accent()));
                }
            }
        }
        best.map(|(_, t, a)| (t, a)).unwrap_or_else(|| (fallback.to_string(), false))
    }

    pub fn pill_center(&self, fallback: &str) -> String {
        self.pill_text(fallback).0
    }

    pub fn poll_all(&mut self, now_ms: i64) -> bool {
        let mut changed = false;
        for m in &mut self.modules {
            changed |= m.poll(now_ms);
        }
        changed
    }

    pub fn prepare_all(&mut self, c: &mut dyn Canvas) {
        for m in &mut self.modules {
            m.prepare(c);
        }
    }

    pub fn pill_card(&self, now_ms: i64) -> Option<PillCard> {
        self.modules.iter().find_map(|m| m.pill_card(now_ms))
    }

    pub fn pill_battery(&self, now_ms: i64) -> Option<PillBattery> {
        self.modules.iter().find_map(|m| m.pill_battery(now_ms))
    }

    pub fn any_wide_pill(&self) -> bool {
        self.modules.iter().any(|m| m.wide_pill())
    }

    /// All left-slot items, in module order.
    pub fn pill_items(&self) -> Vec<PillItem> {
        self.modules.iter().flat_map(|m| m.pill_items()).collect()
    }

    pub fn earliest_wake(&self, now_ms: i64) -> Option<i64> {
        self.modules.iter().filter_map(|m| m.next_wake_ms(now_ms)).min()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::canvas::{Canvas, Rect, fit_text};
    use crate::testing::RecordingCanvas;

    struct Fake {
        id: &'static str,
        center: Option<String>,
        priority: u8,
        wake: Option<i64>,
        page: bool,
    }
    impl Fake {
        fn new(id: &'static str) -> Self {
            Self { id, center: None, priority: 0, wake: None, page: true }
        }
    }
    impl Module for Fake {
        fn id(&self) -> &'static str { self.id }
        fn title(&self) -> &'static str { self.id }
        fn pill_left(&self) -> Option<PillItem> { None }
        fn pill_center(&self) -> Option<String> { self.center.clone() }
        fn pill_priority(&self) -> u8 { self.priority }
        fn has_page(&self) -> bool { self.page }
        fn draw_panel(&mut self, _c: &mut dyn Canvas, _a: Rect, _now: i64) {}
        fn on_event(&mut self, _e: &Event, _a: Rect, _now: i64) -> Reaction { Reaction::None }
        fn next_wake_ms(&self, _now: i64) -> Option<i64> { self.wake }
    }

    #[test]
    fn fit_text_returns_whole_when_fits() {
        let mut c = RecordingCanvas::default();
        assert_eq!(fit_text(&mut c, "Hello", 12.0, 400, 100.0), "Hello");
    }

    #[test]
    fn fit_text_truncates_with_ellipsis() {
        let mut c = RecordingCanvas::default();
        let out = fit_text(&mut c, "A very long song title indeed", 12.0, 400, 70.0);
        assert!(out.ends_with('…'));
        assert!(c.measure_text(&out, 12.0, 400) <= 70.0);
        assert!(out.len() > 3);
    }

    #[test]
    fn fit_text_handles_cjk_and_emoji() {
        let mut c = RecordingCanvas::default();
        let out = fit_text(&mut c, "日本語のタイトル🎵🎵🎵", 12.0, 400, 35.0);
        assert!(out.ends_with('…'));
        assert!(c.measure_text(&out, 12.0, 400) <= 35.0);
    }

    #[test]
    fn fit_text_empty() {
        let mut c = RecordingCanvas::default();
        assert_eq!(fit_text(&mut c, "", 12.0, 400, 50.0), "");
        assert_eq!(fit_text(&mut c, "abc", 12.0, 400, 0.0), "");
    }

    #[test]
    fn registry_go_wraps() {
        let mut r = Registry::new();
        for id in ["a", "b", "c"] { r.add(Box::new(Fake::new(id))); }
        assert_eq!(r.page_count(), 3);
        r.go(-1);
        assert_eq!(r.current_page_id(), Some("c"));
        r.go(1);
        assert_eq!(r.current_page_id(), Some("a"));
    }

    #[test]
    fn registry_skips_modules_without_a_page() {
        let mut r = Registry::new();
        let mut clock = Fake::new("clock");
        clock.page = false;
        r.add(Box::new(clock));
        r.add(Box::new(Fake::new("music")));
        assert_eq!(r.page_count(), 1);
        assert_eq!(r.current_page_id(), Some("music"));
    }

    #[test]
    fn registry_priority_picks_highest_center_text() {
        let mut r = Registry::new();
        let mut a = Fake::new("a");
        a.center = Some("low".into());
        a.priority = 1;
        let mut b = Fake::new("b");
        b.center = Some("24:59".into());
        b.priority = 10;
        r.add(Box::new(a));
        r.add(Box::new(b));
        assert_eq!(r.pill_center("10:42"), "24:59");
        assert_eq!(Registry::new().pill_center("10:42"), "10:42");
    }

    #[test]
    fn earliest_wake_is_minimum() {
        let mut r = Registry::new();
        let mut a = Fake::new("a");
        a.wake = Some(500);
        let mut b = Fake::new("b");
        b.wake = Some(200);
        r.add(Box::new(a));
        r.add(Box::new(b));
        r.add(Box::new(Fake::new("c")));
        assert_eq!(r.earliest_wake(0), Some(200));
        assert_eq!(Registry::new().earliest_wake(0), None);
    }
}
