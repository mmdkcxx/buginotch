// SPDX-License-Identifier: GPL-3.0-or-later
//! Notch state: hover, open/close, and the springs that animate its size.

use isle_logic::layout::{Hover, target};
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
}

impl App {
    pub fn new(settings: Settings) -> Self {
        let (w, h, r) = closed_dims(&settings, false);
        App {
            registry: Registry::new(),
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
