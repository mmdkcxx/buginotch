// SPDX-License-Identifier: GPL-3.0-or-later
//! Shelf page: park files on the notch. Drop files on it, drag them back out into any app,
//! or click one to copy it. Files are referenced by path and never copied.

use super::button::Button;
use crate::shell_win::ThumbWorker;
use crate::system::data_path;
use isle_logic::canvas::{Align, Canvas, Color, ImageId, Rect, fit_text};
use isle_logic::module::{Event, Module, Reaction};
use isle_logic::shelf::{Shelf, extension, file_name};
use isle_logic::shelf_layout::{ShelfHit, TILE_W, hit, layout, max_scroll, remove_rect, tile_rect};
use std::collections::HashMap;

const WHITE: Color = Color(245, 246, 248, 255);
const DIM: Color = Color(139, 144, 153, 255);
const ROW: Color = Color(20, 22, 26, 255);
const FLASH: Color = Color(18, 48, 31, 255);
const GREEN: Color = Color(52, 211, 153, 255);
const AMBER: Color = Color(251, 191, 36, 255);
const BADGE: Color = Color(43, 47, 54, 255);
const ENTER_MS: f32 = 320.0;
const COPIED_MS: i64 = 1000;
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
        }
    }

    fn save(&self) {
        let path = data_path("shelf.json");
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(path, serde_json::to_string(&self.shelf.paths()).unwrap_or_default());
    }
}

