//! The UI side of the daemon IPC: poll the status, send one-shot requests.
//!
//! Extracted verbatim from the headless stand-in (research D8 / FR-009) so the
//! poll behaviour has exactly one home for both surfaces. Per-request timeout
//! is 250 ms -- well under the 1 s daemon tick, so a stalled daemon can never
//! wedge a poll (research D6).

use fand::proto::{Request, Response, Status, SOCKET_PATH};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::time::Duration;

/// The daemon normally replies in microseconds; this only has to catch a
/// wedged one (daemon.rs handles clients inline with its own 250 ms timeout).
pub const REQUEST_TIMEOUT: Duration = Duration::from_millis(250);

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

fn path() -> &'static str {
    SOCKET_PATH
}
