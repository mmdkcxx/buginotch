// SPDX-License-Identifier: GPL-3.0-or-later
//! Focus timer (Pomodoro). While it runs, the closed pill shows the countdown.

use crate::settings_io;
use crate::system::{data_path, epoch_ms};
use isle_logic::canvas::{Align, Canvas, Color, Rect};
use isle_logic::module::{Event, Module, PillItem, Reaction};
use isle_logic::peek::Peek;
use isle_logic::pomodoro::{Phase, Pomodoro, PomodoroConfig};
use isle_logic::timefmt::format_mmss;
use isle_logic::timer_layout::{TimerHit, hit, layout, step_break, step_work};
use isle_logic::timer_store::{from_json, to_json};

const WHITE: Color = Color(245, 246, 248, 255);
const DIM: Color = Color(139, 144, 153, 255);
const TRACK: Color = Color(43, 47, 54, 255);
const CAPSULE: Color = Color(32, 36, 42, 255);
const DIVIDER: Color = Color(58, 63, 71, 255);
const GREEN: Color = Color(52, 211, 153, 255);
const AMBER: Color = Color(245, 196, 81, 255);
const BLUE: Color = Color(90, 169, 255, 255);
const ON_GREEN: Color = Color(4, 19, 13, 255);
const ON_AMBER: Color = Color(29, 21, 0, 255);

pub struct TimerModule {
    pomodoro: Pomodoro,
    peek: Peek,
    last_pill: String,
    /// The gear was pressed: durations are shown (and editable) instead of the countdown.
    settings_open: bool,
    /// Animation state (all time-based, evaluated in `draw_panel`).
    press: Option<(TimerHit, i64)>,
    s_from: f32,
    s_start: i64,
    amber_from: f32,
    amber_to: f32,
    amber_start: i64,
    bar_from: f32,
    bar_to: f32,
    bar_start: i64,
    time_from: f32,
    time_to: f32,
    time_start: i64,
    time_key: (Phase, bool),
    anim_until: i64,
}

fn ease(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    1.0 - (1.0 - t).powi(3)
}
fn tween(from: f32, to: f32, start: i64, now: i64, ms: f32) -> f32 {
    from + (to - from) * ease((now - start) as f32 / ms)
}
fn mix(a: Color, b: Color, k: f32) -> Color {
    let f = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * k).round() as u8;
    Color(f(a.0, b.0), f(a.1, b.1), f(a.2, b.2), f(a.3, b.3))
}
fn fade(c: Color, k: f32) -> Color {
    Color(c.0, c.1, c.2, (c.3 as f32 * k.clamp(0.0, 1.0)) as u8)
}
fn inset(r: Rect, d: f32) -> Rect {
    Rect { x: r.x + d, y: r.y + d, w: (r.w - 2.0 * d).max(1.0), h: (r.h - 2.0 * d).max(1.0) }
}
const PRESS_MS: f32 = 260.0;

impl TimerModule {
    pub fn new(config: PomodoroConfig) -> Self {
        let state = std::fs::read_to_string(data_path("timer.json")).ok().and_then(|t| from_json(&t));
        let pomodoro = match state {
            Some(s) => Pomodoro::restore(config, s),
            None => Pomodoro::new(config),
        };
        Self { pomodoro, peek: Peek::default(), last_pill: String::new(), settings_open: false,
            press: None, s_from: 0.0, s_start: 0, amber_from: 0.0, amber_to: 0.0, amber_start: 0,
            bar_from: -1.0, bar_to: 0.0, bar_start: 0,
            time_from: -1.0, time_to: -1.0, time_start: 0, time_key: (Phase::Idle, false), anim_until: 0 }
    }

    fn save(&self) {
        let path = data_path("timer.json");
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(path, to_json(&self.pomodoro.state()));
    }

    /// 0..1..0 pulse for a control that was just pressed.
    fn pulse(&self, h: TimerHit, now: i64) -> f32 {
        match self.press {
            Some((p, t0)) if p == h => {
                let t = (now - t0) as f32 / PRESS_MS;
                if (0.0..1.0).contains(&t) { (std::f32::consts::PI * t).sin() } else { 0.0 }
            }
            _ => 0.0,
        }
    }

    fn settings_t(&self, now: i64) -> f32 {
        tween(self.s_from, if self.settings_open { 1.0 } else { 0.0 }, self.s_start, now, 280.0)
    }

