// SPDX-License-Identifier: GPL-3.0-or-later
//! Geometry of the To-do page. `a` is the page area (452 x 122 DIPs).

use crate::canvas::Rect;

pub const ROW_H: f32 = 30.0;
pub const ROW_GAP: f32 = 6.0;
const FIELD_H: f32 = 28.0;
const LEFT_W: f32 = 150.0;
const COLUMN_GAP: f32 = 20.0;
const BUTTON_H: f32 = 28.0;
/// Width of the ✕ zone at the right end of a row in edit mode.
pub const DELETE_W: f32 = 34.0;

pub struct TodoLayout {
    pub count: Rect,
    pub caption: Rect,
    pub add: Rect,
    pub edit: Rect,
    /// The new-task text box (used only while adding).
    pub field: Rect,
    /// Where the rows live (below the text box while adding).
    pub list: Rect,
}

pub fn layout(a: Rect, adding: bool) -> TodoLayout {
    let right_x = a.x + LEFT_W + COLUMN_GAP;
    let right_w = a.w - LEFT_W - COLUMN_GAP;
    let field = Rect { x: right_x, y: a.y, w: right_w, h: FIELD_H };
    let top = if adding { a.y + FIELD_H + ROW_GAP } else { a.y };
    TodoLayout {
        count: Rect { x: a.x, y: a.y + 12.0, w: LEFT_W, h: 44.0 },
        caption: Rect { x: a.x, y: a.y + 58.0, w: LEFT_W, h: 16.0 },
        add: Rect { x: a.x, y: a.y + a.h - BUTTON_H, w: 84.0, h: BUTTON_H },
        edit: Rect { x: a.x + 90.0, y: a.y + a.h - BUTTON_H, w: 58.0, h: BUTTON_H },
        field,
        list: Rect { x: right_x, y: top, w: right_w, h: a.y + a.h - top },
    }
}

/// Rect of row `i` with the list scrolled by `scroll` DIPs.
pub fn row_rect(l: &TodoLayout, i: usize, scroll: f32) -> Rect {
    Rect { x: l.list.x, y: l.list.y + i as f32 * (ROW_H + ROW_GAP) - scroll, w: l.list.w, h: ROW_H }
}

/// How far the list can scroll.
pub fn max_scroll(l: &TodoLayout, rows: usize) -> f32 {
    let content = if rows == 0 { 0.0 } else { rows as f32 * (ROW_H + ROW_GAP) - ROW_GAP };
    (content - l.list.h).max(0.0)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TodoHit {
    Add,
    Edit,
    Field,
    Row(usize),
    Delete(usize),
}

fn inside(r: Rect, x: f32, y: f32) -> bool {
    x >= r.x && x < r.x + r.w && y >= r.y && y < r.y + r.h
}

pub fn hit(a: Rect, adding: bool, editing: bool, scroll: f32, rows: usize, x: f32, y: f32) -> Option<TodoHit> {
    let l = layout(a, adding);
    if inside(l.add, x, y) {
        return Some(TodoHit::Add);
    }
    if inside(l.edit, x, y) {
        return Some(TodoHit::Edit);
    }
    if adding && inside(l.field, x, y) {
        return Some(TodoHit::Field);
    }
    if !inside(l.list, x, y) {
        return None;
    }
    (0..rows).find_map(|i| {
        let r = row_rect(&l, i, scroll);
        if !inside(r, x, y) {
            return None;
        }
        let in_delete = x >= r.x + r.w - DELETE_W;
        Some(match (editing, in_delete) {
            (true, true) => TodoHit::Delete(i),
            (true, false) => return None,
            (false, _) => TodoHit::Row(i),
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: Rect = Rect { x: 44.0, y: 36.0, w: 452.0, h: 122.0 };

    #[test]
    fn everything_stays_inside_the_page_area() {
        for adding in [false, true] {
            let l = layout(A, adding);
            for r in [l.count, l.caption, l.add, l.edit, l.field, l.list] {
                assert!(r.x >= A.x && r.x + r.w <= A.x + A.w && r.y >= A.y && r.y + r.h <= A.y + A.h, "{r:?}");
            }
            assert!(l.edit.x >= l.add.x + l.add.w);
            assert!(l.list.x >= l.add.x + l.add.w);
        }
    }

    #[test]
    fn the_text_box_pushes_the_rows_down() {
        let plain = layout(A, false);
        let adding = layout(A, true);
        assert_eq!(plain.list.y, A.y);
        assert!(adding.list.y >= adding.field.y + adding.field.h);
    }

    #[test]
    fn buttons_and_rows_are_hit() {
        let l = layout(A, false);
        let at = |r: Rect| (r.x + r.w / 2.0, r.y + r.h / 2.0);
        let (x, y) = at(l.add);
        assert_eq!(hit(A, false, false, 0.0, 3, x, y), Some(TodoHit::Add));
        let (x, y) = at(l.edit);
        assert_eq!(hit(A, false, false, 0.0, 3, x, y), Some(TodoHit::Edit));
        let (x, y) = at(row_rect(&l, 1, 0.0));
        assert_eq!(hit(A, false, false, 0.0, 3, x, y), Some(TodoHit::Row(1)));
        assert_eq!(hit(A, false, false, 0.0, 3, l.list.x + 5.0, l.list.y + 3.0 * 36.0 + 5.0), None);
    }

    #[test]
    fn the_text_box_only_exists_while_adding() {
        let l = layout(A, true);
        let (x, y) = (l.field.x + 10.0, l.field.y + 10.0);
        assert_eq!(hit(A, true, false, 0.0, 2, x, y), Some(TodoHit::Field));
        assert_eq!(hit(A, false, false, 0.0, 0, x, y), None);
    }

    #[test]
    fn in_edit_mode_only_the_cross_deletes() {
        let l = layout(A, false);
        let r = row_rect(&l, 0, 0.0);
        assert_eq!(hit(A, false, true, 0.0, 2, r.x + r.w - 10.0, r.y + 10.0), Some(TodoHit::Delete(0)));
        assert_eq!(hit(A, false, true, 0.0, 2, r.x + 20.0, r.y + 10.0), None);
        assert_eq!(hit(A, false, false, 0.0, 2, r.x + r.w - 10.0, r.y + 10.0), Some(TodoHit::Row(0)));
    }

    #[test]
    fn scrolling_moves_the_rows_and_is_limited_to_the_content() {
        let l = layout(A, false);
        assert_eq!(max_scroll(&l, 3), 0.0);
        let many = max_scroll(&l, 10);
        assert_eq!(many, 10.0 * 36.0 - 6.0 - 122.0);
        assert_eq!(row_rect(&l, 4, 50.0).y, row_rect(&l, 4, 0.0).y - 50.0);
    }
}