impl Module for ShelfModule {
    fn id(&self) -> &'static str {
        "shelf"
    }
    fn title(&self) -> &'static str {
        "Shelf"
    }
    fn pill_left(&self) -> Option<isle_logic::module::PillItem> {
        None
    }
    fn set_dragging(&mut self, over: bool) {
        self.over = over;
    }
    fn on_files(&mut self, paths: &[String], now_ms: i64) -> bool {
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
    fn take_drag(&mut self) -> Option<Vec<String>> {
        self.to_drag.take()
    }
    fn take_copy_files(&mut self) -> Option<Vec<String>> {
        self.to_copy.take()
    }

    fn poll(&mut self, _now_ms: i64) -> bool {
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
        let l = layout(area);
        let n = self.shelf.items().len();
        let caption = match (n, self.over) {
            (_, true) => "Lepaskan untuk menaruh di Shelf".to_string(),
            (0, _) => "Shelf kosong".to_string(),
            (n, _) => format!("{n} file · seret keluar atau klik untuk menyalin"),
        };
        c.draw_text(&caption, l.caption, 12.0, 400, if self.over { GREEN } else { DIM }, Align::Left);
        self.clear_btn.draw(c, l.clear, "Kosongkan", false, now_ms);

        self.scroll = self.scroll.clamp(0.0, max_scroll(&l, n));
        if n == 0 {
            // An empty shelf is a drop zone.
            let zone = Rect { x: l.tiles.x, y: l.tiles.y, w: l.tiles.w, h: l.tiles.h };
            let edge = if self.over { GREEN } else { Color(58, 63, 72, 255) };
            c.fill_round_rect(zone, 14.0, edge);
            c.fill_round_rect(Rect { x: zone.x + 1.5, y: zone.y + 1.5, w: zone.w - 3.0, h: zone.h - 3.0 }, 12.5, if self.over { FLASH } else { ROW });
            c.draw_text("Seret file ke sini", Rect { y: zone.y + 28.0, h: 18.0, ..zone }, 13.0, 650, WHITE, Align::Center);
            c.draw_text("Dari Explorer atau jendela mana pun", Rect { y: zone.y + 48.0, h: 16.0, ..zone }, 11.5, 400, DIM, Align::Center);
        }
        c.push_clip(l.tiles, 0.0);
        for (i, item) in self.shelf.items().iter().enumerate() {
            let mut r = tile_rect(&l, i, self.scroll);
            if r.x + r.w < l.tiles.x || r.x > l.tiles.x + l.tiles.w {
                continue;
            }
            // A newly dropped tile settles in from slightly above.
            let enter = self.arrived.get(&item.id).map_or(1.0, |&t0| {
                let k = ((now_ms - t0) as f32 / ENTER_MS).clamp(0.0, 1.0);
                1.0 - (1.0 - k).powi(3)
            });
            r.y -= 14.0 * (1.0 - enter);
            let a = (enter * 255.0) as u8;
            let fade = |col: Color| Color(col.0, col.1, col.2, ((col.3 as f32) * enter) as u8);
            let k = match self.copied {
                Some((id, t0)) if id == item.id => {
                    let ms = (now_ms - t0) as f32;
                    (ms / 120.0).min((COPIED_MS as f32 - ms) / 280.0).clamp(0.0, 1.0)
                }
                _ => 0.0,
            };
            let mixc = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * k).round() as u8;
            c.fill_round_rect(r, 12.0, Color(mixc(ROW.0, FLASH.0), mixc(ROW.1, FLASH.1), mixc(ROW.2, FLASH.2), a));
            let icon = Rect { x: r.x + (r.w - 46.0) / 2.0, y: r.y + 10.0, w: 46.0, h: 46.0 };
            match self.thumbs.get(&item.path) {
                Some(Thumb::Ready(id)) => c.draw_image_alpha(*id, icon, 8.0, enter),
                _ => {
                    c.fill_round_rect(icon, 10.0, fade(BADGE));
                    c.draw_text(&extension(&item.path), icon, 11.5, 700, fade(DIM), Align::Center);
                }
            }
            let name = file_name(&item.path);
            let name_box = Rect { x: r.x + 6.0, y: r.y + 62.0, w: r.w - 12.0, h: 16.0 };
            let shown = fit_text(c, name, 11.5, 500, name_box.w);
            c.draw_text(&shown, Rect { y: name_box.y - 3.0 * k, ..name_box }, 11.5, 500, Color(WHITE.0, WHITE.1, WHITE.2, ((1.0 - k) * a as f32) as u8), Align::Center);
            if k > 0.0 {
                c.draw_text("✓ Tersalin", Rect { y: name_box.y + 5.0 * (1.0 - k), ..name_box }, 11.5, 650, Color(GREEN.0, GREEN.1, GREEN.2, (k * 255.0) as u8), Align::Center);
            }
            let ext = extension(&item.path);
            if !ext.is_empty() && k < 0.5 {
                c.draw_text(&ext, Rect { y: r.y + 76.0, h: 14.0, ..name_box }, 10.0, 500, fade(DIM), Align::Center);
            }
            // The small remove button.
            let x = remove_rect(r);
            c.fill_circle(x.x + x.w / 2.0, x.y + x.h / 2.0, 7.0, Color(0, 0, 0, (a as f32 * 0.55) as u8));
            c.draw_text("×", x, 13.0, 400, fade(DIM), Align::Center);
        }
        c.pop_clip();
        let max = max_scroll(&l, n);
        if max > 0.0 {
            let track = l.tiles.w;
            let thumb = (track * track / (track + max)).max(20.0);
            let x = l.tiles.x + (track - thumb) * (self.scroll / max);
            c.fill_round_rect(Rect { x, y: l.tiles.y + l.tiles.h + 3.0, w: thumb, h: 2.0 }, 1.0, Color(90, 96, 106, 200));
        }
        self.arrived.retain(|_, &mut t0| ((now_ms - t0) as f32) < ENTER_MS);
        if self.copied.is_some_and(|(_, t0)| now_ms - t0 > COPIED_MS) {
            self.copied = None;
        }
    }

    fn on_event(&mut self, e: &Event, area: Rect, now_ms: i64) -> Reaction {
        let n = self.shelf.items().len();
        match e {
            Event::PointerDown(x, y) => match hit(area, self.scroll, n, *x, *y) {
                Some(ShelfHit::Clear) => {
                    self.clear_btn.press(now_ms);
                    self.shelf.clear();
                    self.save();
                    Reaction::Redraw
                }
                Some(ShelfHit::Remove(i)) => {
                    let id = self.shelf.items()[i].id;
                    self.shelf.remove(id);
                    self.save();
                    Reaction::Redraw
                }
                Some(ShelfHit::Tile(i)) => {
                    self.press = Some((i, *x, *y));
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
                self.scroll = (self.scroll - steps * (TILE_W + 8.0)).clamp(0.0, max_scroll(&l, n));
                Reaction::Redraw
            }
            _ => Reaction::None,
        }
    }

    fn next_wake_ms(&self, now_ms: i64) -> Option<i64> {
        let busy = now_ms < self.anim_until || self.clear_btn.busy(now_ms) || self.thumbs.values().any(|t| matches!(t, Thumb::Loading)) || !self.fresh.is_empty();
        // While pictures are on their way, look again soon (cheap: no redraw unless one arrives).
        busy.then_some(now_ms + 40)
    }
}
