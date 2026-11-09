// SPDX-License-Identifier: GPL-3.0-or-later
//! Focus timer (Pomodoro). While it runs, the closed pill shows the countdown.

use crate::settings_io;
use crate::system::{data_path, epoch_ms};
use isle_logic::canvas::{Align, Canvas, Color, Rect};
use isle_logic::module::{Event, Module, PillItem, Reaction};
use isle_logic::peek::Peek;
use isle_logic::pomodoro::{Phase, Pomodoro, PomodoroConfig};
use isle_logic::timefmt::format_mmss;
use isle_logic::timer_layout::{RING_RADIUS, TimerHit, hit, layout, step_break, step_work};
use isle_logic::timer_store::{from_json, to_json};

const WHITE: Color = Color(245, 246, 248, 255);
const DIM: Color = Color(139, 144, 153, 255);
const TRACK: Color = Color(43, 47, 54, 255);
const BUTTON: Color = Color(28, 31, 36, 255);
const GREEN: Color = Color(52, 211, 153, 255);

pub struct TimerModule {
    pomodoro: Pomodoro,
    peek: Peek,
    last_pill: String,
}

impl TimerModule {
    pub fn new(config: PomodoroConfig) -> Self {
        let state = std::fs::read_to_string(data_path("timer.json")).ok().and_then(|t| from_json(&t));
        let pomodoro = match state {
            Some(s) => Pomodoro::restore(config, s),
            None => Pomodoro::new(config),
        };
        Self { pomodoro, peek: Peek::default(), last_pill: String::new() }
    }

    fn save(&self) {
        let path = data_path("timer.json");
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(path, to_json(&self.pomodoro.state()));
    }

    fn running(&self) -> bool {
        self.pomodoro.deadline().is_some()
    }

    fn phase_name(phase: Phase) -> &'static str {
        match phase {
            Phase::Idle | Phase::Focus => "Fokus",
            Phase::ShortBreak | Phase::LongBreak => "Istirahat",
        }
    }
}

fn button(c: &mut dyn Canvas, r: Rect, label: &str, bg: Color, fg: Color, size: f32) {
    c.fill_round_rect(r, r.h / 2.0, bg);
    c.draw_text(label, r, size, 650, fg, Align::Center);
}

impl Module for TimerModule {
    fn id(&self) -> &'static str {
        "timer"
    }
    fn title(&self) -> &'static str {
        "Focus timer"
    }
    fn pill_left(&self) -> Option<PillItem> {
        None
    }
    fn pill_center(&self) -> Option<String> {
        let now = epoch_ms();
        if let Some(t) = self.peek.current(now) {
            return Some(t.to_string());
        }
        self.running().then(|| format_mmss(self.pomodoro.remaining(now)))
    }
    fn pill_priority(&self) -> u8 {
        10
    }
    fn pill_accent(&self) -> bool {
        true
    }
    fn wide_pill(&self) -> bool {
        self.peek.current(epoch_ms()).is_some()
    }
    fn poll(&mut self, now_ms: i64) -> bool {
        let mut changed = false;
        if let Some(t) = self.pomodoro.tick(now_ms) {
            let text = if t.next == Phase::Focus { "Fokus lagi" } else { "Istirahat" };
            self.peek.show(text.to_string(), now_ms, 4000);
            self.save();
            changed = true;
        }
        let pill = self.pill_center().unwrap_or_default();
        changed |= pill != self.last_pill;
        self.last_pill = pill;
        changed
    }
    fn draw_panel(&mut self, c: &mut dyn Canvas, area: Rect, now_ms: i64) {
        let l = layout(area);
        let v = self.pomodoro.view(now_ms);
        let frac = if v.duration_ms > 0 { (v.remaining_ms as f32 / v.duration_ms as f32).clamp(0.0, 1.0) } else { 1.0 };
        c.stroke_arc(l.ring.0, l.ring.1, RING_RADIUS - 3.5, -90.0, 359.9, 7.0, TRACK);
        if frac > 0.0 {
            c.stroke_arc(l.ring.0, l.ring.1, RING_RADIUS - 3.5, -90.0, 359.9 * frac, 7.0, GREEN);
        }
        let ring_box = Rect { x: l.ring.0 - RING_RADIUS, y: l.ring.1 - 15.0, w: RING_RADIUS * 2.0, h: 20.0 };
        c.draw_text(&format_mmss(v.remaining_ms), ring_box, 17.0, 650, WHITE, Align::Center);
        let sub = Rect { y: l.ring.1 + 3.0, ..ring_box };
        c.draw_text(Self::phase_name(v.phase), sub, 10.5, 400, DIM, Align::Center);

        let cfg = self.pomodoro.config();
        for (label_r, minus, value_r, plus, label, value) in [
            (l.work_label, l.work_minus, l.work_value, l.work_plus, "Kerja", cfg.focus_minutes),
            (l.break_label, l.break_minus, l.break_value, l.break_plus, "Istirahat", cfg.short_break_minutes),
        ] {
            c.draw_text(label, label_r, 12.0, 400, DIM, Align::Left);
            button(c, minus, "−", BUTTON, WHITE, 14.0);
            c.draw_text(&format!("{value} mnt"), value_r, 12.0, 650, WHITE, Align::Center);
            button(c, plus, "+", BUTTON, WHITE, 14.0);
        }
        button(c, l.start, if self.running() { "Jeda" } else { "Mulai" }, GREEN, Color(0, 19, 12, 255), 12.5);
        button(c, l.reset, "Reset", BUTTON, WHITE, 12.5);
    }
    fn on_event(&mut self, e: &Event, area: Rect, now_ms: i64) -> Reaction {
        let Event::PointerDown(x, y) = e else { return Reaction::None };
        let Some(h) = hit(area, *x, *y) else { return Reaction::None };
        let mut cfg = self.pomodoro.config().clone();
        match h {
            TimerHit::StartPause => {
                if self.running() {
                    self.pomodoro.pause(now_ms);
                } else {
                    self.pomodoro.start(now_ms);
                }
            }
            TimerHit::Reset => self.pomodoro.stop(),
            TimerHit::WorkMinus | TimerHit::WorkPlus => {
                cfg.focus_minutes = step_work(cfg.focus_minutes, if h == TimerHit::WorkPlus { 1 } else { -1 });
                self.pomodoro.set_config(cfg.clone());
                settings_io::update(|s| s.pomodoro = cfg);
            }
            TimerHit::BreakMinus | TimerHit::BreakPlus => {
                cfg.short_break_minutes = step_break(cfg.short_break_minutes, if h == TimerHit::BreakPlus { 1 } else { -1 });
                self.pomodoro.set_config(cfg.clone());
                settings_io::update(|s| s.pomodoro = cfg);
            }
        }
        self.save();
        Reaction::Redraw
    }
    /// Wakes when the displayed second changes (and when a peek expires).
    fn next_wake_ms(&self, now_ms: i64) -> Option<i64> {
        let tick = self.running().then(|| {
            let r = self.pomodoro.remaining(now_ms);
            now_ms + ((r - 1).max(0) % 1000) + 1
        });
        let peek = self.peek.next_wake_ms().filter(|&t| t > now_ms);
        [tick, peek].into_iter().flatten().min()
    }
}
