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
}

impl TimerModule {
    pub fn new(config: PomodoroConfig) -> Self {
        let state = std::fs::read_to_string(data_path("timer.json")).ok().and_then(|t| from_json(&t));
        let pomodoro = match state {
            Some(s) => Pomodoro::restore(config, s),
            None => Pomodoro::new(config),
        };
        Self { pomodoro, peek: Peek::default(), last_pill: String::new(), settings_open: false }
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

        if self.settings_open {
            for (row, label, value) in [(l.rows[0], "Kerja", cfg.focus_minutes), (l.rows[1], "Istirahat", cfg.short_break_minutes)] {
                c.draw_text(label, row.label, 13.0, 400, DIM, Align::Left);
                for (r, glyph) in [(row.minus, "−"), (row.plus, "+")] {
                    c.fill_round_rect(r, r.h / 2.0, TRACK);
                    c.draw_text(glyph, r, 14.0, 650, WHITE, Align::Center);
                }
                c.draw_text(&format!("{value} mnt"), row.value, 14.0, 650, WHITE, Align::Center);
            }
        } else {
            let time_col = if paused { Color(WHITE.0, WHITE.1, WHITE.2, 140) } else { WHITE };
            c.draw_text(&format_mmss(v.remaining_ms), l.time, 52.0, 700, time_col, Align::Left);
            let total = v.sessions_before_long_break.max(1);
            let label = if paused {
                "Dijeda".to_string()
            } else if on_break {
                format!("Istirahat · sesi {} selesai", v.completed_sessions.max(1))
            } else {
                format!("Fokus · sesi {} dari {}", (v.completed_sessions % total) + 1, total)
            };
            c.draw_text(&label, l.label, 13.0, 400, DIM, Align::Left);
            c.fill_round_rect(l.bar, 2.0, TRACK);
            let done = if v.phase == Phase::Idle || v.duration_ms <= 0 { 0.0 } else { 1.0 - (v.remaining_ms as f32 / v.duration_ms as f32).clamp(0.0, 1.0) };
            if done > 0.0 {
                c.fill_round_rect(Rect { w: (l.bar.w * done).max(4.0), ..l.bar }, 2.0, if on_break { BLUE } else { GREEN });
            }
        }

        // The capsule: main action, reset (or skip during a break), and the gear.
        c.fill_round_rect(l.capsule, l.capsule.h / 2.0, CAPSULE);
        let (main_text, main_bg, main_fg) = if self.settings_open {
            ("Selesai", GREEN, ON_GREEN)
        } else if v.running {
            ("Jeda", AMBER, ON_AMBER)
        } else if paused {
            ("Lanjut", GREEN, ON_GREEN)
        } else {
            ("Mulai", GREEN, ON_GREEN)
        };
        c.fill_round_rect(l.main, l.main.h / 2.0, main_bg);
        c.draw_text(main_text, l.main, 12.5, 650, main_fg, Align::Center);
        if on_break {
            c.draw_text("Lewati", l.second, 12.0, 650, WHITE, Align::Center);
        } else {
            c.draw_text("↺", l.second, 17.0, 400, WHITE, Align::Center);
        }
        c.fill_round_rect(Rect { x: l.gear.x, y: l.gear.y + 9.0, w: 1.0, h: l.gear.h - 18.0 }, 0.5, DIVIDER);
        if self.settings_open {
            c.draw_text("✓", l.gear, 16.0, 650, GREEN, Align::Center);
        } else {
            c.draw_text("⚙", l.gear, 16.0, 400, WHITE, Align::Center);
        }
    }
    fn on_event(&mut self, e: &Event, area: Rect, now_ms: i64) -> Reaction {
        let Event::PointerDown(x, y) = e else { return Reaction::None };
        let Some(h) = hit(area, self.settings_open, *x, *y) else { return Reaction::None };
        let mut cfg = self.pomodoro.config().clone();
        match h {
            TimerHit::Main => {
                if self.settings_open {
                    self.settings_open = false;
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
            TimerHit::Gear => self.settings_open = !self.settings_open,
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
