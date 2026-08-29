# Implementation Plan: TopFanMac — finish fan control (write path, wake, menu bar, install)

**Branch**: `001-topfan-complete` | **Date**: 2026-08-30 | **Spec**: [`spec.md`](./spec.md)

**Input**: Feature specification from `/specs/001-topfan-complete/spec.md`

## Summary

The daemon core, CLI, pure governor, and verified SMC *read* path already exist
(23 tests). This plan covers the four remaining independently-shippable features
in dependency order:

1. **Proven write path** (P1) — execute and verify the never-run `Smc::write`
   against hardware, with write-back verification surfaced as
   `fan_control_available`.
2. **Wake re-assertion** (P2) — subscribe to macOS sleep/wake notifications;
   on wake, restore auto before the next control tick (pure `on_wake` step).
3. **Real menu bar** (P3) — NSStatusItem + NSMenu via `objc2` 0.6.4 /
   `objc2-app-kit` 0.3.2 on top of the existing headless client; title from the
   existing pure `render_title`, actions as existing `Request::SetMode`.
4. **One-shot install/uninstall** (P4) — package the existing plist + manual
   launchd steps into an idempotent command.

Technical approach stays inside the existing architecture: everything above
`crates/smc` stays on the `FanControl` trait; `unsafe` stays in `crates/smc`
(+ the two documented exceptions in `fand`); the daemon remains single-threaded
and the governor pure.

## Technical Context

**Language/Version**: Rust 1.93.1, edition 2021, rust-version 1.80 (workspace)

**Primary Dependencies**: std-only core; `serde`/`serde_json` (wire protocol),
`clap` 4.6.6 (CLI), `anyhow` (CLI errors). New for the menu bar only:
`objc2` 0.6.4 + `objc2-app-kit` 0.3.2 (+ `objc2-foundation`), per CLAUDE.md.
Wake notifications use IOKit power notification FFI, declared by hand in
`crates/smc/src/ffi.rs` beside the existing externs (no new heavy deps).

**Storage**: N/A. No config, no persistence. Daemon logs to
`/var/log/topfan.log` via launchd standard paths. State is the SMC itself.

**Testing**: `cargo test` (unit + governor policy against `MockFans`, no root,
no hardware); on-device verification per `quickstart.md` for the thin FFI layer.

**Target Platform**: macOS 26.x on Apple Silicon (verified host: Mac15,8,
M3 Max); root daemon `fand` under launchd, unprivileged `topfan` and `menubar`.

**Project Type**: system-service workspace (4 crates: hardware lib, daemon,
CLI, menu-bar app)

**Performance Goals**: control tick 1 s; client request turnaround ≤ 250 ms;
wake → fans restored to auto within one tick of wake (≤ 1 s).

**Constraints**: safety invariants I–IV from the constitution are hard gates:
never leave fans forced on any exit path; raise-never-lower while driving; per-
fan hardware bounds; fans only (no thermals/power); zero `unsafe` outside
`crates/smc` + 2 documented exceptions; single-threaded daemon (no `Sync`
demands on raw IOKit handles).

**Scale/Scope**: single user, single machine, 2 fans, 1 socket client protocol;
feature surface = 4 user stories in `spec.md`.

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

| Gate | Status | Notes |
|------|--------|-------|
| I. Auto always | ✅ PASS | Wake handler restores auto *before* re-ticking (FR-007); uninstall restores auto before bootout (FR-015); failed writes degrade to `fan_control_available=false` with auto restored (FR-003/005). |
| II. Raise, never lower | ✅ PASS | No new downward writes; `off` is the single deliberate hand-back. Raise-only clamp unchanged. |
| III. Hardware bounds per fan | ✅ PASS | Per-fan `F0Mn`/`F0Mx` clamping retained; spec edge case pins the differing ranges. |
| IV. Fans-only / pure governor / single thread | ✅ PASS | Wake is detected inside the existing 1 s tick via a pure clock-gap predicate (research.md D3) — no run loop, no second thread, no `Sync` demands. No thermal/power APIs. |
| V. Minimal unsafe | ✅ PASS *(post-design)* | Wake detection needs **zero** new unsafe (std `Instant`/`SystemTime` only); no IOKit power-notification FFI required. Remaining new unsafe is confined to `menubar/src/app.rs` (AppKit presentation) and the existing `crates/smc` write path. |
| VI. Honest status | ✅ PASS | FR-003/005: read-back verification, truthful `fan_control_available`. |

No violations requiring Complexity Tracking.

## Project Structure

### Documentation (this feature)

```text
specs/001-topfan-complete/
├── plan.md              # This file
├── research.md          # Phase 0 output
├── data-model.md        # Phase 1 output
├── quickstart.md        # Phase 1 output
├── contracts/
│   ├── ipc-protocol.md  # Line-JSON request/response, extended only
│   ├── cli.md           # topfan/menubar command surface
│   └── smc-write.md     # SMC write path + verification contract
└── tasks.md             # Phase 2 output (/speckit-tasks — NOT created here)
```

### Source Code (repository root)

```text
crates/
├── smc/
│   └── src/smc.rs       # write path + read-back verification (no new FFI needed)
├── fand/
│   ├── src/governor.rs  # + pure wake-gap predicate (no IOKit, no clock of its own)
│   ├── src/daemon.rs    # consumes predicate per tick; wake → restore → re-tick, inline
│   └── src/main.rs      # `managed` stays daemon entry
├── topfan/
│   └── src/main.rs      # unchanged commands; `install`/`uninstall` subcommands (Story 4)
└── menubar/
    ├── src/api.rs       # existing headless client + render_title (unchanged, tested)
    └── src/app.rs       # NEW: NSStatusItem/NSMenu presentation (AppKit unsafe lives here)
packaging/com.topfan.fand.plist   # existing, unchanged
```

**Structure Decision**: Keep the existing 4-crate workspace untouched in shape.
New capabilities map onto existing crates: wake = new module in `smc` +
serialised event in `fand`; menu bar = new `app.rs` presentation module beside
the unchanged headless client; install = new CLI subcommands in `topfan` that
shelling out to `launchctl`/`cp`/`launchctl print` (running via sudo, never
linking root helpers at runtime).

## Complexity Tracking

> No constitution violations to justify. The only watch item is Principle V:
> AppKit/power-notification unsafe must stay confined to the two UI/FFI
> modules listed above; the governor and daemon logic remain `unsafe`-free.