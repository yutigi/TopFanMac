//! The control loop and IPC server.

use crate::governor::{Curve, Governor, Mode};
use crate::proto::{Request, Response, Status, SOCKET_PATH};
use crate::signals;
use smc::{FanControl, FanMode, Smc, Thermals};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::sync::Mutex;
use std::time::Duration;

/// How often to sample. Fast enough to catch a load spike before the machine
/// heat-soaks, slow enough not to spin a core.
pub const TICK: Duration = Duration::from_millis(1000);

/// A client gets this long to say what it wants before the loop moves on.
/// Must stay well under TICK so a hung client cannot delay a fan response.
pub const CLIENT_TIMEOUT: Duration = Duration::from_millis(250);

pub struct Daemon {
    governor: Mutex<Governor>,
    fans: Option<Smc>,
    thermals: Option<Thermals>,
}

impl Daemon {
    pub fn new(mode: Mode) -> Self {
        // Neither path is fatal if missing: without thermals we cannot run the
        // curve, without the SMC we cannot act -- but `status` should still
        // work and say so, rather than the daemon refusing to start.
        let fans = match Smc::open() {
            Ok(s) => Some(s),
            Err(e) => {
                eprintln!("fand: SMC unavailable ({e}); running read-only");
                None
            }
        };
        let thermals = Thermals::open();
        if thermals.is_none() {
            eprintln!("fand: thermal sensors unavailable; curve cannot run");
        }
        Daemon {
            governor: Mutex::new(Governor::new(Curve::default(), mode)),
            fans,
            thermals,
        }
    }

    /// Whether we can actually drive the fans, as opposed to only observing.
    pub fn fan_control_available(&self) -> bool {
        self.fans
            .as_ref()
            .map(|s| FanControl::fan_count(s).is_ok())
            .unwrap_or(false)
    }

    fn apply(&self, duty: f32, mode: Mode) {
        let Some(smc) = self.fans.as_ref() else {
            return;
        };
        let Ok(count) = FanControl::fan_count(smc) else {
            return;
        };
        for i in 0..count {
            let Ok(fan) = FanControl::fan(smc, i) else {
                continue;
            };
            match mode {
                Mode::Auto => {
                    let _ = smc.set_mode(i, FanMode::Auto);
                }
                Mode::Managed | Mode::Full => {
                    let target = fan.rpm_for_duty(duty);
                    // Invariant 2: never ask for less than the fan is already
                    // doing under the SMC's own control.
                    let target = target.max(fan.actual_rpm.min(fan.max_rpm));
                    let _ = smc.set_mode(i, FanMode::Forced);
                    let _ = smc.set_target_rpm(i, target);
                }
            }
        }
    }

    pub fn status(&self) -> Status {
        let g = self.governor.lock().expect("governor lock");
        Status {
            mode: g.mode,
            hottest_die_c: self.thermals.as_ref().and_then(|t| t.hottest_die()),
            duty: g.duty(),
            fans: self
                .fans
                .as_ref()
                .and_then(|s| FanControl::fans(s).ok())
                .unwrap_or_default(),
            fan_control_available: self.fan_control_available(),
        }
    }

    pub fn set_mode(&self, mode: Mode) {
        let mut g = self.governor.lock().expect("governor lock");
        g.set_mode(mode);
        let duty = g.duty();
        drop(g);
        self.apply(duty, mode);
    }

    /// One iteration of the control loop.
    pub fn tick(&self) {
        let Some(t) = self.thermals.as_ref() else {
            return;
        };
        let Some(temp) = t.hottest_die() else {
            return;
        };
        let mut g = self.governor.lock().expect("governor lock");
        let duty = g.update(temp);
        let mode = g.mode;
        drop(g);
        self.apply(duty, mode);
    }

