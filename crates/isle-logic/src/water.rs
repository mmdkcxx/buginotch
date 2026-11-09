// SPDX-License-Identifier: GPL-3.0-or-later
//! Water tracking: a daily goal, glasses drunk per day, a seven-day chart and the
//! reminder schedule.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const DEFAULT_GOAL_ML: u32 = 2000;
pub const DEFAULT_GLASS_ML: u32 = 250;
/// Days of history kept.
pub const KEEP_DAYS: i32 = 30;
/// Reminders run from this minute of the day (08:00) …
pub const REMIND_START: u16 = 8 * 60;
/// … to this one (21:00), once per slot.
pub const REMIND_END: u16 = 21 * 60;
pub const REMIND_EVERY: u16 = 60;
/// A reminder can still fire this long after its slot begins.
pub const REMIND_WINDOW: u16 = 5;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WaterLog {
    #[serde(default = "default_goal")]
    pub goal_ml: u32,
    #[serde(default = "default_glass")]
    pub glass_ml: u32,
    /// Day number (see `todo::day_number`) -> millilitres drunk.
    #[serde(default)]
    days: BTreeMap<i32, u32>,
}

fn default_goal() -> u32 {
    DEFAULT_GOAL_ML
}
fn default_glass() -> u32 {
    DEFAULT_GLASS_ML
}

impl Default for WaterLog {
    fn default() -> Self {
        Self { goal_ml: DEFAULT_GOAL_ML, glass_ml: DEFAULT_GLASS_ML, days: BTreeMap::new() }
    }
}

impl WaterLog {
    pub fn drunk(&self, day: i32) -> u32 {
        self.days.get(&day).copied().unwrap_or(0)
    }

    pub fn add_glass(&mut self, day: i32) {
        *self.days.entry(day).or_insert(0) += self.glass_ml;
    }

    /// Takes one glass back (a mis-tap), never below zero.
    pub fn undo_glass(&mut self, day: i32) {
        let now = self.drunk(day).saturating_sub(self.glass_ml);
        self.days.insert(day, now);
    }

    /// Progress towards the goal, 0..=1.
    pub fn progress(&self, day: i32) -> f32 {
        if self.goal_ml == 0 { 1.0 } else { (self.drunk(day) as f32 / self.goal_ml as f32).min(1.0) }
    }

    /// The last seven days, oldest first, ending with `today`.
    pub fn last_week(&self, today: i32) -> [u32; 7] {
        std::array::from_fn(|i| self.drunk(today - 6 + i as i32))
    }

    /// Drops history older than `KEEP_DAYS`. Returns whether anything went.
    pub fn prune(&mut self, today: i32) -> bool {
        let before = self.days.len();
        self.days.retain(|&d, _| today - d < KEEP_DAYS);
        self.days.len() != before
    }
}

/// Weekday of a day number, 0 = Sunday (1970-01-01 was a Thursday).
pub fn weekday(day: i32) -> u32 {
    (day + 4).rem_euclid(7) as u32
}

/// Short Indonesian weekday initial for chart labels.
pub fn weekday_label(day: i32) -> &'static str {
    ["M", "S", "S", "R", "K", "J", "S"][weekday(day) as usize]
}

/// Number of reminder slots in a day.
fn slots() -> u32 {
    ((REMIND_END - REMIND_START) / REMIND_EVERY) as u32
}

/// The reminder slot (1-based) that `minute` falls in the first minutes of, if any.
pub fn slot_at(minute: u16) -> Option<u32> {
    if minute < REMIND_START || minute >= REMIND_END {
        return None;
    }
    let since = minute - REMIND_START;
    (since % REMIND_EVERY <= REMIND_WINDOW).then_some((since / REMIND_EVERY) as u32 + 1)
}

