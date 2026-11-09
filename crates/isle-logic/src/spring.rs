// SPDX-License-Identifier: GPL-3.0-or-later
//! Damped spring used for the notch's open/close animation.

const MAX_SUBSTEP: f32 = 1.0 / 240.0;

/// A damped spring chasing `target`.
#[derive(Debug, Clone, Copy)]
pub struct Spring {
    pub value: f32,
    pub velocity: f32,
    pub target: f32,
    stiffness: f32,
    damping: f32,
}

impl Spring {
    pub fn new(value: f32, stiffness: f32, damping: f32) -> Self {
        Self { value, velocity: 0.0, target: value, stiffness, damping }
    }

    /// Advances by `dt` seconds (semi-implicit Euler, substeps of at most 1/240 s).
    pub fn step(&mut self, dt: f32) {
        let dt = dt.clamp(0.0, 0.25);
        let n = (dt / MAX_SUBSTEP).ceil().max(1.0);
        let h = dt / n;
        for _ in 0..n as u32 {
            let accel = self.stiffness * (self.target - self.value) - self.damping * self.velocity;
            self.velocity += accel * h;
            self.value += self.velocity * h;
        }
    }

    pub fn is_settled(&self) -> bool {
        (self.value - self.target).abs() < 0.05 && self.velocity.abs() < 0.05
    }

    pub fn snap(&mut self) {
        self.value = self.target;
        self.velocity = 0.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(mut s: Spring, frames: usize) -> (Spring, f32) {
        let mut max = f32::MIN;
        for _ in 0..frames {
            s.step(1.0 / 60.0);
            max = max.max(s.value);
        }
        (s, max)
    }

    #[test]
    fn converges_to_target() {
        let mut s = Spring::new(0.0, 300.0, 26.0);
        s.target = 100.0;
        let (s, _) = run(s, 120);
        assert!(s.is_settled());
        assert!((s.value - 100.0).abs() < 0.05);
    }

    #[test]
    fn overshoots_when_underdamped() {
        let mut s = Spring::new(0.0, 300.0, 8.0);
        s.target = 100.0;
        let (_, max) = run(s, 120);
        assert!(max > 100.0);
    }

    #[test]
    fn does_not_overshoot_when_critically_damped() {
        let mut s = Spring::new(0.0, 300.0, 2.0 * 300f32.sqrt());
        s.target = 100.0;
        let (_, max) = run(s, 240);
        assert!(max <= 100.01);
    }

    #[test]
    fn large_dt_stays_finite() {
        let mut s = Spring::new(0.0, 300.0, 22.0);
        s.target = 100.0;
        s.step(0.5);
        assert!(s.value.is_finite() && s.velocity.is_finite());
    }
}
