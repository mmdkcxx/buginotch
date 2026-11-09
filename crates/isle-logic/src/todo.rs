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
    /// Reminder time as minutes after midnight, typed as a leading "HH:MM".
    #[serde(default)]
    pub due: Option<u16>,
    /// Day number on which the reminder last fired.
    #[serde(default)]
    pub reminded_day: Option<i32>,
}

/// How long after the due time a reminder can still fire (so a missed one stays quiet).
pub const REMIND_WINDOW_MIN: u16 = 5;

/// Splits a leading "HH:MM " (or "HH.MM ") off a task's text. Anything else is no time.
pub fn parse_due(text: &str) -> (Option<u16>, String) {
    let text = text.trim();
    let none = || (None, text.to_string());
    let Some((head, rest)) = text.split_once(' ') else { return none() };
    let Some((h, m)) = head.split_once([':', '.']) else { return none() };
    let digits = |s: &str, max: usize| !s.is_empty() && s.len() <= max && s.bytes().all(|b| b.is_ascii_digit());
    if !digits(h, 2) || m.len() != 2 || !digits(m, 2) || rest.trim().is_empty() {
        return none();
    }
    let (h, m): (u16, u16) = (h.parse().unwrap_or(99), m.parse().unwrap_or(99));
    if h > 23 || m > 59 {
        return none();
    }
    (Some(h * 60 + m), rest.trim().to_string())
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
        let (due, text) = parse_due(text);
        let text: String = text.chars().take(MAX_TASK_CHARS).collect();
        if text.is_empty() {
            return None;
        }
        self.next_id += 1;
        self.tasks.insert(0, Task { id: self.next_id, text, done: false, done_day: None, due, reminded_day: None });
        Some(self.next_id)
    }

    /// An open task whose reminder is due at `minute` (minutes after midnight) today and
    /// has not fired yet today.
    pub fn due_now(&self, today: i32, minute: u16) -> Option<&Task> {
        self.tasks.iter().find(|t| {
            !t.done
                && t.reminded_day != Some(today)
                && t.due.is_some_and(|d| minute >= d && minute - d <= REMIND_WINDOW_MIN)
        })
    }

    pub fn mark_reminded(&mut self, id: u64, today: i32) {
        if let Some(t) = self.tasks.iter_mut().find(|t| t.id == id) {
            t.reminded_day = Some(today);
        }
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

    #[test]
    fn a_leading_time_becomes_the_due_time() {
        assert_eq!(parse_due("14:30 Rapat tim"), (Some(14 * 60 + 30), "Rapat tim".to_string()));
        assert_eq!(parse_due("9.05 minum obat"), (Some(9 * 60 + 5), "minum obat".to_string()));
        assert_eq!(parse_due("Rapat 14:30"), (None, "Rapat 14:30".to_string()));
        assert_eq!(parse_due("25:00 salah"), (None, "25:00 salah".to_string()));
        assert_eq!(parse_due("12:61 salah"), (None, "12:61 salah".to_string()));
        assert_eq!(parse_due("14:30"), (None, "14:30".to_string()));
    }

    #[test]
    fn adding_keeps_the_due_time_out_of_the_text() {
        let mut l = TodoList::default();
        l.add("08:00 Sarapan");
        assert_eq!((l.tasks()[0].text.as_str(), l.tasks()[0].due), ("Sarapan", Some(480)));
    }

    #[test]
    fn a_reminder_fires_once_in_the_five_minutes_after_the_due_time() {
        let mut l = TodoList::default();
        let id = l.add("10:00 Rapat").unwrap();
        assert_eq!(l.due_now(100, 599), None);
        assert_eq!(l.due_now(100, 600).map(|t| t.id), Some(id));
        assert_eq!(l.due_now(100, 605).map(|t| t.id), Some(id));
        assert_eq!(l.due_now(100, 606), None);
        l.mark_reminded(id, 100);
        assert_eq!(l.due_now(100, 601), None);
        assert_eq!(l.due_now(101, 601).map(|t| t.id), Some(id) , "a new day reminds again");
    }

    #[test]
    fn finished_tasks_do_not_remind() {
        let mut l = TodoList::default();
        let id = l.add("10:00 Rapat").unwrap();
        l.toggle(id, 100);
        assert_eq!(l.due_now(100, 600), None);
    }

    #[test]
    fn old_saved_lists_without_due_times_still_load() {
        let l = from_json(r#"{"tasks":[{"id":1,"text":"a","done":false}],"next_id":1}"#);
        assert_eq!((l.tasks().len(), l.tasks()[0].due), (1, None));
    }
}
