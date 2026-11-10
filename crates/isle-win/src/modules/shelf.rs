// SPDX-License-Identifier: GPL-3.0-or-later
//! Shelf page: park files on the notch. Drop files on it, drag them back out into any app,
//! or click one to copy it. Files are referenced by path and never copied.

use super::button::Button;
use crate::shell_win::ThumbWorker;
use crate::phone;
use crate::system::{data_path, epoch_ms};
use isle_logic::canvas::{Align, Canvas, Color, ImageId, Rect, fit_text};
use isle_logic::module::{Event, Module, PillItem, Reaction};
use isle_logic::peek::Peek;
use isle_logic::shelf::{Shelf, extension, file_name};
use isle_logic::shelf_layout::{ShelfHit, TILE_GAP, TILE_W, hit, layout, max_scroll, remove_rect, send_rect, tile_rect};
use std::collections::{HashMap, HashSet};

const BLUE: Color = Color(59, 130, 246, 255);
const PEEK_MS: i64 = 4500;
const VIEW_MS: f32 = 340.0;
const VIEW_SLIDE: f32 = 36.0;

const WHITE: Color = Color(245, 246, 248, 255);
const DIM: Color = Color(139, 144, 153, 255);
const ROW: Color = Color(20, 22, 26, 255);
const FLASH: Color = Color(18, 48, 31, 255);
const GREEN: Color = Color(52, 211, 153, 255);
const AMBER: Color = Color(251, 191, 36, 255);
const BADGE: Color = Color(43, 47, 54, 255);
const ENTER_MS: f32 = 320.0;
const COPIED_MS: i64 = 1000;
/// A removed tile shrinks and fades first; only then do its neighbours glide into the gap.
const FADE_MS: f32 = 220.0;
const SLIDE_DELAY_MS: f32 = 120.0;
const SLIDE_MS: f32 = 420.0;
/// Total time a removal keeps animating.
const LEAVE_MS: f32 = SLIDE_DELAY_MS + SLIDE_MS;

fn e_x(e: &Event) -> f32 {
    match e {
        Event::PointerDown(x, _) | Event::PointerDrag(x, _) => *x,
        _ => -1.0,
    }
}

fn e_y(e: &Event) -> f32 {
    match e {
        Event::PointerDown(_, y) | Event::PointerDrag(_, y) => *y,
        _ => -1.0,
    }
}

fn ease_out(t: f32) -> f32 {
    1.0 - (1.0 - t.clamp(0.0, 1.0)).powi(3)
}

fn ease_in_out(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Settles with a small overshoot, so the neighbours land softly instead of stopping dead.
fn ease_out_soft(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0) - 1.0;
    let c = 1.1;
    1.0 + (c + 1.0) * t * t * t + c * t * t
}

/// A tile on its way out: it keeps its old slot while it fades.
struct Leaving {
    path: String,
    id: u64,
    index: usize,
    t0: i64,
}
/// Pointer travel before a press becomes a drag out.
const DRAG_PX: f32 = 6.0;

enum Thumb {
    /// Asked the worker for it.
    Loading,
    Ready(ImageId),
    /// No picture could be made; show the extension badge.
    None,
}

pub struct ShelfModule {
    shelf: Shelf,
    scroll: f32,
    clear_btn: Button,
    /// Tile pressed and where, until it turns into a click or a drag.
    press: Option<(usize, f32, f32)>,
    to_drag: Option<Vec<String>>,
    to_copy: Option<Vec<String>>,
    thumbs: HashMap<String, Thumb>,
    worker: ThumbWorker,
    /// Pictures made by the worker, waiting for the next frame to upload them.
    fresh: Vec<(String, Option<(Vec<u8>, u32, u32)>)>,
    /// Files being dragged over the notch right now.
    over: bool,
    /// When each tile (by id) arrived, for its slide-in.
    arrived: HashMap<u64, i64>,
    /// The tile last copied and when.
    copied: Option<(u64, i64)>,
    anim_until: i64,
    leaving: Vec<Leaving>,
    /// Showing the phone link (QR code and buttons) instead of the tiles.
    phone_view: bool,
    view_anim: Option<(i64, bool)>,
    phone_btn: Button,
    done_btn: Button,
    stop_btn: Button,
    forget_btn: Button,
    status: phone::Status,
    /// The QR code for the current address: (address, side, dark cells).
    qr: Option<(String, usize, Vec<bool>)>,
    /// Tiles (by id) the phone may download.
    sent: HashSet<u64>,
    /// Files arriving from the phone right now.
    pending: Vec<phone::Transfer>,
    err: Option<(String, i64)>,
    peek: Peek,
    peek_n: usize,
    peek_name: String,
}

