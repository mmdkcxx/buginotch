// SPDX-License-Identifier: GPL-3.0-or-later
//! Timer Digioh: the room code and the timer state shared with timer.digioh.id.
//!
//! The site keeps one JSON state per room and relays it between everyone in the room. A
//! controller changes the timer by sending the whole state back, so every change here edits
//! the state that arrived and keeps every field it does not know (appearance, layouts).

use serde_json::{Value, json};

/// Durations offered on the control page, in seconds (the same ones the website offers).
pub const PRESETS: [i64; 6] = [60, 180, 300, 600, 900, 1800];
pub const MIN_ROOM: usize = 5;
pub const MAX_ROOM: usize = 8;
/// The last minute turns the timer amber.
pub const WARN_SECONDS: f64 = 60.0;

/// A room code as the site writes it: capital letters and digits, up to 8.
pub fn normalize_room(s: &str) -> String {
    s.to_uppercase().chars().filter(|c| c.is_ascii_alphanumeric()).take(MAX_ROOM).collect()
}

pub fn valid_room(s: &str) -> bool {
    (MIN_ROOM..=MAX_ROOM).contains(&s.len()) && s.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
}

/// The WebSocket path for a room.
pub fn ws_path(room: &str) -> String {
    format!("/ws?room={room}")
}

/// "03:12", "1:05:00" or "-00:23".
pub fn format_time(seconds: f64) -> String {
    let t = seconds.abs().floor() as i64;
    let (h, m, s) = (t / 3600, t % 3600 / 60, t % 60);
    let body = if h > 0 { format!("{h}:{m:02}:{s:02}") } else { format!("{m:02}:{s:02}") };
    if seconds < 0.0 { format!("-{body}") } else { body }
}

