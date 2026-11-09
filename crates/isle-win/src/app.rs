// SPDX-License-Identifier: GPL-3.0-or-later
//! Notch state: hover, open/close, and the springs that animate its size.

use crate::modules::{charging::ChargingModule, clock::ClockModule, timer::TimerModule};
use crate::system::{epoch_ms, local_time};
use crate::ui::{self, Header};
use isle_logic::canvas::{Align, Canvas, Color, Rect};
use isle_logic::chrome::{ChromeAction, chrome_hit};
use isle_logic::module::{Event, PillItem, Reaction};
use isle_logic::layout::{Hover, target};
use isle_logic::timefmt::{format_clock, format_date};
use isle_logic::module::Registry;
use isle_logic::settings::Settings;
use isle_logic::spring::Spring;
use std::time::Instant;

pub struct Poll {
    pub redraw: bool,
    /// The closed pill's width changed: the window must be resized.
    pub relayout: bool,
}

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
        registry.add(Box::new(ChargingModule::new()));
                registry.add(Box::new(TimerModule::new(settings.pomodoro.clone())));
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
        // The corner radius follows how far the notch has grown (not its own spring), so the
        // shape rounds off in step with its size instead of looking like a box that rounds later.
        let (_, closed_h, closed_r) = closed_dims(&self.settings, self.wide);
        let open = target(true, self.wide, self.lyrics);
        let p = ((self.h.value - closed_h) / (open.size.h - closed_h)).clamp(0.0, 1.0);
        (self.w.value, self.h.value, closed_r + (open.radius - closed_r) * p)
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

    /// What the window region must cover now (DIPs): the shape as drawn, and where it is heading.
    pub fn region_dims(&self) -> (f32, f32) {
        let (tw, th) = self.window_dims(self.window_big);
        if self.window_big {
            (tw.max(self.w.value.ceil() + 1.0), th.max(self.h.value.ceil() + 1.0))
        } else {
            (tw, th)
        }
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    /// Follows the current page's wish for a wide panel (lyrics). Returns true when
    /// the open size changed, so the caller must resize the window.
    pub fn sync_lyrics(&mut self) -> bool {
        let want = self.registry.current_page().is_some_and(|m| m.wants_wide_panel());
        if want == self.lyrics {
            return false;
        }
        self.lyrics = want;
        if self.open {
            // Widening or narrowing the open panel should glide, not bounce past its size.
            self.w.set_damping(self.w.critical_damping());
            let t = target(true, self.wide, self.lyrics);
            self.w.target = t.size.w;
            self.h.target = t.size.h;
            self.r.target = t.radius;
        }
        true
    }

    /// Polls the modules (once a second and on wake-ups). Returns whether anything
    /// visible changed.
    pub fn poll(&mut self) -> Poll {
        let polled = self.registry.poll_all(epoch_ms());
        let (text, _) = self.registry.pill_text("");
        let text_changed = text != self.last_pill;
        self.last_pill = text;
        let wide = self.registry.any_wide_pill();
        let relayout = wide != self.wide;
        if relayout {
            self.wide = wide;
            if !self.open {
                let (w, h, r) = closed_dims(&self.settings, self.wide);
                self.w.target = w;
                self.h.target = h;
                self.r.target = r;
            }
        }
        let resized = self.sync_lyrics();
        Poll { redraw: polled || text_changed || self.open || relayout || resized, relayout: relayout || resized }
    }

    /// When the modules next need a redraw (epoch ms).
    pub fn next_wake_ms(&self) -> Option<i64> {
        self.registry.earliest_wake(epoch_ms())
    }

    /// Passes a pointer event to the current page (coordinates relative to the notch).
    pub fn page_event(&mut self, e: &Event) -> bool {
        if !self.open {
            return false;
        }
        let (w, h, _) = self.dims();
        let body = ui::page_area(Rect { x: 0.0, y: 0.0, w, h });
        let now = epoch_ms();
        match self.registry.current_page_mut() {
            Some(page) => page.on_event(e, body, now) != Reaction::None,
            None => false,
        }
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
            None => return self.page_event(&Event::PointerDown(x, y)),
        }
        true
    }

    /// Draws the notch shape and its content. `win_w` is the window width in DIPs.
    pub fn paint(&mut self, c: &mut dyn Canvas, win_w: f32) {
        let (w, h, r) = self.dims();
        let x = ((win_w - w) / 2.0).max(0.0);
        c.fill_round_rect(Rect { x, y: -r, w, h: h + r }, r, Color(0, 0, 0, 255));

        // Progress of the open/close from the height, which does not change when the lyrics
        // column widens the open panel (a width-based value jumped and flashed the pill).
        let (_, closed_h, _) = closed_dims(&self.settings, self.wide);
        let open_h = target(true, self.wide, self.lyrics).size.h;
        let p = ((h - closed_h) / (open_h - closed_h)).clamp(0.0, 1.0);
        let t = local_time();
        let clock = format_clock(t.hour, t.minute);
        self.registry.prepare_all(c);

        if p < 0.98 {
            let (text, accent) = self.registry.pill_text(&clock);
            let a = ((1.0 - p * 1.6).clamp(0.0, 1.0) * 255.0) as u8;
            let color = if accent { Color(52, 211, 153, a) } else { Color(245, 246, 248, a) };
            c.draw_text(&text, Rect { x, y: 0.0, w, h: self.settings.pill_height }, 12.0, 600, color, Align::Center);
            let mut lx = x + 10.0;
            let ph = self.settings.pill_height;
            let alpha = a as f32 / 255.0;
            for item in self.registry.pill_items() {
                match item {
                    PillItem::MiniCover(id) => {
                        c.draw_image_alpha(id, Rect { x: lx, y: (ph - 15.0) / 2.0, w: 15.0, h: 15.0 }, 4.0, alpha);
                        lx += 20.0;
                    }
                    PillItem::Equaliser { active } => {
                        let t = epoch_ms() as f32 / 1000.0;
                        for i in 0..3 {
                            let bar = if active { 3.0 + 9.0 * (0.5 + 0.5 * (t * 7.0 + i as f32 * 2.1).sin()) } else { 3.0 + i as f32 };
                            c.fill_round_rect(Rect { x: lx + i as f32 * 4.0, y: (ph + 12.0) / 2.0 - bar, w: 2.0, h: bar }, 1.0, Color(52, 211, 153, (alpha * 255.0) as u8));
                        }
                        lx += 14.0;
                    }
                }
            }
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
            self.w.set_damping(22.0);
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
