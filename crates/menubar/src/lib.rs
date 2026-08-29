//! Status-bar app logic library.
//!
//! Since the workspace root needs a default-run binary (so plain `cargo run`
//! launches the GUI), everything formerly in `main.rs` lives here and the
//! binary in `main.rs` is a thin shim. GUI by default (see [`ui`]);
//! `--headless` keeps the original poll-and-print loop for scripting. The
//! non-presentation halves are pure and headless tested: `client.rs` for IPC,
//! `render_title` (moved here from main.rs; D8's constraint was "don't churn
//! the tested logic when adding the GUI" -- the relocation is that churn,
//! performed for the cargo-run story), `state.rs` for surface-state
//! derivation, `actions.rs` for the action -> command mapping, `delegate.rs`
//! for the privilege-delegation contract.

mod actions;
mod client;
mod delegate;
mod state;
mod ui;

use std::time::Duration;

/// What the menu-bar item displays. Pure, so it can be tested without AppKit.
pub fn render_title(s: &fand::proto::Status) -> String {
    let temp = match s.hottest_die_c {
        Some(c) => format!("{c:.0}C"),
        None => "--".into(),
    };
    let rpm = s
        .fans
        .iter()
        .map(|f| f.actual_rpm)
        .fold(f32::NEG_INFINITY, f32::max);
    if rpm.is_finite() {
        format!("{temp}  {rpm:.0}rpm")
    } else {
        temp
    }
}

/// Entry point shared by the `menubar` binary and the workspace-root
/// `cargo run` shim. GUI by default; `--headless` keeps the original
/// poll-and-print loop for scripting (T010).
pub fn run() {
    if std::env::args().any(|arg| arg == "--headless") {
        headless();
    } else {
        ui::run();
    }
}

/// The original behaviour of this crate, kept for scripting.
fn headless() {
    eprintln!(
        "menubar: headless mode; polling {}",
        fand::proto::SOCKET_PATH
    );
    loop {
        match client::poll() {
            client::PollOutcome::Reached(s) => println!("{}", render_title(&s)),
            client::PollOutcome::Unreachable(e) => eprintln!("menubar: {e}"),
        }
        std::thread::sleep(Duration::from_secs(2));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fand::governor::Mode;
    use fand::proto::Status;

    fn status(temp: Option<f32>, fans: Vec<smc::FanState>) -> Status {
        Status {
            mode: Mode::Managed,
            hottest_die_c: temp,
            duty: 0.5,
            fans,
            fan_control_available: true,
        }
    }

    #[test]
    fn title_degrades_when_sensors_are_missing() {
        assert_eq!(render_title(&status(None, vec![])), "--");
    }

    #[test]
    fn title_shows_the_loudest_fan() {
        let mk = |index, rpm| smc::FanState {
            index,
            actual_rpm: rpm,
            target_rpm: rpm,
            min_rpm: 1200.0,
            max_rpm: 5400.0,
            mode: smc::FanMode::Forced,
        };
        let s = status(Some(63.4), vec![mk(0, 2100.0), mk(1, 3300.0)]);
        assert_eq!(render_title(&s), "63C  3300rpm");
    }
}
