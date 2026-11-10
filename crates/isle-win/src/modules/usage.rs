// SPDX-License-Identifier: GPL-3.0-or-later
//! Claude usage page: how much of the 5-hour and weekly allowance is used, as two bars, with a
//! heads-up in the pill shortly before and at each reset.

use super::digioh::offset_minutes;
use crate::system::epoch_ms;
use crate::usage::{self, Snapshot, Status};
use isle_logic::canvas::{Align, Canvas, Color, Rect};
use isle_logic::module::{Event, Module, PillItem, Reaction};
use isle_logic::peek::Peek;
use isle_logic::usage::{Level, Notice, Notifier, PILL_PERCENT, Usage, Which, format_left, level, reset_text};

const WHITE: Color = Color(245, 246, 248, 255);
const DIM: Color = Color(134, 140, 151, 255);
const TRACK: Color = Color(35, 39, 46, 255);
const BORDER: Color = Color(44, 48, 55, 255);
const BUTTON: Color = Color(20, 22, 26, 255);
const ORANGE: Color = Color(217, 119, 87, 255);
const YELLOW: Color = Color(245, 196, 81, 255);
const RED: Color = Color(255, 90, 79, 255);
const STALE: Color = Color(91, 97, 108, 255);

pub struct UsageModule {
    snap: Snapshot,
    notifier: Notifier,
    peek: Peek,
    seen_version: u64,
    last_key: String,
    /// A second reading to take just after a reset, when the first may still show the old window.
    refresh_at: Option<i64>,
}

fn fade(c: Color, k: f32) -> Color {
    Color(c.0, c.1, c.2, (c.3 as f32 * k.clamp(0.0, 1.0)) as u8)
}

fn level_color(p: f32) -> Color {
    match level(p) {
        Level::Ok => ORANGE,
        Level::Warn => YELLOW,
        Level::High => RED,
    }
}

fn notice_text(n: Notice) -> String {
    match n {
        Notice::Soon(Which::Five, m) => format!("Usage 5 jam reset {m} mnt lagi"),
        Notice::Soon(Which::Week, m) => format!("Usage mingguan reset {m} mnt lagi"),
        Notice::Reset(Which::Five) => "Usage 5 jam sudah reset".to_string(),
        Notice::Reset(Which::Week) => "Usage mingguan sudah reset".to_string(),
    }
}

/// The "Cek lagi" button of the no-login screens, relative to the page area.
fn retry_rect(a: Rect) -> Rect {
    Rect { x: a.x + 8.0, y: a.y + 78.0, w: 84.0, h: 30.0 }
}

impl UsageModule {
    pub fn new() -> Self {
        usage::start();
        Self { snap: usage::snapshot(), notifier: Notifier::default(), peek: Peek::default(), seen_version: u64::MAX, last_key: String::new(), refresh_at: None }
    }

    fn has_data(&self) -> bool {
        self.snap.usage.is_some()
    }

    fn pill_percent(&self) -> Option<f32> {
        (self.snap.status == Status::Ok).then_some(()).and(self.snap.usage.as_ref()).and_then(Usage::highest).filter(|p| *p >= PILL_PERCENT)
    }

    fn key(&self, now: i64) -> String {
        let Some(u) = self.snap.usage else { return format!("{:?}", self.snap.status) };
        let left = |w: Option<isle_logic::usage::Window>| w.and_then(|w| w.resets_at_ms).map(|t| format_left(t - now)).unwrap_or_default();
        format!("{:?}|{}|{}", self.snap.status, left(u.five), left(u.week))
    }

    fn draw_row(&self, c: &mut dyn Canvas, area: Rect, y: f32, which: Which, now: i64, stale: bool) {
        let w = self.snap.usage.and_then(|u| u.get(which));
        let x = area.x + 8.0;
        let width = area.w - 16.0;
        c.draw_text(match which { Which::Five => "Sesi 5 jam", Which::Week => "Mingguan" }, Rect { x, y, w: width * 0.6, h: 16.0 }, 13.0, 600, WHITE, Align::Left);
        let bar = Rect { x, y: y + 21.0, w: width, h: 9.0 };
        c.fill_round_rect(bar, 4.5, TRACK);
        match w {
            Some(win) => {
                let col = if stale { STALE } else { level_color(win.percent) };
                c.draw_text(&format!("{}%", win.percent.round() as i32), Rect { x: x + width * 0.6, y, w: width * 0.4, h: 16.0 }, 14.0, 700, col, Align::Right);
                if win.percent > 0.0 {
                    c.fill_round_rect(Rect { w: (bar.w * win.percent / 100.0).max(9.0), ..bar }, 4.5, col);
                }
                let text = match win.resets_at_ms {
                    Some(t) if t > now => format!("Reset {} · {}", format_left(t - now), reset_text(t, now, offset_minutes(now))),
                    Some(_) => "Baru saja reset".to_string(),
                    None => "Belum ada pemakaian di jendela ini".to_string(),
                };
                c.draw_text(&text, Rect { x, y: y + 33.0, w: width, h: 14.0 }, 11.5, 400, DIM, Align::Left);
            }
            None => {
                c.draw_text("--%", Rect { x: x + width * 0.6, y, w: width * 0.4, h: 16.0 }, 14.0, 700, STALE, Align::Right);
            }
        }
    }
}

