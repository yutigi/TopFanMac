//! Surface-state derivation (research D5, data-model.md, FR-006/FR-007).
//!
//! Pure: no IOKit, no clock, no display. This is the FR-009 seam -- every
//! surface renders from a [`SurfaceState`], never from a click.

use crate::client::PollOutcome;
use crate::render_title;
use fand::proto::Status;
use serde::Serialize;

/// What each surface renders (data-model.md).
#[derive(Debug, Clone, PartialEq)]
pub enum SurfaceState {
    /// Reached && fan_control_available: full readout, controls enabled.
    Live(Status),
    /// Reached && !fan_control_available: live values continue, controls disabled.
    ReadOnly(Status),
    /// Unreachable: distinct presentation, no numbers, no stale data.
    Unavailable,
}

/// The truth table (per tick, unconditional -- never sticky).
///
/// Derivation has no memory: two identical outcomes always yield identical
/// states; `Unavailable` replaces the last good `Status` entirely; recovery
/// happens on the first successful poll with no user action.
pub fn derive(outcome: PollOutcome) -> SurfaceState {
    match outcome {
        PollOutcome::Reached(s) => {
            if s.fan_control_available {
                SurfaceState::Live(s)
            } else {
                SurfaceState::ReadOnly(s)
            }
        }
        PollOutcome::Unreachable(_) => SurfaceState::Unavailable,
    }
}

// ---------------------------------------------------------------------------
// The serialization the dashboard web view receives (data-model.md
// DashboardBridge: one inbound `window.topfan.setStatus(json)`).
// ---------------------------------------------------------------------------

