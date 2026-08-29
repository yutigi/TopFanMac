# Contract: CLI surface (`topfan`, `menubar`)

## `topfan` (unprivileged binary, invoked via sudo only for mode changes/install)

Existing commands, unchanged semantics. New subcommands added by this feature.

| Command | Run as | Effect | Exit codes |
|---|---|---|---|
| `topfan status` | any | one IPC `status`, print human-readable (mode, hottest die, per-fan RPM) | 0 ok / 2 daemon unreachable |
| `topfan full` | **root** | IPC `set_mode full`; print per-fan result incl. read-back verification | 0 ok / 1 rejected or unverified |
| `topfan off` | **root** | IPC `set_mode auto`; print confirmation | 0 ok / 1 rejected |
| `topfan managed` | **root** | IPC `set_mode managed` (foreground daemon entry uses the governor directly) | as above |
| `topfan install` *(new)* | **root** | Story 4 install sequence → [research.md D5](../research.md) | 0 installed+verified / nonzero, with the failing step named |
| `topfan uninstall` *(new)* | **root** | Story 4 uninstall sequence (auto-restore **before** bootout) | as above |

Output rules: status and mode-change results print one line per fan with the
SMC-verified state (Principle VI — the CLI prints the daemon's authoritative
reply, never its intent). `install`/`uninstall` print each step as it runs and
stop at the first failed step with a name-and-remedy message; they are
idempotent (re-running converges, see below).

Install/uninstall convergence:

```text
install:   NotInstalled|Installed(any) ──► Installed(Running), status verified ≤5 s
uninstall: any ──► fans Md=Auto (verified via status/CLI off) ──► bootout ──► files rm
```

## `menubar` (unprivileged GUI binary)

- Reads: direct IPC `status` poll on the existing 1–2 s cadence; title from
  pure `render_title` (unchanged function — its format tests remain the
  contract: e.g. `"63C  3300rpm"`, `"--"` when unknown).
- Privileged actions (Auto/Managed/Full/Off): delegated via osascript admin
  prompt → `topfan <mode>` (research.md D1); menu state ticks derive from the
  next polled `Status`, never from the click.
- Daemon down: title shows daemon-down state; bounded reconnect backoff; no crash.
- Headless/CI: no AppKit code may run outside `menubar/src/app.rs`; everything
  else (`render_title`, action→command mapping, poll) stays testable without a
  window server.

## Error presentation contract

| Condition | CLI | Menu bar |
|---|---|---|
| daemon unreachable | `error: cannot reach fand at /var/run/topfan.sock` + exit 2 | daemon-down title, backoff reconnect |
| not root for privileged cmd | daemon's `error` reply verbatim | admin prompt; if declined/failed → show fallback text ("run `sudo topfan full`") |
| write failed verification | per-fan result with `fan_control_available:false` notice | next status shows unknown/unavailable state |