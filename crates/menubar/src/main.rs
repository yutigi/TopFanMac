//! Status-bar app.
//!
//! NOT YET IMPLEMENTED as a real NSStatusItem. This is a working headless
//! stand-in that polls the daemon and prints the line the menu bar will show,
//! so the client half of the app is exercised and testable before any AppKit
//! code exists.
//!
//! To finish it: add `objc2` 0.6 + `objc2-app-kit` 0.3, create an
//! `NSStatusItem` of variable length, set its button title from `render_title`
//! below on each poll, and hang an `NSMenu` off it with Auto / Full / Off items
//! that send the same `Request::SetMode` this already sends. Keep the polling
//! and formatting here -- only the presentation should need AppKit.

use fand::proto::{Request, Response, Status, SOCKET_PATH};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::time::Duration;

/// What the menu-bar item displays. Pure, so it can be tested without AppKit.
pub fn render_title(s: &Status) -> String {
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

fn poll() -> anyhow::Result<Status> {
    let stream = UnixStream::connect(SOCKET_PATH)?;
    let mut writer = stream.try_clone()?;
    let mut line = serde_json::to_string(&Request::Status)?;
    line.push('\n');
    writer.write_all(line.as_bytes())?;
    writer.flush()?;
    let mut reader = BufReader::new(stream);
    let mut response = String::new();
    reader.read_line(&mut response)?;
    match serde_json::from_str::<Response>(&response)? {
        Response::Status(s) => Ok(*s),
        other => Err(anyhow::anyhow!("unexpected reply: {other:?}")),
    }
}

fn main() {
    eprintln!("menubar: headless stand-in; polling {SOCKET_PATH}");
    loop {
        match poll() {
            Ok(s) => println!("{}", render_title(&s)),
            Err(e) => eprintln!("menubar: {e}"),
        }
        std::thread::sleep(Duration::from_secs(2));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fand::governor::Mode;

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
