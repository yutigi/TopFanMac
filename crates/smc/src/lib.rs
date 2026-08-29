//! Hardware access for TopFanMac.
//!
//! Two independent paths, with very different confidence levels:
//!
//! - [`hid`] -- thermal sensors. Verified working unprivileged on M3 Max.
//! - [`smc`] -- fan read/write. Unproven on Apple Silicon; see CLAUDE.md Spike 0.
//!
//! Everything above this crate talks to [`FanControl`], never to IOKit, so the
//! governor can be tested against [`MockFans`] with no hardware and no root.

pub mod error;
pub mod ffi;
pub mod hid;
pub mod smc;

pub use error::{Error, Result};
pub use hid::{Reading, Thermals};
pub use smc::{Key, Smc, Value};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FanMode {
    /// SMC runs its own curve. The safe state, and the one we must always
    /// return to on exit.
    Auto,
    /// We drive the target RPM.
    Forced,
}

impl FanMode {
    pub fn as_f32(self) -> f32 {
        match self {
            FanMode::Auto => 0.0,
            FanMode::Forced => 1.0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct FanState {
    pub index: u8,
    pub actual_rpm: f32,
    pub target_rpm: f32,
    pub min_rpm: f32,
    pub max_rpm: f32,
    pub mode: FanMode,
}

impl FanState {
    /// Where this fan sits between its own min and max, 0.0..=1.0.
    pub fn duty(&self) -> f32 {
        let span = self.max_rpm - self.min_rpm;
        if span <= 0.0 {
            return 0.0;
        }
        ((self.actual_rpm - self.min_rpm) / span).clamp(0.0, 1.0)
    }

    /// Convert a 0.0..=1.0 duty into an RPM within this fan's reported bounds.
    /// Bounds come from the hardware, never from constants -- they differ per
    /// model and per fan.
    pub fn rpm_for_duty(&self, duty: f32) -> f32 {
        let d = duty.clamp(0.0, 1.0);
        self.min_rpm + (self.max_rpm - self.min_rpm) * d
    }
}

/// The hardware interface the daemon codes against.
pub trait FanControl {
    fn fan_count(&self) -> Result<u8>;
    fn fan(&self, index: u8) -> Result<FanState>;
    fn set_mode(&self, index: u8, mode: FanMode) -> Result<()>;
    fn set_target_rpm(&self, index: u8, rpm: f32) -> Result<()>;

    fn fans(&self) -> Result<Vec<FanState>> {
        (0..self.fan_count()?).map(|i| self.fan(i)).collect()
    }

    /// Hand every fan back to the SMC. The safe state.
    fn restore_all_to_auto(&self) -> Result<()> {
        for i in 0..self.fan_count()? {
            self.set_mode(i, FanMode::Auto)?;
        }
        Ok(())
    }
}

impl FanControl for Smc {
    fn fan_count(&self) -> Result<u8> {
        Smc::fan_count(self)
    }

    fn fan(&self, index: u8) -> Result<FanState> {
        let num = |suffix: &[u8; 2]| -> Result<f32> {
            self.read(Key::fan(index, suffix))?
                .as_f32()
                .ok_or(Error::UnexpectedType { key: "fan" })
        };
        Ok(FanState {
            index,
            actual_rpm: num(b"Ac")?,
            target_rpm: num(b"Tg").unwrap_or(0.0),
            min_rpm: num(b"Mn").unwrap_or(0.0),
            max_rpm: num(b"Mx").unwrap_or(0.0),
            mode: if num(b"Md").unwrap_or(0.0) >= 0.5 {
                FanMode::Forced
            } else {
                FanMode::Auto
            },
        })
    }

    fn set_mode(&self, index: u8, mode: FanMode) -> Result<()> {
        self.write(Key::fan(index, b"Md"), mode.as_f32())
    }

    fn set_target_rpm(&self, index: u8, rpm: f32) -> Result<()> {
        let fan = FanControl::fan(self, index)?;
        // Invariant 3: clamp to what the hardware reports, never to constants.
        let clamped = rpm.clamp(fan.min_rpm, fan.max_rpm);
        self.write(Key::fan(index, b"Tg"), clamped)
    }
}

/// In-memory fans for testing the governor with no hardware and no root.
#[derive(Debug, Clone)]
pub struct MockFans {
    pub fans: std::cell::RefCell<Vec<FanState>>,
}

impl MockFans {
    /// Two fans with plausible MacBook Pro bounds.
    pub fn two() -> Self {
        let mk = |index| FanState {
            index,
            actual_rpm: 1200.0,
            target_rpm: 1200.0,
            min_rpm: 1200.0,
            max_rpm: 5400.0,
            mode: FanMode::Auto,
        };
        MockFans {
            fans: std::cell::RefCell::new(vec![mk(0), mk(1)]),
        }
    }
}

impl FanControl for MockFans {
    fn fan_count(&self) -> Result<u8> {
        Ok(self.fans.borrow().len() as u8)
    }
    fn fan(&self, index: u8) -> Result<FanState> {
        self.fans
            .borrow()
            .get(index as usize)
            .copied()
            .ok_or(Error::NoSuchFan(index))
    }
    fn set_mode(&self, index: u8, mode: FanMode) -> Result<()> {
        let mut f = self.fans.borrow_mut();
        f.get_mut(index as usize)
            .ok_or(Error::NoSuchFan(index))?
            .mode = mode;
        Ok(())
    }
    fn set_target_rpm(&self, index: u8, rpm: f32) -> Result<()> {
        let mut f = self.fans.borrow_mut();
        let fan = f.get_mut(index as usize).ok_or(Error::NoSuchFan(index))?;
        let clamped = rpm.clamp(fan.min_rpm, fan.max_rpm);
        fan.target_rpm = clamped;
        fan.actual_rpm = clamped;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duty_maps_to_rpm_within_hardware_bounds() {
        let f = MockFans::two().fan(0).unwrap();
        assert_eq!(f.rpm_for_duty(0.0), 1200.0);
        assert_eq!(f.rpm_for_duty(1.0), 5400.0);
        assert_eq!(f.rpm_for_duty(0.5), 3300.0);
        // Out-of-range duty must not escape the bounds.
        assert_eq!(f.rpm_for_duty(-1.0), 1200.0);
        assert_eq!(f.rpm_for_duty(2.0), 5400.0);
    }

    #[test]
    fn set_target_clamps_to_reported_max() {
        let m = MockFans::two();
        m.set_target_rpm(0, 99_000.0).unwrap();
        assert_eq!(m.fan(0).unwrap().target_rpm, 5400.0);
    }

    #[test]
    fn restore_all_to_auto_covers_every_fan() {
        let m = MockFans::two();
        m.set_mode(0, FanMode::Forced).unwrap();
        m.set_mode(1, FanMode::Forced).unwrap();
        m.restore_all_to_auto().unwrap();
        assert!(m.fans().unwrap().iter().all(|f| f.mode == FanMode::Auto));
    }
}