impl Module for UsageModule {
    fn id(&self) -> &'static str {
        "usage"
    }
    fn title(&self) -> &'static str {
        "Pemakaian Claude"
    }
    fn pill_left(&self) -> Option<PillItem> {
        None
    }
    fn pill_center(&self) -> Option<String> {
        if let Some(t) = self.peek.current(epoch_ms()) {
            return Some(t.to_string());
        }
        self.pill_percent().map(|p| format!("{}%", p.round() as i32))
    }
    fn pill_priority(&self) -> u8 {
        // A reset reminder outranks everything else in the pill while it shows (the to-do,
        // water, notification and update texts all rank above the usage percentage and used
        // to hide it); the plain percentage stays low.
        if self.peek.current(epoch_ms()).is_some() { 60 } else { 8 }
    }
    fn pill_accent(&self) -> bool {
        true
    }
    fn wide_pill(&self) -> bool {
        self.peek.current(epoch_ms()).is_some()
    }

    fn poll(&mut self, now: i64) -> bool {
        let mut changed = false;
        let snap = usage::snapshot();
        if snap.version != self.seen_version {
            self.seen_version = snap.version;
            if let Some(u) = snap.usage {
                self.notifier.observe(Which::Five, u.five.and_then(|w| w.resets_at_ms), now);
                self.notifier.observe(Which::Week, u.week.and_then(|w| w.resets_at_ms), now);
            }
            changed = true;
        }
        self.snap = snap;
        for n in self.notifier.due(now) {
            crate::log(&format!("usage reminder: {n:?}"));
            self.peek.show(notice_text(n), now, 8000);
            if matches!(n, Notice::Reset(_)) {
                usage::refresh();
                self.refresh_at = Some(now + 15_000);
            }
            changed = true;
        }
        if self.refresh_at.is_some_and(|t| now >= t) {
            self.refresh_at = None;
            usage::refresh();
        }
        let key = self.key(now) + &self.pill_center().unwrap_or_default();
        changed |= key != self.last_key;
        self.last_key = key;
        changed
    }

    fn draw_panel(&mut self, c: &mut dyn Canvas, area: Rect, now: i64) {
        let status = self.snap.status;
        if !self.has_data() && matches!(status, Status::NoLogin | Status::Expired) {
            let (title, body) = if status == Status::NoLogin {
                ("Belum ada login Claude Code", "Masuk di Claude Code di PC ini, lalu tekan Cek lagi.")
            } else {
                ("Login Claude Code sudah kedaluwarsa", "Buka Claude Code sebentar. Widget pulih sendiri begitu token baru ditulis.")
            };
            c.fill_circle(area.x + 30.0, area.y + 32.0, 22.0, BUTTON);
            // A padlock: body and shackle.
            c.fill_round_rect(Rect { x: area.x + 22.0, y: area.y + 32.0, w: 16.0, h: 11.0 }, 3.0, YELLOW);
            c.stroke_arc(area.x + 30.0, area.y + 31.0, 5.0, 180.0, 180.0, 2.0, YELLOW);
            c.draw_text(title, Rect { x: area.x + 62.0, y: area.y + 12.0, w: area.w - 70.0, h: 22.0 }, 15.0, 700, WHITE, Align::Left);
            c.draw_text(body, Rect { x: area.x + 62.0, y: area.y + 34.0, w: area.w - 70.0, h: 20.0 }, 12.0, 400, DIM, Align::Left);
            let r = retry_rect(area);
            c.fill_round_rect(r, 15.0, BORDER);
            c.fill_round_rect(Rect { x: r.x + 1.0, y: r.y + 1.0, w: r.w - 2.0, h: r.h - 2.0 }, 14.0, BUTTON);
            c.draw_text("Cek lagi", r, 12.5, 600, WHITE, Align::Center);
            return;
        }
        let stale = self.has_data() && status != Status::Ok;
        self.draw_row(c, area, area.y, Which::Five, now, stale);
        self.draw_row(c, area, area.y + 54.0, Which::Week, now, stale);
        let note = match status {
            Status::Ok => String::new(),
            Status::Loading => "Memuat…".to_string(),
            Status::Offline => "Tidak terhubung".to_string(),
            Status::Expired => "Login kedaluwarsa".to_string(),
            Status::NoLogin => "Belum ada login".to_string(),
        };
        if !note.is_empty() {
            let when = if self.snap.fetched_ms > 0 { format!(" · data {}", isle_logic::digioh::clock_text(self.snap.fetched_ms, offset_minutes(now))) } else { String::new() };
            c.fill_circle(area.x + 12.0, area.y + 112.0, 3.5, if status == Status::Loading { DIM } else { YELLOW });
            c.draw_text(&format!("{note}{when}"), Rect { x: area.x + 22.0, y: area.y + 104.0, w: area.w - 30.0, h: 16.0 }, 11.5, 400, if status == Status::Loading { DIM } else { fade(YELLOW, 0.95) }, Align::Left);
        }
    }

    fn on_event(&mut self, e: &Event, area: Rect, _now: i64) -> Reaction {
        let Event::PointerDown(x, y) = e else { return Reaction::None };
        let r = retry_rect(area);
        let on_screen = !self.has_data() && matches!(self.snap.status, Status::NoLogin | Status::Expired);
        if on_screen && *x >= r.x && *x <= r.x + r.w && *y >= r.y && *y <= r.y + r.h {
            usage::refresh();
            return Reaction::Redraw;
        }
        Reaction::None
    }

    fn next_wake_ms(&self, now: i64) -> Option<i64> {
        let peek = self.peek.next_wake_ms().filter(|&t| t > now);
        let refresh = self.refresh_at.filter(|&t| t > now);
        [peek, refresh].into_iter().flatten().min()
    }
}