impl ShelfModule {
    pub fn new() -> Self {
        let mut shelf = Shelf::default();
        let saved: Vec<String> = std::fs::read_to_string(data_path("shelf.json")).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
        shelf.add(&saved);
        shelf.retain_existing(|p| std::path::Path::new(p).exists());
        Self {
            shelf,
            scroll: 0.0,
            clear_btn: Button::new(AMBER),
            press: None,
            to_drag: None,
            to_copy: None,
            thumbs: HashMap::new(),
            worker: ThumbWorker::spawn(),
            fresh: Vec::new(),
            over: false,
            arrived: HashMap::new(),
            copied: None,
            anim_until: 0,
            leaving: Vec::new(),
            phone_view: false,
            view_anim: None,
            phone_btn: Button::new(GREEN),
            done_btn: Button::new(AMBER),
            stop_btn: Button::new(AMBER),
            forget_btn: Button::new(AMBER),
            status: phone::status(),
            qr: None,
            sent: HashSet::new(),
            pending: Vec::new(),
            err: None,
            peek: Peek::default(),
            peek_n: 0,
            peek_name: String::new(),
        }
    }

    fn poll_thumbs(&mut self) -> bool {
        // Ask for any picture not yet requested, and collect what the worker has made.
        for i in self.shelf.items() {
            if !self.thumbs.contains_key(&i.path) {
                self.worker.request(&i.path);
                self.thumbs.insert(i.path.clone(), Thumb::Loading);
            }
        }
        let made = self.worker.take();
        let any = !made.is_empty();
        self.fresh.extend(made);
        any
    }

