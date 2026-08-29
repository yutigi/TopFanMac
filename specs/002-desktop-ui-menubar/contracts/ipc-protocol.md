# Contract: IPC protocol (daemon ⇄ clients)

**Unchanged in this feature.** The authoritative contract is feature 001's
[ipc-protocol.md](../001-topfan-complete/contracts/ipc-protocol.md); this file
pins the deltas (there are none) so the 002 package is self-contained.

## Pinned for 002

- Transport: `/var/run/topfan.sock`, one JSON per line, reply then close.
- Messages used by the UI: `Status` (read, unprivileged) — and nothing else.
  The GUI **never sends `SetMode` over the socket**: the daemon rejects
  non-root peers by design (`getpeereid`), and 002 mode changes are delegated
  through the existing CLI instead — [surfaces.md](./surfaces.md),
  [cli-delegation.md](./cli-delegation.md).
- `Status.fan_control_available` keeps the 001 semantics, read-backed-write
  verification: it alone drives `SurfaceState::ReadOnly` (FR-007).
- No new request or response variants; no protocol version bump. The daemon
  binary is not rebuilt for this feature.

## Compatibility rules carried over

- Unknown `cmd` ⇒ `error`, never a crash/hang; 250 ms client timeout unchanged.
- Clients written against 001 (`topfan`, the headless `menubar`) require zero
  changes; the headless client remains runnable (`menubar --headless`) and
  keeps its formatting tests passing unchanged (FR-009, SC-006).