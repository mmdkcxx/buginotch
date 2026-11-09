// SPDX-License-Identifier: GPL-3.0-or-later
//! Water page: today's progress ring, a + button for each glass, a seven-day chart, and
//! reminders that pop in the closed pill. Saved to `%APPDATA%\IsleWin\water.json`.

use crate::system::{data_path, epoch_ms, local_time};
use isle_logic::canvas::{Align, Canvas, Color, Rect};
use isle_logic::module::{Event, Module, Reaction};
use isle_logic::peek::Peek;
use isle_logic::todo::day_number;
use isle_logic::water::{WaterLog, from_json, reminder, to_json, weekday_label};

const WHITE: Color = Color(245, 246, 248, 255);
const DIM: Color = Color(139, 144, 153, 255);
const TRACK: Color = Color(43, 47, 54, 255);
const BAR: Color = Color(63, 69, 78, 255);
const BLUE: Color = Color(96, 165, 250, 255);
const GREEN: Color = Color(52, 211, 153, 255);
const BUTTON: Color = Color(28, 31, 36, 255);
const ON_BLUE: Color = Color(6, 18, 34, 255);
const FILL_MS: f32 = 650.0;
const PRESS_MS: f32 = 480.0;
const REMIND_SHOW_MS: i64 = 6000;

fn ease_out(t: f32) -> f32 {
    1.0 - (1.0 - t.clamp(0.0, 1.0)).powi(3)
}

/// "1.250" with a dot between thousands (Indonesian style).
fn thousands(n: u32) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, ch) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            out.push('.');
        }
        out.push(ch);
    }
    out
}

fn mix(a: Color, b: Color, k: f32) -> Color {
    let f = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * k).round() as u8;
    Color(f(a.0, b.0), f(a.1, b.1), f(a.2, b.2), 255)
}

fn fade(c: Color, a: f32) -> Color {
    Color(c.0, c.1, c.2, (a.clamp(0.0, 1.0) * 255.0) as u8)
}

#[derive(Clone, Copy, PartialEq)]
enum Btn {
    Plus,
    Minus,
}

pub struct WaterModule {
    log: WaterLog,
    day: i32,
    /// The amount shown (animates towards today's total): from, to, start ms.
    shown: (f32, f32, i64),
    press: Option<(Btn, i64)>,
    peek: Peek,
    last_slot: Option<u32>,
    /// The goal was already announced today.
    celebrated: bool,
    anim_until: i64,
}

fn today() -> i32 {
    let t = local_time();
    day_number(t.year, t.month, t.day)
}

impl WaterModule {
    pub fn new() -> Self {
        let mut log = std::fs::read_to_string(data_path("water.json")).map(|t| from_json(&t)).unwrap_or_default();
        let day = today();
        if log.prune(day) {
            save(&log);
        }
        let now = log.drunk(day) as f32;
        let celebrated = log.drunk(day) >= log.goal_ml;
        Self { log, day, shown: (now, now, 0), press: None, peek: Peek::default(), last_slot: None, celebrated, anim_until: 0 }
    }

    fn shown_ml(&self, now_ms: i64) -> f32 {
        let (from, to, t0) = self.shown;
        from + (to - from) * ease_out((now_ms - t0) as f32 / FILL_MS)
    }

    fn retarget(&mut self, now_ms: i64) {
        self.shown = (self.shown_ml(now_ms), self.log.drunk(self.day) as f32, now_ms);
        self.anim_until = now_ms + FILL_MS as i64 + 40;
    }

    fn pulse(&self, b: Btn, now_ms: i64) -> f32 {
        match self.press {
            Some((p, t0)) if p == b => ((now_ms - t0) as f32 / PRESS_MS).clamp(0.0, 1.0),
            _ => 1.0,
        }
    }
}

fn save(log: &WaterLog) {
    let path = data_path("water.json");
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(path, to_json(log));
}

/// Where things sit, relative to the page area.
struct Spots {
    ring: (f32, f32),
    plus: (f32, f32),
    minus: (f32, f32),
    chart: Rect,
}

const RING_R: f32 = 48.0;
const PLUS_R: f32 = 20.0;
const MINUS_R: f32 = 13.0;

fn spots(a: Rect) -> Spots {
    Spots {
        ring: (a.x + 56.0, a.y + 61.0),
        plus: (a.x + 140.0, a.y + 42.0),
        minus: (a.x + 140.0, a.y + 88.0),
        chart: Rect { x: a.x + 188.0, y: a.y + 4.0, w: a.w - 188.0, h: a.h - 4.0 },
    }
}