    fn draw_tiles(&mut self, c: &mut dyn Canvas, area: Rect, now_ms: i64) {
        let l = layout(area);
        let p = self.pending.len();
        let n = self.shelf.items().len();
        let caption = match (n, self.over) {
            _ if self.err.is_some() => self.err.as_ref().map(|(m, _)| m.clone()).unwrap_or_default(),
            (_, true) => "Lepaskan untuk menaruh di Shelf".to_string(),
            (0, _) => "Shelf kosong".to_string(),
            (n, _) => format!("{n} file · seret keluar atau klik untuk menyalin"),
        };
        c.draw_text(&caption, l.caption, 12.0, 400, if self.over || self.err.is_some() { GREEN } else { DIM }, Align::Left);
        let running = self.status.running;
        self.phone_btn.draw(c, l.phone, "Ponsel", running, now_ms);
        self.clear_btn.draw(c, l.clear, "Kosongkan", false, now_ms);

        self.scroll = self.scroll.clamp(0.0, max_scroll(&l, n + p));
        if n + p == 0 {
            // An empty shelf is a drop zone.
            let zone = Rect { x: l.tiles.x, y: l.tiles.y, w: l.tiles.w, h: l.tiles.h };
            let edge = if self.over { GREEN } else { Color(58, 63, 72, 255) };
            c.fill_round_rect(zone, 14.0, edge);
            c.fill_round_rect(Rect { x: zone.x + 1.5, y: zone.y + 1.5, w: zone.w - 3.0, h: zone.h - 3.0 }, 12.5, if self.over { FLASH } else { ROW });
            c.draw_text("Seret file ke sini", Rect { y: zone.y + 28.0, h: 18.0, ..zone }, 13.0, 650, WHITE, Align::Center);
            c.draw_text("Dari Explorer atau jendela mana pun", Rect { y: zone.y + 48.0, h: 16.0, ..zone }, 11.5, 400, DIM, Align::Center);
        }
        c.push_clip(l.tiles, 0.0);
        // Files still arriving from the phone, with a progress ring.
        for (k, t) in self.pending.iter().enumerate() {
            let r = tile_rect(&l, k, self.scroll);
            let frac = if t.total > 0 { t.done as f32 / t.total as f32 } else { 0.0 };
            c.fill_round_rect(r, 12.0, ROW);
            let (cx, cy) = (r.x + r.w / 2.0, r.y + 33.0);
            c.stroke_arc(cx, cy, 19.0, 0.0, 359.9, 3.0, Color(38, 42, 49, 255));
            c.stroke_arc(cx, cy, 19.0, -90.0, 359.0 * frac.clamp(0.01, 1.0), 3.0, BLUE);
            c.draw_text(&format!("{}%", (frac * 100.0) as u32), Rect { x: cx - 20.0, y: cy - 8.0, w: 40.0, h: 16.0 }, 11.0, 650, WHITE, Align::Center);
            let shown = fit_text(c, &t.name, 11.5, 500, r.w - 12.0);
            c.draw_text(&shown, Rect { x: r.x + 6.0, y: r.y + 62.0, w: r.w - 12.0, h: 16.0 }, 11.5, 500, WHITE, Align::Center);
            c.draw_text("dari ponsel", Rect { x: r.x + 6.0, y: r.y + 76.0, w: r.w - 12.0, h: 14.0 }, 10.0, 500, Color(96, 165, 250, 255), Align::Center);
        }
        for (i, item) in self.shelf.items().iter().enumerate() {
            let mut r = tile_rect(&l, i + p, self.scroll);
            // Tiles after a removed one glide left into the gap.
            for g in &self.leaving {
                if i >= g.index {
                    let e = ease_out_soft(((now_ms - g.t0).max(0) as f32 - SLIDE_DELAY_MS) / SLIDE_MS);
                    r.x += (TILE_W + TILE_GAP) * (1.0 - e);
                }
            }
            if r.x + r.w < l.tiles.x || r.x > l.tiles.x + l.tiles.w {
                continue;
            }
            // A newly dropped tile settles in from slightly above.
            let enter = self.arrived.get(&item.id).map_or(1.0, |&t0| ease_out((now_ms - t0) as f32 / ENTER_MS));
            self.draw_tile(c, r, &item.path, item.id, enter, true, now_ms);
        }
        // Tiles being removed shrink and fade where they were.
        for g in &self.leaving {
            let e = ease_in_out(((now_ms - g.t0).max(0)) as f32 / FADE_MS);
            let r = tile_rect(&l, g.index + p, self.scroll);
            let s = 1.0 - 0.3 * e;
            let r = Rect { x: r.x + r.w * (1.0 - s) / 2.0, y: r.y + r.h * (1.0 - s) / 2.0 - 6.0 * e, w: r.w * s, h: r.h * s };
            self.draw_tile(c, r, &g.path, g.id, 1.0 - e, false, now_ms);
        }
        c.pop_clip();
        let max = max_scroll(&l, n + p);
        if max > 0.0 {
            let track = l.tiles.w;
            let thumb = (track * track / (track + max)).max(20.0);
            let x = l.tiles.x + (track - thumb) * (self.scroll / max);
            c.fill_round_rect(Rect { x, y: l.tiles.y + l.tiles.h + 3.0, w: thumb, h: 2.0 }, 1.0, Color(90, 96, 106, 200));
        }
        self.arrived.retain(|_, &mut t0| ((now_ms - t0) as f32) < ENTER_MS);
        self.leaving.retain(|g| ((now_ms - g.t0) as f32) < LEAVE_MS);
        if self.copied.is_some_and(|(_, t0)| now_ms - t0 > COPIED_MS) {
            self.copied = None;
        }
    }

    /// Switches between the tiles and the QR page, with a slide.
    fn set_view(&mut self, phone: bool, now_ms: i64) {
        if self.phone_view != phone {
            self.phone_view = phone;
            self.view_anim = Some((now_ms, phone));
            self.anim_until = self.anim_until.max(now_ms + VIEW_MS as i64 + 40);
        }
    }

    fn sync_outgoing(&self) {
        phone::set_outgoing(self.shelf.items().iter().filter(|i| self.sent.contains(&i.id)).map(|i| (i.id, i.path.clone())).collect());
    }

    fn add_files(&mut self, paths: &[String], now_ms: i64) -> bool {
        let before: Vec<u64> = self.shelf.items().iter().map(|i| i.id).collect();
        self.shelf.add(paths);
        for i in self.shelf.items() {
            if !before.contains(&i.id) {
                self.arrived.insert(i.id, now_ms);
            }
        }
        self.scroll = 0.0;
        self.anim_until = now_ms + ENTER_MS as i64 + 60;
        self.save();
        true
    }

