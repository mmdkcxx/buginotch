// SPDX-License-Identifier: GPL-3.0-or-later
//! A single-line text box for pages: drawing (caret, selection, IME underline, horizontal
//! scroll) plus mouse and keyboard handling. Pages own one and decide what Enter and
//! Escape mean.

use isle_logic::canvas::{Align, Canvas, Color, Rect};
use isle_logic::module::{EditKey, Event, Reaction};
use isle_logic::text_edit::TextEdit;
use std::cell::Cell;

const WHITE: Color = Color(245, 246, 248, 255);
const DIM: Color = Color(139, 144, 153, 255);
const FILL: Color = Color(20, 22, 26, 255);
const GREEN: Color = Color(52, 211, 153, 255);
const BORDER: Color = Color(58, 63, 72, 255);
pub const BLINK_MS: i64 = 530;
const PAD: f32 = 10.0;

pub struct TextBox {
    pub edit: TextEdit,
    /// Horizontal scroll of the text inside the box, to keep the caret visible.
    hscroll: f32,
    blink_from: i64,
    dragging: bool,
    /// Average char width from the last draw, for mapping clicks to a position.
    avg: Cell<f32>,
    /// Where the caret was last drawn, relative to the page area.
    caret: Cell<Option<(f32, f32)>>,
}

impl TextBox {
    pub fn new(max_chars: usize) -> Self {
        Self { edit: TextEdit::new(max_chars), hscroll: 0.0, blink_from: 0, dragging: false, avg: Cell::new(7.0), caret: Cell::new(None) }
    }

    pub fn text(&self) -> &str {
        self.edit.text()
    }

    pub fn clear(&mut self) {
        self.edit.clear();
        self.hscroll = 0.0;
        self.dragging = false;
    }

    pub fn touch(&mut self, now_ms: i64) {
        self.blink_from = now_ms;
    }

    /// Caret position (page-area relative) from the last draw; None when it was not shown.
    pub fn caret(&self) -> Option<(f32, f32)> {
        self.caret.get()
    }

    pub fn hide_caret(&self) {
        self.caret.set(None);
    }

    /// When the caret next flips, for scheduling a redraw.
    pub fn next_blink_ms(&self, now_ms: i64) -> i64 {
        now_ms + BLINK_MS - (now_ms - self.blink_from).rem_euclid(BLINK_MS)
    }

    fn index_at(&self, x: f32, field: Rect) -> usize {
        let local = x - (field.x + PAD) + self.hscroll;
        self.edit.index_at(local, &mut |s: &str| s.chars().count() as f32 * self.avg.get())
    }

    pub fn draw(&mut self, c: &mut dyn Canvas, field: Rect, area: Rect, placeholder: &str, focused: bool, now_ms: i64) {
        c.fill_round_rect(field, 10.0, if focused { GREEN } else { BORDER });
        c.fill_round_rect(Rect { x: field.x + 1.0, y: field.y + 1.0, w: field.w - 2.0, h: field.h - 2.0 }, 9.0, FILL);
        let inner = Rect { x: field.x + PAD, y: field.y, w: field.w - 2.0 * PAD, h: field.h };
        let shown = self.edit.display_text();
        if shown.is_empty() {
            c.draw_text(placeholder, inner, 12.5, 400, DIM, Align::Left);
        }
        let prefix_w = c.measure_text(&self.edit.caret_prefix(), 12.5, 400);
        if !shown.is_empty() {
            self.avg.set((c.measure_text(&shown, 12.5, 400) / shown.chars().count() as f32).max(3.0));
        }
        if prefix_w - self.hscroll > inner.w - 2.0 {
            self.hscroll = prefix_w - (inner.w - 2.0);
        }
        if prefix_w - self.hscroll < 0.0 {
            self.hscroll = prefix_w;
        }
        c.push_clip(inner, 0.0);
        let prefix_of = |c: &mut dyn Canvas, n: usize| c.measure_text(&shown.chars().take(n).collect::<String>(), 12.5, 400);
        if let Some((a, b)) = self.edit.selection() {
            let (xa, xb) = (prefix_of(c, a), prefix_of(c, b));
            c.fill_round_rect(Rect { x: inner.x + xa - self.hscroll, y: field.y + 6.0, w: xb - xa, h: field.h - 12.0 }, 3.0, Color(52, 211, 153, 80));
        }
        let text_rect = Rect { x: inner.x - self.hscroll, y: field.y, w: c.measure_text(&shown, 12.5, 400) + 8.0, h: field.h };
        c.draw_text(&shown, text_rect, 12.5, 400, WHITE, Align::Left);
        if let Some((a, b)) = self.edit.composition_range() {
            let (xa, xb) = (prefix_of(c, a), prefix_of(c, b));
            c.fill_round_rect(Rect { x: inner.x + xa - self.hscroll, y: field.y + field.h - 8.0, w: xb - xa, h: 1.0 }, 0.5, WHITE);
        }
        let caret_x = inner.x + prefix_w - self.hscroll;
        if focused && ((now_ms - self.blink_from) / BLINK_MS) % 2 == 0 {
            c.fill_round_rect(Rect { x: caret_x, y: field.y + 7.0, w: 1.5, h: field.h - 14.0 }, 0.75, WHITE);
        }
        c.pop_clip();
        self.caret.set(focused.then_some((caret_x - area.x, field.y + field.h - area.y)));
    }

    pub fn pointer_down(&mut self, x: f32, field: Rect, now_ms: i64) {
        let i = self.index_at(x, field);
        self.edit.set_caret(i, false);
        self.dragging = true;
        self.blink_from = now_ms;
    }

    pub fn pointer_drag(&mut self, x: f32, field: Rect) -> bool {
        if !self.dragging {
            return false;
        }
        let i = self.index_at(x, field);
        self.edit.set_caret(i, true);
        true
    }

    pub fn pointer_up(&mut self) {
        self.dragging = false;
    }

    /// Typing, caret keys, selection and the a/c/x shortcuts. Enter and Escape are left to
    /// the page (`None`); everything else not about editing is `Reaction::None`.
    pub fn on_event(&mut self, e: &Event, now_ms: i64) -> Option<Reaction> {
        self.blink_from = now_ms;
        Some(match e {
            Event::Text(s) => {
                self.edit.insert(s);
                Reaction::Redraw
            }
            Event::Compose(c) => {
                self.edit.set_composition(c.clone());
                Reaction::Redraw
            }
            Event::Shortcut(k) => match k {
                'a' => {
                    self.edit.select_all();
                    Reaction::Redraw
                }
                'c' => self.edit.selected_text().map_or(Reaction::None, Reaction::Copy),
                'x' => self.edit.cut().map_or(Reaction::None, Reaction::Copy),
                _ => Reaction::None,
            },
            Event::Edit { key, shift } => {
                match key {
                    EditKey::Backspace => self.edit.backspace(),
                    EditKey::Delete => self.edit.delete(),
                    EditKey::Left => self.edit.move_left(*shift),
                    EditKey::Right => self.edit.move_right(*shift),
                    EditKey::Home => self.edit.home(*shift),
                    EditKey::End => self.edit.end(*shift),
                    EditKey::Enter | EditKey::Escape => return None,
                }
                Reaction::Redraw
            }
            _ => return None,
        })
    }
}
