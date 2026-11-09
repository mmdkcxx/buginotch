// SPDX-License-Identifier: GPL-3.0-or-later
//! To-do page: today's tasks with a counter, a text box to add, tick to complete, and an
//! edit mode to delete. Saved to `%APPDATA%\IsleWin\todo.json`.

use crate::system::{data_path, epoch_ms, local_time};
use isle_logic::canvas::{Align, Canvas, Color, Rect, fit_text};
use isle_logic::module::{EditKey, Event, Module, PillItem, Reaction};
use super::textbox::TextBox;
use isle_logic::peek::Peek;
use isle_logic::todo::{TodoList, day_number, from_json, to_json};
use isle_logic::todo_layout::{DELETE_W, TodoHit, ROW_GAP, ROW_H, hit, layout, max_scroll, row_rect};
use std::collections::HashMap;

const WHITE: Color = Color(245, 246, 248, 255);
const DIM: Color = Color(139, 144, 153, 255);
const ROW: Color = Color(20, 22, 26, 255);
const ROW_DONE: Color = Color(18, 48, 31, 255);
const RING: Color = Color(91, 97, 108, 255);
const GREEN: Color = Color(52, 211, 153, 255);
const ON_GREEN: Color = Color(4, 19, 13, 255);
const CHIP: Color = Color(28, 31, 36, 255);
const FIELD_BORDER: Color = Color(58, 63, 72, 255);
const DONE_TEXT: Color = Color(185, 196, 189, 255);
const RED: Color = Color(255, 122, 122, 255);
const BLINK_MS: i64 = 530;
const REMIND_SHOW_MS: i64 = 7000;
const CHECK_MS: f32 = 220.0;

pub struct TodoModule {
    list: TodoList,
    adding: bool,
    editing: bool,
    field: TextBox,
    scroll: f32,
    /// Tick animations: task id -> (start ms, ticking on).
    ticks: HashMap<u64, (i64, bool)>,
    day: i32,
    anim_until: i64,
    /// A due reminder shown in the closed pill.
    peek: Peek,
}

fn today() -> i32 {
    let t = local_time();
    day_number(t.year, t.month, t.day)
}

impl TodoModule {
    pub fn new() -> Self {
        let mut list = std::fs::read_to_string(data_path("todo.json")).map(|t| from_json(&t)).unwrap_or_default();
        let day = today();
        if list.prune(day) {
            save(&list);
        }
        Self { list, adding: false, editing: false, field: TextBox::new(isle_logic::todo::MAX_TASK_CHARS), scroll: 0.0, ticks: HashMap::new(), day, anim_until: 0, peek: Peek::default() }
    }

    fn save(&self) {
        save(&self.list);
    }

    fn start_adding(&mut self, now: i64) {
        self.adding = true;
        self.editing = false;
        self.field.touch(now);
    }

    fn stop_adding(&mut self) {
        self.adding = false;
        self.field.clear();
    }

    fn tick_progress(&self, id: u64, done: bool, now: i64) -> f32 {
        match self.ticks.get(&id) {
            Some(&(t0, on)) => {
                let k = ((now - t0) as f32 / CHECK_MS).clamp(0.0, 1.0);
                let k = 1.0 - (1.0 - k).powi(3);
                if on { k } else { 1.0 - k }
            }
            None => f32::from(done),
        }
    }
}

fn save(list: &TodoList) {
    let path = data_path("todo.json");
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(path, to_json(list));
}

fn chip(c: &mut dyn Canvas, r: Rect, label: &str, active: bool) {
    c.fill_round_rect(r, r.h / 2.0, if active { GREEN } else { CHIP });
    c.draw_text(label, r, 12.5, 650, if active { ON_GREEN } else { WHITE }, Align::Center);
}

/// A line of thickness `w` from `a` to `b`.
fn thick_line(c: &mut dyn Canvas, a: (f32, f32), b: (f32, f32), w: f32, col: Color) {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let len = (dx * dx + dy * dy).sqrt().max(0.001);
    let (nx, ny) = (-dy / len * w / 2.0, dx / len * w / 2.0);
    c.fill_polygon(&[(a.0 + nx, a.1 + ny), (b.0 + nx, b.1 + ny), (b.0 - nx, b.1 - ny), (a.0 - nx, a.1 - ny)], col);
}

fn mix(a: Color, b: Color, k: f32) -> Color {
    let f = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * k).round() as u8;
    Color(f(a.0, b.0), f(a.1, b.1), f(a.2, b.2), 255)
}