    /// Where the three buttons of the phone view sit: done, disconnect, forget.
    fn phone_buttons(a: Rect) -> [Rect; 3] {
        let x = a.x + a.h.min(118.0) + 16.0;
        let w = (a.x + a.w - x - 16.0) / 3.0;
        let y = a.y + a.h - 28.0;
        [0.0, 1.0, 2.0].map(|k| Rect { x: x + k * (w + 8.0), y, w, h: 26.0 })
    }

    fn draw_phone(&mut self, c: &mut dyn Canvas, a: Rect, now_ms: i64) {
        let side = a.h.min(118.0);
        let frame = Rect { x: a.x, y: a.y, w: side, h: side };
        c.fill_round_rect(frame, 10.0, Color(255, 255, 255, 255));
        if let Some((_, n, cells)) = &self.qr {
            let cell = ((side - 8.0) / (*n as f32)).floor().max(1.0);
            let ox = frame.x + (side - cell * *n as f32) / 2.0;
            let oy = frame.y + (side - cell * *n as f32) / 2.0;
            for y in 0..*n {
                let mut x = 0;
                while x < *n {
                    if cells[y * n + x] {
                        let start = x;
                        while x < *n && cells[y * n + x] {
                            x += 1;
                        }
                        c.fill_round_rect(Rect { x: ox + start as f32 * cell, y: oy + y as f32 * cell, w: (x - start) as f32 * cell, h: cell }, 0.0, Color(0, 0, 0, 255));
                    } else {
                        x += 1;
                    }
                }
            }
        }
        let x = a.x + side + 16.0;
        let w = a.x + a.w - x;
        let line = |y: f32| Rect { x, y: a.y + y, w, h: 18.0 };
        c.draw_text("Pindai dengan kamera ponsel", line(0.0), 13.0, 650, WHITE, Align::Left);
        c.draw_text(&format!("{} · satu Wi-Fi dengan PC", self.status.address), line(20.0), 11.5, 400, DIM, Align::Left);
        let info = match self.status.active.first() {
            Some(t) if t.total > 0 => format!("Menerima {} · {}%", t.name, t.done * 100 / t.total),
            Some(t) => format!("Menerima {}", t.name),
            None => format!("Mati sendiri dalam {} menit jika tidak dipakai", self.status.minutes_left),
        };
        let shown = fit_text(c, &info, 11.5, 400, w);
        c.draw_text(&shown, line(38.0), 11.5, 400, GREEN, Align::Left);
        let [done, stop, forget] = Self::phone_buttons(a);
        self.done_btn.draw(c, done, "Selesai", false, now_ms);
        self.stop_btn.draw(c, stop, "Putuskan", false, now_ms);
        self.forget_btn.draw(c, forget, "Lupakan", false, now_ms);
    }

    fn save(&self) {
        let path = data_path("shelf.json");
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(path, serde_json::to_string(&self.shelf.paths()).unwrap_or_default());
    }
}

