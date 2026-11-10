// SPDX-License-Identifier: GPL-3.0-or-later
//! Timer Digioh page: enter a room code, then control that room's timer from the notch.

use crate::digioh::{self, Conn, Snapshot};
use crate::system::{epoch_ms, local_time};
use isle_logic::canvas::{Align, Canvas, Color, Rect};
use isle_logic::digioh::{parse_duration, MAX_ROOM, PRESETS, Phase, Timer, clock_text, format_time, normalize_room, valid_room};
use isle_logic::digioh_layout::{DigiohHit, hit, layout};
use isle_logic::module::{EditKey, Event, Module, PillItem, Reaction};

const WHITE: Color = Color(245, 246, 248, 255);
const DIM: Color = Color(134, 140, 151, 255);
const TRACK: Color = Color(35, 39, 46, 255);
const BORDER: Color = Color(44, 48, 55, 255);
const BUTTON: Color = Color(20, 22, 26, 255);
const GREEN: Color = Color(61, 220, 151, 255);
const AMBER: Color = Color(255, 176, 32, 255);
const RED: Color = Color(255, 90, 79, 255);
const ON_GREEN: Color = Color(4, 19, 13, 255);
const BLUE: Color = Color(76, 141, 255, 255);
const CELL: Color = Color(16, 18, 22, 255);
const PAPER: Color = Color(244, 245, 247, 255);
const BLINK_MS: i64 = 530;
const TICKS: usize = 60;

pub struct DigiohModule {
    code: String,
    focused: bool,
    blink_from: i64,
    snap: Snapshot,
    last_pill: String,
    /// A short complaint under the field ("kode tidak valid"), until this time.
    error: Option<(&'static str, i64)>,
    last: String,
    /// A length being typed (the digits were tapped).
    editing: Option<String>,
}

fn fade(c: Color, k: f32) -> Color {
    Color(c.0, c.1, c.2, (c.3 as f32 * k.clamp(0.0, 1.0)) as u8)
}

/// A dashed rounded outline (an empty code cell).
fn dashed(c: &mut dyn Canvas, r: Rect, col: Color) {
    let (dash, gap, inset) = (4.0, 3.0, 11.0);
    let mut x = r.x + inset;
    while x + dash <= r.x + r.w - inset + 0.5 {
        c.fill_round_rect(Rect { x, y: r.y, w: dash, h: 1.0 }, 0.5, col);
        c.fill_round_rect(Rect { x, y: r.y + r.h - 1.0, w: dash, h: 1.0 }, 0.5, col);
        x += dash + gap;
    }
    let mut y = r.y + inset;
    while y + dash <= r.y + r.h - inset + 0.5 {
        c.fill_round_rect(Rect { x: r.x, y, w: 1.0, h: dash }, 0.5, col);
        c.fill_round_rect(Rect { x: r.x + r.w - 1.0, y, w: 1.0, h: dash }, 0.5, col);
        y += dash + gap;
    }
}

fn offset_minutes(now_ms: i64) -> i64 {
    let t = local_time();
    let local = (t.hour * 60 + t.minute) as i64;
    let utc = (now_ms / 60_000).rem_euclid(1440);
    let mut d = (local - utc).rem_euclid(1440);
    if d > 840 {
        d -= 1440;
    }
    d
}

fn phase_color(p: Phase) -> Color {
    match p {
        Phase::Ready => WHITE,
        Phase::Run => GREEN,
        Phase::Warn | Phase::Paused => AMBER,
        Phase::Over => RED,
    }
}

impl DigiohModule {
    pub fn new() -> Self {
        Self { code: String::new(), focused: false, blink_from: 0, snap: digioh::snapshot(), last_pill: String::new(), error: None, last: digioh::last_room(), editing: None }
    }

    fn online(&self) -> bool {
        self.snap.conn != Conn::Off
    }

