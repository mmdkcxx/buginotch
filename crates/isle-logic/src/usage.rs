// SPDX-License-Identifier: GPL-3.0-or-later
//! Claude plan usage: the 5-hour and weekly windows, how they read, and when to tell the user
//! a reset is near. The numbers come from the same endpoint Claude Code uses; it is not a
//! documented API, so every reader here returns `None` instead of guessing.

use serde_json::Value;

/// Yellow from here.
pub const WARN_PERCENT: f32 = 75.0;
/// Red from here.
pub const HIGH_PERCENT: f32 = 90.0;
/// The pill shows the usage from here up.
pub const PILL_PERCENT: f32 = WARN_PERCENT;
const MIN: i64 = 60_000;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Window {
    /// 0..=100 of the allowance used.
    pub percent: f32,
    pub resets_at_ms: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Usage {
    pub five: Option<Window>,
    pub week: Option<Window>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Which {
    Five,
    Week,
}

impl Which {
    pub fn name(self) -> &'static str {
        match self {
            Which::Five => "5 jam",
            Which::Week => "Mingguan",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Ok,
    Warn,
    High,
}

pub fn level(percent: f32) -> Level {
    if percent >= HIGH_PERCENT {
        Level::High
    } else if percent >= WARN_PERCENT {
        Level::Warn
    } else {
        Level::Ok
    }
}

impl Usage {
    pub fn get(&self, w: Which) -> Option<Window> {
        match w {
            Which::Five => self.five,
            Which::Week => self.week,
        }
    }

    /// The higher of the two, for the closed pill.
    pub fn highest(&self) -> Option<f32> {
        [self.five, self.week].into_iter().flatten().map(|w| w.percent).reduce(f32::max)
    }
}

/// "2026-11-10T05:30:00.067026+00:00" (or "Z") as epoch milliseconds.
pub fn parse_iso(s: &str) -> Option<i64> {
    let b = s.as_bytes();
    if b.len() < 19 || b[4] != b'-' || b[7] != b'-' || !(b[10] == b'T' || b[10] == b' ') || b[13] != b':' || b[16] != b':' {
        return None;
    }
    let num = |a: usize, z: usize| s.get(a..z)?.parse::<i64>().ok();
    let (y, mo, d, h, mi, sec) = (num(0, 4)?, num(5, 7)?, num(8, 10)?, num(11, 13)?, num(14, 16)?, num(17, 19)?);
    if !(1..=12).contains(&mo) || !(1..=31).contains(&d) || h > 23 || mi > 59 || sec > 60 {
        return None;
    }
    let mut rest = &s[19..];
    let mut millis = 0i64;
    if let Some(frac) = rest.strip_prefix('.') {
        let n = frac.bytes().take_while(u8::is_ascii_digit).count();
        if n == 0 {
            return None;
        }
        let three: String = frac[..n].chars().chain("000".chars()).take(3).collect();
        millis = three.parse().ok()?;
        rest = &frac[n..];
    }
    let offset_min = match rest {
        "" | "Z" => 0,
        r if r.len() == 6 && (r.starts_with('+') || r.starts_with('-')) && r.as_bytes()[3] == b':' => {
            let v = r[1..3].parse::<i64>().ok()? * 60 + r[4..6].parse::<i64>().ok()?;
            if r.starts_with('-') { -v } else { v }
        }
        _ => return None,
    };
    // Days since 1970-01-01 (civil-from-days, Howard Hinnant).
    let (yy, m) = if mo <= 2 { (y - 1, mo + 9) } else { (y, mo - 3) };
    let era = yy.div_euclid(400);
    let yoe = yy - era * 400;
    let doy = (153 * m + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    Some(((days * 86_400 + h * 3600 + mi * 60 + sec - offset_min * 60) * 1000) + millis)
}

fn window(v: &Value) -> Option<Window> {
    if !v.is_object() {
        return None;
    }
    let percent = v["utilization"].as_f64()?.clamp(0.0, 100.0) as f32;
    Some(Window { percent, resets_at_ms: v["resets_at"].as_str().and_then(parse_iso) })
}

/// Reads the endpoint's JSON. `None` unless at least one window is there.
pub fn parse_usage(text: &str) -> Option<Usage> {
    let v: Value = serde_json::from_str(text).ok()?;
    let u = Usage { five: window(&v["five_hour"]), week: window(&v["seven_day"]) };
    (u.five.is_some() || u.week.is_some()).then_some(u)
}

#[derive(Clone, PartialEq, Eq)]
pub struct Credentials {
    pub token: String,
    pub expires_ms: i64,
}

// The token never shows up in logs or panics.
impl std::fmt::Debug for Credentials {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Credentials {{ token: <hidden>, expires_ms: {} }}", self.expires_ms)
    }
}

/// The Claude Code login in `.credentials.json`.
pub fn parse_credentials(text: &str) -> Option<Credentials> {
    let v: Value = serde_json::from_str(text).ok()?;
    let o = &v["claudeAiOauth"];
    let token = o["accessToken"].as_str().filter(|t| !t.is_empty())?.to_string();
    Some(Credentials { token, expires_ms: o["expiresAt"].as_i64().unwrap_or(0) })
}

/// "3h 21j" (days, hours), "2j 10m", "48m", "<1m".
pub fn format_left(ms: i64) -> String {
    let m = ms.max(0) / MIN;
    let (d, h, mi) = (m / 1440, m % 1440 / 60, m % 60);
    if d > 0 {
        format!("{d}h {h}j")
    } else if h > 0 {
        format!("{h}j {mi:02}m")
    } else if m > 0 {
        format!("{m}m")
    } else {
        "<1m".to_string()
    }
}

/// "Sen" .. "Min" for an epoch time shifted by the local offset.
pub fn weekday_short(epoch_ms: i64, utc_offset_min: i64) -> &'static str {
    let days = (epoch_ms / 1000 + utc_offset_min * 60).div_euclid(86_400);
    // 1970-01-01 was a Thursday.
    ["Kam", "Jum", "Sab", "Min", "Sen", "Sel", "Rab"][days.rem_euclid(7) as usize]
}

/// When a window resets: just the time within a day ("13:55"), else with the weekday ("Jum 09:00").
pub fn reset_text(resets_at_ms: i64, now_ms: i64, utc_offset_min: i64) -> String {
    let clock = crate::digioh::clock_text(resets_at_ms, utc_offset_min);
    if resets_at_ms - now_ms < 20 * 3_600_000 {
        clock
    } else {
        format!("{} {clock}", weekday_short(resets_at_ms, utc_offset_min))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Notice {
    /// This many minutes (10 or 5) to the reset.
    Soon(Which, i64),
    /// The reset time has passed.
    Reset(Which),
}

#[derive(Debug, Clone, Copy, Default)]
struct Track {
    target: Option<i64>,
    sent10: bool,
    sent5: bool,
    done: bool,
}

/// Decides when to tell the user about a reset: once at 10 minutes, once at 5, once at the
/// reset. Whatever is already past when a target is first seen stays quiet, so starting the
/// app 7 minutes before a reset does not announce the 10-minute mark.
#[derive(Debug, Clone, Default)]
pub struct Notifier {
    five: Track,
    week: Track,
}

impl Notifier {
    fn track(&mut self, w: Which) -> &mut Track {
        match w {
            Which::Five => &mut self.five,
            Which::Week => &mut self.week,
        }
    }

    /// A fresh reading. Only a different reset time starts a new round.
    pub fn observe(&mut self, w: Which, resets_at_ms: Option<i64>, now_ms: i64) {
        let t = self.track(w);
        let Some(target) = resets_at_ms else { return };
        if t.target.is_some_and(|old| (old - target).abs() < MIN) {
            return;
        }
        let rem = target - now_ms;
        *t = Track { target: Some(target), sent10: rem <= 10 * MIN, sent5: rem <= 5 * MIN, done: rem <= 0 };
    }

    /// What is due now. Call it often (every second is fine).
    pub fn due(&mut self, now_ms: i64) -> Vec<Notice> {
        let mut out = Vec::new();
        for w in [Which::Five, Which::Week] {
            let t = self.track(w);
            let Some(target) = t.target else { continue };
            let rem = target - now_ms;
            if rem <= 0 {
                if !t.done {
                    t.done = true;
                    t.sent10 = true;
                    t.sent5 = true;
                    out.push(Notice::Reset(w));
                }
            } else if rem <= 5 * MIN {
                if !t.sent5 {
                    t.sent5 = true;
                    t.sent10 = true;
                    out.push(Notice::Soon(w, 5));
                }
            } else if rem <= 10 * MIN && !t.sent10 {
                t.sent10 = true;
                out.push(Notice::Soon(w, 10));
            }
        }
        out
    }

    /// The soonest reset still ahead, for deciding how often to ask again.
    pub fn nearest(&self, now_ms: i64) -> Option<i64> {
        [self.five.target, self.week.target].into_iter().flatten().map(|t| t - now_ms).filter(|r| *r > -3 * MIN).min()
    }
}

/// Seconds to wait before asking the endpoint again: every 3 minutes, every minute around a reset.
pub fn poll_seconds(nearest_reset_ms: Option<i64>) -> u64 {
    match nearest_reset_ms {
        Some(r) if r <= 12 * MIN && r > -3 * MIN => 60,
        _ => 180,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"{"five_hour":{"utilization":55.0,"resets_at":"2026-11-10T05:30:00.067026+00:00","limit_dollars":null},"seven_day":{"utilization":42.0,"resets_at":"2026-11-14T16:00:00.067043+00:00"},"seven_day_opus":null,"limits":[]}"#;

    #[test]
    fn iso_times() {
        assert_eq!(parse_iso("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(parse_iso("2026-11-10T05:30:00.067026+00:00"), Some(1_794_288_600_067));
        assert_eq!(parse_iso("2026-11-10T12:30:00+07:00"), Some(1_794_288_600_000));
        assert_eq!(parse_iso("2024-02-29T00:00:00Z"), Some(1_709_164_800_000));
        assert_eq!(parse_iso("2026-13-10T05:30:00Z"), None);
        assert_eq!(parse_iso("nonsense"), None);
        assert_eq!(parse_iso("2026-11-10T05:30:00+0700"), None);
    }

    #[test]
    fn the_endpoint_reply_is_read() {
        let u = parse_usage(SAMPLE).unwrap();
        assert_eq!(u.five.unwrap().percent, 55.0);
        assert_eq!(u.week.unwrap().resets_at_ms, Some(parse_iso("2026-11-14T16:00:00Z").unwrap() + 67));
        assert_eq!(u.highest(), Some(55.0));
        assert!(parse_usage("{}").is_none() && parse_usage("[]").is_none() && parse_usage("x").is_none());
        let only_week = parse_usage(r#"{"five_hour":null,"seven_day":{"utilization":120,"resets_at":null}}"#).unwrap();
        assert!(only_week.five.is_none());
        assert_eq!(only_week.week.unwrap().percent, 100.0, "clamped");
        assert_eq!(only_week.week.unwrap().resets_at_ms, None);
    }

    #[test]
    fn credentials_are_read_and_hidden() {
        let c = parse_credentials(r#"{"claudeAiOauth":{"accessToken":"sk-secret","expiresAt":1794288600000},"mcpOAuth":{}}"#).unwrap();
        assert_eq!(c.expires_ms, 1_794_288_600_000);
        assert!(!format!("{c:?}").contains("sk-secret"));
        assert!(parse_credentials(r#"{"mcpOAuth":{}}"#).is_none());
        assert!(parse_credentials(r#"{"claudeAiOauth":{"accessToken":""}}"#).is_none());
    }

    #[test]
    fn levels_and_text() {
        assert_eq!(level(74.9), Level::Ok);
        assert_eq!(level(75.0), Level::Warn);
        assert_eq!(level(90.0), Level::High);
        assert_eq!(format_left(2 * 3_600_000 + 10 * MIN + 5000), "2j 10m");
        assert_eq!(format_left(48 * MIN), "48m");
        assert_eq!(format_left(30_000), "<1m");
        assert_eq!(format_left(3 * 86_400_000 + 21 * 3_600_000), "3h 21j");
        // 2026-11-13 is a Friday.
        let fri = parse_iso("2026-11-13T02:00:00Z").unwrap();
        assert_eq!(weekday_short(fri, 0), "Jum");
        assert_eq!(weekday_short(fri, 7 * 60), "Jum");
        assert_eq!(weekday_short(fri, -3 * 60), "Kam");
        assert_eq!(reset_text(fri, fri - 3_600_000, 7 * 60), "09:00");
        assert_eq!(reset_text(fri, fri - 3 * 86_400_000, 7 * 60), "Jum 09:00");
    }

    const T: i64 = 10_000_000_000;

    #[test]
    fn notices_come_once_each_in_order() {
        let mut n = Notifier::default();
        n.observe(Which::Five, Some(T), T - 30 * MIN);
        assert!(n.due(T - 30 * MIN).is_empty());
        assert!(n.due(T - 11 * MIN).is_empty());
        assert_eq!(n.due(T - 10 * MIN), vec![Notice::Soon(Which::Five, 10)]);
        assert!(n.due(T - 9 * MIN).is_empty(), "only once");
        assert_eq!(n.due(T - 5 * MIN), vec![Notice::Soon(Which::Five, 5)]);
        assert!(n.due(T - 4 * MIN).is_empty());
        assert_eq!(n.due(T), vec![Notice::Reset(Which::Five)]);
        assert!(n.due(T + MIN).is_empty());
    }

    #[test]
    fn a_late_start_stays_quiet_about_what_is_past() {
        let mut n = Notifier::default();
        n.observe(Which::Five, Some(T), T - 7 * MIN);
        assert!(n.due(T - 7 * MIN).is_empty(), "the 10 minute mark is already past");
        assert_eq!(n.due(T - 5 * MIN), vec![Notice::Soon(Which::Five, 5)]);
        let mut late = Notifier::default();
        late.observe(Which::Week, Some(T), T + MIN);
        assert!(late.due(T + MIN).is_empty(), "already reset when first seen");
    }

    #[test]
    fn a_jump_straight_to_the_reset_says_only_the_reset() {
        let mut n = Notifier::default();
        n.observe(Which::Five, Some(T), T - 30 * MIN);
        assert_eq!(n.due(T + 2 * MIN), vec![Notice::Reset(Which::Five)], "the laptop slept through it");
    }

    #[test]
    fn a_new_reset_time_starts_a_new_round() {
        let mut n = Notifier::default();
        n.observe(Which::Five, Some(T), T - 20 * MIN);
        n.due(T);
        // The next window: reset five hours later.
        let next = T + 5 * 3_600_000;
        n.observe(Which::Five, Some(next + 67), T + 20 * MIN);
        assert!(n.due(T + 20 * MIN).is_empty());
        assert_eq!(n.due(next + 67 - 10 * MIN), vec![Notice::Soon(Which::Five, 10)]);
        // The same time reported again (a few ms apart) changes nothing.
        n.observe(Which::Five, Some(next + 80), next - 9 * MIN);
        assert!(n.due(next - 9 * MIN).is_empty());
        // A missing reading keeps what is tracked.
        n.observe(Which::Five, None, next - 8 * MIN);
        assert_eq!(n.due(next + 67), vec![Notice::Reset(Which::Five)]);
    }

    #[test]
    fn both_windows_are_tracked_separately() {
        let mut n = Notifier::default();
        n.observe(Which::Five, Some(T), T - 20 * MIN);
        n.observe(Which::Week, Some(T + 3 * 86_400_000), T - 20 * MIN);
        assert_eq!(n.due(T), vec![Notice::Reset(Which::Five)]);
        assert_eq!(n.nearest(T + 3 * 86_400_000 - MIN), Some(MIN));
    }

    #[test]
    fn polling_speeds_up_around_a_reset() {
        assert_eq!(poll_seconds(None), 180);
        assert_eq!(poll_seconds(Some(3_600_000)), 180);
        assert_eq!(poll_seconds(Some(11 * MIN)), 60);
        assert_eq!(poll_seconds(Some(-MIN)), 60);
        assert_eq!(poll_seconds(Some(-10 * MIN)), 180);
    }
}