impl Module for WaterModule {
    fn id(&self) -> &'static str {
        "water"
    }
    fn title(&self) -> &'static str {
        "Air minum"
    }
    fn pill_left(&self) -> Option<isle_logic::module::PillItem> {
        None
    }
    fn pill_center(&self) -> Option<String> {
        self.peek.current(epoch_ms()).map(str::to_string)
    }
    fn pill_priority(&self) -> u8 {
        28
    }
    fn wide_pill(&self) -> bool {
        self.peek.current(epoch_ms()).is_some()
    }

    fn poll(&mut self, now_ms: i64) -> bool {
        let before = self.peek.current(now_ms).is_some();
        let d = today();
        if d != self.day {
            self.day = d;
            self.last_slot = None;
            self.celebrated = false;
            self.log.prune(d);
            self.save_log();
            self.retarget(now_ms);
        }
        let t = local_time();
        let minute = (t.hour * 60 + t.minute) as u16;
        if let Some(missing) = reminder(minute, self.last_slot, self.log.drunk(self.day), self.log.goal_ml) {
            self.last_slot = isle_logic::water::slot_at(minute);
            self.peek.show(format!("💧 Waktunya minum · sisa {} ml", thousands(missing)), now_ms, REMIND_SHOW_MS);
        }
        self.peek.current(now_ms).is_some() != before
    }

    fn draw_panel(&mut self, c: &mut dyn Canvas, area: Rect, now_ms: i64) {
        let s = spots(area);
        let goal = self.log.goal_ml.max(1) as f32;
        let shown = self.shown_ml(now_ms);
        let k = (shown / goal).clamp(0.0, 1.0);
        let done = self.log.drunk(self.day) >= self.log.goal_ml;
        let accent = if done { GREEN } else { BLUE };

        // Progress ring, filling clockwise from the top.
        c.stroke_arc(s.ring.0, s.ring.1, RING_R, -90.0, 359.9, 9.0, TRACK);
        c.stroke_arc(s.ring.0, s.ring.1, RING_R, -90.0, 359.9 * k, 9.0, accent);
        let inner = Rect { x: s.ring.0 - 40.0, y: s.ring.1 - 22.0, w: 80.0, h: 28.0 };
        c.draw_text(&thousands(shown.round() as u32), inner, 22.0, 700, WHITE, Align::Center);
        c.draw_text(&format!("dari {} ml", thousands(self.log.goal_ml)), Rect { y: s.ring.1 + 6.0, h: 16.0, ..inner }, 10.5, 400, DIM, Align::Center);

        // The + button: dips when pressed and sends out a ripple.
        let pp = self.pulse(Btn::Plus, now_ms);
        let dip = (std::f32::consts::PI * pp).sin();
        let r = PLUS_R * (1.0 - 0.1 * dip);
        c.fill_circle(s.plus.0, s.plus.1, r, mix(accent, WHITE, 0.2 * dip));
        if pp < 1.0 {
            let e = ease_out(pp);
            c.stroke_arc(s.plus.0, s.plus.1, PLUS_R + 14.0 * e, 0.0, 359.9, 2.0, fade(accent, 1.0 - e));
        }
        let arm = Rect { x: s.plus.0 - 7.0 * (1.0 - 0.1 * dip), y: s.plus.1 - 1.3, w: 14.0 * (1.0 - 0.1 * dip), h: 2.6 };
        c.fill_round_rect(arm, 1.3, ON_BLUE);
        c.fill_round_rect(Rect { x: s.plus.0 - 1.3, y: s.plus.1 - 7.0 * (1.0 - 0.1 * dip), w: 2.6, h: 14.0 * (1.0 - 0.1 * dip) }, 1.3, ON_BLUE);
        c.draw_text(&format!("+{} ml", self.log.glass_ml), Rect { x: s.plus.0 - 26.0, y: s.plus.1 + 22.0, w: 52.0, h: 14.0 }, 9.5, 500, DIM, Align::Center);

        // Undo (a small −).
        let mp = self.pulse(Btn::Minus, now_ms);
        let mdip = (std::f32::consts::PI * mp).sin();
        c.fill_circle(s.minus.0, s.minus.1 + 8.0, MINUS_R * (1.0 - 0.1 * mdip), mix(BUTTON, WHITE, 0.14 * mdip));
        c.fill_round_rect(Rect { x: s.minus.0 - 5.0, y: s.minus.1 + 7.0, w: 10.0, h: 2.0 }, 1.0, DIM);

        self.draw_chart(c, s.chart, shown, now_ms);

        if self.press.is_some_and(|(_, t0)| now_ms - t0 > PRESS_MS as i64) {
            self.press = None;
        }
    }

    fn on_event(&mut self, e: &Event, area: Rect, now_ms: i64) -> Reaction {
        let Event::PointerDown(x, y) = e else { return Reaction::None };
        let s = spots(area);
        let near = |p: (f32, f32), r: f32| (x - p.0).powi(2) + (y - p.1).powi(2) <= r * r;
        if near(s.plus, PLUS_R + 4.0) {
            self.press = Some((Btn::Plus, now_ms));
            let before = self.log.drunk(self.day);
            self.log.add_glass(self.day);
            if before < self.log.goal_ml && self.log.drunk(self.day) >= self.log.goal_ml && !self.celebrated {
                self.celebrated = true;
                self.peek.show("🎉 Target air hari ini tercapai".to_string(), now_ms, REMIND_SHOW_MS);
            }
        } else if near((s.minus.0, s.minus.1 + 8.0), MINUS_R + 4.0) {
            self.press = Some((Btn::Minus, now_ms));
            self.log.undo_glass(self.day);
        } else {
            return Reaction::None;
        }
        self.save_log();
        self.retarget(now_ms);
        Reaction::Redraw
    }

    fn next_wake_ms(&self, now_ms: i64) -> Option<i64> {
        let animating = now_ms < self.anim_until || self.press.is_some_and(|(_, t0)| now_ms - t0 < PRESS_MS as i64);
        let animating = animating.then_some(now_ms + 16);
        // Check the reminder schedule at the start of each minute.
        let minute = Some(now_ms + 60_000 - now_ms.rem_euclid(60_000) + 200);
        let shown = self.peek.next_wake_ms().filter(|&t| t > now_ms);
        [animating, minute, shown].into_iter().flatten().min()
    }
}

