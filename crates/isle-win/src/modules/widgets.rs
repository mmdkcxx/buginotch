// SPDX-License-Identifier: GPL-3.0-or-later
//! Widgets page: a world clock (cities from `settings.toml`) and the battery.

use crate::power;
use crate::system::{Zone, local_time};
use isle_logic::canvas::{Align, Canvas, Color, Rect};
use isle_logic::module::{Event, Module, PillItem, Reaction};
use isle_logic::settings::WorldClock;
use isle_logic::timefmt::format_clock;
use isle_logic::widgets::{day_tag, format_remaining};

const WHITE: Color = Color(245, 246, 248, 255);
const DIM: Color = Color(139, 144, 153, 255);
const CARD: Color = Color(20, 22, 26, 255);
const TRACK: Color = Color(43, 47, 54, 255);
const GREEN: Color = Color(52, 211, 153, 255);
const AMBER: Color = Color(251, 191, 36, 255);
const RED: Color = Color(248, 113, 113, 255);
const GAP: f32 = 10.0;
const PAD: f32 = 12.0;
const MAX_CITIES: usize = 3;

struct City {
    label: String,
    zone: Option<Zone>,
    /// "19:29" and "Besok", refreshed by `poll`.
    time: String,
    tag: String,
}

pub struct WidgetsModule {
    cities: Vec<City>,
    battery: (bool, Option<u8>, Option<String>),
}

impl WidgetsModule {
    pub fn new(clocks: &[WorldClock]) -> Self {
        let cities = clocks
            .iter()
            .take(MAX_CITIES)
            .map(|c| City { label: c.label.clone(), zone: Zone::load(&c.zone), time: String::new(), tag: String::new() })
            .collect();
        let mut m = Self { cities, battery: (false, None, None) };
        m.refresh();
        m
    }

    /// Re-reads the clocks and the battery; returns whether anything shown changed.
    fn refresh(&mut self) -> bool {
        let mut changed = false;
        let t = local_time();
        for c in &mut self.cities {
            let (time, tag) = match c.zone.as_ref().and_then(|z| z.now()) {
                Some(z) => (format_clock(z.hour, z.minute), day_tag((t.year, t.month, t.day), (z.year, z.month, z.day))),
                None => ("--:--".to_string(), String::new()),
            };
            changed |= time != c.time || tag != c.tag;
            c.time = time;
            c.tag = tag;
        }
        let d = power::detail();
        let b = (d.on_ac, d.percent, if d.on_ac { None } else { format_remaining(d.remaining_secs) });
        changed |= b != self.battery;
        self.battery = b;
        changed
    }
}

impl Module for WidgetsModule {
    fn id(&self) -> &'static str {
        "widgets"
    }
    fn title(&self) -> &'static str {
        "Widget"
    }
    fn pill_left(&self) -> Option<PillItem> {
        None
    }

    fn poll(&mut self, _now_ms: i64) -> bool {
        self.refresh()
    }

    fn draw_panel(&mut self, c: &mut dyn Canvas, area: Rect, _now_ms: i64) {
        let w = (area.w - GAP) / 2.0;
        let left = Rect { x: area.x, y: area.y, w, h: area.h };
        let right = Rect { x: area.x + w + GAP, y: area.y, w, h: area.h };

        // World clock.
        c.fill_round_rect(left, 13.0, CARD);
        let inner = Rect { x: left.x + PAD, y: left.y + 9.0, w: left.w - PAD * 2.0, h: 14.0 };
        c.draw_text("Jam dunia", inner, 11.0, 400, DIM, Align::Left);
        for (i, city) in self.cities.iter().enumerate() {
            let y = left.y + 28.0 + i as f32 * 30.0;
            let row = |x: f32, w: f32| Rect { x, y, w, h: 24.0 };
            c.draw_text(&city.label, row(inner.x, 84.0), 13.0, 600, WHITE, Align::Left);
            c.draw_text(&city.tag, row(inner.x + 78.0, 56.0), 10.5, 400, DIM, Align::Right);
            c.draw_text(&city.time, row(inner.x + inner.w - 54.0, 54.0), 15.0, 700, WHITE, Align::Right);
        }

        // Battery.
        c.fill_round_rect(right, 13.0, CARD);
        let inner = Rect { x: right.x + PAD, y: right.y + 9.0, w: right.w - PAD * 2.0, h: 14.0 };
        c.draw_text("Baterai", inner, 11.0, 400, DIM, Align::Left);
        match self.battery.1 {
            Some(p) => {
                let color = if self.battery.0 || p > 40 { GREEN } else if p > 20 { AMBER } else { RED };
                c.draw_text(&format!("{p}%"), Rect { x: inner.x, y: right.y + 30.0, w: inner.w, h: 36.0 }, 30.0, 700, WHITE, Align::Left);
                let bar = Rect { x: inner.x, y: right.y + 78.0, w: inner.w, h: 6.0 };
                c.fill_round_rect(bar, 3.0, TRACK);
                c.fill_round_rect(Rect { w: (bar.w * p as f32 / 100.0).max(6.0), ..bar }, 3.0, color);
                let status = if self.battery.0 {
                    "Terhubung ke listrik".to_string()
                } else {
                    match &self.battery.2 {
                        Some(t) => format!("Sisa {t}"),
                        None => "Memakai baterai".to_string(),
                    }
                };
                c.draw_text(&status, Rect { x: inner.x, y: right.y + 92.0, w: inner.w, h: 16.0 }, 11.0, 400, DIM, Align::Left);
            }
            None => {
                c.draw_text("Tidak ada baterai", Rect { x: inner.x, y: right.y + 30.0, w: inner.w, h: 40.0 }, 13.0, 600, WHITE, Align::Left);
                c.draw_text("PC ini memakai listrik langsung", Rect { x: inner.x, y: right.y + 92.0, w: inner.w, h: 16.0 }, 11.0, 400, DIM, Align::Left);
            }
        }
    }

    fn on_event(&mut self, _e: &Event, _area: Rect, _now_ms: i64) -> Reaction {
        Reaction::None
    }

    fn next_wake_ms(&self, now_ms: i64) -> Option<i64> {
        Some((now_ms / 60_000 + 1) * 60_000)
    }
}