/// Whether to remind now: a new slot has begun, the goal is not met, and the day's drinking
/// is behind the even pace (goal spread over the slots). Returns how many ml are still missing.
pub fn reminder(minute: u16, last_slot: Option<u32>, drunk_ml: u32, goal_ml: u32) -> Option<u32> {
    let slot = slot_at(minute)?;
    if last_slot.is_some_and(|l| l >= slot) || drunk_ml >= goal_ml {
        return None;
    }
    let pace = goal_ml * slot / slots();
    (drunk_ml < pace).then_some(goal_ml - drunk_ml)
}

pub fn to_json(log: &WaterLog) -> String {
    serde_json::to_string_pretty(log).unwrap_or_default()
}

pub fn from_json(text: &str) -> WaterLog {
    serde_json::from_str(text).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn glasses_add_up_and_undo_never_goes_below_zero() {
        let mut w = WaterLog::default();
        w.add_glass(10);
        w.add_glass(10);
        assert_eq!(w.drunk(10), 500);
        w.undo_glass(10);
        w.undo_glass(10);
        w.undo_glass(10);
        assert_eq!(w.drunk(10), 0);
        assert_eq!(w.drunk(11), 0);
    }

    #[test]
    fn progress_is_capped_at_the_goal() {
        let mut w = WaterLog { goal_ml: 1000, ..Default::default() };
        for _ in 0..2 {
            w.add_glass(1);
        }
        assert_eq!(w.progress(1), 0.5);
        for _ in 0..10 {
            w.add_glass(1);
        }
        assert_eq!(w.progress(1), 1.0);
        assert_eq!(WaterLog { goal_ml: 0, ..Default::default() }.progress(1), 1.0);
    }

    #[test]
    fn the_week_ends_today_and_old_days_are_pruned() {
        let mut w = WaterLog::default();
        w.add_glass(100);
        w.add_glass(94);
        w.add_glass(60);
        assert_eq!(w.last_week(100), [250, 0, 0, 0, 0, 0, 250]);
        assert!(w.prune(100));
        assert_eq!(w.drunk(60), 0);
        assert_eq!(w.drunk(94), 250);
        assert!(!w.prune(100));
    }

    #[test]
    fn weekdays_follow_the_calendar() {
        assert_eq!(weekday(0), 4, "1970-01-01 was a Thursday");
        assert_eq!(weekday(20_766), 1, "2026-11-09 is a Monday");
        assert_eq!(weekday_label(20_766), "S");
        assert_eq!(weekday(-1), 3);
    }

    #[test]
    fn slots_start_on_the_hour_and_last_five_minutes() {
        assert_eq!(slot_at(7 * 60 + 59), None);
        assert_eq!(slot_at(8 * 60), Some(1));
        assert_eq!(slot_at(8 * 60 + 5), Some(1));
        assert_eq!(slot_at(8 * 60 + 6), None);
        assert_eq!(slot_at(20 * 60), Some(13));
        assert_eq!(slot_at(21 * 60), None);
    }

    #[test]
    fn a_reminder_fires_only_when_behind_pace_and_once_per_slot() {
        // Slot 4 (11:00): pace = 2000 * 4 / 13 = 615 ml.
        let m = 11 * 60;
        assert_eq!(reminder(m, None, 500, 2000), Some(1500));
        assert_eq!(reminder(m, None, 700, 2000), None, "ahead of pace");
        assert_eq!(reminder(m, Some(4), 0, 2000), None, "already reminded this slot");
        assert_eq!(reminder(m, Some(3), 0, 2000), Some(2000));
        assert_eq!(reminder(m, None, 2000, 2000), None, "goal met");
        assert_eq!(reminder(7 * 60, None, 0, 2000), None, "too early");
    }

    #[test]
    fn saved_logs_round_trip_and_old_or_broken_files_load() {
        let mut w = WaterLog::default();
        w.add_glass(5);
        assert_eq!(from_json(&to_json(&w)), w);
        assert_eq!(from_json("{}"), WaterLog::default());
        assert_eq!(from_json("nope"), WaterLog::default());
        assert_eq!(from_json(r#"{"goal_ml":3000}"#).goal_ml, 3000);
    }
}
