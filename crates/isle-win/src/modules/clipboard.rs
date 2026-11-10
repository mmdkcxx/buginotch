// SPDX-License-Identifier: GPL-3.0-or-later
//! Clipboard history page: recent copied text with search, pin, click-to-copy, pause and
//! clear. History lives in memory only; pinned entries are saved to `clipboard.json`.

use super::button::Button;
use super::textbox::TextBox;
use crate::system::{data_path, epoch_ms};
use isle_logic::canvas::{Align, Canvas, Color, Rect, fit_text};
use isle_logic::clipboard::{History, Kind};
use isle_logic::module::{EditKey, Event, Module, Reaction};
use isle_logic::todo_layout::{DELETE_W, ROW_GAP, ROW_H, TodoHit, hit, layout, max_scroll, row_rect};

const WHITE: Color = Color(245, 246, 248, 255);
const DIM: Color = Color(139, 144, 153, 255);
const ROW: Color = Color(20, 22, 26, 255);
const FLASH: Color = Color(18, 48, 31, 255);
const GREEN: Color = Color(52, 211, 153, 255);
const LINK: Color = Color(125, 178, 255, 255);
const AMBER: Color = Color(251, 191, 36, 255);
/// How long a copied row says "Tersalin".
const FLASH_MS: i64 = 1000;

pub struct ClipboardModule {
    history: History,
    search: TextBox,
    pause_btn: Button,
    clear_btn: Button,
    /// The star last pressed, for a pop animation.
    star_pop: Option<(u64, i64)>,
    focused: bool,
    scroll: f32,
    /// Text we put on the clipboard ourselves; the change it causes is not recorded.
    own: Option<String>,
    to_copy: Option<String>,
    /// The entry last copied from the list, and when (for a short highlight).
    flashed: Option<(u64, i64)>,
    anim_until: i64,
}

impl ClipboardModule {
    pub fn new() -> Self {
        let mut history = History::default();
        let saved: Vec<String> = std::fs::read_to_string(data_path("clipboard.json"))
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default();
        history.restore(saved, epoch_ms());
        Self { history, search: TextBox::new(60), pause_btn: Button::new(AMBER), clear_btn: Button::new(AMBER), star_pop: None, focused: false, scroll: 0.0, own: None, to_copy: None, flashed: None, anim_until: 0 }
    }

    fn save_pins(&self) {
        let pins: Vec<&str> = self.history.pinned().iter().map(|e| e.text.as_str()).collect();
        let path = data_path("clipboard.json");
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(path, serde_json::to_string(&pins).unwrap_or_default());
    }
}

