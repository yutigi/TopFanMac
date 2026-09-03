//! The UI side of the daemon IPC: poll the status, send one-shot requests.
//!
//! Extracted verbatim from the headless stand-in (research D8 / FR-009) so the
//! poll behaviour has exactly one home for both surfaces. The per-request
//! timeout is derived from the daemon's own tick period rather than picked --
//! see [`REQUEST_TIMEOUT`] for why anything shorter reports a healthy daemon
//! as unreachable (research D6).

use fand::daemon;
use fand::governor::Mode;
use fand::proto::{Request, Response, Status, SOCKET_PATH};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::time::Duration;

/// How long one request may wait for the daemon.
///
/// This is **not** a "how fast is the daemon" number -- it is bounded by the
/// daemon's *shape*. `fand`'s control loop is single-threaded: it ticks, drains
/// whatever clients arrived during that tick with non-blocking `accept`, then
/// sleeps [`daemon::TICK`]. A client that connects one instant after the drain
/// is not looked at until the sleep ends, so its worst-case wait is a whole
/// `TICK` plus the next tick's sensor work -- even though the reply itself
/// costs microseconds.
///
/// Measured on this machine (40 polls, 2026-09-02): median 198 ms, p90 235 ms,
/// max **1071 ms** -- the tail is exactly that missed-drain case. Any timeout
/// below `TICK` therefore turns a healthy daemon into `Unreachable` on the tail
/// polls, which the surfaces faithfully render as "fand unreachable" (a 500 ms
/// value flapped on 3 of 20 polls in validation). Deriving the budget from
/// `TICK` keeps the two in step if the daemon's cadence ever changes.
///
/// The margin stays under the 2 s poll cadence so a slow poll can never
/// overlap the next one.
pub const REQUEST_TIMEOUT: Duration = daemon::TICK.saturating_add(Duration::from_millis(500));

/// Raw result of one poll tick: what `state::derive` turns into a
/// [`SurfaceState`](crate::state::SurfaceState).
pub enum PollOutcome {
    /// A well-formed `Status` reply arrived within the timeout.
    Reached(Status),
    /// Connect, timeout, or decode failure. The string is the reason (logs).
    Unreachable(String),
}

pub fn request(req: &Request, timeout: Duration) -> anyhow::Result<Response> {
    // No connect_timeout on UnixStream on macOS (unstable); connect is
    // instant anyway (ECONNREFUSED), the timeouts below catch stalls.
    let stream = UnixStream::connect(path())?;
    stream.set_read_timeout(Some(timeout))?;
    stream.set_write_timeout(Some(timeout))?;
    let mut writer = stream.try_clone()?;
    let mut line = serde_json::to_string(req)?;
    line.push('\n');
    writer.write_all(line.as_bytes())?;
    writer.flush()?;
    let mut reader = BufReader::new(stream);
    let mut response = String::new();
    reader.read_line(&mut response)?;
    if response.is_empty() {
        anyhow::bail!("daemon closed the connection without a reply");
    }
    Ok(serde_json::from_str::<Response>(&response)?)
}

/// Ask the daemon for the current status, classified as a [`PollOutcome`].
pub fn poll() -> PollOutcome {
    match request(&Request::Status, REQUEST_TIMEOUT) {
        Ok(Response::Status(s)) => PollOutcome::Reached(*s),
        Ok(other) => PollOutcome::Unreachable(format!("unexpected reply: {other:?}")),
        Err(e) => PollOutcome::Unreachable(format!("{e:#}")),
    }
}

/// Result of asking the daemon to change mode over the socket directly.
pub enum SetMode {
    /// The daemon accepted and applied it. No prompt was raised at all.
    Applied,
    /// The daemon refused on authorization grounds: this process is neither
    /// root nor the console user. That is also exactly how a pre-2026-09-03
    /// root-only daemon answers, which is what the admin-prompt fallback in
    /// `ui::apply_mode` exists to handle.
    NeedsElevation,
    /// Unreachable, or refused for some other reason. The string is the
    /// reason (logs); the surfaces show the generic failure hint.
    Refused(String),
}

/// Ask the daemon to change mode as *this* user. Since 2026-09-03 the daemon
/// authorises the console user as well as root, so the menu-bar app's own
/// request is normally accepted and no authorization prompt is needed.
pub fn set_mode(mode: Mode) -> SetMode {
    match request(&Request::SetMode { mode }, REQUEST_TIMEOUT) {
        Ok(Response::Ok) => SetMode::Applied,
        Ok(Response::Error { message }) if is_authorization_refusal(&message) => {
            SetMode::NeedsElevation
        }
        Ok(Response::Error { message }) => SetMode::Refused(message),
        Ok(other) => SetMode::Refused(format!("unexpected reply: {other:?}")),
        Err(e) => SetMode::Refused(format!("{e:#}")),
    }
}

/// The daemon's authorization refusal, matched by substring on purpose: the
/// fallback must recognise both the current wording ("...requires root or the
/// console user") and the older root-only daemon's ("...requires root"), since
/// an old daemon is precisely the case the admin prompt still covers.
fn is_authorization_refusal(message: &str) -> bool {
    message.contains("requires root")
}

fn path() -> &'static str {
    SOCKET_PATH
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Pins the fallback trigger against both daemon vintages. If this stops
    /// matching, mode changes silently stop working on an old daemon: the
    /// direct request is refused and the admin prompt is never raised.
    #[test]
    fn authorization_refusals_are_recognised_from_either_daemon() {
        assert!(is_authorization_refusal(
            "changing mode requires root or the console user"
        ));
        assert!(is_authorization_refusal("changing mode requires root"));
        // Anything else is a real failure, not a reason to prompt.
        assert!(!is_authorization_refusal("bad request: expected value"));
        assert!(!is_authorization_refusal(""));
    }

    /// The bug this pins: a request budget at or below the daemon's tick period
    /// makes a *healthy* daemon look unreachable, because a client that misses
    /// one accept-drain waits the whole `sleep(TICK)` before it is served. The
    /// surfaces then honestly render a lie. Keep the budget above `TICK`.
    #[test]
    fn request_budget_outlasts_a_missed_accept_drain() {
        assert!(
            REQUEST_TIMEOUT > daemon::TICK,
            "REQUEST_TIMEOUT ({REQUEST_TIMEOUT:?}) must exceed the daemon's \
             tick period ({:?}); a client arriving just after an accept-drain \
             waits a full tick, so a shorter budget flaps Unavailable on a \
             live daemon",
            daemon::TICK,
        );
    }

    /// The other side of the squeeze: the poll cadence (2 s, ui/mod.rs) must
    /// still be able to absorb a worst-case poll, so one slow request can never
    /// overlap the next tick.
    #[test]
    fn request_budget_fits_inside_the_poll_cadence() {
        assert!(
            REQUEST_TIMEOUT < Duration::from_secs(2),
            "REQUEST_TIMEOUT ({REQUEST_TIMEOUT:?}) must stay under the 2 s poll \
             cadence so polls cannot pile up",
        );
    }
}
