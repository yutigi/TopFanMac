# TopFanMac Constitution

## Core Principles

### I. Auto is the fallback, always
Every exit path — normal exit, signal, crash, SIGKILL, sleep/wake, uninstall —
must leave fans in SMC-managed auto mode (`F0Md = 0`). `Drop` does not run on
`SIGKILL`, so the guarantee is structural: `KeepAlive` in the launchd plist PLUS
`restore_all_to_auto()` on *startup* before any other action. Signal handlers
are convenience, never the guarantee.

### II. Only raise, never lower
This tool cools harder than macOS would, never less. Any driven target must be
clamped to at least the fan's current actual RPM. No feature may introduce a
downward write except an explicit, deliberate hand-back to auto (`off`).

### III. Bounds come from the hardware
Fan minimum/maximum RPM are read per fan from `F0Mn`/`F0Mx`. The two fans on
this machine have different ranges; hard-coding bounds or sharing one fan's
bounds with the other is a defect.

### IV. Fans only; a pure governor; single-threaded daemon
- Never touch thermal throttling, power limits, or anything beyond fan keys.
- All control policy lives in the pure `fand::governor` (no IOKit, no root, no
  clock) and is tested off-device against `MockFans`. New policy goes in the
  governor, never in `daemon.rs`.
- The daemon is single-threaded on purpose: clients handled inline with a
  250 ms timeout, far under the 1 s tick. Do not introduce threads/tasks that
  force `Sync` onto raw IOKit handles.

### V. Minimal unsafe, verified at the boundary
`unsafe` is confined to `crates/smc` except two documented exceptions in
`fand` (`signals.rs` handlers, `daemon.rs::peer_is_root` via `getpeereid`).
Hardware behaviour that cannot be tested here is proven by deliberate,
observed, one-at-a-time runs (`smc-probe`, first `sudo topfan full`) — never
incidentally.

### VI. Honest status, no silent failure
`status` reflects hardware truth as-is (including the OS's own forced mode
under load). Any failure to control fans is surfaced (`fan_control_available`,
error replies, logs), never swallowed or pretended away.

## Additional Constraints

- Authorisation is by peer uid (`getpeereid`), enforced by the daemon, not by
  socket file permissions. Socket is 0666 so reads stay unprivileged.
- SMC selectors and encodings follow the verified facts in `CLAUDE.md` — reads
  via `IOConnectCallStructMethod(conn, 2, …)`, values `flt ` LE float or `fpe2`
  depending on the type tag that arrives with each read. `Md=1` on startup
  usually means macOS is doing its job, not that a daemon crashed.
- No persistence layer, no config files, no thermal/power APIs unless a
  ratified amendment says otherwise.

## Development Workflow

- `cargo test` (all 23+ tests, no root, no hardware) and
  `cargo clippy --all-targets -- -D warnings` must pass before any commit is
  considered done; on-device runs are reserved for the thin FFI layer.
- Hardware-verification runs are done deliberately: one command, one observed
  outcome, result recorded in `CLAUDE.md` hardware facts.
- Every feature spec must state which principle it touches and how its exit
  paths preserve principle I.

**Version**: 1.0.0 | **Ratified**: 2026-08-30 | **Last Amended**: 2026-08-30