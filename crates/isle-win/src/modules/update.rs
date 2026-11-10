// SPDX-License-Identifier: GPL-3.0-or-later
//! Pembaruan page: version, release notes and the update buttons; a short peek in the closed
//! pill when a newer release is found. The work itself is done by `crate::updater`.

use super::button::Button;
use crate::system::{epoch_ms, local_time};
use crate::updater::{self, State};
use isle_logic::canvas::{Align, Canvas, Color, Rect, fit_text, wrap_paragraphs};
use isle_logic::module::{Event, Module, PillItem, Reaction};
use isle_logic::notifs::scroll_offset;
use isle_logic::peek::Peek;
use isle_logic::timefmt::format_clock;
use isle_logic::update::format_bytes;

const WHITE: Color = Color(245, 246, 248, 255);
const DIM: Color = Color(139, 144, 153, 255);
const BODY: Color = Color(214, 217, 223, 255);
const CARD: Color = Color(20, 22, 26, 255);
const TRACK: Color = Color(43, 47, 54, 255);
const BLUE: Color = Color(59, 130, 246, 255);
const GREEN: Color = Color(52, 211, 153, 255);
const RED: Color = Color(255, 107, 107, 255);
const GAP: f32 = 12.0;
const ROW_GAP: f32 = 6.0;
const PEEK_MS: i64 = 6000;
const LINES: usize = 3;

#[derive(Clone, Copy, PartialEq)]
enum Act {
    Check,
    ToggleAuto,
    Download,
    Snooze,
    Skip,
    Cancel,
    InstallNow,
    InstallOnExit,
    Retry,
    DismissFailure,
    Show,
}

pub struct UpdateModule {
    peek: Peek,
    /// What the pill tile shows while the peek is up: (glyph, colour).
    peek_tile: (char, (u8, u8, u8)),
    seen_changes: u64,
    announced: Option<String>,
    /// "Nanti": the page shows a one-line reminder until the user taps Lihat.
    snoozed: bool,
    was_checking: bool,
    buttons: [Button; 3],
    shown: Option<(String, i64)>,
    last_draw_ms: i64,
    animating: bool,
    /// Download speed: the last sample (bytes, ms) and a smoothed bytes/s.
    sample: (u64, i64),
    speed: f32,
}

impl UpdateModule {
    pub fn new() -> Self {
        updater::spawn();
        let mut me = Self {
            peek: Peek::default(),
            peek_tile: ('↓', (59, 130, 246)),
            seen_changes: 0,
            announced: None,
            snoozed: false,
            was_checking: false,
            buttons: [Button::new(BLUE), Button::new(BLUE), Button::new(BLUE)],
            shown: None,
            last_draw_ms: i64::MIN / 2,
            animating: false,
            sample: (0, 0),
            speed: 0.0,
        };
        // After an update: say so once.
        let current = updater::current_version();
        let last = crate::settings_io::load().last_version;
        if last != current {
            if !last.is_empty() {
                me.peek_tile = ('✓', (52, 211, 153));
                me.peek.show(format!("Diperbarui ke {current}"), epoch_ms(), PEEK_MS);
            }
            let v = current.to_string();
            crate::settings_io::update(|s| s.last_version = v);
        }
        me
    }

    /// The buttons for a state, top to bottom: what they do, their label, and whether primary.
    fn buttons_for(&self, state: &State) -> Vec<(Act, String, bool)> {
        let b = |a, l: &str, p| (a, l.to_string(), p);
        match state {
            State::Idle | State::UpToDate { .. } => {
                let auto = if crate::settings_io::load().auto_update { "Otomatis: aktif" } else { "Otomatis: mati" };
                vec![b(Act::Check, "Periksa sekarang", true), b(Act::ToggleAuto, auto, false)]
            }
            State::Checking => vec![],
            State::Available(_) if self.snoozed => vec![b(Act::Show, "Lihat", true)],
            State::Available(_) => vec![b(Act::Download, "Perbarui sekarang", true), b(Act::Snooze, "Nanti", false), b(Act::Skip, "Lewati versi ini", false)],
            State::Downloading { .. } => vec![b(Act::Cancel, "Batal", false)],
            State::Ready { .. } if self.snoozed => vec![b(Act::Show, "Lihat", true)],
            State::Ready { .. } => {
                let later = if updater::install_on_exit_pending() { "Terjadwal saat keluar" } else { "Pasang saat keluar" };
                vec![b(Act::InstallNow, "Mulai ulang & pasang", true), b(Act::InstallOnExit, later, false), b(Act::Snooze, "Nanti", false)]
            }
            State::Failed { .. } => vec![b(Act::Retry, "Coba lagi", true), b(Act::DismissFailure, "Tutup", false)],
        }
    }
}

