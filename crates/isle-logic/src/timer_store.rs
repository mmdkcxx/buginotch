// SPDX-License-Identifier: GPL-3.0-or-later
//! Saving and restoring the Pomodoro state across restarts.

use crate::pomodoro::TimerState;

pub fn to_json(state: &TimerState) -> String {
    serde_json::to_string(state).unwrap_or_default()
}

/// None for anything that is not a valid saved state.
pub fn from_json(text: &str) -> Option<TimerState> {
    serde_json::from_str(text).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pomodoro::{Phase, Pomodoro, PomodoroConfig};

    #[test]
    fn state_round_trips_through_json() {
        let mut p = Pomodoro::new(PomodoroConfig::default());
        p.start(1_000);
        let json = to_json(&p.state());
        assert_eq!(from_json(&json), Some(p.state()));
    }

    #[test]
    fn garbage_gives_none() {
        assert_eq!(from_json("not json"), None);
        assert_eq!(from_json(""), None);
    }

    #[test]
    fn timer_survives_resume() {
        // Started at t=0 for 25 min. The PC sleeps; the app only looks again at t=10 min.
        let mut p = Pomodoro::new(PomodoroConfig::default());
        p.start(0);
        assert_eq!(p.view(600_000).remaining_ms, 900_000);
        // And after a restart from the saved state at that moment:
        let restored = Pomodoro::restore(PomodoroConfig::default(), from_json(&to_json(&p.state())).unwrap());
        assert_eq!(restored.view(600_000).remaining_ms, 900_000);
        assert_eq!(restored.phase(), Phase::Focus);
    }

    #[test]
    fn finished_while_asleep_advances_on_next_tick() {
        let mut p = Pomodoro::new(PomodoroConfig::default());
        p.start(0);
        let t = p.tick(40 * 60_000).expect("focus finished while asleep");
        assert_eq!(t.finished, Phase::Focus);
    }
}