impl Module for TodoModule {
    fn id(&self) -> &'static str {
        "todo"
    }
    fn title(&self) -> &'static str {
        "To-do"
    }
    fn pill_left(&self) -> Option<PillItem> {
        None
    }
    fn wants_text(&self) -> bool {
        self.adding
    }
    fn text_caret(&self) -> Option<(f32, f32)> {
        self.field.caret()
    }
    fn pill_center(&self) -> Option<String> {
        self.peek.current(epoch_ms()).map(str::to_string)
    }
    fn pill_priority(&self) -> u8 {
        30
    }
    fn wide_pill(&self) -> bool {
        self.peek.current(epoch_ms()).is_some()
    }
    fn poll(&mut self, now_ms: i64) -> bool {
        let t = local_time();
        let minute = (t.hour * 60 + t.minute) as u16;
        let before = self.peek.current(now_ms).is_some();
        let hit = self.list.due_now(self.day, minute).map(|t| (t.id, t.text.clone()));
        if let Some((id, text)) = hit {
            self.list.mark_reminded(id, self.day);
            self.save();
            self.peek.show(format!("⏰ {text}"), now_ms, REMIND_SHOW_MS);
        }
        let reminded = self.peek.current(now_ms).is_some() != before;
        let d = today();
        if d != self.day {
            self.day = d;
            if self.list.prune(d) {
                self.save();
                return true;
            }
        }
        reminded
    }

    fn draw_panel(&mut self, c: &mut dyn Canvas, area: Rect, now_ms: i64) {
        let l = layout(area, self.adding);
        let (done, total) = self.list.counts();
        c.draw_text(&format!("{done}/{total}"), l.count, 40.0, 700, WHITE, Align::Left);
        c.draw_text("Tugas selesai hari ini", l.caption, 12.0, 400, DIM, Align::Left);
        chip(c, l.add, if self.adding { "Batal" } else { "+ Tambah" }, false);
        chip(c, l.edit, if self.editing { "Selesai" } else { "Ubah" }, self.editing);

        self.field.hide_caret();
        if self.adding {
            self.field.draw(c, l.field, area, "Tugas baru, atau 14:30 Rapat", true, now_ms);
        }

        let max = max_scroll(&l, self.list.tasks().len());
        self.scroll = self.scroll.clamp(0.0, max);
        if self.list.tasks().is_empty() {
            let text = if self.adding { "" } else { "Belum ada tugas. Tekan + Tambah." };
            c.draw_text(text, l.list, 12.5, 400, DIM, Align::Center);
        }
        c.push_clip(l.list, 0.0);
        for (i, task) in self.list.tasks().iter().enumerate() {
            let r = row_rect(&l, i, self.scroll);
            if r.y + r.h < l.list.y || r.y > l.list.y + l.list.h {
                continue;
            }
            let k = self.tick_progress(task.id, task.done, now_ms);
            c.fill_round_rect(r, 10.0, mix(ROW, ROW_DONE, k));
            // The check circle fills green as the task is ticked off.
            let (cx, cy) = (r.x + 18.5, r.y + r.h / 2.0);
            c.fill_circle(cx, cy, 8.5, mix(RING, GREEN, k));
            c.fill_circle(cx, cy, 7.0, mix(ROW, GREEN, k));
            if k > 0.05 {
                let a = (k * 255.0) as u8;
                let ink = Color(ON_GREEN.0, ON_GREEN.1, ON_GREEN.2, a);
                thick_line(c, (cx - 3.6, cy + 0.2), (cx - 1.0, cy + 2.8), 1.8, ink);
                thick_line(c, (cx - 1.0, cy + 2.8), (cx + 3.8, cy - 2.6), 1.8, ink);
            }
            let due = task.due.map(|m| format!("{:02}:{:02}", m / 60, m % 60));
            let due_w = if due.is_some() { 44.0 } else { 0.0 };
            let right_pad = if self.editing { DELETE_W } else { 12.0 } + due_w;
            if let Some(d) = &due {
                let slot = Rect { x: r.x + r.w - right_pad, y: r.y, w: due_w, h: r.h };
                c.draw_text(d, slot, 11.5, 500, if k > 0.5 { DIM } else { GREEN }, Align::Right);
            }
            let text_box = Rect { x: r.x + 36.0, y: r.y, w: r.w - 36.0 - right_pad, h: r.h };
            let shown = fit_text(c, &task.text, 12.5, 400, text_box.w);
            let col = mix(WHITE, DONE_TEXT, k);
            c.draw_text(&shown, text_box, 12.5, 400, col, Align::Left);
            if k > 0.5 {
                let w = c.measure_text(&shown, 12.5, 400);
                c.fill_round_rect(Rect { x: text_box.x, y: r.y + r.h / 2.0, w, h: 1.0 }, 0.5, col);
            }
            if self.editing {
                c.draw_text("✕", Rect { x: r.x + r.w - DELETE_W, y: r.y, w: DELETE_W, h: r.h }, 13.0, 400, RED, Align::Center);
            }
        }
        c.pop_clip();
        if max > 0.0 {
            let track = l.list.h;
            let thumb = (track * l.list.h / (l.list.h + max)).max(14.0);
            let y = l.list.y + (track - thumb) * (self.scroll / max);
            c.fill_round_rect(Rect { x: l.list.x + l.list.w - 3.0, y, w: 2.0, h: thumb }, 1.0, Color(90, 96, 106, 200));
        }
        if self.ticks.values().any(|&(t0, _)| (now_ms - t0) as f32 > CHECK_MS) {
            self.ticks.retain(|_, &mut (t0, _)| (now_ms - t0) as f32 <= CHECK_MS);
        }
        if !self.ticks.is_empty() {
            self.anim_until = self.anim_until.max(now_ms + 40);
        }
    }

    fn on_event(&mut self, e: &Event, area: Rect, now_ms: i64) -> Reaction {
        match e {
            Event::PointerDown(x, y) => {
                let rows = self.list.tasks().len();
                match hit(area, self.adding, self.editing, self.scroll, rows, *x, *y) {
                    Some(TodoHit::Add) => {
                        if self.adding {
                            self.stop_adding();
                        } else {
                            self.start_adding(now_ms);
                        }
                    }
                    Some(TodoHit::Edit) => {
                        self.stop_adding();
                        self.editing = !self.editing;
                    }
                    Some(TodoHit::Field) => {
                        let l = layout(area, true);
                        self.field.pointer_down(*x, l.field, now_ms);
                    }
                    Some(TodoHit::Row(i)) => {
                        let (id, was_done) = (self.list.tasks()[i].id, self.list.tasks()[i].done);
                        self.list.toggle(id, self.day);
                        self.ticks.insert(id, (now_ms, !was_done));
                        self.anim_until = now_ms + 300;
                        self.save();
                    }
                    Some(TodoHit::Delete(i)) => {
                        let id = self.list.tasks()[i].id;
                        self.list.remove(id);
                        self.save();
                    }
                    None => return Reaction::None,
                }
                Reaction::Redraw
            }
            Event::PointerDrag(x, _) if self.adding => {
                let l = layout(area, true);
                if self.field.pointer_drag(*x, l.field) { Reaction::Redraw } else { Reaction::None }
            }
            Event::PointerUp(..) => {
                self.field.pointer_up();
                Reaction::None
            }
            Event::PointerWheel(steps) => {
                let l = layout(area, self.adding);
                self.scroll = (self.scroll - steps * (ROW_H + ROW_GAP)).clamp(0.0, max_scroll(&l, self.list.tasks().len()));
                Reaction::Redraw
            }
            _ if !self.adding => Reaction::None,
            Event::Edit { key: EditKey::Enter, .. } => {
                self.field.touch(now_ms);
                if self.list.add(self.field.text()).is_some() {
                    self.save();
                    self.scroll = 0.0;
                }
                self.field.clear();
                Reaction::Redraw
            }
            Event::Edit { key: EditKey::Escape, .. } => {
                self.stop_adding();
                Reaction::Redraw
            }
            _ => self.field.on_event(e, now_ms).unwrap_or(Reaction::None),
        }
    }

    fn next_wake_ms(&self, now_ms: i64) -> Option<i64> {
        let blink = self.adding.then(|| self.field.next_blink_ms(now_ms));
        let anim = (now_ms < self.anim_until).then_some(now_ms + 16);
        // Look again at the start of each minute while a reminder is waiting.
        let waiting = self.list.tasks().iter().any(|t| !t.done && t.due.is_some());
        let minute = waiting.then(|| now_ms + 60_000 - now_ms.rem_euclid(60_000) + 200);
        let shown = self.peek.next_wake_ms().filter(|&t| t > now_ms);
        [blink, anim, minute, shown].into_iter().flatten().min()
    }
}
