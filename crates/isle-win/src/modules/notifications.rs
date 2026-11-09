// SPDX-License-Identifier: GPL-3.0-or-later
//! Notifications page: the newest Windows notification large, the three before it small.
//! A new one also shows in the closed pill for a few seconds.

use crate::notif_win;
use crate::system::{epoch_ms, local_time};
use isle_logic::canvas::{Align, Canvas, Color, ImageId, Rect, fit_text, wrap_lines};
use std::collections::HashMap;
use isle_logic::module::{Event, Module, PillItem, Reaction};
use isle_logic::notifs::{Feed, Notif, arrange, letter, snippet, tile_color};
use isle_logic::peek::Peek;
use isle_logic::timefmt::format_clock;

const WHITE: Color = Color(245, 246, 248, 255);
const DIM: Color = Color(139, 144, 153, 255);
const BODY: Color = Color(214, 217, 223, 255);
const CARD: Color = Color(20, 22, 26, 255);
const GAP: f32 = 12.0;
const SMALL_GAP: f32 = 6.0;
const PEEK_MS: i64 = 5000;
const PILL_CHARS: usize = 38;

pub struct NotificationsModule {
    feed: Feed,
    peek: Peek,
    /// Uploaded logos by app name (`None`: this app has no readable logo, so it keeps a letter tile).
    icons: HashMap<String, Option<ImageId>>,
    /// An older notification the user opened into the large card (by id).
    selected: Option<u32>,
    /// When the large card last changed, for its fade-in (epoch ms).
    swapped_at: i64,
}

const SWAP_MS: f32 = 240.0;

/// The large card and the three side rows, relative to the page area.
fn layout(area: Rect) -> (Rect, [Rect; 3]) {
    let main = Rect { w: (area.w - GAP) * 0.6, ..area };
    let side_x = main.x + main.w + GAP;
    let side_w = area.w - main.w - GAP;
    let row_h = (area.h - SMALL_GAP * 2.0) / 3.0;
    let row = |i: usize| Rect { x: side_x, y: area.y + i as f32 * (row_h + SMALL_GAP), w: side_w, h: row_h };
    (main, [row(0), row(1), row(2)])
}

impl NotificationsModule {
    pub fn new() -> Self {
        notif_win::spawn();
        Self { feed: Feed::default(), peek: Peek::default(), icons: HashMap::new(), selected: None, swapped_at: 0 }
    }
}

/// "17:44" for a notification that arrived `at_ms`, in local time.
fn clock_of(at_ms: i64, now_ms: i64) -> String {
    let t = local_time();
    let now_min = (t.hour * 60 + t.minute) as i64;
    let age_min = ((now_ms - at_ms).max(0) / 60_000) % 1440;
    let m = (now_min - age_min).rem_euclid(1440);
    format_clock((m / 60) as u32, (m % 60) as u32)
}

fn tile(c: &mut dyn Canvas, r: Rect, app: &str, size: f32, logo: Option<ImageId>) {
    if let Some(id) = logo {
        c.fill_round_rect(r, r.w * 0.27, Color(43, 47, 54, 255));
        c.draw_image(id, r, r.w * 0.27);
        return;
    }
    let (cr, cg, cb) = tile_color(app);
    c.fill_round_rect(r, r.w * 0.27, Color(cr, cg, cb, 255));
    c.draw_text(&letter(app).to_string(), Rect { y: r.y + (r.h - size - 4.0) / 2.0, h: size + 4.0, ..r }, size, 700, WHITE, Align::Center);
}

fn short(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        text.to_string()
    } else {
        format!("{}…", text.chars().take(max - 1).collect::<String>().trim_end())
    }
}

