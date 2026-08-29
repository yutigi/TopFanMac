# Contract: IPC protocol (daemon ⇄ clients)

Unchanged transport, tightened semantics. The protocol itself does not change
shape in this feature — no client/daemon version bump is required.

## Transport

- Unix domain socket at `/var/run/topfan.sock`, 0666 (reads unprivileged;
  authorisation by peer uid in the daemon — `getpeereid`).
- One JSON document per line, request then response, stream closed after reply.
- Requests: `{"cmd":"status"}` | `{"cmd":"set_mode","mode":"auto"|"managed"|"full"}`
- Responses: `{"reply":"status", …}` | `{"reply":"ok"}` | `{"reply":"error","message":"…"}`

## Semantics this feature changes or pins

### `Status.fan_control_available` (FR-003/005 — pinned)

`true` **iff** this daemon process lifetime includes ≥1 SMC write whose byte
read-back matched the value written. Any failed write or mismatched read-back
sets it `false` until a subsequent verified write. Never synthesized from
mode or RPM plausibility.

### `set_mode` results (FR-001/002/004/006)

| Situation | Response |
|---|---|
| root peer, SMC accepts, read-back verifies | `ok` |
| root peer, write or read-back fails | `error` with the SMC error; daemon sets `fan_control_available=false`; any fan left Forced by the failed sequence is restored to Auto by the next tick |
| non-root peer | `error` "SetMode requires root" — fans untouched |
| unknown mode string | `error` (serde rejects) |

`set_mode` with `mode:"full"` targets each fan's **own** `F0Mx`; `mode:"auto"`
calls `restore_all_to_auto()`. The daemon never lowers a driven target below
current actual RPM (invariant II) — except the explicit auto hand-back.

### Wake handling is invisible to clients except via `status`

After a detected wake, the daemon's next `status` shows fans in Auto (or
re-asserted Managed/Full from the tick after; last-writer-wins). No new request
or response variant is added.

## Compatibility rules

- Unknown `cmd` → `error` (never a crash, never a hang; 250 ms client timeout unchanged).
- A client that disappears mid-line is dropped at timeout; other clients unaffected.
- Clients written against the current protocol (topfan, menubar headless mode)
  require zero changes; the menubar GUI adds no new message types (it delegates
  privileged commands via the CLI, see D1 in research.md).