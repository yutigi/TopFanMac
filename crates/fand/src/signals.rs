//! Signal handling, so a terminating daemon hands the fans back.
//!
//! This is the one module outside `smc` that needs `unsafe`, and it is confined
//! to installing two handlers that do nothing but set an atomic flag.
//!
//! SIGKILL cannot be caught. That is why the flag is a convenience, not the
//! safety mechanism -- the real guarantee has to come from launchd restarting
//! the daemon (KeepAlive) and from the daemon restoring auto mode on startup
//! before it does anything else.

use std::sync::atomic::{AtomicBool, Ordering};

static SHUTDOWN: AtomicBool = AtomicBool::new(false);

const SIGINT: i32 = 2;
const SIGTERM: i32 = 15;

extern "C" {
    fn signal(sig: i32, handler: extern "C" fn(i32)) -> usize;
}

extern "C" fn on_signal(_sig: i32) {
    // Async-signal-safe: a relaxed atomic store and nothing else.
    SHUTDOWN.store(true, Ordering::SeqCst);
}

/// Install handlers for SIGINT and SIGTERM.
pub fn install() {
    // SAFETY: `on_signal` is an extern "C" fn that only performs an atomic
    // store, which is async-signal-safe.
    unsafe {
        signal(SIGINT, on_signal);
        signal(SIGTERM, on_signal);
    }
}

pub fn shutdown_requested() -> bool {
    SHUTDOWN.load(Ordering::SeqCst)
}
