// SPDX-License-Identifier: GPL-3.0-or-later
//! Notifications page: the newest Windows notification large, the three before it small.
//! A new one also shows in the closed pill for a few seconds.

use crate::notif_win;
use crate::system::{epoch_ms, local_time};
use isle_logic::canvas::{Align, Canvas, Color, ImageId, Rect, fit_text, wrap_lines};
use std::collections::HashMap;
use isle_logic::module::{Event, Module, PillItem, Reaction};
use isle_logic::notifs::{Feed, Notif, arrange, letter, scroll_offset, snippet, tile_color};
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
    /// A swap in progress: where each card was, and when it started (epoch ms).
    swap: Option<(Vec<(u32, Spot)>, i64)>,
    /// The card now in the large slot and when it got there, to time the scrolling of long text.
    shown: Option<(u32, i64)>,
    /// When the panel was last drawn, and whether some text on it is currently scrolling.
    last_draw_ms: i64,
    scrolling: bool,
}

/// Where a card sits: the large slot or one of the three side rows.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Spot {
    Main,
    Side(usize),
}

const SWAP_MS: f32 = 420.0;

/// The large card and the three side rows, relative to the page area.
fn layout(area: Rect) -> (Rect, [Rect; 3]) {
    let main = Rect { w: (area.w - GAP) * 0.6, ..area };
    let side_x = main.x + main.w + GAP;
    let side_w = area.w - main.w - GAP;
    let row_h = (area.h - SMALL_GAP * 2.0) / 3.0;
    let row = |i: usize| Rect { x: side_x, y: area.y + i as f32 * (row_h + SMALL_GAP), w: side_w, h: row_h };
    (main, [row(0), row(1), row(2)])
}

fn rect_of(spot: Spot, area: Rect) -> (Rect, f32) {
    let (main, rows) = layout(area);
    match spot {
        Spot::Main => (main, 1.0),
        Spot::Side(i) => (rows[i], 0.0),
    }
}

fn lerp(a: f32, b: f32, k: f32) -> f32 {
    a + (b - a) * k
}

