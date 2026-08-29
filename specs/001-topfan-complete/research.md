# Phase 0 Research: TopFanMac completion features

Resolves every NEEDS CLARIFICATION from `spec.md` and fixes the design decisions
Phases 1+ build on. Consolidated in one file; each decision records why and what
was rejected.

---

## D1 — Privilege delegation for mode changes from the menu bar

(Resolves the spec's NEEDS CLARIFICATION. FR-012 / Story 3.)

**Decision**: The menu bar sends privileged `SetMode` commands via the
AppleScript admin-prompt mechanism (`do shell script "… with administrator
privileges"`, invoked by the app with `/usr/bin/osascript`). The prompt appears
once per command; the shell command it runs is the existing
`topfan full|off|managed` CLI against the same daemon. Unprivileged reads
(title, temperature) keep using the direct socket, as today.

**Rationale**: Keeps exactly one authorisation model (root peer-uid at the
daemon, unchanged); introduces no new root-owned binary, setuid bit, or
second protocol; the auth prompt is system-drawn, so we never handle the
password. A pure-socket path from an unprivileged GUI process is rejected by the
daemon by design (FR-006), so delegation is required, not optional.

**Alternatives considered**:
- *Setuid helper binary* — works but widens the trust surface on a personal
  machine (any local process can invoke it); setuid on self-built binaries is
  increasingly awkward on macOS.
- *Root LaunchAgent proxy for menu requests* — a second root service for
  convenience only; violates "one daemon owns the hardware" simplicity.
- *Menu shows "run `sudo topfan full`"* — kept as the fallback when osascript
  fails (non-GUI session). Simplest possible, worst UX.

**User veto point**: if the password prompt per command is unacceptable, the
alternative is a root LaunchDaemon-blessed helper — return to this decision
before implementing `menubar/src/app.rs`.

---

## D2 — SMC write semantics (existing code, now to be proven)

**Decision**: Keep the implemented write path unchanged: `IOConnectCallStructMethod`
at the same `SMC_CMD_*` layer, with `data8: SMC_CMD_WRITE_BYTES = 6` writing
encoded bytes for `F0Md` (`flo`/`ui8` per `Md`'s type tag) and `F0Tg`. After every
write, read the key back and compare; only a matching read-back flips
`fan_control_available` to true. First proof run: `sudo topfan full`, observed
via `topfan status` and a `smc-probe` before/after, deliberately, once
(quickstart.md §V1). Result gets recorded in `CLAUDE.md` hardware facts —
the project convention for hardware truth.

**Rationale**: The write machinery is typed, unit-hinted, and consistent with
the verified read protocol (`READ_BYTES=5`, `READ_KEYINFO=9` — the trap-free
selectors this codebase already fixed). Changing it before it has been tried
once would be planning a guess twice. The selector trap note (every key returns
`result = 137` when selectors are swapped) is recorded as a diagnostic in
quickstart.

**Alternatives considered**: rewriting the write path against other gists'
selector tables — rejected outright: those are the sources of the trap CLAUDE.md
warns about.

---

## D3 — Wake detection without breaking the single-threaded daemon

(Refines FR-007/FR-008/FR-009 — see the spec edit note below.)

**Decision**: Detect wake by **time discontinuity** in the existing 1 s tick:
`std::time::Instant` (monotonic, does not advance during sleep) vs
`std::time::SystemTime` (wall clock, keeps counting into sleep). When wall time
advances ≥ `wake_gap_s` (default 30 s) more than monotonic time between two
ticks, the machine slept and woke. The predicate is a **pure function**
`fn wake_detected(gap_mono: Duration, gap_wall: Duration) -> bool` living in
`governor.rs`; on detection the daemon runs the documented startup sequence
(`restore_all_to_auto()`, rebuild baseline temperatures) *inline in the same
thread*, then continues ticking. Zero `unsafe`, zero new dependencies, no run
loop, no second thread.

**Rationale**: The "right" macOS way — `IORegisterForSystemPower` — requires a
CFRunLoop draining an `IONotificationPort` inside the daemon process. The daemon
is deliberately single-threaded with raw IOKit handles that must stay `!Sync`
(Principle IV); either a second thread (banned) or restructuring the control
loop to be a CFRunLoop (larger redesign for the same outcome). The clock-gap
technique is the same one used by cron/vixie-style daemons, is testable as a
pure predicate against `MockFans`, and matches the constitution's "test the
policy off-device" ethos. False positives (a manual wall-clock step of ≥30 s
forward between ticks) cause only a harmless spurious auto-restore.

**Alternatives considered**:
- *IORegisterForSystemPower + CFRunLoop* — precise event, but forces run-loop
  architecture or a helper thread; rejected this round. Revisit only if the
  clock-gap heuristic misfires in practice.
- *NSDistributedNotifications* — still needs a run loop.
- *Wake by polling fan target drift* — confuses OS-forced modes (normal under
  load, per CLAUDE.md) with wake; rejected as dishonest signal (Principle VI).

**Spec edit note**: `spec.md` FR-007 is reworded from "subscribe to sleep/wake
notifications" to "detect wake via time discontinuity each tick" — same user
outcome (FR-007's acceptance scenario is unchanged), different mechanism, so
FR-008 becomes "detection is in-tick and non-fatal". Dated so a future
amendment to event-based wake can supersede this cleanly.

---

## D4 — Menu bar AppKit pattern (objc2)

**Decision**: Per CLAUDE.md: `objc2 = 0.6.4`, `objc2-app-kit = 0.3.2` (+ the
matching `objc2-foundation`). Shape:

```text
menubar/src/main.rs   – builds App root; reuses api.rs poll/render_title as-is
menubar/src/api.rs    – (existing) poll + render_title, unchanged and still tested headlessly
menubar/src/app.rs    – the only AppKit-touching module:
    NSApplication shared app; setActivationPolicy(.accessory) → no Dock icon;
    NSStatusBar.systemStatusBar().statusItemWithLength(NSVariableStatusItemLength);
    button title ← render_title(&status) on each poll tick (dispatch/timer);
    NSMenu with Auto | Managed | Full | Off | Quit (Quit = NSApplication.terminate).
```

Polling keeps the existing 1–2 s cadence; all AppKit calls happen on the main
thread; the unprivileged socket client is unchanged. Menu actions run the
osascript delegation from D1 when root is required; the current `Status.reply`
is authoritative for what the menu ticks (Principle VI) — read-backed failure
shows as an unchecked state + logged error, never a ticked lie.

Version pinning comes from `docs.rs` availability of `objc2-app-kit` 0.3.2's
`NSStatusItem`/`NSStatusBar` feature flags (`feature = "NSStatusBar",
"NSStatusItem", "NSMenu", "NSRunningApplication"`); exact feature names are
confirmed during implementation — this is a compile-check, not a design risk,
and `cargo check -p menubar` proves it without a window server.

**Alternatives considered**: raw `cocoa` crate (unmaintained, CLAUDE.md already
chose objc2); `tauri`/`tao` tray (a GUI framework for one menu = Principle VII
nonsense); SwiftUI (would need an app bundle + Swift toolchain).

---

## D5 — Install/uninstall mechanics

**Decision**: `sudo topfan install` (and `uninstall`) — run with sudo, so no
privilege trick is needed inside the command. Steps:

```text
install:   cp target binary → /usr/local/libexec/fand  (must already exist at known path;
           refuse with a clear message if built artifact missing)
           cp packaging/com.topfan.fand.plist → /Library/LaunchDaemons/
           launchctl bootout system/com.topfan.fand 2>/dev/null (idempotency guard)
           launchctl bootstrap system /Library/LaunchDaemons/com.topfan.fand.plist
           verify: `topfan status` succeeds (≤5 s wait, else fail loudly)
uninstall: `launchctl bootout system/com.topfan.fand`
           after bootout: restore auto happens in two layers — the daemon's own
           SIGTERM path (signals.rs) and, because bootout → launchd kills → no drop
           guarantee, `sudo topfan off` is issued *before* bootout when the socket
           answers. Verify `Md=Auto` first; proceed on failed verify with a loud
           warning only.
           rm the copied files.
```

**Rationale**: Reuses the exact commands already documented in CLAUDE.md's
Commands section (they are known-good), one code path to test, no installer
framework, no Homebrew formula yet (could come later as packaging sugar).

**Alternatives considered**: a `make install` target (fine, but it duplicates a
second entry point); a root LaunchDaemon that *is* the installer (circular).

---

## Open items carried into tasks

- `fan_control_available` currently exists in `Status`; definition is tightened
  by FR-003/FR-005 (true only after ≥1 successful write-back verify) — small
  daemon diff, no protocol change.
- Exact `objc2-app-kit` feature-flag spellings (compile-time detail, D4).
- Whether the OS's own Forced mode under load should be reflected back to the
  user differently in the UI title (render_title change — pure, tested).