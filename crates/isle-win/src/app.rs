// SPDX-License-Identifier: GPL-3.0-or-later
//! Notch state: hover, open/close, and the springs that animate its size.

use crate::modules::{clock::ClockModule, placeholder::Placeholder};
use crate::system::{epoch_ms, local_time};
use crate::ui::{self, Header};
use isle_logic::canvas::{Align, Canvas, Color, Rect};
use isle_logic::chrome::{ChromeAction, chrome_hit};
use isle_logic::layout::{Hover, target};
use isle_logic::timefmt::{format_clock, format_date};
use isle_logic::module::Registry;
use isle_logic::settings::Settings;
use isle_logic::spring::Spring;
use std::time::Instant;

pub struct Step {
    /// Set when this step switched the notch to open.
    pub opened: bool,
    /// The notch finished closing and the window can shrink back to the pill.
    pub fully_closed: bool,
    pub keep_ticking: bool,
}

pub struct App {
    pub registry: Registry,
    pub settings: Settings,
    pub wide: bool,
    pub lyrics: bool,
    w: Spring,
    h: Spring,
    r: Spring,
    hover: Hover,
    inside: bool,
    open: bool,
    /// The window is currently as large as the open panel.
    pub window_big: bool,
    start: Instant,
    last: Option<Instant>,
    last_pill: String,
}

impl App {
    pub fn new(settings: Settings) -> Self {
        let (w, h, r) = closed_dims(&settings, false);
        let mut registry = Registry::new();
        registry.add(Box::new(ClockModule));
        registry.add(Box::new(Placeholder::new("music", "Musik")));
        registry.add(Box::new(Placeholder::new("timer", "Focus timer")));
        App {
            registry,
            settings,
            wide: false,
            lyrics: false,
            w: Spring::new(w, 300.0, 22.0),
            h: Spring::new(h, 300.0, 22.0),
            r: Spring::new(r, 260.0, 20.0),
            hover: Hover::new(),
            inside: false,
            open: false,
            window_big: false,
            start: Instant::now(),
            last: None,
            last_pill: String::new(),
        }
    }

    pub fn now_ms(&self) -> i64 {
        self.start.elapsed().as_millis() as i64
    }

    pub fn pointer(&mut self, inside: bool) {
        self.inside = inside;
        let now = self.now_ms();
        self.hover.pointer_inside(inside, now);
    }

    /// Current animated shape in DIPs: width, height, bottom-corner radius.
    pub fn dims(&self) -> (f32, f32, f32) {
        (self.w.value, self.h.value, self.r.value.max(0.0))
    }

    /// Window size (DIPs) for the open or closed state.
    pub fn window_dims(&self, big: bool) -> (f32, f32) {
        if big {
            let t = target(true, self.wide, self.lyrics).size;
            (t.w, t.h)
        } else {
            let (w, h, _) = closed_dims(&self.settings, self.wide);
            (w, h)
        }
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    /// Called once a second; returns whether anything visible changed.
    pub fn on_second(&mut self) -> bool {
        let text = self.registry.pill_center("");
        let changed = text != self.last_pill || self.open;
        self.last_pill = text;
        changed
    }

    /// Handles a left click; coordinates are DIPs relative to the notch's top-left.
    /// Returns whether a redraw is needed.
    pub fn click(&mut self, x: f32, y: f32) -> bool {
        if !self.open {
            return false;
        }
        let (w, h, _) = self.dims();
        let area = Rect { x: 0.0, y: 0.0, w, h };
        match chrome_hit(area, x, y, self.registry.page_count()) {
            Some(ChromeAction::Prev) => self.registry.go(-1),
            Some(ChromeAction::Next) => self.registry.go(1),
            Some(ChromeAction::Dot(i)) => self.registry.set_page(i),
            None => return false,
        }
        true
    }

    /// Draws the notch shape and its content. `win_w` is the window width in DIPs.
    pub fn paint(&mut self, c: &mut dyn Canvas, win_w: f32) {
        let (w, h, r) = self.dims();
        let x = ((win_w - w) / 2.0).max(0.0);
        c.fill_round_rect(Rect { x, y: -r, w, h: h + r }, r, Color(0, 0, 0, 255));

        let (closed_w, _, _) = closed_dims(&self.settings, self.wide);
        let open_w = target(true, self.wide, self.lyrics).size.w;
        let p = ((w - closed_w) / (open_w - closed_w)).clamp(0.0, 1.0);
        let t = local_time();
        let clock = format_clock(t.hour, t.minute);

        if p < 0.98 {
            let text = self.registry.pill_center(&clock);
            let a = ((1.0 - p * 1.6).clamp(0.0, 1.0) * 255.0) as u8;
            c.draw_text(&text, Rect { x, y: 0.0, w, h: self.settings.pill_height }, 12.0, 600, Color(245, 246, 248, a), Align::Center);
        }
        if p > 0.05 {
            let alpha = ((p - 0.3) / 0.7).clamp(0.0, 1.0);
            let area = Rect { x, y: 0.0, w, h };
            let date = format_date(t.year, t.month, t.day, t.weekday);
            let pages = self.registry.page_count();
            let current = self.registry.current();
            let title = self.registry.current_page().map(|m| m.title()).unwrap_or("");
            ui::draw_chrome(c, area, &Header { title, date: &date, time: &clock }, pages, current, alpha);
            if alpha > 0.5 {
                let now = epoch_ms();
                let body = ui::page_area(area);
                if let Some(page) = self.registry.current_page_mut() {
                    c.push_clip(body, 0.0);
                    page.draw_panel(c, body, now);
                    c.pop_clip();
                }
            }
        }
    }

    pub fn step(&mut self) -> Step {
        let now = Instant::now();
        let dt = self.last.map(|l| (now - l).as_secs_f32()).unwrap_or(1.0 / 60.0).min(0.05);
        self.last = Some(now);

        let want_open = self.hover.update(self.now_ms());
        let mut opened = false;
        if want_open != self.open {
            self.open = want_open;
            opened = want_open;
            let (w, h, r) = if want_open {
                let t = target(true, self.wide, self.lyrics);
                (t.size.w, t.size.h, t.radius)
            } else {
                closed_dims(&self.settings, self.wide)
            };
            self.w.target = w;
            self.h.target = h;
            self.r.target = r;
        }
        self.w.step(dt);
        self.h.step(dt);
        self.r.step(dt);

        let settled = self.w.is_settled() && self.h.is_settled() && self.r.is_settled();
        if settled {
            self.w.snap();
            self.h.snap();
            self.r.snap();
        }
        let fully_closed = settled && !self.open && self.window_big;
        let hover_pending = self.inside != self.open;
        if !settled {
            // Keep a frame cadence while moving.
        } else {
            self.last = None;
        }
        Step { opened, fully_closed, keep_ticking: !settled || hover_pending }
    }
}

fn closed_dims(settings: &Settings, wide: bool) -> (f32, f32, f32) {
    let w = if wide { settings.pill_width + 44.0 } else { settings.pill_width };
    (w, settings.pill_height, 13.0)
}