fn ease_in_out(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn alpha(c: Color, a: f32) -> Color {
    Color(c.0, c.1, c.2, (c.3 as f32 * a.clamp(0.0, 1.0)) as u8)
}

/// Where every shown card sits for this selection (item index, spot).
fn placements(items: &[Notif], selected: Option<u32>) -> Vec<(usize, Spot)> {
    let (main, side) = arrange(items, selected);
    main.map(|m| (m, Spot::Main)).into_iter().chain(side.into_iter().enumerate().map(|(slot, i)| (i, Spot::Side(slot)))).collect()
}

impl NotificationsModule {
    pub fn new() -> Self {
        notif_win::spawn();
        Self { feed: Feed::default(), peek: Peek::default(), icons: HashMap::new(), selected: None, swap: None, shown: None, last_draw_ms: i64::MIN / 2, scrolling: false }
    }

    fn swapping(&self, now_ms: i64) -> bool {
        self.swap.as_ref().is_some_and(|(_, t0)| ((now_ms - t0) as f32) < SWAP_MS)
    }

    /// One card at `r`, between a compact row (`e` = 0) and the large card (`e` = 1).
    fn draw_card(&self, c: &mut dyn Canvas, r: Rect, n: &Notif, e: f32, big_w: f32, now_ms: i64, shown_ms: i64) -> bool {
        let mut scrolling = false;
        let logo = self.icons.get(&n.app).copied().flatten();
        c.fill_round_rect(r, lerp(11.0, 13.0, e), CARD);
        c.push_clip(r, lerp(11.0, 13.0, e));
        let (size, tx0, ty0) = (lerp(20.0, 28.0, e), lerp(r.x + 9.0, r.x + 13.0, e), lerp(r.y + (r.h - 20.0) / 2.0, r.y + 11.0, e));
        tile(c, Rect { x: tx0, y: ty0, w: size, h: size }, &n.app, lerp(10.0, 13.0, e), logo);
        if e < 0.98 {
            let label = fit_text(c, &side_label(n), 12.0, 600, (big_w - 46.0).min(r.w - 46.0).max(10.0));
            c.draw_text(&label, Rect { x: r.x + 37.0, y: r.y + (r.h - 18.0) / 2.0, w: r.w - 46.0, h: 18.0 }, 12.0, 600, alpha(WHITE, 1.0 - e * 2.0), Align::Left);
        }
        if e > 0.02 {
            let a = ((e - 0.35) / 0.65).clamp(0.0, 1.0);
            let tx = r.x + 50.0;
            let tw = big_w - 63.0;
            let full_title = if n.title.is_empty() { &n.app } else { &n.title };
            let title_w = c.measure_text(full_title, 13.0, 650);
            if e > 0.98 && title_w > tw {
                // Too long for the card: glide it sideways until the end shows.
                scrolling = true;
                let dx = scroll_offset(title_w - tw, shown_ms);
                c.push_clip(Rect { x: tx, y: r.y + 9.0, w: tw, h: 18.0 }, 0.0);
                c.draw_text(full_title, Rect { x: tx - dx, y: r.y + 9.0, w: title_w + 24.0, h: 18.0 }, 13.0, 650, alpha(WHITE, a), Align::Left);
                c.pop_clip();
            } else {
                let title = fit_text(c, full_title, 13.0, 650, tw);
                c.draw_text(&title, Rect { x: tx, y: r.y + 9.0, w: tw, h: 18.0 }, 13.0, 650, alpha(WHITE, a), Align::Left);
            }
            let meta = fit_text(c, &format!("{} · {}", n.app, clock_of(n.at_ms, now_ms)), 10.5, 400, tw);
            c.draw_text(&meta, Rect { x: tx, y: r.y + 26.0, w: tw, h: 14.0 }, 10.5, 400, alpha(DIM, a), Align::Left);
            let body = if n.title.is_empty() { "" } else { n.body.as_str() };
            const VISIBLE: usize = 3;
            let lines = wrap_lines(c, body, 13.0, 400, big_w - 26.0, 60);
            let dy = if e > 0.98 && lines.len() > VISIBLE {
                // More lines than fit: roll them upwards until the last one is shown.
                scrolling = true;
                scroll_offset((lines.len() - VISIBLE) as f32 * 18.0, shown_ms)
            } else {
                0.0
            };
            let area = Rect { x: r.x + 13.0, y: r.y + 50.0 + 5.0 * (1.0 - a), w: big_w - 26.0, h: VISIBLE as f32 * 18.0 + 4.0 };
            c.push_clip(area, 0.0);
            // While the card is still moving, show three lines with an ellipsis; once settled,
            // the clip hides the rest and the lines roll by.
            let settled = e > 0.98;
            for (i, line) in lines.iter().enumerate().take(if settled { lines.len() } else { VISIBLE }) {
                let cut = !settled && lines.len() > VISIBLE && i + 1 == VISIBLE;
                let text = if cut { fit_text(c, &format!("{line}…"), 13.0, 400, big_w - 26.0) } else { line.clone() };
                c.draw_text(&text, Rect { x: area.x, y: r.y + 50.0 + i as f32 * 18.0 + 5.0 * (1.0 - a) - dy, w: big_w - 26.0, h: 18.0 }, 13.0, 400, alpha(BODY, a), Align::Left);
            }
            c.pop_clip();
        }
        c.pop_clip();
        scrolling
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
                self.swap = None;
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
        let items = self.feed.items();
        let now_places = placements(items, self.selected);
        if now_places.is_empty() {
            c.fill_round_rect(area, 13.0, CARD);
            c.draw_text("Belum ada notifikasi", Rect { y: area.y + area.h / 2.0 - 10.0, h: 20.0, ..area }, 13.0, 600, DIM, Align::Center);
            return;
        }
        let (main, _) = layout(area);
        let main_id = now_places.iter().find(|(_, s)| *s == Spot::Main).map(|&(i, _)| items[i].id);
        // Start the scrolling clock when a card takes the large slot, or when the panel reopens.
        let reopened = now_ms - self.last_draw_ms > 400;
        if reopened || self.shown.map(|(id, _)| id) != main_id {
            self.shown = main_id.map(|id| (id, now_ms));
        }
        self.last_draw_ms = now_ms;
        let shown_ms = self.shown.map_or(0, |(_, t0)| now_ms - t0);
        let mut scrolling = false;
        let k = match &self.swap {
            Some((_, t0)) => ease_in_out((now_ms - t0) as f32 / SWAP_MS),
            None => 1.0,
        };
        // Side cards first, the large card last so it slides over the others.
        let mut order = now_places.clone();
        order.sort_by_key(|(_, s)| matches!(s, Spot::Main));
        for (idx, spot) in order {
            let n = &items[idx];
            let (end, end_e) = rect_of(spot, area);
            let (r, e) = match self.swap.as_ref().and_then(|(from, _)| from.iter().find(|(id, _)| *id == n.id)) {
                Some(&(_, from_spot)) if k < 1.0 => {
                    let (start, start_e) = rect_of(from_spot, area);
                    (Rect { x: lerp(start.x, end.x, k), y: lerp(start.y, end.y, k), w: lerp(start.w, end.w, k), h: lerp(start.h, end.h, k) }, lerp(start_e, end_e, k))
                }
                _ => (end, end_e),
            };
            scrolling |= self.draw_card(c, r, n, e, main.w, now_ms, shown_ms);
        }
        self.scrolling = scrolling;
    }

    fn on_event(&mut self, e: &Event, area: Rect, now_ms: i64) -> Reaction {
        let Event::PointerDown(x, y) = *e else {
            return Reaction::None;
        };
        if self.swapping(now_ms) {
            return Reaction::None;
        }
        let items = self.feed.items();
        let (_, rows) = layout(area);
        let before = placements(items, self.selected);
        for &(idx, spot) in &before {
            let Spot::Side(slot) = spot else { continue };
            let r = rows[slot];
            if x >= r.x && x < r.x + r.w && y >= r.y && y < r.y + r.h {
                self.swap = Some((before.iter().map(|&(i, s)| (items[i].id, s)).collect(), now_ms));
                self.selected = Some(items[idx].id);
                return Reaction::Redraw;
            }
        }
        Reaction::None
    }

    fn next_wake_ms(&self, now_ms: i64) -> Option<i64> {
        // Keep frames coming while the cards trade places.
        let moving = self.swapping(now_ms).then_some(now_ms + 16);
        // Long text is scrolling while this page is on screen (it was drawn a moment ago).
        let rolling = (self.scrolling && now_ms - self.last_draw_ms < 100).then_some(now_ms + 16);
        [self.peek.wake_after(now_ms), moving, rolling].into_iter().flatten().min()
    }
}

fn side_label(n: &Notif) -> String {
    if n.title.is_empty() { n.body.clone() } else { n.title.clone() }
}