    fn running(&self) -> bool {
        self.pomodoro.deadline().is_some()
    }
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
        let on_break = matches!(v.phase, Phase::ShortBreak | Phase::LongBreak);
        let paused = !v.running && v.phase != Phase::Idle;
        let cfg = self.pomodoro.config().clone();
        let st = self.settings_t(now_ms);

        // Retarget tweens when the underlying state changed.
        let amber_target = if v.running && !self.settings_open { 1.0 } else { 0.0 };
        if amber_target != self.amber_to {
            self.amber_from = tween(self.amber_from, self.amber_to, self.amber_start, now_ms, 240.0);
            self.amber_to = amber_target;
            self.amber_start = now_ms;
            self.anim_until = now_ms + 300;
        }
        let done = if v.phase == Phase::Idle || v.duration_ms <= 0 { 0.0 } else { 1.0 - (v.remaining_ms as f32 / v.duration_ms as f32).clamp(0.0, 1.0) };
        if self.bar_from < 0.0 {
            self.bar_from = done;
            self.bar_to = done;
        } else if (done - self.bar_to).abs() < 0.02 {
            // Normal countdown drift: follow directly, no animation (and no extra frames).
            self.bar_from = done;
            self.bar_to = done;
        } else {
            // A jump (start, reset, skip, phase change): glide there.
            self.bar_from = tween(self.bar_from, self.bar_to, self.bar_start, now_ms, 500.0);
            self.bar_to = done;
            self.bar_start = now_ms;
            self.anim_until = self.anim_until.max(now_ms + 520);
        }
        // The big time glides to its new value on a reset, skip or duration change, and
        // follows the live countdown directly otherwise.
        let target = v.remaining_ms as f32;
        let key = (v.phase, v.running);
        if self.time_to < 0.0 {
            self.time_from = target;
            self.time_to = target;
        } else if (key != self.time_key || !v.running) && (target - self.time_to).abs() > 500.0 {
            self.time_from = tween(self.time_from, self.time_to, self.time_start, now_ms, 550.0);
            self.time_to = target;
            self.time_start = now_ms;
            self.anim_until = self.anim_until.max(now_ms + 570);
        } else {
            self.time_to = target;
            if now_ms - self.time_start >= 550 {
                self.time_from = target;
            }
        }
        self.time_key = key;
        let shown_ms = tween(self.time_from, self.time_to, self.time_start, now_ms, 550.0).round() as i64;
        if st > 0.0 && st < 1.0 {
            self.anim_until = self.anim_until.max(now_ms + 40);
        }

        // Countdown group fades/slides out while the settings group fades/slides in.
        let main_k = 1.0 - st;
        if main_k > 0.01 {
            let dy = -8.0 * st;
            let shift = |r: Rect| Rect { y: r.y + dy, ..r };
            let time_col = if paused { Color(WHITE.0, WHITE.1, WHITE.2, 140) } else { WHITE };
            c.draw_text(&format_mmss(shown_ms), shift(l.time), 52.0, 700, fade(time_col, main_k), Align::Left);
            let total = v.sessions_before_long_break.max(1);
            let label = if paused {
                "Dijeda".to_string()
            } else if on_break {
                format!("Istirahat · sesi {} selesai", v.completed_sessions.max(1))
            } else {
                format!("Fokus · sesi {} dari {}", (v.completed_sessions % total) + 1, total)
            };
            c.draw_text(&label, shift(l.label), 13.0, 400, fade(DIM, main_k), Align::Left);
            c.fill_round_rect(shift(l.bar), 2.0, fade(TRACK, main_k));
            let frac = tween(self.bar_from, self.bar_to, self.bar_start, now_ms, 500.0);
            if frac > 0.0 {
                c.fill_round_rect(Rect { w: (l.bar.w * frac).max(4.0), ..shift(l.bar) }, 2.0, fade(if on_break { BLUE } else { GREEN }, main_k));
            }
        }
        if st > 0.01 {
            let dy = 8.0 * (1.0 - st);
            let shift = |r: Rect| Rect { y: r.y + dy, ..r };
            let rows = [
                (l.rows[0], "Kerja", cfg.focus_minutes, TimerHit::WorkMinus, TimerHit::WorkPlus),
                (l.rows[1], "Istirahat", cfg.short_break_minutes, TimerHit::BreakMinus, TimerHit::BreakPlus),
            ];
            for (row, label, value, hm, hp) in rows {
                c.draw_text(label, shift(row.label), 13.0, 400, fade(DIM, st), Align::Left);
                for (r, glyph, h) in [(row.minus, "−", hm), (row.plus, "+", hp)] {
                    let p = self.pulse(h, now_ms);
                    let rr = inset(shift(r), p * 2.5);
                    c.fill_round_rect(rr, rr.h / 2.0, fade(mix(TRACK, GREEN, p * 0.6), st));
                    c.draw_text(glyph, rr, 14.0, 650, fade(WHITE, st), Align::Center);
                }
                c.draw_text(&format!("{value} mnt"), shift(row.value), 14.0, 650, fade(WHITE, st), Align::Center);
            }
        }

