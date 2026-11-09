// SPDX-License-Identifier: GPL-3.0-or-later
//! Short-lived messages shown in the closed pill (e.g. "⚡ 82%").

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PowerState {
    pub on_ac: bool,
    pub percent: Option<u8>,
}

/// A message to show once, from plugging in the charger. `prev` is the reading before
/// `now` (None for the very first reading, which never fires).
pub fn charge_event(prev: Option<PowerState>, now: PowerState) -> Option<String> {
    let prev = prev?;
    if prev.on_ac || !now.on_ac {
        return None;
    }
    Some(match now.percent {
        Some(p) => format!("⚡ {p}%"),
        None => "⚡".to_string(),
    })
}

#[derive(Debug, Default)]
pub struct Peek {
    text: String,
    until_ms: i64,
}

impl Peek {
    pub fn show(&mut self, text: String, now_ms: i64, duration_ms: i64) {
        self.text = text;
        self.until_ms = now_ms + duration_ms;
    }

    pub fn current(&self, now_ms: i64) -> Option<&str> {
        (!self.text.is_empty() && now_ms < self.until_ms).then_some(self.text.as_str())
    }

    /// When the message expires, if that is still ahead of `now_ms`. Modules return this from
    /// their wake-up schedule: a time already past would wake the window again at once, forever.
    pub fn wake_after(&self, now_ms: i64) -> Option<i64> {
        self.next_wake_ms().filter(|&t| t > now_ms)
    }

    /// When the current message expires, for scheduling a redraw.
    pub fn next_wake_ms(&self) -> Option<i64> {
        (!self.text.is_empty()).then_some(self.until_ms)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn peek_expires_after_duration() {
        let mut p = Peek::default();
        p.show("⚡ 82%".into(), 0, 3200);
        assert_eq!(p.current(3199), Some("⚡ 82%"));
        assert_eq!(p.current(3200), None);
    }

    #[test]
    fn peek_reports_expiry_as_next_wake() {
        let mut p = Peek::default();
        assert_eq!(p.next_wake_ms(), None);
        p.show("x".into(), 100, 3200);
        assert_eq!(p.next_wake_ms(), Some(3300));
    }

    #[test]
    fn an_expired_peek_asks_for_no_more_wake_ups() {
        let mut p = Peek::default();
        p.show("x".into(), 100, 3200);
        assert_eq!(p.wake_after(3000), Some(3300));
        assert_eq!(p.wake_after(3300), None);
        assert_eq!(p.wake_after(9000), None);
    }

    #[test]
    fn newer_peek_replaces_older() {
        let mut p = Peek::default();
        p.show("a".into(), 0, 1000);
        p.show("b".into(), 500, 1000);
        assert_eq!(p.current(1200), Some("b"));
    }

    #[test]
    fn charge_event_fires_on_plug_in() {
        let before = Some(PowerState { on_ac: false, percent: Some(80) });
        let now = PowerState { on_ac: true, percent: Some(82) };
        assert_eq!(charge_event(before, now), Some("⚡ 82%".to_string()));
    }

    #[test]
    fn charge_event_without_percent_shows_only_the_bolt() {
        let before = Some(PowerState { on_ac: false, percent: None });
        let now = PowerState { on_ac: true, percent: None };
        assert_eq!(charge_event(before, now), Some("⚡".to_string()));
    }

    #[test]
    fn charge_event_silent_when_already_on_ac() {
        let s = PowerState { on_ac: true, percent: Some(90) };
        assert_eq!(charge_event(Some(s), s), None);
    }

    #[test]
    fn charge_event_silent_on_unplug() {
        let before = Some(PowerState { on_ac: true, percent: Some(90) });
        let now = PowerState { on_ac: false, percent: Some(90) };
        assert_eq!(charge_event(before, now), None);
    }

    #[test]
    fn charge_event_silent_on_first_reading() {
        let now = PowerState { on_ac: true, percent: Some(90) };
        assert_eq!(charge_event(None, now), None);
    }
}