impl ShelfModule {
    #[allow(clippy::too_many_arguments)]
    fn draw_tile(&self, c: &mut dyn Canvas, r: Rect, path: &str, id: u64, enter: f32, with_remove: bool, now_ms: i64) {
        let mut r = r;
        r.y -= 14.0 * (1.0 - enter).min(1.0) * f32::from(with_remove);
            let a = (enter * 255.0) as u8;
            let fade = |col: Color| Color(col.0, col.1, col.2, ((col.3 as f32) * enter) as u8);
            let k = match self.copied {
                Some((cid, t0)) if cid == id => {
                    let ms = (now_ms - t0) as f32;
                    (ms / 120.0).min((COPIED_MS as f32 - ms) / 280.0).clamp(0.0, 1.0)
                }
                _ => 0.0,
            };
            let mixc = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * k).round() as u8;
            c.fill_round_rect(r, 12.0, Color(mixc(ROW.0, FLASH.0), mixc(ROW.1, FLASH.1), mixc(ROW.2, FLASH.2), a));
            let icon = Rect { x: r.x + (r.w - 46.0) / 2.0, y: r.y + 10.0, w: 46.0, h: 46.0 };
            match self.thumbs.get(path) {
                Some(Thumb::Ready(id)) => c.draw_image_alpha(*id, icon, 8.0, enter),
                _ => {
                    c.fill_round_rect(icon, 10.0, fade(BADGE));
                    c.draw_text(&extension(path), icon, 11.5, 700, fade(DIM), Align::Center);
                }
            }
            let name = file_name(path);
            let name_box = Rect { x: r.x + 6.0, y: r.y + 62.0, w: r.w - 12.0, h: 16.0 };
            let shown = fit_text(c, name, 11.5, 500, name_box.w);
            c.draw_text(&shown, Rect { y: name_box.y - 3.0 * k, ..name_box }, 11.5, 500, Color(WHITE.0, WHITE.1, WHITE.2, ((1.0 - k) * a as f32) as u8), Align::Center);
            if k > 0.0 {
                c.draw_text("✓ Tersalin", Rect { y: name_box.y + 5.0 * (1.0 - k), ..name_box }, 11.5, 650, Color(GREEN.0, GREEN.1, GREEN.2, (k * 255.0) as u8), Align::Center);
            }
            let ext = extension(path);
            if !ext.is_empty() && k < 0.5 {
                c.draw_text(&ext, Rect { y: r.y + 76.0, h: 14.0, ..name_box }, 10.0, 500, fade(DIM), Align::Center);
            }
            if phone::is_from_phone(path) {
                c.fill_circle(r.x + 9.0, r.y + r.h - 9.0, 3.0, fade(BLUE));
            }
            if with_remove && (self.status.running || self.sent.contains(&id)) {
                let b = send_rect(r);
                let on = self.sent.contains(&id);
                c.fill_circle(b.x + b.w / 2.0, b.y + b.h / 2.0, 7.0, if on { fade(BLUE) } else { Color(0, 0, 0, (a as f32 * 0.55) as u8) });
                let ink = if on { Color(4, 18, 42, a) } else { fade(DIM) };
                c.fill_round_rect(Rect { x: b.x + b.w / 2.0 - 3.0, y: b.y + b.h / 2.0 - 5.0, w: 6.0, h: 10.0 }, 1.5, ink);
                c.fill_round_rect(Rect { x: b.x + b.w / 2.0 - 2.0, y: b.y + b.h / 2.0 - 3.5, w: 4.0, h: 6.0 }, 0.5, if on { BLUE } else { Color(0, 0, 0, a) });
            }
        // The small remove button.
            if !with_remove {
                return;
            }
            let x = remove_rect(r);
            c.fill_circle(x.x + x.w / 2.0, x.y + x.h / 2.0, 7.0, Color(0, 0, 0, (a as f32 * 0.55) as u8));
            c.draw_text("×", x, 13.0, 400, fade(DIM), Align::Center);
    }
}

