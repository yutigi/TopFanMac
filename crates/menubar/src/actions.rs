//! Surface action → command mapping (research D3/D4, contracts/surfaces.md).
//!
//! Pure data: one table drives the menu, the dashboard buttons, and the
//! one-shot delegation runner. The UI invents no command (FR-003) -- verbs are
//! exactly the existing `topfan` CLI's, and "Off" and "Auto" are two labels
//! for the same hand-back intent (research D3).

use crate::state::SurfaceState;
use fand::governor::Mode;

/// One FR-002 menu row. `state_item` is which protocol `Mode` this item
/// *displays* (the checkmark attaches there); "Off" and the two non-mode
/// items are action-only.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Auto,
    Managed,
    Full,
    Off,
    OpenDashboard,
    Quit,
}

impl Action {
    pub fn label(self) -> &'static str {
        match self {
            Action::Auto => "Auto",
            Action::Managed => "Managed",
            Action::Full => "Full",
            Action::Off => "Off",
            Action::OpenDashboard => "Open Dashboard",
            Action::Quit => "Quit",
        }
    }

    /// The CLI verb this action delegates (`topfan <verb>` with administrator
    /// privileges). `None` for the purely local actions.
    pub fn verb(self) -> Option<&'static str> {
        match self {
            Action::Auto | Action::Off => Some("off"),
            Action::Managed => Some("auto"),
            Action::Full => Some("full"),
            Action::OpenDashboard | Action::Quit => None,
        }
    }

    /// Which daemon `Mode` this item reflects-checkmarks when polled.
    /// "Off" is an action phrasing of the hand-back, never checkmarked.
    pub fn state_item(self) -> Option<Mode> {
        match self {
            Action::Auto => Some(Mode::Auto),
            Action::Managed => Some(Mode::Managed),
            Action::Full => Some(Mode::Full),
            Action::Off | Action::OpenDashboard | Action::Quit => None,
        }
    }
}

/// Menu structure, top to bottom (FR-002, research D3).
pub const MENU: [Action; 6] = [
    Action::Auto,
    Action::Managed,
    Action::Full,
    Action::Off,
    Action::OpenDashboard,
    Action::Quit,
];

/// The mode buttons the dashboard's segmented control shows (same mapping as
/// the menu, contracts/surfaces.md: "one behaviour, three access points").
pub const MODE_CONTROLS: [Action; 4] = [Action::Auto, Action::Managed, Action::Full, Action::Off];

/// The state item carrying the checkmark for a polled daemon `Mode`. Exactly
/// one of Auto/Managed/Full always matches (`Mode` is exhaustive); "Off"
/// never ticks (research D3).
pub fn checked_item(mode: Mode) -> Action {
    MODE_CONTROLS
        .into_iter()
        .find(|a| a.state_item() == Some(mode))
        .expect("Mode is exhaustive; one state item per mode")
}

/// Every mode action renders enabled only in [`SurfaceState::Live`].
/// ReadOnly disables with the one-line reason, Unavailable disables with the
/// CLI fallback hint (FR-007, contracts/surfaces.md rendering table).
pub fn can_control(state: &SurfaceState) -> bool {
    matches!(state, SurfaceState::Live(_))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::PollOutcome;
    use crate::state;
    use fand::proto::Status;

    fn status(mode: Mode, available: bool) -> Status {
        Status {
            mode,
            hottest_die_c: Some(63.4),
            duty: 0.5,
            fans: vec![],
            fan_control_available: available,
        }
    }

    #[test]
    fn mode_actions_map_to_exactly_the_existing_cli_verbs() {
        // research D3 table, verbatim: Auto and Off are both the hand-back.
        assert_eq!(Action::Auto.verb(), Some("off"));
        assert_eq!(Action::Managed.verb(), Some("auto"));
        assert_eq!(Action::Full.verb(), Some("full"));
        assert_eq!(Action::Off.verb(), Some("off"));

        // The command set is exactly the four existing verbs -- no invented
        // commands (FR-003: "no second control path").
        let verbs: Vec<_> = MENU.iter().filter_map(|a| a.verb()).collect();
        assert_eq!(verbs.len(), 4, "four mode items, four commands: {verbs:?}");
        let mut sorted = verbs.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(sorted, ["auto", "full", "off"]);

        // Non-mode items map to local actions, not commands.
        assert_eq!(Action::OpenDashboard.verb(), None);
        assert_eq!(Action::Quit.verb(), None);
    }

    #[test]
    fn exactly_one_state_item_matches_each_daemon_mode() {
        for (mode, item) in [
            (Mode::Auto, Action::Auto),
            (Mode::Managed, Action::Managed),
            (Mode::Full, Action::Full),
        ] {
            let ticks: Vec<_> = MENU
                .iter()
                .filter(|a| a.state_item() == Some(mode))
                .copied()
                .collect();
            assert_eq!(ticks, [item], "for mode {mode:?}");
        }
    }

    #[test]
    fn off_is_an_action_item_and_never_ticks() {
        assert_eq!(Action::Off.state_item(), None);
        assert_eq!(
            Action::Off.verb(),
            Action::Auto.verb(),
            "same hand-back intent"
        );
    }

    #[test]
    fn all_mode_actions_disabled_when_control_unavailable() {
        for label in [Action::Auto, Action::Managed, Action::Full, Action::Off] {
            // ReadOnly: reached but write path unverified.
            let ro = state::derive(PollOutcome::Reached(status(Mode::Managed, false)));
            assert!(
                !can_control(&ro),
                "{:?} must be disabled in ReadOnly",
                label
            );
            assert!(!matches!(&ro, SurfaceState::Live(_)));
            // Unavailable: nothing controls.
            let dead = state::derive(PollOutcome::Unreachable("x".into()));
            assert!(
                !can_control(&dead),
                "{:?} must be disabled while unavailable",
                label
            );
        }
        // And the menu's own status type must agree with can_control.
        for s in [status(Mode::Auto, true), status(Mode::Auto, false)] {
            let derived = state::derive(PollOutcome::Reached(s));
            assert_eq!(
                can_control(&derived),
                matches!(derived, SurfaceState::Live(_))
            );
        }
    }

    #[test]
    fn menu_has_the_six_fr002_items_in_order() {
        let labels: Vec<_> = MENU.iter().map(|a| a.label()).collect();
        assert_eq!(
            labels,
            ["Auto", "Managed", "Full", "Off", "Open Dashboard", "Quit"]
        );
    }
}