impl Module for NotificationsModule {
    fn id(&self) -> &'static str {
        "notifications"
    }
    fn title(&self) -> &'static str {
        "Notifikasi"
    }
    fn pill_left(&self) -> Option<PillItem> {
        None
    }
    fn pill_center(&self) -> Option<String> {
        self.peek.current(epoch_ms()).map(str::to_string)
    }
    fn pill_priority(&self) -> u8 {
        35
    }
    fn wide_pill(&self) -> bool {
        self.peek.current(epoch_ms()).is_some()
    }

    fn poll(&mut self, now_ms: i64) -> bool {
        let before = self.peek.current(now_ms).is_some();
        let mut changed = false;
        if let Some(all) = notif_win::take() {
            let old: Vec<u32> = self.feed.items().iter().map(|n| n.id).collect();
            let fresh = self.feed.update(all);
            changed = old != self.feed.items().iter().map(|n| n.id).collect::<Vec<_>>();
            if let Some(newest) = fresh.first() {
                self.selected = None;
                self.swapped_at = now_ms;
                self.peek.show(short(&snippet(newest), PILL_CHARS), now_ms, PEEK_MS);
            }
        }
        changed || self.peek.current(now_ms).is_some() != before
    }

    fn draw_panel(&mut self, c: &mut dyn Canvas, area: Rect, now_ms: i64) {
        // Upload any logo the worker has found since the last draw.
        for n in self.feed.items().iter().take(4) {
            if self.icons.get(&n.app).is_none_or(|i| i.is_none()) {
                if let Some(ic) = notif_win::icon(&n.app) {
                    self.icons.insert(n.app.clone(), Some(c.load_image(&ic.rgba, ic.w, ic.h)));
                }
            }
        }
        let logo = |app: &str| self.icons.get(app).copied().flatten();
        let items = self.feed.items();
        let (main_idx, side) = arrange(items, self.selected);
        let Some(first) = main_idx.map(|i| &items[i]) else {
            c.fill_round_rect(area, 13.0, CARD);
            c.draw_text("Belum ada notifikasi", Rect { y: area.y + area.h / 2.0 - 10.0, h: 20.0, ..area }, 13.0, 600, DIM, Align::Center);
            return;
        };
        let (main, rows) = layout(area);
        let k = ((now_ms - self.swapped_at) as f32 / SWAP_MS).clamp(0.0, 1.0);
        let fade = |col: Color| Color(col.0, col.1, col.2, (col.3 as f32 * (0.15 + 0.85 * (1.0 - (1.0 - k).powi(3)))) as u8);
        c.fill_round_rect(main, 13.0, CARD);
        tile(c, Rect { x: main.x + 13.0, y: main.y + 11.0, w: 28.0, h: 28.0 }, &first.app, 13.0, logo(&first.app));
        let tx = main.x + 50.0;
        let tw = main.w - 63.0;
        let title = fit_text(c, if first.title.is_empty() { &first.app } else { &first.title }, 13.0, 650, tw);
        c.draw_text(&title, Rect { x: tx, y: main.y + 9.0, w: tw, h: 18.0 }, 13.0, 650, fade(WHITE), Align::Left);
        let meta = fit_text(c, &format!("{} · {}", first.app, clock_of(first.at_ms, now_ms)), 10.5, 400, tw);
        c.draw_text(&meta, Rect { x: tx, y: main.y + 26.0, w: tw, h: 14.0 }, 10.5, 400, fade(DIM), Align::Left);
        let body = if first.title.is_empty() { "" } else { first.body.as_str() };
        for (i, line) in wrap_lines(c, body, 13.0, 400, main.w - 26.0, 3).iter().enumerate() {
            c.draw_text(line, Rect { x: main.x + 13.0, y: main.y + 50.0 + i as f32 * 18.0 + 4.0 * (1.0 - k), w: main.w - 26.0, h: 18.0 }, 13.0, 400, fade(BODY), Align::Left);
        }

        for (slot, &idx) in side.iter().enumerate() {
            let n = &items[idx];
            let r = rows[slot];
            c.fill_round_rect(r, 11.0, CARD);
            tile(c, Rect { x: r.x + 9.0, y: r.y + (r.h - 20.0) / 2.0, w: 20.0, h: 20.0 }, &n.app, 10.0, logo(&n.app));
            let label = side_label(n);
            let text = fit_text(c, &label, 12.0, 600, r.w - 46.0);
            c.draw_text(&text, Rect { x: r.x + 37.0, y: r.y + (r.h - 18.0) / 2.0, w: r.w - 46.0, h: 18.0 }, 12.0, 600, WHITE, Align::Left);
        }
    }

    fn on_event(&mut self, e: &Event, area: Rect, now_ms: i64) -> Reaction {
        let Event::PointerDown(x, y) = *e else {
            return Reaction::None;
        };
        let items = self.feed.items();
        let (_, side) = arrange(items, self.selected);
        let (_, rows) = layout(area);
        for (slot, &idx) in side.iter().enumerate() {
            let r = rows[slot];
            if x >= r.x && x < r.x + r.w && y >= r.y && y < r.y + r.h {
                self.selected = Some(items[idx].id);
                self.swapped_at = now_ms;
                return Reaction::Redraw;
            }
        }
        Reaction::None
    }

    fn next_wake_ms(&self, now_ms: i64) -> Option<i64> {
        // The large card fades in after a swap: keep frames coming until it has.
        let fading = ((now_ms - self.swapped_at) as f32) < SWAP_MS + 40.0;
        let fade = fading.then_some(now_ms + 16);
        [self.peek.next_wake_ms(), fade].into_iter().flatten().min()
    }
}

fn side_label(n: &Notif) -> String {
    if n.title.is_empty() { n.body.clone() } else { n.title.clone() }
}