    fn show_time(t: &Timer, now: i64) -> String {
        let r = t.remaining(now);
        format_time(if r > 0.0 { r.ceil() } else { r })
    }

    fn act(&mut self, f: impl FnOnce(&Timer) -> Option<(Timer, String)>, now: i64) {
        let Some(t) = self.snap.timer.clone() else { return };
        if let Some((next, msg)) = f(&t) {
            if !digioh::send(next, &msg) {
                self.error = Some(("Belum terhubung", now + 3000));
            }
            self.snap = digioh::snapshot();
        }
    }

    fn submit(&mut self, now: i64) {
        let room = normalize_room(&self.code);
        if !valid_room(&room) {
            self.error = Some(("Kode room 5 sampai 8 huruf atau angka", now + 3500));
            return;
        }
        self.join(&room);
    }

    fn join(&mut self, room: &str) {
        digioh::join(room);
        self.snap = digioh::snapshot();
        self.last = room.to_string();
        self.code.clear();
        self.focused = false;
        self.error = None;
    }
}

impl Module for DigiohModule {
    fn id(&self) -> &'static str {
        "digioh"
    }
    fn title(&self) -> &'static str {
        "Timer Digioh"
    }
    fn pill_left(&self) -> Option<PillItem> {
        None
    }
    fn pill_center(&self) -> Option<String> {
        if self.snap.conn != Conn::Online {
            return None;
        }
        let now = epoch_ms();
        let t = self.snap.timer.as_ref()?;
        (!t.is_idle()).then(|| Self::show_time(t, now))
    }
    fn pill_priority(&self) -> u8 {
        9
    }
    fn pill_accent(&self) -> bool {
        true
    }
    fn wants_text(&self) -> bool {
        (!self.online() && self.focused) || self.editing.is_some()
    }
    fn text_caret(&self) -> Option<(f32, f32)> {
        None
    }
    fn poll(&mut self, _now_ms: i64) -> bool {
        let s = digioh::snapshot();
        let mut changed = s.version != self.snap.version;
        self.snap = s;
        let pill = self.pill_center().unwrap_or_default();
        changed |= pill != self.last_pill;
        self.last_pill = pill;
        changed
    }

    fn draw_panel(&mut self, c: &mut dyn Canvas, area: Rect, now: i64) {
        let l = layout(area);
        if self.error.is_some_and(|(_, until)| until <= now) {
            self.error = None;
        }
        if !self.online() {
            c.draw_text("Kode room dari halaman timer", l.prompt, 12.0, 400, DIM, Align::Left);
            let n = self.code.chars().count();
            let blink_on = (now - self.blink_from) / BLINK_MS % 2 == 0;
            for (i, cell) in l.cells.iter().enumerate() {
                let active = self.focused && i == n;
                if let Some(ch) = self.code.chars().nth(i) {
                    c.fill_round_rect(*cell, 12.0, BORDER);
                    c.fill_round_rect(Rect { x: cell.x + 1.0, y: cell.y + 1.0, w: cell.w - 2.0, h: cell.h - 2.0 }, 11.0, CELL);
                    c.draw_display(&ch.to_string(), *cell, 26.0, 0.0, WHITE, Align::Center);
                } else if active {
                    c.fill_round_rect(*cell, 12.0, BLUE);
                    c.fill_round_rect(Rect { x: cell.x + 1.5, y: cell.y + 1.5, w: cell.w - 3.0, h: cell.h - 3.0 }, 10.5, CELL);
                    if blink_on {
                        c.fill_round_rect(Rect { x: cell.x + cell.w / 2.0 - 1.0, y: cell.y + 11.0, w: 2.0, h: cell.h - 22.0 }, 1.0, BLUE);
                    }
                } else {
                    dashed(c, *cell, BORDER);
                }
            }
            let ready = valid_room(&normalize_room(&self.code));
            c.fill_round_rect(l.join, 14.0, if ready { PAPER } else { fade(PAPER, 0.5) });
            c.draw_text("Masuk", l.join, 14.0, 700, Color(0, 0, 0, 255), Align::Center);
            let row = Rect { x: area.x + 8.0, y: l.last.y, w: area.w - 16.0, h: l.last.h };
            if let Some((msg, _)) = self.error {
                c.draw_text(msg, row, 12.0, 400, RED, Align::Left);
            } else if !self.last.is_empty() {
                c.draw_text("Dipakai terakhir", Rect { x: row.x, w: 110.0, ..row }, 12.0, 400, DIM, Align::Left);
                c.fill_round_rect(l.last, 13.0, BORDER);
                c.fill_round_rect(Rect { x: l.last.x + 1.0, y: l.last.y + 1.0, w: l.last.w - 2.0, h: l.last.h - 2.0 }, 12.0, Color(0, 0, 0, 255));
                // History glyph: an open ring with an arrow tip.
                c.stroke_arc(l.last.x + 15.0, l.last.y + 13.0, 4.5, -60.0, 300.0, 1.5, DIM);
                c.draw_display(&self.last, Rect { x: l.last.x + 25.0, w: l.last.w - 30.0, ..l.last }, 14.0, 1.4, WHITE, Align::Left);
            }
            return;
        }

        let conn = self.snap.conn;
        let timer = self.snap.timer.clone();
        let phase = timer.as_ref().map(|t| t.phase(now)).unwrap_or(Phase::Ready);
        let accent = if conn == Conn::Online { phase_color(phase) } else { AMBER };

        // Header: dot, room code, status, leave.
        let dot_c = match conn {
            Conn::Online => GREEN,
            _ => AMBER,
        };
        c.fill_circle(l.dot.x + 4.0, l.dot.y + 4.0, 6.0, fade(dot_c, 0.2));
        c.fill_circle(l.dot.x + 4.0, l.dot.y + 4.0, 4.0, dot_c);
        c.draw_display(&self.snap.room, l.code, 15.0, 1.5, WHITE, Align::Left);
        let status = match conn {
            Conn::Online => "Terhubung".to_string(),
            Conn::Connecting => "Menghubungkan…".to_string(),
            _ => "Terputus, mencoba lagi…".to_string(),
        };
        let status = match self.error {
            Some((msg, _)) => msg.to_string(),
            None => status,
        };
        c.draw_text(&status, l.status, 11.5, 400, if self.error.is_some() { RED } else { DIM }, Align::Left);
        c.fill_round_rect(l.leave, 11.0, BORDER);
        c.fill_round_rect(Rect { x: l.leave.x + 1.0, y: l.leave.y + 1.0, w: l.leave.w - 2.0, h: l.leave.h - 2.0 }, 10.0, BUTTON);
        c.draw_text("Keluar", l.leave, 11.5, 600, WHITE, Align::Center);

        let Some(t) = timer.filter(|_| conn == Conn::Online) else {
            c.draw_text("Menunggu data room…", Rect { x: area.x + 8.0, y: area.y + 48.0, w: area.w - 16.0, h: 24.0 }, 14.0, 500, DIM, Align::Left);
            return;
        };

        if let Some(text) = &self.editing {
            let shown = if text.is_empty() { "0".to_string() } else { text.clone() };
            let col = if text.is_empty() { DIM } else { WHITE };
            c.draw_display(&shown, l.time, 56.0, 0.0, col, Align::Left);
            let w = c.measure_display(if text.is_empty() { "" } else { text }, 56.0, 0.0);
            if (now - self.blink_from) / BLINK_MS % 2 == 0 {
                c.fill_round_rect(Rect { x: l.time.x + w + 3.0, y: l.time.y + 8.0, w: 2.5, h: l.time.h - 16.0 }, 1.0, BLUE);
            }
        } else {
            c.draw_display(&Self::show_time(&t, now), l.time, 56.0, 0.0, accent, Align::Left);
        }

        // Under the digits: the end time, or the length choices while stopped.
        if self.editing.is_some() {
            c.draw_text("Ketik menit atau mm:ss, lalu Enter", l.info, 12.0, 400, BLUE, Align::Left);
        } else if t.is_idle() {
            let w = l.info.w / PRESETS.len() as f32;
            for (i, p) in PRESETS.iter().enumerate() {
                let on = *p == t.duration();
                let r = Rect { x: l.info.x + w * i as f32, y: l.info.y, w, h: l.info.h };
                c.draw_text(&format!("{}", p / 60), r, 12.0, if on { 800 } else { 500 }, if on { WHITE } else { DIM }, Align::Center);
            }
        } else {
            let end = t.ends_at(now).map(|e| clock_text(e, offset_minutes(now))).unwrap_or_default();
            let msg = match phase {
                Phase::Over => "Waktu habis".to_string(),
                Phase::Paused => format!("Dijeda · selesai {end} bila lanjut"),
                _ => format!("Selesai pukul {end}"),
            };
            c.draw_text(&msg, l.info, 12.0, 400, DIM, Align::Left);
        }

        // Ruler: one tick per sixtieth of the length; lit ticks are what is left.
        let lit = (t.fraction_left(now) * TICKS as f32).ceil() as usize;
        let lit = if t.is_idle() || phase == Phase::Over { if phase == Phase::Over { 0 } else { TICKS } } else { lit };
        let pitch = l.ruler.w / TICKS as f32;
        let over_c = if phase == Phase::Over { fade(RED, 0.9) } else { accent };
        for i in 0..TICKS {
            let on = i < lit;
            let col = if on { if t.is_idle() { fade(WHITE, 0.55) } else { over_c } } else { TRACK };
            c.fill_round_rect(Rect { x: l.ruler.x + pitch * i as f32, y: l.ruler.y, w: (pitch * 0.55).max(1.6), h: l.ruler.h }, 1.0, col);
        }

        // Controls: reset, big start/pause with a ring, +1 menit.
        for (r, label) in [(l.reset, "↺"), (l.plus, "+1m")] {
            c.fill_round_rect(r, r.h / 2.0, BORDER);
            c.fill_round_rect(Rect { x: r.x + 1.0, y: r.y + 1.0, w: r.w - 2.0, h: r.h - 2.0 }, (r.h - 2.0) / 2.0, BUTTON);
            c.draw_text(label, r, if label == "↺" { 16.0 } else { 11.5 }, 700, WHITE, Align::Center);
        }
        let (cx, cy) = (l.main.x + 32.0, l.main.y + 32.0);
        c.stroke_arc(cx, cy, 29.0, -90.0, 359.9, 3.0, TRACK);
        let sweep = if t.is_idle() { 0.0 } else if phase == Phase::Over { 359.9 } else { (t.fraction_left(now) * 359.9).max(1.0) };
        if sweep > 0.0 {
            c.stroke_arc(cx, cy, 29.0, -90.0, sweep, 3.0, accent);
        }
        c.fill_circle(cx, cy, 25.0, Color(20, 22, 26, 255));
        let running = !t.is_idle() && !t.is_paused();
        if running {
            for dx in [-7.0, 2.0] {
                c.fill_round_rect(Rect { x: cx + dx, y: cy - 9.0, w: 5.0, h: 18.0 }, 1.6, WHITE);
            }
        } else {
            c.fill_polygon(&[(cx - 6.0, cy - 10.0), (cx - 6.0, cy + 10.0), (cx + 11.0, cy)], WHITE);
        }
    }

    fn on_event(&mut self, e: &Event, area: Rect, now: i64) -> Reaction {
        if !self.online() {
            self.blink_from = now;
            match e {
                Event::PointerDown(x, y) => {
                    match hit(area, false, *x, *y) {
                        Some(DigiohHit::Field) => self.focused = true,
                        Some(DigiohHit::Join) => self.submit(now),
                        Some(DigiohHit::Last) => {
                            let r = self.last.clone();
                            if !r.is_empty() {
                                self.join(&r);
                            }
                        }
                        _ => self.focused = false,
                    }
                    return Reaction::Redraw;
                }
                _ if !self.focused => return Reaction::None,
                Event::Edit { key: EditKey::Enter, .. } => self.submit(now),
                Event::Edit { key: EditKey::Escape, .. } => self.focused = false,
                Event::Edit { key: EditKey::Backspace | EditKey::Delete, .. } => {
                    self.code.pop();
                }
                Event::Text(s) => {
                    let room = normalize_room(&format!("{}{}", self.code, normalize_room(s)));
                    self.code = room;
                    self.error = None;
                }
                _ => return Reaction::None,
            }
            return Reaction::Redraw;
        }
        if self.editing.is_some() {
            self.blink_from = now;
            match e {
                Event::Edit { key: EditKey::Enter, .. } => {
                    let typed = self.editing.take().unwrap_or_default();
                    match parse_duration(&typed) {
                        Some(secs) => self.act(|t| Some(t.set_duration(secs)), now),
                        None => self.error = Some(("Panjang tidak valid", now + 3000)),
                    }
                }
                Event::Edit { key: EditKey::Escape, .. } => self.editing = None,
                Event::Edit { key: EditKey::Backspace | EditKey::Delete, .. } => {
                    if let Some(t) = &mut self.editing {
                        t.pop();
                    }
                }
                Event::Text(s) => {
                    if let Some(t) = &mut self.editing {
                        for ch in s.chars().filter(|c| c.is_ascii_digit() || *c == ':') {
                            if t.len() < 8 && (ch != ':' || !t.contains(':')) {
                                t.push(ch);
                            }
                        }
                    }
                }
                Event::PointerDown(..) => self.editing = None,
                _ => return Reaction::None,
            }
            self.snap = digioh::snapshot();
            return Reaction::Redraw;
        }
        let Event::PointerDown(x, y) = e else { return Reaction::None };
        match hit(area, true, *x, *y) {
            Some(DigiohHit::Leave) => digioh::leave(),
            Some(DigiohHit::Main) => self.act(|t| t.toggle(now), now),
            Some(DigiohHit::Reset) => self.act(|t| Some(t.reset()), now),
            Some(DigiohHit::Plus) => self.act(|t| Some(t.add_seconds(60)), now),
            Some(DigiohHit::Time) => {
                if self.snap.timer.as_ref().is_some_and(|t| t.is_idle()) {
                    self.editing = Some(String::new());
                    self.blink_from = now;
                }
            }
            Some(DigiohHit::Preset(i)) => self.act(|t| t.is_idle().then(|| t.set_duration(PRESETS[i])), now),
            _ => return Reaction::None,
        }
        self.snap = digioh::snapshot();
        Reaction::Redraw
    }

    fn next_wake_ms(&self, now: i64) -> Option<i64> {
        let mut wake = self.error.map(|(_, until)| until);
        if self.snap.conn == Conn::Online {
            if let Some(t) = &self.snap.timer {
                if !t.is_idle() && !t.is_paused() {
                    let r = t.remaining(now);
                    let ms = ((r.fract() * 1000.0) as i64).rem_euclid(1000);
                    let tick = now + if r > 0.0 { ms } else { 1000 - ms.rem_euclid(1000) } + 5;
                    wake = Some(wake.map_or(tick, |w| w.min(tick)));
                }
            }
        }
        if (self.focused && !self.online()) || self.editing.is_some() {
            let b = now + BLINK_MS - (now - self.blink_from).rem_euclid(BLINK_MS);
            wake = Some(wake.map_or(b, |w| w.min(b)));
        }
        wake
    }
}
