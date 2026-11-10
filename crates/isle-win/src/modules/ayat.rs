// SPDX-License-Identifier: GPL-3.0-or-later
//! Ayat page: one passage from the whole Bible, a new one every half hour, fetched from the
//! internet (no Bible text ships with the app). When it changes while the notch is closed, the
//! pill shows the reference for a few seconds.

use crate::ayat_win::fetch_chapter;
use crate::system::epoch_ms;
use isle_logic::ayat::{Passage, chapter_for, chapter_query, choose, next_change_ms, parse_chapter, progress, slot_of};
use isle_logic::canvas::{Align, Canvas, Color, Rect, wrap_lines};
use isle_logic::module::{Event, Module, PillItem, Reaction};
use isle_logic::peek::Peek;
use std::sync::{Arc, Mutex};

const WHITE: Color = Color(245, 246, 248, 255);
const DIM: Color = Color(139, 144, 153, 255);
const CARD: Color = Color(20, 22, 26, 255);
const FADE_MS: i64 = 450;
const PEEK_MS: i64 = 4500;
/// Chapters tried per half hour before giving up until the next retry.
const TRIES: u32 = 4;

type Slot = Arc<Mutex<Option<(i64, Option<Passage>)>>>;

pub struct AyatModule {
    passage: Option<Passage>,
    shown_slot: Option<i64>,
    /// The half hour being fetched right now.
    fetching: Option<i64>,
    retry_at: i64,
    failures: u32,
    result: Slot,
    changed_ms: i64,
    peek: Peek,
    last_key: String,
}

fn fade(c: Color, k: f32) -> Color {
    Color(c.0, c.1, c.2, (c.3 as f32 * k.clamp(0.0, 1.0)) as u8)
}

impl AyatModule {
    pub fn new() -> Self {
        Self { passage: None, shown_slot: None, fetching: None, retry_at: 0, failures: 0, result: Arc::default(), changed_ms: 0, peek: Peek::default(), last_key: String::new() }
    }

    fn start_fetch(&mut self, slot: i64) {
        self.fetching = Some(slot);
        let out = Arc::clone(&self.result);
        std::thread::spawn(move || {
            let mut found = None;
            for attempt in 0..TRIES {
                let (book, chapter, seed) = chapter_for(slot, attempt);
                match fetch_chapter(&chapter_query(book, chapter)) {
                    Ok(xml) => {
                        if let Some(p) = choose(book, &parse_chapter(&xml), seed, chapter) {
                            found = Some(p);
                            break;
                        }
                    }
                    Err(_) => break, // no connection: do not hammer the server
                }
            }
            *out.lock().unwrap() = Some((slot, found));
        });
    }
}

impl Module for AyatModule {
    fn id(&self) -> &'static str {
        "ayat"
    }
    fn title(&self) -> &'static str {
        "Ayat"
    }
    fn pill_left(&self) -> Option<PillItem> {
        None
    }
    fn pill_center(&self) -> Option<String> {
        self.peek.current(epoch_ms()).map(str::to_string)
    }
    fn pill_priority(&self) -> u8 {
        12
    }
    fn wide_pill(&self) -> bool {
        self.peek.current(epoch_ms()).is_some()
    }

    fn poll(&mut self, now: i64) -> bool {
        let mut changed = false;
        let done = self.result.lock().unwrap().take();
        if let Some((slot, found)) = done {
            self.fetching = None;
            match found {
                Some(p) => {
                    let first = self.passage.is_none();
                    // Only the latest half hour counts; a late answer for an old one is still better than nothing.
                    self.passage = Some(p.clone());
                    self.shown_slot = Some(slot);
                    self.failures = 0;
                    self.changed_ms = now;
                    if !first {
                        self.peek.show(format!("Ayat · {}", p.reference), now, PEEK_MS);
                    }
                }
                None => {
                    self.failures += 1;
                    self.retry_at = now + if self.failures < 4 { 8_000 } else { 60_000 };
                }
            }
            changed = true;
        }
        let slot = slot_of(now);
        if self.shown_slot != Some(slot) && self.fetching.is_none() && now >= self.retry_at {
            self.start_fetch(slot);
            changed = true;
        }
        let key = format!("{}|{}|{}", self.passage.as_ref().map_or("", |p| p.reference.as_str()), (next_change_ms(now) - now) / 60_000, self.peek.current(now).is_some());
        changed |= key != self.last_key;
        self.last_key = key;
        changed
    }

    fn draw_panel(&mut self, c: &mut dyn Canvas, area: Rect, now: i64) {
        c.fill_round_rect(area, 13.0, CARD);
        let x = area.x + 16.0;
        let w = area.w - 32.0;
        c.draw_text("Ayat", Rect { x, y: area.y + 9.0, w: 80.0, h: 14.0 }, 11.0, 400, DIM, Align::Left);
        let mins = ((next_change_ms(now) - now) / 60_000).max(1);
        c.draw_text(&format!("berikutnya {mins} mnt"), Rect { x: x + w - 140.0, y: area.y + 9.0, w: 140.0, h: 14.0 }, 10.5, 400, DIM, Align::Right);
        match &self.passage {
            Some(p) => {
                let k = ((now - self.changed_ms) as f32 / FADE_MS as f32).clamp(0.0, 1.0);
                let lines = wrap_lines(c, &p.text, 13.0, 400, w, 4);
                for (i, line) in lines.iter().enumerate() {
                    c.draw_text(line, Rect { x, y: area.y + 28.0 + i as f32 * 17.0 + (1.0 - k) * 4.0, w, h: 17.0 }, 13.0, 400, fade(WHITE, k), Align::Left);
                }
                c.draw_text(&p.reference, Rect { x, y: area.y + area.h - 24.0, w, h: 14.0 }, 11.5, 700, fade(WHITE, k), Align::Left);
            }
            None => {
                let text = if self.failures > 0 { "Belum tersambung. Mencoba lagi…" } else { "Mengambil ayat…" };
                c.draw_text(text, Rect { x, y: area.y + 44.0, w, h: 20.0 }, 13.0, 400, DIM, Align::Left);
            }
        }
        // Time left until the next passage.
        let bar = Rect { x: area.x + 13.0, y: area.y + area.h - 3.0, w: area.w - 26.0, h: 2.0 };
        c.fill_round_rect(Rect { w: (bar.w * progress(now)).max(2.0), ..bar }, 1.0, Color(WHITE.0, WHITE.1, WHITE.2, 110));
    }

    fn on_event(&mut self, _e: &Event, _area: Rect, _now: i64) -> Reaction {
        Reaction::None
    }

    fn next_wake_ms(&self, now: i64) -> Option<i64> {
        let fade = (self.changed_ms > 0 && now - self.changed_ms < FADE_MS).then_some(now + 16);
        let peek = self.peek.next_wake_ms().filter(|&t| t > now);
        let retry = (self.shown_slot != Some(slot_of(now)) && self.fetching.is_none()).then_some(self.retry_at.max(now + 1));
        let minute = Some((now / 60_000 + 1) * 60_000);
        [fade, peek, retry, minute].into_iter().flatten().min()
    }
}