/// The large card on the left and three button rows on the right.
fn layout(area: Rect) -> (Rect, [Rect; 3]) {
    let card = Rect { w: (area.w - GAP) * 0.6, ..area };
    let x = card.x + card.w + GAP;
    let w = area.w - card.w - GAP;
    let h = (area.h - ROW_GAP * 2.0) / 3.0;
    let row = |i: usize| Rect { x, y: area.y + i as f32 * (h + ROW_GAP), w, h };
    (card, [row(0), row(1), row(2)])
}

/// "09:54" for `at_ms`, in local time (the same trick the Notifications page uses).
fn clock_of(at_ms: i64, now_ms: i64) -> String {
    let t = local_time();
    let now_min = (t.hour * 60 + t.minute) as i64;
    let age_min = ((now_ms - at_ms).max(0) / 60_000) % 1440;
    let m = (now_min - age_min).rem_euclid(1440);
    format_clock((m / 60) as u32, (m % 60) as u32)
}

fn tile(c: &mut dyn Canvas, r: Rect, glyph: char, color: Color, ink: Color) {
    c.fill_round_rect(r, r.w * 0.27, color);
    c.draw_text(&glyph.to_string(), Rect { y: r.y + (r.h - 17.0) / 2.0 - 1.0, h: 18.0, ..r }, 14.0, 700, ink, Align::Center);
}