    /// Hand the hardware back. Called on every exit path we can intercept.
    pub fn restore(&self) {
        if let Some(smc) = self.fans.as_ref() {
            if let Err(e) = smc.restore_all_to_auto() {
                eprintln!("fand: FAILED to restore auto mode: {e}");
            } else {
                eprintln!("fand: fans returned to auto");
            }
        }
    }
}

/// Run until signalled. Always restores before returning.
pub fn run(mode: Mode) -> anyhow::Result<()> {
    signals::install();
    let daemon = Daemon::new(mode);

    // Invariant 1: start from a known state. If a previous instance was killed
    // with SIGKILL and left fans forced, this is what undoes it.
    daemon.restore();

    let _ = std::fs::remove_file(SOCKET_PATH);
    let listener = UnixListener::bind(SOCKET_PATH)?;
    listener.set_nonblocking(true)?;
    set_socket_permissions(SOCKET_PATH);

    daemon.set_mode(mode);
    eprintln!("fand: listening on {SOCKET_PATH}, mode {mode:?}");

    while !signals::shutdown_requested() {
        daemon.tick();
        // Drain any clients that arrived during this tick.
        loop {
            match listener.accept() {
                Ok((stream, _)) => handle_client(stream, &daemon),
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(e) => {
                    eprintln!("fand: accept failed: {e}");
                    break;
                }
            }
        }
        std::thread::sleep(TICK);
    }

    eprintln!("fand: shutting down");
    daemon.restore();
    let _ = std::fs::remove_file(SOCKET_PATH);
    Ok(())
}

/// Handled inline on the control-loop thread: the daemon is deliberately
/// single-threaded, so the hardware handles never need to be `Sync` and the
/// governor never contends. The read timeout is what keeps a stalled client
/// from stalling fan control.
fn handle_client(stream: UnixStream, daemon: &Daemon) {
    let _ = stream.set_read_timeout(Some(CLIENT_TIMEOUT));
    let _ = stream.set_write_timeout(Some(CLIENT_TIMEOUT));
    let peer_is_root = peer_is_root(&stream);
    let mut writer = match stream.try_clone() {
        Ok(w) => w,
        Err(_) => return,
    };
    let reader = BufReader::new(stream);
    for line in reader.lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        let response = match serde_json::from_str::<Request>(&line) {
            Ok(Request::Status) => Response::Status(Box::new(daemon.status())),
            Ok(Request::SetMode { mode }) => {
                if peer_is_root {
                    daemon.set_mode(mode);
                    Response::Ok
                } else {
                    Response::Error {
                        message: "changing mode requires root".into(),
                    }
                }
            }
            Err(e) => Response::Error {
                message: format!("bad request: {e}"),
            },
        };
        let Ok(mut json) = serde_json::to_string(&response) else {
            break;
        };
        json.push('\n');
        if writer.write_all(json.as_bytes()).is_err() {
            break;
        }
    }
}

/// Authorise by peer credentials, not by anything the client tells us.
fn peer_is_root(stream: &UnixStream) -> bool {
    use std::os::unix::io::AsRawFd;
    let mut uid: u32 = u32::MAX;
    let mut gid: u32 = u32::MAX;
    // SAFETY: getpeereid writes two u32s through the pointers we supply for a
    // valid socket fd. Confined here rather than in a shared module because it
    // is the only privileged decision the daemon makes.
    let rc = unsafe { getpeereid(stream.as_raw_fd(), &mut uid, &mut gid) };
    rc == 0 && uid == 0
}

fn set_socket_permissions(path: &str) {
    use std::os::unix::fs::PermissionsExt;
    // 0666: any user may connect and ask for status. Authorisation for writes
    // is by peer uid, above -- the permission bits are not the security border.
    if let Ok(meta) = std::fs::metadata(path) {
        let mut perms = meta.permissions();
        perms.set_mode(0o666);
        let _ = std::fs::set_permissions(path, perms);
    }
}

extern "C" {
    fn getpeereid(fd: i32, uid: *mut u32, gid: *mut u32) -> i32;
}