impl WaterModule {
    fn save_log(&self) {
        save(&self.log);
    }

    fn draw_chart(&self, c: &mut dyn Canvas, r: Rect, shown_today: f32, now_ms: i64) {
        let week = self.log.last_week(self.day);
        let goal = self.log.goal_ml as f32;
        let top = week.iter().map(|&v| v as f32).fold(goal, f32::max).max(1.0);
        let plot = Rect { x: r.x, y: r.y + 12.0, w: r.w, h: r.h - 12.0 - 20.0 };
        let slot = plot.w / 7.0;
        // The goal line, dashed.
        let gy = plot.y + plot.h * (1.0 - goal / top);
        let mut x = plot.x;
        while x < plot.x + plot.w {
            c.fill_round_rect(Rect { x, y: gy, w: 4.0, h: 1.0 }, 0.5, Color(90, 96, 106, 255));
            x += 8.0;
        }
        c.draw_text("target", Rect { x: plot.x + plot.w - 40.0, y: gy - 13.0, w: 40.0, h: 12.0 }, 9.0, 400, DIM, Align::Right);
        for (i, &v) in week.iter().enumerate() {
            let is_today = i == 6;
            let value = if is_today { shown_today } else { v as f32 };
            // Bars rise from the baseline, each a touch after the one before.
            let h = (plot.h * value / top).max(if value > 0.0 { 3.0 } else { 0.0 });
            let bx = plot.x + slot * i as f32 + (slot - 18.0) / 2.0;
            c.fill_round_rect(Rect { x: bx, y: plot.y, w: 18.0, h: plot.h }, 5.0, Color(20, 22, 26, 255));
            if h > 0.0 {
                let col = if is_today { if value >= goal { GREEN } else { BLUE } } else if v as f32 >= goal { Color(40, 120, 96, 255) } else { BAR };
                c.fill_round_rect(Rect { x: bx, y: plot.y + plot.h - h, w: 18.0, h }, 5.0, col);
            }
            let day = self.day - 6 + i as i32;
            c.draw_text(weekday_label(day), Rect { x: bx - 6.0, y: plot.y + plot.h + 4.0, w: 30.0, h: 14.0 }, 10.5, if is_today { 650 } else { 400 }, if is_today { WHITE } else { DIM }, Align::Center);
            if is_today && value > 0.0 {
                let l = format!("{:.1}", value / 1000.0).replace('.', ",");
                c.draw_text(&format!("{l} L"), Rect { x: bx - 12.0, y: plot.y + plot.h - h - 14.0, w: 42.0, h: 12.0 }, 9.5, 600, WHITE, Align::Center);
            }
        }
    }
}