impl Module for UpdateModule {
    fn id(&self) -> &'static str {
        "update"
    }
    fn title(&self) -> &'static str {
        "Pembaruan"
    }
    fn pill_left(&self) -> Option<PillItem> {
        None
    }
    fn pill_items(&self) -> Vec<PillItem> {
        if self.peek.current(epoch_ms()).is_none() {
            return Vec::new();
        }
        vec![PillItem::AppTile { image: None, letter: self.peek_tile.0, color: self.peek_tile.1 }]
    }
    fn pill_center(&self) -> Option<String> {
        self.peek.current(epoch_ms()).map(str::to_string)
    }
    fn pill_priority(&self) -> u8 {
        40
    }
    fn wide_pill(&self) -> bool {
        self.peek.current(epoch_ms()).is_some()
    }

    fn has_page(&self) -> bool {
        updater::page_wanted()
    }

    fn poll(&mut self, now_ms: i64) -> bool {
        // A check the user asked for that finds nothing has no page to show, so say it in the pill.
        let checking = matches!(updater::state(), State::Checking);
        if self.was_checking && !checking && matches!(updater::state(), State::UpToDate { .. }) {
            self.peek_tile = ('✓', (52, 211, 153));
            self.peek.show("IsleWin sudah terbaru".to_string(), now_ms, PEEK_MS);
        }
        self.was_checking = checking;
        let before = self.peek.current(now_ms).is_some();
        let n = updater::changes();
        let mut changed = n != self.seen_changes;
        self.seen_changes = n;
        if let State::Available(r) = updater::state() {
            // Announce each new version once per run, and never one the user postponed again.
            if self.announced.as_deref() != Some(r.version.as_str()) {
                self.announced = Some(r.version.clone());
                self.snoozed = false;
                self.peek_tile = ('↓', (59, 130, 246));
                self.peek.show(format!("Update {} tersedia", r.version), now_ms, PEEK_MS);
                changed = true;
            }
        }
        changed || self.peek.current(now_ms).is_some() != before
    }

    fn draw_panel(&mut self, c: &mut dyn Canvas, area: Rect, now_ms: i64) {
        let state = updater::state();
        let current = updater::current_version();
        let (card, rows) = layout(area);
        // Restart the text scroll when the panel reopens or the content changes.
        let key = format!("{:?}{}", std::mem::discriminant(&state), self.snoozed);
        if now_ms - self.last_draw_ms > 400 || self.shown.as_ref().map(|(k, _)| k) != Some(&key) {
            self.shown = Some((key, now_ms));
        }
        self.last_draw_ms = now_ms;
        let shown_ms = self.shown.as_ref().map_or(0, |(_, t)| now_ms - t);

        // (glyph, tile colour, title, meta line, meta colour, body paragraphs)
        let mut glyph = ('↓', BLUE, Color(255, 255, 255, 255));
        let mut meta_color = DIM;
        let (title, meta, body): (String, String, String) = match &state {
            State::Idle => {
                glyph = ('↻', TRACK, WHITE);
                ("Pembaruan".into(), format!("Versi {current}"), "Belum diperiksa. Tekan Periksa sekarang.".into())
            }
            State::Checking => {
                glyph = ('…', TRACK, WHITE);
                ("Memeriksa pembaruan…".into(), format!("Versi {current}"), String::new())
            }
            State::UpToDate { at_ms } => {
                glyph = ('✓', GREEN, Color(5, 46, 31, 255));
                let hours = crate::settings_io::load();
                let note = if hours.auto_update { format!("Pengecekan otomatis tiap {} jam.", hours.update_check_hours.max(1)) } else { "Pengecekan otomatis mati.".to_string() };
                ("IsleWin sudah terbaru".into(), format!("Versi {current} · dicek {}", clock_of(*at_ms, now_ms)), note)
            }
            State::Available(r) if self.snoozed => (format!("Update {} ditunda", r.version), "Tekan Lihat untuk melanjutkan".into(), String::new()),
            State::Available(r) => {
                let size = if r.size > 0 { format!(" · {}", format_bytes(r.size)) } else { String::new() };
                let notes = if r.notes.is_empty() { "Tanpa catatan rilis.".to_string() } else { r.notes.join("\n") };
                (format!("IsleWin {} tersedia", r.version), format!("Sekarang {current}{size}"), notes)
            }
            State::Downloading { release, done, total } => {
                let of = if *total > 0 { format!("{} dari {}", format_bytes(*done), format_bytes(*total)) } else { format_bytes(*done) };
                (format!("Mengunduh IsleWin {}", release.version), of, String::new())
            }
            State::Ready { release, .. } if self.snoozed => (format!("{} siap dipasang", release.version), "Tekan Lihat untuk melanjutkan".into(), String::new()),
            State::Ready { release, .. } => {
                glyph = ('✓', GREEN, Color(5, 46, 31, 255));
                meta_color = GREEN;
                let note = if updater::install_on_exit_pending() { "Akan dipasang saat Anda keluar dari IsleWin." } else { "IsleWin menutup sekitar 5 detik, memasang, lalu terbuka lagi sendiri." };
                (format!("{} siap dipasang", release.version), "Terverifikasi · SHA-256 cocok".into(), note.into())
            }
            State::Failed { msg, .. } => {
                glyph = ('!', RED, Color(42, 10, 10, 255));
                ("Gagal memperbarui".into(), format!("Versi {current} tetap berjalan"), msg.clone())
            }
        };

        // Card
        c.fill_round_rect(card, 13.0, CARD);
        c.push_clip(card, 13.0);
        tile(c, Rect { x: card.x + 13.0, y: card.y + 11.0, w: 28.0, h: 28.0 }, glyph.0, glyph.1, glyph.2);
        let tx = card.x + 50.0;
        let tw = card.w - 63.0;
        let title = fit_text(c, &title, 13.0, 650, tw);
        c.draw_text(&title, Rect { x: tx, y: card.y + 9.0, w: tw, h: 18.0 }, 13.0, 650, WHITE, Align::Left);
        let meta = fit_text(c, &meta, 10.5, 400, tw);
        c.draw_text(&meta, Rect { x: tx, y: card.y + 26.0, w: tw, h: 14.0 }, 10.5, 400, meta_color, Align::Left);

        let mut rolling = false;
        if let State::Downloading { done, total, .. } = &state {
            let bar = Rect { x: card.x + 13.0, y: card.y + 56.0, w: card.w - 26.0, h: 8.0 };
            c.fill_round_rect(bar, 4.0, TRACK);
            let frac = if *total > 0 { (*done as f32 / *total as f32).clamp(0.0, 1.0) } else { 0.0 };
            if frac > 0.0 {
                c.fill_round_rect(Rect { w: (bar.w * frac).max(8.0), ..bar }, 4.0, BLUE);
            }
            if now_ms - self.sample.1 >= 500 {
                let dt = (now_ms - self.sample.1).max(1) as f32 / 1000.0;
                let inst = done.saturating_sub(self.sample.0) as f32 / dt;
                self.speed = if self.speed == 0.0 { inst } else { self.speed * 0.6 + inst * 0.4 };
                self.sample = (*done, now_ms);
            }
            let speed = if self.speed > 1.0 { format!("{}/dtk", format_bytes(self.speed as u64)) } else { String::new() };
            c.draw_text(&format!("{}%", (frac * 100.0) as u32), Rect { x: card.x + 13.0, y: card.y + 70.0, w: 60.0, h: 22.0 }, 17.0, 700, WHITE, Align::Left);
            c.draw_text(&speed, Rect { x: card.x + 70.0, y: card.y + 73.0, w: card.w - 83.0, h: 16.0 }, 11.5, 400, DIM, Align::Right);
            rolling = true;
        } else if !body.is_empty() {
            let lines = wrap_paragraphs(c, &body, 13.0, 400, card.w - 26.0, 60);
            let dy = if lines.len() > LINES {
                rolling = true;
                scroll_offset((lines.len() - LINES) as f32 * 18.0, shown_ms)
            } else {
                0.0
            };
            let area = Rect { x: card.x + 13.0, y: card.y + 50.0, w: card.w - 26.0, h: LINES as f32 * 18.0 + 4.0 };
            c.push_clip(area, 0.0);
            for (i, line) in lines.iter().enumerate() {
                c.draw_text(line, Rect { x: area.x, y: area.y + i as f32 * 18.0 - dy, w: area.w, h: 18.0 }, 13.0, 400, BODY, Align::Left);
            }
            c.pop_clip();
        }
        c.pop_clip();

        // Buttons
        let list = self.buttons_for(&state);
        let mut busy = false;
        for (i, (_, label, primary)) in list.iter().enumerate() {
            self.buttons[i].draw(c, rows[i], label, *primary, now_ms);
            busy |= self.buttons[i].busy(now_ms);
        }
        self.animating = rolling || busy || matches!(state, State::Checking);
    }

    fn on_event(&mut self, e: &Event, area: Rect, now_ms: i64) -> Reaction {
        let Event::PointerDown(x, y) = *e else {
            return Reaction::None;
        };
        let state = updater::state();
        let (_, rows) = layout(area);
        let list = self.buttons_for(&state);
        for (i, (act, _, _)) in list.iter().enumerate() {
            let r = rows[i];
            if x < r.x || x >= r.x + r.w || y < r.y || y >= r.y + r.h {
                continue;
            }
            self.buttons[i].press(now_ms);
            match act {
                Act::Check | Act::Retry if matches!(act, Act::Check) || !matches!(state, State::Failed { release: Some(_), .. }) => updater::check_now(),
                Act::Retry | Act::Download => updater::download(),
                Act::ToggleAuto => crate::settings_io::update(|s| s.auto_update = !s.auto_update),
                Act::Snooze => self.snoozed = true,
                Act::Show => self.snoozed = false,
                Act::Skip => {
                    if let State::Available(r) = &state {
                        updater::skip(&r.version);
                    }
                }
                Act::Cancel => updater::cancel_download(),
                Act::InstallNow => updater::install_now(),
                Act::InstallOnExit => updater::install_on_exit(),
                Act::DismissFailure => updater::dismiss_failure(),
                _ => {}
            }
            return Reaction::Redraw;
        }
        Reaction::None
    }

    fn next_wake_ms(&self, now_ms: i64) -> Option<i64> {
        // While this page is on screen and something moves (a download, scrolling notes, a
        // button), one frame per refresh; otherwise only the peek's expiry.
        let moving = (self.animating && now_ms - self.last_draw_ms < 120).then_some(now_ms + 16);
        [self.peek.wake_after(now_ms), moving].into_iter().flatten().min()
    }
}
