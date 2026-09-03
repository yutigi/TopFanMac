# Contract: CLI delegation (app ⇄ osascript ⇄ topfan)

Implements research D4 (= 001 research D1): privileged mode changes from the
UI run the **existing** `topfan` CLI under the system admin-authorization
prompt. One authorization model (root peer-uid at the daemon); the app never
handles a password and never touches the SMC.

## Invocation

```
/usr/bin/osascript -e 'do shell script "topfan <verb>" with administrator privileges'
```

- `<verb>` ∈ {`auto`, `full`, `off`} per the mapping in
  [surfaces.md](./surfaces.md). (UI "Managed" ⇒ verb `auto`, matching the
  existing CLI's own naming.)
- The child process is spawned from a background thread; the UI main thread is
  never blocked. A safety-net timeout of 120 s kills a stuck child and maps to
  the failed-outcome below.

## topfan binary discovery

`delegate.rs` probes, in order:

1. `/usr/local/bin/topfan` (packaged location),
2. `<manifest-dir>/../../target/release/topfan` (development).

If none exists: **no prompt is raised** — the action is disabled-with-hint /
fails-with-hint ("run `sudo topfan <verb>`"). Prompting with a command that
cannot succeed would be a dishonest surface (Constitution VI).

## Outcomes (must be total — the UI cannot hang or nag on any of these)

| Outcome | Detection | UI behaviour |
|---|---|---|
| Applied | child exits 0 | nothing special — next poll's `SurfaceState` shows the new mode |
| User declined the prompt | child exits non-zero, user-cancel | state unchanged; one short hint with the CLI fallback; state item still on the old (polled) mode |
| Command failed (daemon down / SMC error) | child exits non-zero, non-cancel | same as declined, hint text = CLI fallback; poll layer independently shows `Unavailable` on the next tick |
| topfan missing | discovery fails | controls show disabled-with-hint (or failure hint if mid-action); no prompt |
| Headless/remote session (prompt can't display) | child errors promptly | same as failed — no hang, no retry loop |
| Hung child | 120 s kill timer | same as failed; child killed |

All outcomes are derived from the child's exit status only (string-typed for
tests — the real execution is a thin function around
`std::process::Command`, not run in unit tests).

## Honesty rules (FR-005, Constitution VI)

- The surfaces confirm **nothing** from the delegation outcome — not even
  exit-0. The next poll is the confirmation (last-writer-wins; a succeeded
  command immediately followed by an external change must still show truth).
- The pending affordance clears on the next injected poll regardless of the
  command's outcome.
- Declining is a normal path, not an error path: non-alarming copy, no dialogs,
  no focus stealing.

## What this contract forbids

- No `setuid` bits, no new privileged helper, no second protocol (001 D1).
- No direct socket `SetMode` from the app process.
- No password capture by the app — the prompt is system-drawn inside osascript.