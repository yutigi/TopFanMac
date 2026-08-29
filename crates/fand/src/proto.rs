//! Wire protocol between the daemon and its unprivileged clients.
//!
//! Line-delimited JSON over a Unix socket. Reads are unprivileged; mode changes
//! are not, and the daemon enforces that by socket permissions rather than by
//! trusting the client.

use crate::governor::Mode;
use serde::{Deserialize, Serialize};
use smc::FanState;

/// Where the daemon listens. Root-owned, world-readable so `status` works
/// without sudo; writes are rejected for non-root peers.
pub const SOCKET_PATH: &str = "/var/run/topfan.sock";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "cmd", rename_all = "lowercase")]
pub enum Request {
    Status,
    SetMode { mode: Mode },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "reply", rename_all = "lowercase")]
pub enum Response {
    Status(Box<Status>),
    Ok,
    Error { message: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Status {
    pub mode: Mode,
    pub hottest_die_c: Option<f32>,
    pub duty: f32,
    pub fans: Vec<FanState>,
    /// False when the SMC write path is unavailable -- the daemon still reports
    /// temperatures, it just cannot act on them. See Spike 0.
    pub fan_control_available: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requests_roundtrip_as_one_line() {
        let r = Request::SetMode { mode: Mode::Full };
        let line = serde_json::to_string(&r).unwrap();
        assert!(!line.contains('\n'), "framing assumes one request per line");
        let back: Request = serde_json::from_str(&line).unwrap();
        assert!(matches!(back, Request::SetMode { mode: Mode::Full }));
    }

    #[test]
    fn status_response_roundtrips() {
        let s = Status {
            mode: Mode::Managed,
            hottest_die_c: Some(63.4),
            duty: 0.5,
            fans: vec![],
            fan_control_available: false,
        };
        let line = serde_json::to_string(&Response::Status(Box::new(s))).unwrap();
        let back: Response = serde_json::from_str(&line).unwrap();
        match back {
            Response::Status(s) => assert_eq!(s.hottest_die_c, Some(63.4)),
            other => panic!("wrong variant: {other:?}"),
        }
    }
}