// Wired into the dashboard payload in US2 (T017/T018); the derivation is
// headless-tested here so the web layer stays presentation-only.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[allow(dead_code)]
pub struct FanSnapshot {
    pub index: u32,
    pub actual_rpm: f32,
    pub target_rpm: f32,
    /// Hardware-sourced per-fan bounds (Constitution III -- never constants).
    pub min_rpm: f32,
    pub max_rpm: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[allow(dead_code)]
pub enum SurfaceSnapshot {
    Live {
        title: String,
        mode: String,
        hottest_die_c: Option<f32>,
        duty: f32,
        fans: Vec<FanSnapshot>,
    },
    ReadOnly {
        title: String,
        mode: String,
        hottest_die_c: Option<f32>,
        duty: f32,
        fans: Vec<FanSnapshot>,
        reason: String,
    },
    Unavailable {
        /// The CLI fallback hint (contracts/surfaces.md Unavailable row).
        hint: String,
    },
}

/// One-line reason shown when the write path is unavailable (FR-007).
pub const READ_ONLY_REASON: &str =
    "fan control unavailable: the daemon's write path is not verified";

/// The CLI fallback hint shown while the daemon is unreachable.
pub const UNAVAILABLE_HINT: &str =
    "fand unreachable -- use `sudo topfan off|auto|full` from a terminal";

#[allow(dead_code)] // US2 (T018)
fn fans(s: &Status) -> Vec<FanSnapshot> {
    s.fans
        .iter()
        .map(|f| FanSnapshot {
            index: u32::from(f.index),
            actual_rpm: f.actual_rpm,
            target_rpm: f.target_rpm,
            min_rpm: f.min_rpm,
            max_rpm: f.max_rpm,
        })
        .collect()
}

/// The JSON payload handed to both-decision presentation layers. The mode
/// string is the daemon protocol's own naming (`auto|managed|full`).
#[allow(dead_code)] // US2 (T017/T018)
pub fn snapshot(state: &SurfaceState) -> SurfaceSnapshot {
    match state {
        SurfaceState::Live(s) => SurfaceSnapshot::Live {
            title: render_title(s),
            mode: mode_name(s.mode),
            hottest_die_c: s.hottest_die_c,
            duty: s.duty,
            fans: fans(s),
        },
        SurfaceState::ReadOnly(s) => SurfaceSnapshot::ReadOnly {
            title: render_title(s),
            mode: mode_name(s.mode),
            hottest_die_c: s.hottest_die_c,
            duty: s.duty,
            fans: fans(s),
            reason: READ_ONLY_REASON.into(),
        },
        SurfaceState::Unavailable => SurfaceSnapshot::Unavailable {
            hint: UNAVAILABLE_HINT.into(),
        },
    }
}

/// Wire naming for [`fand::governor::Mode`] (matches what the JS bridge sends
/// back and what `topfan`'s CLI names its verbs, research D3).
#[allow(dead_code)] // US2 (T018)
pub fn mode_name(mode: fand::governor::Mode) -> String {
    match mode {
        fand::governor::Mode::Auto => "auto".into(),
        fand::governor::Mode::Managed => "managed".into(),
        fand::governor::Mode::Full => "full".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fand::governor::Mode;
    use smc::FanMode;

    fn status(available: bool) -> Status {
        Status {
            mode: Mode::Managed,
            hottest_die_c: Some(63.4),
            duty: 0.5,
            fans: vec![smc::FanState {
                index: 0,
                actual_rpm: 3300.0,
                target_rpm: 3400.0,
                min_rpm: 2317.0,
                max_rpm: 6898.0,
                mode: FanMode::Forced,
            }],
            fan_control_available: available,
        }
    }

    #[test]
    fn reached_and_available_is_live() {
        assert!(matches!(
            derive(PollOutcome::Reached(status(true))),
            SurfaceState::Live(_)
        ));
    }

    #[test]
    fn reached_and_unavailable_is_read_only() {
        assert!(matches!(
            derive(PollOutcome::Reached(status(false))),
            SurfaceState::ReadOnly(_)
        ));
    }

    #[test]
    fn unreachable_is_unavailable() {
        assert!(matches!(
            derive(PollOutcome::Unreachable("refused".into())),
            SurfaceState::Unavailable
        ));
    }

    #[test]
    fn derivation_is_memoryless() {
        // Same outcome twice => identical state (rules on data-model.md).
        let a = derive(PollOutcome::Reached(status(true)));
        let b = derive(PollOutcome::Reached(status(true)));
        assert_eq!(a, b);
        let c = derive(PollOutcome::Unreachable("x".into()));
        let d = derive(PollOutcome::Unreachable("x".into()));
        assert_eq!(c, d);

        // ...and different kinds never collide.
        let live = derive(PollOutcome::Reached(status(true)));
        let ro = derive(PollOutcome::Reached(status(false)));
        let dead = derive(PollOutcome::Unreachable("x".into()));
        assert_ne!(live, ro);
        assert_ne!(live, dead);
    }

    #[test]
    fn unavailable_carries_no_status_data() {
        // Structural: the variant has no payload at all -- compile-checked by
        // construction; this pins that no stale numbers are smuggled in via
        // the snapshot either.
        match derive(PollOutcome::Unreachable("x".into())) {
            SurfaceState::Unavailable => {}
            other => panic!("expected Unavailable, got {other:?}"),
        }
        let json = serde_json::to_string(&snapshot(&SurfaceState::Unavailable)).unwrap();
        assert!(
            !json.contains("rpm"),
            "unavailable snapshot must not contain numbers: {json}"
        );
    }

    #[test]
    fn recovery_happens_on_first_successful_poll() {
        let dead = derive(PollOutcome::Unreachable("x".into()));
        let recovered = derive(PollOutcome::Reached(status(true)));
        assert!(matches!(recovered, SurfaceState::Live(_)), "after {dead:?}");
    }

    #[test]
    fn read_only_snapshot_keeps_live_values_and_has_a_reason() {
        let json =
            serde_json::to_string(&snapshot(&derive(PollOutcome::Reached(status(false))))).unwrap();
        assert!(json.contains("3300"), "live values must continue: {json}");
        assert!(json.contains("reason"));
    }

    #[test]
    fn live_snapshot_carries_title_mode_and_per_fan_range() {
        let snap = snapshot(&derive(PollOutcome::Reached(status(true))));
        match &snap {
            SurfaceSnapshot::Live {
                title, mode, fans, ..
            } => {
                assert_eq!(title, "63C  3300rpm");
                assert_eq!(mode, "managed");
                assert_eq!(fans[0].min_rpm, 2317.0);
                assert_eq!(fans[0].max_rpm, 6898.0);
            }
            other => panic!("expected Live, got {other:?}"),
        }
    }
}