impl Module for ShelfModule {
    fn id(&self) -> &'static str {
        "shelf"
    }
    fn title(&self) -> &'static str {
        "Shelf"
    }
    fn pill_left(&self) -> Option<PillItem> {
        None
    }
    fn pill_items(&self) -> Vec<PillItem> {
        if self.peek.current(epoch_ms()).is_none() {
            return Vec::new();
        }
        vec![PillItem::AppTile { image: None, letter: '↓', color: (59, 130, 246) }]
    }
    fn pill_center(&self) -> Option<String> {
        self.peek.current(epoch_ms()).map(str::to_string)
    }
    fn pill_priority(&self) -> u8 {
        45
    }
    fn wide_pill(&self) -> bool {
        self.peek.current(epoch_ms()).is_some()
    }
    fn set_dragging(&mut self, over: bool) {
        self.over = over;
    }
    fn on_files(&mut self, paths: &[String], now_ms: i64) -> bool {
        self.add_files(paths, now_ms)
    }
    fn take_drag(&mut self) -> Option<Vec<String>> {
        self.to_drag.take()
    }
    fn take_copy_files(&mut self) -> Option<Vec<String>> {
        self.to_copy.take()
    }

    fn poll(&mut self, now_ms: i64) -> bool {
        let mut changed = false;
        let status = phone::status();
        let landed = phone::take_finished();
        if !landed.is_empty() {
            if self.peek.current(now_ms).is_none() {
                self.peek_n = 0;
            }
            self.peek_n += landed.len();
            self.peek_name = file_name(&landed[0]).to_string();
            let text = if self.peek_n == 1 { format!("Ponsel · {}", self.peek_name) } else { format!("Ponsel · {} file masuk", self.peek_n) };
            self.peek.show(text, now_ms, PEEK_MS);
            self.add_files(&landed, now_ms);
            changed = true;
        }
        if status.active.len() != self.pending.len() || status.active.iter().zip(&self.pending).any(|(a, b)| a.done != b.done) || status.running != self.status.running || status.minutes_left != self.status.minutes_left {
            changed = true;
        }
        if !status.running && self.status.running {
            // Stopped (idle or by the user): leave the QR page and clear the download marks.
            self.set_view(false, now_ms);
            self.sent.clear();
        }
        self.pending = status.active.clone();
        self.status = status;
        if self.status.running && self.qr.as_ref().is_none_or(|(u, _, _)| *u != self.status.url) {
            self.qr = isle_logic::phone::qr(&self.status.url).map(|(n, cells)| (self.status.url.clone(), n, cells));
        }
        if self.err.as_ref().is_some_and(|(_, t)| now_ms - t > 4000) {
            self.err = None;
            changed = true;
        }
        if self.peek.current(now_ms).is_none() && self.peek_n > 0 {
            self.peek_n = 0;
            changed = true;
        }
        changed | self.poll_thumbs()
    }


    /// Uploads finished pictures to the canvas.
    fn prepare(&mut self, c: &mut dyn Canvas) {
        for (path, picture) in self.fresh.drain(..) {
            let thumb = match picture {
                Some((rgba, w, h)) => Thumb::Ready(c.load_image(&rgba, w, h)),
                None => Thumb::None,
            };
            self.thumbs.insert(path, thumb);
        }
    }

    fn draw_panel(&mut self, c: &mut dyn Canvas, area: Rect, now_ms: i64) {
        // Changing between the tiles and the QR page slides and cross-fades them.
        if let Some((t0, to_phone)) = self.view_anim {
            let k = (now_ms - t0) as f32 / VIEW_MS;
            if k < 1.0 {
                let e = ease_out(k);
                let (tiles_dx, tiles_a, phone_dx, phone_a) = if to_phone { (-VIEW_SLIDE * e, 1.0 - e, VIEW_SLIDE * (1.0 - e), e) } else { (-VIEW_SLIDE * (1.0 - e), e, VIEW_SLIDE * e, 1.0 - e) };
                c.push_shift(tiles_dx, tiles_a);
                self.draw_tiles(c, area, now_ms);
                c.pop_shift();
                c.push_shift(phone_dx, phone_a);
                self.draw_phone(c, area, now_ms);
                c.pop_shift();
                return;
            }
            self.view_anim = None;
        }
        if self.phone_view && self.status.running {
            self.draw_phone(c, area, now_ms);
        } else {
            self.draw_tiles(c, area, now_ms);
        }
    }

    fn on_event(&mut self, e: &Event, area: Rect, now_ms: i64) -> Reaction {
        let n = self.shelf.items().len();
        let p = self.pending.len();
        if self.phone_view && self.status.running {
            if let Event::PointerDown(x, y) = e {
                let [done, stop, forget] = Self::phone_buttons(area);
                let inside = |r: Rect| *x >= r.x && *x < r.x + r.w && *y >= r.y && *y < r.y + r.h;
                if inside(done) {
                    self.done_btn.press(now_ms);
                    self.set_view(false, now_ms);
                } else if inside(stop) {
                    self.stop_btn.press(now_ms);
                    phone::stop();
                    self.set_view(false, now_ms);
                    self.sent.clear();
                    self.sync_outgoing();
                    self.status = phone::status();
                } else if inside(forget) {
                    self.forget_btn.press(now_ms);
                    phone::forget();
                    self.status = phone::status();
                    self.qr = isle_logic::phone::qr(&self.status.url).map(|(n, cells)| (self.status.url.clone(), n, cells));
                }
                self.anim_until = now_ms + 300;
                return Reaction::Redraw;
            }
            return Reaction::None;
        }
        // Tiles for files still arriving come first and cannot be clicked.
        let found = match hit(area, self.scroll, n + p, e_x(e), e_y(e)) {
            Some(ShelfHit::Tile(j)) => (j >= p).then(|| ShelfHit::Tile(j - p)),
            Some(ShelfHit::Remove(j)) => (j >= p).then(|| ShelfHit::Remove(j - p)),
            Some(ShelfHit::Send(j)) => (j >= p).then(|| ShelfHit::Send(j - p)),
            other => other,
        };
        match e {
            Event::PointerDown(..) => match found {
                Some(ShelfHit::Phone) => {
                    self.phone_btn.press(now_ms);
                    if !self.status.running {
                        if let Err(msg) = phone::start() {
                            self.err = Some((msg, now_ms));
                        }
                        self.status = phone::status();
                        if self.status.running {
                            self.qr = isle_logic::phone::qr(&self.status.url).map(|(n, cells)| (self.status.url.clone(), n, cells));
                        }
                    }
                    self.set_view(self.status.running, now_ms);
                    Reaction::Redraw
                }
                Some(ShelfHit::Send(i)) => {
                    let id = self.shelf.items()[i].id;
                    if !self.sent.remove(&id) {
                        self.sent.insert(id);
                    }
                    self.sync_outgoing();
                    Reaction::Redraw
                }
                Some(ShelfHit::Clear) => {
                    self.clear_btn.press(now_ms);
                    // Every tile leaves, one after the other.
                    for (k, item) in self.shelf.items().iter().enumerate() {
                        self.leaving.push(Leaving { path: item.path.clone(), id: item.id, index: k, t0: now_ms + 35 * k as i64 });
                    }
                    self.anim_until = now_ms + LEAVE_MS as i64 + 35 * n as i64 + 40;
                    self.shelf.clear();
                    self.sent.clear();
                    self.sync_outgoing();
                    self.save();
                    Reaction::Redraw
                }
                Some(ShelfHit::Remove(i)) => {
                    let item = self.shelf.items()[i].clone();
                    self.leaving.push(Leaving { path: item.path.clone(), id: item.id, index: i, t0: now_ms });
                    self.anim_until = now_ms + LEAVE_MS as i64 + 40;
                    self.shelf.remove(item.id);
                    self.sent.remove(&item.id);
                    self.sync_outgoing();
                    self.save();
                    Reaction::Redraw
                }
                Some(ShelfHit::Tile(i)) => {
                    self.press = Some((i, e_x(e), e_y(e)));
                    Reaction::None
                }
                None => Reaction::None,
            },
            Event::PointerDrag(x, y) => {
                if let Some((i, x0, y0)) = self.press {
                    if (x - x0).hypot(y - y0) > DRAG_PX {
                        self.press = None;
                        if let Some(item) = self.shelf.items().get(i) {
                            self.to_drag = Some(vec![item.path.clone()]);
                        }
                    }
                }
                Reaction::None
            }
            Event::PointerUp(..) => {
                if let Some((i, _, _)) = self.press.take() {
                    if let Some(item) = self.shelf.items().get(i) {
                        if std::path::Path::new(&item.path).exists() {
                            self.to_copy = Some(vec![item.path.clone()]);
                            self.copied = Some((item.id, now_ms));
                            self.anim_until = now_ms + COPIED_MS;
                        } else {
                            let id = item.id;
                            self.shelf.remove(id);
                            self.save();
                        }
                        return Reaction::Redraw;
                    }
                }
                Reaction::None
            }
            Event::PointerWheel(steps) => {
                let l = layout(area);
                self.scroll = (self.scroll - steps * (TILE_W + 8.0)).clamp(0.0, max_scroll(&l, n + p));
                Reaction::Redraw
            }
            _ => Reaction::None,
        }
    }

    fn next_wake_ms(&self, now_ms: i64) -> Option<i64> {
        // A running animation needs a frame every refresh (a wake within ~24 ms makes the
        // window run vsync-paced frames); pictures on their way only need an occasional look.
        let buttons = [&self.clear_btn, &self.phone_btn, &self.done_btn, &self.stop_btn, &self.forget_btn].iter().any(|b| b.busy(now_ms));
        let animating = now_ms < self.anim_until || !self.leaving.is_empty() || buttons;
        let waiting = self.thumbs.values().any(|t| matches!(t, Thumb::Loading)) || !self.fresh.is_empty();
        let progress = (!self.pending.is_empty() && !self.phone_view).then_some(now_ms + 60);
        let peek = self.peek.wake_after(now_ms);
        if animating {
            Some(now_ms + 16)
        } else {
            [waiting.then_some(now_ms + 40), progress, peek].into_iter().flatten().min()
        }
    }
}
