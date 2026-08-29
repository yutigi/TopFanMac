//! The control curve. Pure logic -- no IOKit, no root, no hardware.
//!
//! This is deliberately the most heavily tested part of the project: fan
//! oscillation is the likeliest defect in a controller like this, and finding
//! it by melting a real M3 Max is an expensive way to find it.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Mode {
    /// Hand everything back to the SMC.
    Auto,
    /// Follow the curve, ahead of the SMC's own ramp.
    Managed,
    /// Pin to maximum.
    Full,
}

/// Temperature -> duty breakpoints, linearly interpolated between them.
///
/// The default runs the fans ahead of the SMC's curve on purpose: that is the
/// entire point of the tool. It never idles the fans below the hardware minimum
/// because duty 0.0 maps to `min_rpm`, not to a stopped fan.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Curve {
    /// Sorted by temperature, ascending.
    pub points: Vec<(f32, f32)>,
    /// How far the temperature must fall below the level that last raised the
    /// fans before they are allowed to come down. This is what stops chatter.
    pub hysteresis_c: f32,
}

impl Default for Curve {
    fn default() -> Self {
        Curve {
            points: vec![
                (45.0, 0.0),
                (60.0, 0.25),
                (70.0, 0.50),
                (80.0, 0.75),
                (90.0, 1.0),
            ],
            hysteresis_c: 4.0,
        }
    }
}

impl Curve {
    /// Duty for a temperature, clamped and interpolated.
    pub fn duty_at(&self, temp_c: f32) -> f32 {
        let pts = &self.points;
        if pts.is_empty() {
            return 0.0;
        }
        if temp_c <= pts[0].0 {
            return pts[0].1;
        }
        if temp_c >= pts[pts.len() - 1].0 {
            return pts[pts.len() - 1].1;
        }
        for w in pts.windows(2) {
            let (t0, d0) = w[0];
            let (t1, d1) = w[1];
            if temp_c >= t0 && temp_c <= t1 {
                let span = t1 - t0;
                if span <= 0.0 {
                    return d1;
                }
                return d0 + (d1 - d0) * ((temp_c - t0) / span);
            }
        }
        pts[pts.len() - 1].1
    }
}

/// Turns a temperature stream into a duty, with asymmetric response:
/// **rise immediately, fall only reluctantly.**
#[derive(Debug, Clone)]
pub struct Governor {
    pub curve: Curve,
    pub mode: Mode,
    current_duty: f32,
    /// Temperature at the moment the duty last went up. Downward moves are
    /// measured against this, not against the previous sample.
    temp_at_last_raise: f32,
}

impl Governor {
    pub fn new(curve: Curve, mode: Mode) -> Self {
        Governor {
            curve,
            mode,
            current_duty: 0.0,
            temp_at_last_raise: f32::NEG_INFINITY,
        }
    }

    pub fn duty(&self) -> f32 {
        self.current_duty
    }

    pub fn set_mode(&mut self, mode: Mode) {
        self.mode = mode;
        if mode == Mode::Full {
            self.current_duty = 1.0;
        }
    }

    /// Feed a sample, get the duty to apply.
    pub fn update(&mut self, temp_c: f32) -> f32 {
        match self.mode {
            Mode::Full => {
                self.current_duty = 1.0;
                return 1.0;
            }
            Mode::Auto => {
                // The SMC owns the fans; we still track so that switching back
                // to Managed does not jolt.
                self.current_duty = self.curve.duty_at(temp_c);
                return self.current_duty;
            }
            Mode::Managed => {}
        }

        let desired = self.curve.duty_at(temp_c);
        if desired > self.current_duty {
            // Invariant 2: raising is always allowed, immediately.
            self.current_duty = desired;
            self.temp_at_last_raise = temp_c;
        } else if desired < self.current_duty
            && temp_c <= self.temp_at_last_raise - self.curve.hysteresis_c
        {
            self.current_duty = desired;
            self.temp_at_last_raise = temp_c;
        }
        self.current_duty
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gov() -> Governor {
        Governor::new(Curve::default(), Mode::Managed)
    }

    #[test]
    fn curve_interpolates_between_breakpoints() {
        let c = Curve::default();
        assert_eq!(c.duty_at(60.0), 0.25);
        assert_eq!(c.duty_at(70.0), 0.50);
        // Midpoint of the 60->70 segment.
        assert!((c.duty_at(65.0) - 0.375).abs() < 1e-5);
    }

    #[test]
    fn curve_clamps_outside_its_range() {
        let c = Curve::default();
        assert_eq!(c.duty_at(-40.0), 0.0);
        assert_eq!(c.duty_at(150.0), 1.0);
    }

    #[test]
    fn rises_immediately() {
        let mut g = gov();
        g.update(45.0);
        let d = g.update(90.0);
        assert_eq!(d, 1.0, "a temperature spike must ramp fans at once");
    }

    #[test]
    fn does_not_fall_within_hysteresis_band() {
        let mut g = gov();
        g.update(80.0);
        let high = g.duty();
        // 2 C below the peak, inside the 4 C band: must hold.
        let held = g.update(78.0);
        assert_eq!(held, high, "fans dropped inside the hysteresis band");
    }

    #[test]
    fn falls_once_clearly_cooler() {
        let mut g = gov();
        g.update(80.0);
        let high = g.duty();
        let dropped = g.update(70.0);
        assert!(dropped < high, "fans never came down after real cooling");
    }

    /// The oscillation test. A load that flickers across a breakpoint must not
    /// make the fans audibly hunt.
    #[test]
    fn does_not_chatter_across_a_breakpoint() {
        let mut g = gov();
        let mut changes = 0;
        let mut last = g.update(69.0);
        for i in 0..200 {
            let t = if i % 2 == 0 { 71.0 } else { 69.0 };
            let d = g.update(t);
            if (d - last).abs() > 1e-6 {
                changes += 1;
            }
            last = d;
        }
        assert!(
            changes <= 1,
            "fan duty changed {changes} times while temperature flickered 2 C -- \
             hysteresis is not holding"
        );
    }

    #[test]
    fn full_mode_pins_to_max_regardless_of_temperature() {
        let mut g = Governor::new(Curve::default(), Mode::Full);
        assert_eq!(g.update(20.0), 1.0);
        assert_eq!(g.update(95.0), 1.0);
    }

    #[test]
    fn switching_to_full_takes_effect_without_a_sample() {
        let mut g = gov();
        g.update(50.0);
        g.set_mode(Mode::Full);
        assert_eq!(g.duty(), 1.0);
    }

    /// Invariant 2 as a property: over a random-ish walk the duty must never
    /// drop without a real temperature decline behind it.
    #[test]
    fn duty_never_drops_without_cooling() {
        let mut g = gov();
        let mut prev_duty = g.update(50.0);
        let mut prev_temp = 50.0f32;
        let mut t = 50.0f32;
        for i in 0..500 {
            t += ((i * 37 % 17) as f32 - 8.0) * 0.7;
            t = t.clamp(30.0, 100.0);
            let d = g.update(t);
            if d < prev_duty {
                assert!(
                    t < prev_temp,
                    "duty fell from {prev_duty} to {d} while temperature rose \
                     {prev_temp} -> {t}"
                );
            }
            prev_duty = d;
            prev_temp = t;
        }
    }
}
