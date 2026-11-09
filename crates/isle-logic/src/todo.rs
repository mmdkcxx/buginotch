// SPDX-License-Identifier: GPL-3.0-or-later
//! The To-do list: tasks, completion, and saving. Tasks finished on an earlier day are
//! dropped, so the counter always means "done today".

use serde::{Deserialize, Serialize};

/// Longest task text, in chars.
pub const MAX_TASK_CHARS: usize = 80;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Task {
    pub id: u64,
    pub text: String,
    pub done: bool,
    /// Day number (see `day_number`) on which it was ticked off.
    #[serde(default)]
    pub done_day: Option<i32>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TodoList {
    tasks: Vec<Task>,
    next_id: u64,
}

/// Days since 1970-01-01 for a calendar date (proleptic Gregorian).
pub fn day_number(year: i32, month: u32, day: u32) -> i32 {
    let (y, m) = if month <= 2 { (year - 1, month as i32 + 9) } else { (year, month as i32 - 3) };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * m + 2) / 5 + day as i32 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

impl TodoList {
    pub fn tasks(&self) -> &[Task] {
        &self.tasks
    }

    /// (done, total)
    pub fn counts(&self) -> (usize, usize) {
        (self.tasks.iter().filter(|t| t.done).count(), self.tasks.len())
    }

    /// Adds a task at the top. Blank text adds nothing.
    pub fn add(&mut self, text: &str) -> Option<u64> {
        let text: String = text.trim().chars().take(MAX_TASK_CHARS).collect();
        if text.is_empty() {
            return None;
        }
        self.next_id += 1;
        self.tasks.insert(0, Task { id: self.next_id, text, done: false, done_day: None });
        Some(self.next_id)
    }

    pub fn toggle(&mut self, id: u64, today: i32) {
        if let Some(t) = self.tasks.iter_mut().find(|t| t.id == id) {
            t.done = !t.done;
            t.done_day = t.done.then_some(today);
        }
    }

    pub fn remove(&mut self, id: u64) {
        self.tasks.retain(|t| t.id != id);
    }

    /// Drops tasks that were finished before `today`. Returns whether any went.
    pub fn prune(&mut self, today: i32) -> bool {
        let before = self.tasks.len();
        self.tasks.retain(|t| !(t.done && t.done_day.is_some_and(|d| d < today)));
        self.tasks.len() != before
    }
}

pub fn to_json(list: &TodoList) -> String {
    serde_json::to_string_pretty(list).unwrap_or_default()
}

/// An empty list for anything that is not a valid saved list.
pub fn from_json(text: &str) -> TodoList {
    serde_json::from_str(text).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn day_numbers_match_known_dates() {
        assert_eq!(day_number(1970, 1, 1), 0);
        assert_eq!(day_number(2000, 3, 1), 11_017);
        assert_eq!(day_number(2026, 11, 9), 20_766);
        assert_eq!(day_number(2026, 11, 10) - day_number(2026, 11, 9), 1);
        assert_eq!(day_number(2024, 3, 1) - day_number(2024, 2, 28), 2);
    }

    #[test]
    fn new_tasks_go_on_top_and_blank_ones_are_refused() {
        let mut l = TodoList::default();
        l.add("first");
        l.add("  second  ");
        assert_eq!(l.add("   "), None);
        let texts: Vec<_> = l.tasks().iter().map(|t| t.text.as_str()).collect();
        assert_eq!(texts, ["second", "first"]);
    }

    #[test]
    fn long_text_is_cut_to_the_limit() {
        let mut l = TodoList::default();
        l.add(&"x".repeat(200));
        assert_eq!(l.tasks()[0].text.chars().count(), MAX_TASK_CHARS);
    }

    #[test]
    fn toggling_marks_the_day_and_toggling_back_clears_it() {
        let mut l = TodoList::default();
        let id = l.add("a").unwrap();
        l.toggle(id, 100);
        assert_eq!((l.tasks()[0].done, l.tasks()[0].done_day, l.counts()), (true, Some(100), (1, 1)));
        l.toggle(id, 100);
        assert_eq!((l.tasks()[0].done, l.tasks()[0].done_day, l.counts()), (false, None, (0, 1)));
    }

    #[test]
    fn remove_deletes_only_that_task() {
        let mut l = TodoList::default();
        let a = l.add("a").unwrap();
        l.add("b");
        l.remove(a);
        assert_eq!(l.tasks().len(), 1);
        assert_eq!(l.tasks()[0].text, "b");
        l.remove(999);
        assert_eq!(l.tasks().len(), 1);
    }

    #[test]
    fn ids_are_not_reused_after_a_removal() {
        let mut l = TodoList::default();
        let a = l.add("a").unwrap();
        l.remove(a);
        assert_ne!(l.add("b"), Some(a));
    }

    #[test]
    fn a_new_day_drops_only_tasks_finished_before_it() {
        let mut l = TodoList::default();
        let old = l.add("old").unwrap();
        let today = l.add("today").unwrap();
        l.add("open");
        l.toggle(old, 99);
        l.toggle(today, 100);
        assert!(l.prune(100));
        let texts: Vec<_> = l.tasks().iter().map(|t| t.text.as_str()).collect();
        assert_eq!(texts, ["open", "today"]);
        assert!(!l.prune(100));
    }

    #[test]
    fn the_list_round_trips_and_garbage_gives_an_empty_list() {
        let mut l = TodoList::default();
        let id = l.add("a").unwrap();
        l.add("b");
        l.toggle(id, 7);
        assert_eq!(from_json(&to_json(&l)), l);
        assert_eq!(from_json("nope"), TodoList::default());
        assert_eq!(from_json(""), TodoList::default());
    }
}
