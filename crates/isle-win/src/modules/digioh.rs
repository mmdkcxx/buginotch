// SPDX-License-Identifier: GPL-3.0-or-later
//! Timer Digioh page: enter a room code, then control that room's timer from the notch.

use super::textbox::TextBox;
use crate::digioh::{self, Conn, Snapshot};
use crate::system::{epoch_ms, local_time};
use isle_logic::canvas::{Align, Canvas, Color, Rect};
use isle_logic::digioh::{MAX_ROOM, PRESETS, Phase, Timer, clock_text, format_time, normalize_room, valid_room};
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
const TICKS: usize = 60;

pub struct DigiohModule {
    field: TextBox,
    focused: bool,
    snap: Snapshot,
    last_pill: String,
    /// A short complaint under the field ("kode tidak valid"), until this time.
    error: Option<(&'static str, i64)>,
    last: String,
}

fn fade(c: Color, k: f32) -> Color {
    Color(c.0, c.1, c.2, (c.3 as f32 * k.clamp(0.0, 1.0)) as u8)
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
        Self { field: TextBox::new(MAX_ROOM), focused: false, snap: digioh::snapshot(), last_pill: String::new(), error: None, last: digioh::last_room() }
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
        let room = normalize_room(self.field.text());
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
        self.field.clear();
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
        !self.online() && self.focused
    }
    fn text_caret(&self) -> Option<(f32, f32)> {
        self.field.caret()
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
            c.draw_text("Kode room dari timer.digioh.id", l.prompt, 12.5, 600, DIM, Align::Left);
            self.field.draw(c, l.field, area, "mis. K7M2Q", self.focused, now);
            c.fill_round_rect(l.join, 12.0, GREEN);
            c.draw_text("Masuk", l.join, 13.0, 700, ON_GREEN, Align::Center);
            if let Some((msg, _)) = self.error {
                c.draw_text(msg, Rect { x: area.x + 8.0, y: area.y + 72.0, w: area.w - 16.0, h: 16.0 }, 11.5, 400, RED, Align::Left);
            } else if !self.last.is_empty() {
                c.fill_round_rect(l.last, 14.0, BORDER);
                c.fill_round_rect(Rect { x: l.last.x + 1.0, y: l.last.y + 1.0, w: l.last.w - 2.0, h: l.last.h - 2.0 }, 13.0, BUTTON);
                c.draw_text(&format!("Terakhir: {}", self.last), l.last, 12.0, 600, WHITE, Align::Center);
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
        c.draw_text(&self.snap.room, l.code, 13.0, 800, WHITE, Align::Left);
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

        c.draw_text(&Self::show_time(&t, now), l.time, 54.0, 800, accent, Align::Left);

        // Under the digits: the end time, or the length choices while stopped.
        if t.is_idle() {
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
            match e {
                Event::PointerDown(x, y) => {
                    match hit(area, false, *x, *y) {
                        Some(DigiohHit::Field) => {
                            self.focused = true;
                            self.field.pointer_down(*x, layout(area).field, now);
                        }
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
                Event::PointerDrag(x, _) if self.focused => {
                    return if self.field.pointer_drag(*x, layout(area).field) { Reaction::Redraw } else { Reaction::None };
                }
                Event::PointerUp(..) => {
                    self.field.pointer_up();
                    return Reaction::None;
                }
                Event::Edit { key: EditKey::Enter, .. } if self.focused => {
                    self.submit(now);
                    return Reaction::Redraw;
                }
                Event::Edit { key: EditKey::Escape, .. } if self.focused => {
                    self.focused = false;
                    return Reaction::Redraw;
                }
                Event::Text(s) if self.focused => {
                    let ev = Event::Text(normalize_room(s));
                    return self.field.on_event(&ev, now).unwrap_or(Reaction::None);
                }
                _ if self.focused => return self.field.on_event(e, now).unwrap_or(Reaction::None),
                _ => return Reaction::None,
            }
        }
        let Event::PointerDown(x, y) = e else { return Reaction::None };
        match hit(area, true, *x, *y) {
            Some(DigiohHit::Leave) => digioh::leave(),
            Some(DigiohHit::Main) => self.act(|t| t.toggle(now), now),
            Some(DigiohHit::Reset) => self.act(|t| Some(t.reset()), now),
            Some(DigiohHit::Plus) => self.act(|t| Some(t.add_seconds(60)), now),
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
        if self.focused && !self.online() {
            let b = self.field.next_blink_ms(now);
            wake = Some(wake.map_or(b, |w| w.min(b)));
        }
        wake
    }
}
