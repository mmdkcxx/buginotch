// SPDX-License-Identifier: GPL-3.0-or-later
//! Text helpers for the Widgets page: world-clock day tags and battery time left.

use crate::todo::day_number;

/// Where a city's date is compared with ours: "Hari ini", "Besok" or "Kemarin" (further apart: "+2 hari"). Dates are (year, month, day).
pub fn day_tag(local: (i32, u32, u32), zone: (i32, u32, u32)) -> String {
    match day_number(zone.0, zone.1, zone.2) - day_number(local.0, local.1, local.2) {
        0 => "Hari ini".to_string(),
        1 => "Besok".to_string(),
        -1 => "Kemarin".to_string(),
        n => format!("{n:+} hari"),
    }
}

/// "3 j 20 m" for the battery time left, "45 m" under an hour; `None` when the system does not know.
pub fn format_remaining(secs: Option<u32>) -> Option<String> {
    let secs = secs.filter(|&s| s != u32::MAX)?;
    let minutes = (secs / 60).max(1);
    Some(if minutes >= 60 { format!("{} j {} m", minutes / 60, minutes % 60) } else { format!("{minutes} m") })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_day_is_today() {
        assert_eq!(day_tag((2026, 11, 9), (2026, 11, 9)), "Hari ini");
    }

    #[test]
    fn next_and_previous_day_have_names() {
        assert_eq!(day_tag((2026, 11, 9), (2026, 11, 10)), "Besok");
        assert_eq!(day_tag((2026, 11, 9), (2026, 11, 8)), "Kemarin");
    }

    #[test]
    fn crosses_month_and_year_ends() {
        assert_eq!(day_tag((2026, 11, 30), (2026, 12, 1)), "Besok");
        assert_eq!(day_tag((2026, 12, 31), (2027, 1, 1)), "Besok");
        assert_eq!(day_tag((2027, 1, 1), (2026, 12, 31)), "Kemarin");
    }

    #[test]
    fn remaining_time_formats_hours_and_minutes() {
        assert_eq!(format_remaining(Some(3 * 3600 + 20 * 60)).as_deref(), Some("3 j 20 m"));
        assert_eq!(format_remaining(Some(45 * 60 + 30)).as_deref(), Some("45 m"));
        assert_eq!(format_remaining(Some(2 * 3600)).as_deref(), Some("2 j 0 m"));
    }

    #[test]
    fn unknown_or_absurd_remaining_time_is_hidden() {
        assert_eq!(format_remaining(None), None);
        assert_eq!(format_remaining(Some(u32::MAX)), None);
        assert_eq!(format_remaining(Some(30)), Some("1 m".to_string()));
    }
}
