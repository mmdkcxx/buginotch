// SPDX-License-Identifier: GPL-3.0-or-later
//! Small time/date formatters (Indonesian day and month names).

const DAYS: [&str; 7] = ["Min", "Sen", "Sel", "Rab", "Kam", "Jum", "Sab"];
const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "Mei", "Jun", "Jul", "Agu", "Sep", "Okt", "Nov", "Des"];

pub fn format_clock(hour: u32, minute: u32) -> String {
    format!("{hour:02}:{minute:02}")
}

/// "Sen 9 Nov 2026". `weekday` is 0 = Sunday; out-of-range values print "?".
pub fn format_date(year: i32, month: u32, day: u32, weekday: u32) -> String {
    let wd = DAYS.get(weekday as usize).copied().unwrap_or("?");
    let mo = month.checked_sub(1).and_then(|m| MONTHS.get(m as usize)).copied().unwrap_or("?");
    format!("{wd} {day} {mo} {year}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clock_pads_hours_and_minutes() {
        assert_eq!(format_clock(9, 5), "09:05");
        assert_eq!(format_clock(23, 59), "23:59");
    }

    #[test]
    fn date_uses_indonesian_names() {
        // 2026-11-09 is a Monday (weekday 1, Sunday = 0).
        assert_eq!(format_date(2026, 11, 9, 1), "Sen 9 Nov 2026");
        assert_eq!(format_date(2026, 1, 3, 0), "Min 3 Jan 2026");
    }

    #[test]
    fn date_out_of_range_does_not_panic() {
        assert_eq!(format_date(2026, 13, 1, 9), "? 1 ? 2026");
    }
}