impl Module for ClipboardModule {
    fn id(&self) -> &'static str {
        "clipboard"
    }
    fn title(&self) -> &'static str {
        "Clipboard"
    }
    fn pill_left(&self) -> Option<isle_logic::module::PillItem> {
        None
    }
    fn wants_text(&self) -> bool {
        self.focused
    }
    fn text_caret(&self) -> Option<(f32, f32)> {
        self.search.caret()
    }
    fn on_clipboard(&mut self, text: &str, now_ms: i64) -> bool {
        if self.own.as_deref() == Some(text) {
            self.own = None;
            return false;
        }
        self.history.add(text, now_ms)
    }
    fn take_copy(&mut self) -> Option<String> {
        let t = self.to_copy.take()?;
        self.own = Some(t.clone());
        Some(t)
    }
    fn poll(&mut self, now_ms: i64) -> bool {
        self.history.trim(now_ms)
    }

    fn draw_panel(&mut self, c: &mut dyn Canvas, area: Rect, now_ms: i64) {
        let l = layout(area, true);
        let query = self.search.text().to_string();
        let rows = self.history.visible(&query);
        c.draw_text(&format!("{}", self.history.visible("").len()), l.count, 40.0, 700, WHITE, Align::Left);
        c.draw_text(if self.history.paused { "Riwayat dijeda" } else { "Salinan tersimpan" }, l.caption, 12.0, 400, if self.history.paused { AMBER } else { DIM }, Align::Left);
        self.pause_btn.draw(c, l.add, if self.history.paused { "Lanjut" } else { "Jeda" }, self.history.paused, now_ms);
        self.clear_btn.draw(c, l.edit, "Hapus", false, now_ms);

        self.search.hide_caret();
        self.search.draw(c, l.field, area, "Cari salinan", self.focused, now_ms);

        let max = max_scroll(&l, rows.len());
        self.scroll = self.scroll.clamp(0.0, max);
        if rows.is_empty() {
            let msg = if query.trim().is_empty() { "Belum ada salinan. Salin teks apa pun." } else { "Tidak ada yang cocok." };
            c.draw_text(msg, l.list, 12.5, 400, DIM, Align::Center);
        }
        c.push_clip(l.list, 0.0);
        for (i, e) in rows.iter().enumerate() {
            let r = row_rect(&l, i, self.scroll);
            if r.y + r.h < l.list.y || r.y > l.list.y + l.list.h {
                continue;
            }
            // 0..1 envelope of the "copied" message: quick in, hold, soft out.
            let k = match self.flashed {
                Some((id, t0)) if id == e.id => {
                    let ms = (now_ms - t0) as f32;
                    (ms / 120.0).min((FLASH_MS as f32 - ms) / 280.0).clamp(0.0, 1.0)
                }
                _ => 0.0,
            };
            let mix = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * k).round() as u8;
            c.fill_round_rect(r, 10.0, Color(mix(ROW.0, FLASH.0), mix(ROW.1, FLASH.1), mix(ROW.2, FLASH.2), 255));
            let flat: String = e.text.split_whitespace().collect::<Vec<_>>().join(" ");
            let text_box = Rect { x: r.x + 12.0, y: r.y, w: r.w - 12.0 - DELETE_W - 4.0, h: r.h };
            let shown = fit_text(c, &flat, 12.5, 400, text_box.w);
            let base = if e.kind() == Kind::Link { LINK } else { WHITE };
            c.draw_text(&shown, Rect { y: text_box.y - 4.0 * k, ..text_box }, 12.5, 400, Color(base.0, base.1, base.2, ((1.0 - k) * 255.0) as u8), Align::Left);
            if k > 0.0 {
                // The check slides up into place as the message fades in.
                c.draw_text("✓  Tersalin ke clipboard", Rect { y: text_box.y + 6.0 * (1.0 - k), ..text_box }, 12.5, 650, Color(GREEN.0, GREEN.1, GREEN.2, (k * 255.0) as u8), Align::Left);
            }
            let star = Rect { x: r.x + r.w - DELETE_W, y: r.y, w: DELETE_W, h: r.h };
            let pop = match self.star_pop {
                Some((id, t0)) if id == e.id => {
                    let t = (now_ms - t0) as f32 / 260.0;
                    if (0.0..1.0).contains(&t) { (std::f32::consts::PI * t).sin() } else { 0.0 }
                }
                _ => 0.0,
            };
            c.draw_text(if e.pinned { "★" } else { "☆" }, star, 14.0 + 6.0 * pop, 400, if e.pinned { GREEN } else { DIM }, Align::Center);
        }
        c.pop_clip();
        if max > 0.0 {
            let thumb = (l.list.h * l.list.h / (l.list.h + max)).max(14.0);
            let y = l.list.y + (l.list.h - thumb) * (self.scroll / max);
            c.fill_round_rect(Rect { x: l.list.x + l.list.w - 3.0, y, w: 2.0, h: thumb }, 1.0, Color(90, 96, 106, 200));
        }
        if self.flashed.is_some_and(|(_, t0)| now_ms - t0 > FLASH_MS) {
            self.flashed = None;
        }
    }

    fn on_event(&mut self, e: &Event, area: Rect, now_ms: i64) -> Reaction {
        let query = self.search.text().to_string();
        match e {
            Event::PointerDown(x, y) => {
                let ids: Vec<u64> = self.history.visible(&query).iter().map(|e| e.id).collect();
                let hit = hit(area, true, false, self.scroll, ids.len(), *x, *y);
                self.focused = matches!(hit, Some(TodoHit::Field));
                match hit {
                    Some(TodoHit::Add) => {
                        self.pause_btn.press(now_ms);
                        self.history.paused = !self.history.paused;
                    }
                    Some(TodoHit::Edit) => {
                        self.clear_btn.press(now_ms);
                        self.history.clear();
                        self.save_pins();
                    }
                    Some(TodoHit::Field) => {
                        let l = layout(area, true);
                        self.search.pointer_down(*x, l.field, now_ms);
                    }
                    Some(TodoHit::Row(i)) => {
                        let l = layout(area, true);
                        let r = row_rect(&l, i, self.scroll);
                        if *x >= r.x + r.w - DELETE_W {
                            // The star at the row's right end pins or unpins.
                            if let Some(id) = ids.get(i) {
                                self.history.toggle_pin(*id);
                                self.save_pins();
                                self.star_pop = Some((*id, now_ms));
                                self.anim_until = now_ms + 300;
                            }
                        } else if let Some(entry) = self.history.visible(&query).get(i) {
                            self.to_copy = Some(entry.text.clone());
                            self.flashed = Some((entry.id, now_ms));
                            self.anim_until = now_ms + FLASH_MS;
                        }
                    }
                    Some(TodoHit::Delete(_)) | None => return Reaction::Redraw,
                }
                Reaction::Redraw
            }
            Event::PointerDrag(x, _) if self.focused => {
                let l = layout(area, true);
                if self.search.pointer_drag(*x, l.field) { Reaction::Redraw } else { Reaction::None }
            }
            Event::PointerUp(..) => {
                self.search.pointer_up();
                Reaction::None
            }
            Event::PointerWheel(steps) => {
                let l = layout(area, true);
                let n = self.history.visible(&query).len();
                let max = max_scroll(&l, n);
                if max <= 0.0 {
                    return Reaction::None; // nothing to scroll: the wheel turns the page instead
                }
                self.scroll = (self.scroll - steps * (ROW_H + ROW_GAP)).clamp(0.0, max);
                Reaction::Redraw
            }
            _ if !self.focused => Reaction::None,
            Event::Edit { key: EditKey::Escape, .. } => {
                self.search.clear();
                self.focused = false;
                self.scroll = 0.0;
                Reaction::Redraw
            }
            Event::Edit { key: EditKey::Enter, .. } => {
                self.focused = false;
                Reaction::Redraw
            }
            _ => {
                let r = self.search.on_event(e, now_ms).unwrap_or(Reaction::None);
                if r != Reaction::None {
                    self.scroll = 0.0;
                }
                r
            }
        }
    }

    fn next_wake_ms(&self, now_ms: i64) -> Option<i64> {
        let blink = self.focused.then(|| self.search.next_blink_ms(now_ms));
        let busy = self.pause_btn.busy(now_ms) || self.clear_btn.busy(now_ms);
        let anim = (now_ms < self.anim_until || busy).then_some(now_ms + 16);
        [blink, anim].into_iter().flatten().min()
    }
}