/// "11:15" for an epoch time, shifted by the local offset in minutes.
pub fn clock_text(epoch_ms: i64, utc_offset_min: i64) -> String {
    let secs = (epoch_ms / 1000 + utc_offset_min * 60).rem_euclid(86_400);
    format!("{:02}:{:02}", secs / 3600, secs % 3600 / 60)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    /// Stopped at its full length.
    Ready,
    Run,
    /// Running, last minute.
    Warn,
    Paused,
    /// Past zero and counting up.
    Over,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Timer {
    state: Value,
}

impl Default for Timer {
    fn default() -> Self {
        Timer { state: json!({ "status": "idle", "duration": 300, "startedAt": null, "pausedRemaining": null, "displayMode": "timer", "stageMessage": null }) }
    }
}

impl Timer {
    /// Reads a `{"type":"STATE","state":{...}}` message.
    pub fn from_message(text: &str) -> Option<Timer> {
        let v: Value = serde_json::from_str(text).ok()?;
        if v["type"] != "STATE" || !v["state"].is_object() {
            return None;
        }
        Some(Timer { state: v["state"].clone() })
    }

    fn status(&self) -> &str {
        self.state["status"].as_str().unwrap_or("idle")
    }

    pub fn duration(&self) -> i64 {
        self.state["duration"].as_f64().unwrap_or(300.0).max(1.0) as i64
    }

    fn started_at(&self) -> Option<f64> {
        self.state["startedAt"].as_f64()
    }

    fn paused_remaining(&self) -> Option<f64> {
        self.state["pausedRemaining"].as_f64()
    }

    pub fn is_idle(&self) -> bool {
        self.status() == "idle"
    }

    pub fn is_paused(&self) -> bool {
        self.status() == "paused" && self.paused_remaining().is_some()
    }

    /// Seconds left (negative once it has run out), the same way the website counts.
    pub fn remaining(&self, now_ms: i64) -> f64 {
        let d = self.duration() as f64;
        if self.status() == "idle" {
            return d;
        }
        if let (true, Some(p)) = (self.status() == "paused", self.paused_remaining()) {
            return p;
        }
        match self.started_at() {
            Some(s) => d - (now_ms as f64 - s) / 1000.0,
            None => d,
        }
    }

    pub fn phase(&self, now_ms: i64) -> Phase {
        let r = self.remaining(now_ms);
        match self.status() {
            "idle" => Phase::Ready,
            "paused" => Phase::Paused,
            _ if r < 0.0 => Phase::Over,
            _ if r <= WARN_SECONDS => Phase::Warn,
            _ => Phase::Run,
        }
    }

    /// How much of the ruler is still lit: 1 when full, 0 when out; stays 1 while overtime.
    pub fn fraction_left(&self, now_ms: i64) -> f32 {
        (self.remaining(now_ms) / self.duration() as f64).clamp(0.0, 1.0) as f32
    }

    /// When the timer reaches zero, if it is running or paused (a paused one counts from now).
    pub fn ends_at(&self, now_ms: i64) -> Option<i64> {
        if self.is_idle() {
            return None;
        }
        Some(now_ms + (self.remaining(now_ms) * 1000.0) as i64)
    }

    fn message(state: Value) -> (Timer, String) {
        let text = json!({ "type": "STATE", "state": state }).to_string();
        (Timer { state }, text)
    }

    fn set(&self, patch: &[(&str, Value)]) -> Value {
        let mut s = self.state.clone();
        for (k, v) in patch {
            s[*k] = v.clone();
        }
        s
    }

    /// Start, or resume a paused timer. `None` when it is already running.
    pub fn start(&self, now_ms: i64) -> Option<(Timer, String)> {
        if self.is_paused() {
            let remaining = self.paused_remaining()?;
            let started = now_ms as f64 - (self.duration() as f64 - remaining) * 1000.0;
            return Some(Self::message(self.set(&[("status", json!("running")), ("startedAt", json!(started as i64)), ("pausedRemaining", Value::Null)])));
        }
        if self.is_idle() {
            return Some(Self::message(self.set(&[("status", json!("running")), ("startedAt", json!(now_ms)), ("pausedRemaining", Value::Null)])));
        }
        None
    }

    pub fn pause(&self, now_ms: i64) -> Option<(Timer, String)> {
        if !matches!(self.status(), "running" | "overtime") {
            return None;
        }
        Some(Self::message(self.set(&[("status", json!("paused")), ("pausedRemaining", json!(self.remaining(now_ms)))])))
    }

    pub fn reset(&self) -> (Timer, String) {
        Self::message(self.set(&[("status", json!("idle")), ("startedAt", Value::Null), ("pausedRemaining", Value::Null)]))
    }

    /// Adds seconds without stopping: to the time left while paused, else to the length.
    pub fn add_seconds(&self, secs: i64) -> (Timer, String) {
        if let (true, Some(p)) = (self.is_paused(), self.paused_remaining()) {
            return Self::message(self.set(&[("pausedRemaining", json!(p + secs as f64))]));
        }
        Self::message(self.set(&[("duration", json!(self.duration() + secs))]))
    }

    pub fn set_duration(&self, secs: i64) -> (Timer, String) {
        Self::message(self.set(&[("status", json!("idle")), ("duration", json!(secs.max(1))), ("startedAt", Value::Null), ("pausedRemaining", Value::Null)]))
    }

    /// Start or pause, whichever the main button should do now.
    pub fn toggle(&self, now_ms: i64) -> Option<(Timer, String)> {
        self.start(now_ms).or_else(|| self.pause(now_ms))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: i64 = 1_800_000_000_000;

    fn running(started_ago_s: i64, duration: i64) -> Timer {
        Timer::from_message(&json!({ "type": "STATE", "state": { "status": "running", "duration": duration, "startedAt": NOW - started_ago_s * 1000, "pausedRemaining": null, "appearance": { "bgColor": "#123456" } } }).to_string()).unwrap()
    }

    #[test]
    fn room_codes_are_cleaned_and_checked() {
        assert_eq!(normalize_room(" k7m-2q! "), "K7M2Q");
        assert_eq!(normalize_room("abcdefghijk"), "ABCDEFGH");
        assert!(valid_room("K7M2Q") && valid_room("ABCDEFGH"));
        assert!(!valid_room("K7M2") && !valid_room("k7m2q") && !valid_room("ABCDEFGHI"));
        assert_eq!(ws_path("K7M2Q"), "/ws?room=K7M2Q");
    }

    #[test]
    fn times_read_like_the_website() {
        assert_eq!(format_time(192.0), "03:12");
        assert_eq!(format_time(3905.0), "1:05:05");
        assert_eq!(format_time(-23.4), "-00:23");
        assert_eq!(format_time(0.0), "00:00");
        assert_eq!(format_time(59.9), "00:59");
        assert_eq!(clock_text(NOW, 0), "08:00");
        assert_eq!(clock_text(NOW, 420), "15:00");
    }

    #[test]
    fn a_state_message_is_read_and_others_are_not() {
        assert!(Timer::from_message("{\"type\":\"REQUEST_STATE\"}").is_none());
        assert!(Timer::from_message("nonsense").is_none());
        assert!(Timer::from_message("{\"type\":\"STATE\"}").is_none());
        assert_eq!(Timer::default().duration(), 300);
    }

    #[test]
    fn remaining_follows_the_start_time() {
        let t = running(108, 300);
        assert!((t.remaining(NOW) - 192.0).abs() < 1e-6);
        assert_eq!(t.phase(NOW), Phase::Run);
        assert!((t.fraction_left(NOW) - 0.64).abs() < 1e-6);
        assert_eq!(running(250, 300).phase(NOW), Phase::Warn);
        assert_eq!(running(323, 300).phase(NOW), Phase::Over);
        assert_eq!(running(323, 300).fraction_left(NOW), 0.0);
        assert_eq!(Timer::default().phase(NOW), Phase::Ready);
        assert_eq!(Timer::default().remaining(NOW), 300.0);
    }

    #[test]
    fn start_pause_and_resume_keep_the_time_and_every_other_field() {
        let (t, msg) = Timer::default().start(NOW).unwrap();
        assert_eq!(t.phase(NOW), Phase::Run);
        assert!(Timer::default().start(NOW).is_some() && t.start(NOW).is_none());
        let v: Value = serde_json::from_str(&msg).unwrap();
        assert_eq!(v["type"], "STATE");
        assert_eq!(v["state"]["startedAt"], NOW);

        let r = running(100, 300);
        let (p, msg) = r.pause(NOW).unwrap();
        assert!(p.is_paused());
        assert!((p.remaining(NOW + 99_000) - 200.0).abs() < 1e-6, "a paused timer does not move");
        assert!(msg.contains("#123456"), "unknown fields survive: {msg}");
        let (resumed, _) = p.start(NOW + 60_000).unwrap();
        assert!((resumed.remaining(NOW + 60_000) - 200.0).abs() < 1e-3);
        assert!(Timer::default().pause(NOW).is_none());
    }

    #[test]
    fn reset_and_extra_minutes() {
        let r = running(100, 300);
        let (idle, _) = r.reset();
        assert!(idle.is_idle() && idle.duration() == 300);
        let (longer, _) = r.add_seconds(60);
        assert_eq!(longer.duration(), 360);
        assert!((longer.remaining(NOW) - 260.0).abs() < 1e-6, "still running");
        let (p, _) = r.pause(NOW).unwrap();
        let (p2, _) = p.add_seconds(60);
        assert!((p2.remaining(NOW) - 260.0).abs() < 1e-6);
        assert_eq!(p2.duration(), 300, "a paused timer keeps its length");
        let (d, _) = r.set_duration(600);
        assert!(d.is_idle() && d.duration() == 600);
        assert_eq!(r.set_duration(0).0.duration(), 1);
    }

    #[test]
    fn toggle_starts_then_pauses() {
        let t = Timer::default();
        let (run, _) = t.toggle(NOW).unwrap();
        let (paused, _) = run.toggle(NOW + 1000).unwrap();
        assert!(paused.is_paused());
        assert_eq!(run.ends_at(NOW), Some(NOW + 300_000));
        assert_eq!(Timer::default().ends_at(NOW), None);
    }
}
