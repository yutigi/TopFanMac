//! The privileged half of TopFanMac.
//!
//! `governor` is pure logic and fully testable off-device; `daemon` is the thin
//! shell that wires it to hardware, a socket, and a signal handler.

pub mod daemon;
pub mod governor;
pub mod proto;
pub mod signals;