        // The capsule: main action, reset (or skip during a break), and the gear.
        let amber = tween(self.amber_from, self.amber_to, self.amber_start, now_ms, 240.0);
        let main_text = if self.settings_open {
            "Selesai"
        } else if v.running {
            "Jeda"
        } else if paused {
            "Lanjut"
        } else {
            "Mulai"
        };
        c.fill_round_rect(l.capsule, l.capsule.h / 2.0, CAPSULE);
        let pm = self.pulse(TimerHit::Main, now_ms);
        let mr = inset(l.main, pm * 3.0);
        c.fill_round_rect(mr, mr.h / 2.0, mix(mix(GREEN, AMBER, amber), Color(255, 255, 255, 255), pm * 0.25));
        c.draw_text(main_text, mr, 12.5, 650, mix(ON_GREEN, ON_AMBER, amber), Align::Center);

        let ps = self.pulse(TimerHit::Second, now_ms);
        if ps > 0.0 {
            let r = inset(l.second, 2.0 + ps * 2.0);
            c.fill_round_rect(r, r.h / 2.0, Color(255, 255, 255, (ps * 40.0) as u8));
        }
        if on_break {
            c.draw_text("Lewati", l.second, 12.0, 650, WHITE, Align::Center);
        } else {
            c.draw_text("↺", l.second, 17.0, 400, WHITE, Align::Center);
        }
        c.fill_round_rect(Rect { x: l.gear.x, y: l.gear.y + 9.0, w: 1.0, h: l.gear.h - 18.0 }, 0.5, DIVIDER);
        let pg = self.pulse(TimerHit::Gear, now_ms);
        if pg > 0.0 {
            let r = inset(l.gear, 4.0 + pg * 1.5);
            c.fill_round_rect(r, r.h / 2.0, Color(255, 255, 255, (pg * 40.0) as u8));
        }
        c.draw_text("⚙", l.gear, 16.0, 400, fade(WHITE, 1.0 - st), Align::Center);
        c.draw_text("✓", l.gear, 16.0, 650, fade(GREEN, st), Align::Center);
    }
    fn on_event(&mut self, e: &Event, area: Rect, now_ms: i64) -> Reaction {
        let Event::PointerDown(x, y) = e else { return Reaction::None };
        let Some(h) = hit(area, self.settings_open, *x, *y) else { return Reaction::None };
        let mut cfg = self.pomodoro.config().clone();
        self.press = Some((h, now_ms));
        self.anim_until = now_ms + 300;
        let st_now = self.settings_t(now_ms);
        match h {
            TimerHit::Main => {
                if self.settings_open {
                    self.settings_open = false;
                    self.s_from = st_now;
                    self.s_start = now_ms;
                } else if self.running() {
                    self.pomodoro.pause(now_ms);
                } else {
                    self.pomodoro.start(now_ms);
                }
            }
            TimerHit::Second => {
                if matches!(self.pomodoro.phase(), Phase::ShortBreak | Phase::LongBreak) {
                    if let Some(t) = self.pomodoro.skip(now_ms) {
                        let text = if t.next == Phase::Focus { "Fokus lagi" } else { "Istirahat" };
                        self.peek.show(text.to_string(), now_ms, 4000);
                    }
                } else {
                    self.pomodoro.stop();
                }
            }
            TimerHit::Gear => {
                self.settings_open = !self.settings_open;
                self.s_from = st_now;
                self.s_start = now_ms;
            }
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
        let frame = (now_ms < self.anim_until).then_some(now_ms + 16);
        [tick, peek, frame].into_iter().flatten().min()
    }
}
