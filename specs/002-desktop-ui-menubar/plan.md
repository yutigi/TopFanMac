# Implementation Plan: Desktop fan-control UI with menu-bar presence

**Branch**: `002-desktop-ui-menubar` (to be cut from `001-topfan-complete`) | **Date**: 2026-08-30 | **Spec**: [spec.md](./spec.md)

**Input**: Feature specification from `/specs/002-desktop-ui-menubar/spec.md`

## Summary

Turn the headless `menubar` stand-in into the real desktop app: a native
`NSStatusItem` (title rendered by the existing tested `render_title`, menu with
Auto / Managed / Full / Off + Open Dashboard + Quit) and a dashboard window that
mirrors the approved mockup (`ui-design.html`). All non-presentation logic stays
in pure, headless-tested Rust modules (`state`, `actions`, `title`); presentation
is objc2/objc2-app-kit for the status item and an `NSWindow` hosting a
`WKWebView` (objc2-web-kit) for the dashboard, with all state injected from the
Rust poll loop. Mode changes flow through the already-decided privilege model
(research.md D1 of feature 001): osascript admin-authorization prompt running
the existing `topfan` CLI against the same daemon socket. No daemon, protocol,
or governor changes.

## Technical Context

**Language/Version**: Rust 1.93.1 (workspace edition 2021, `rust-version` 1.80)

**Primary Dependencies**: `fand` (proto, governor — read model only), `smc`
(`FanState`/`FanMode` types only), `serde_json`, `anyhow`; NEW for the GUI:
`objc2` 0.6.4, `objc2-app-kit` 0.3.2, `objc2-web-kit` 0.3.2 (feature
`WKWebView` + `objc2-app-kit`), `objc2-foundation` 0.3.x (as pulled by app-kit).
No new third-party UI framework (tauri/SwiftUI rejected — see research.md D1).

**Storage**: N/A. No persistence this round (FR-010) — no settings, no files.
The dashboard's sparkline history is transient, held per-open.

**Testing**: `cargo test` (headless, no root, no hardware; the existing 23
tests plus new pure-module tests). `cargo clippy --all-targets -- -D warnings`
must stay clean. Presentation (AppKit/web layer) is deliberately untested.

**Target Platform**: this MacBook — macOS 26.5.1 (Darwin 25.5.0), Apple M3 Max
(`Mac15,8`), 2-fan SMC. Unprivileged user process (the daemon keeps root).

**Project Type**: Rust workspace desktop app (new GUI binary surface in the
existing `menubar` crate; CLI headless mode retained).

**Performance Goals**: poll cadence 2 s (1–2 s per FR-005, 250 ms client
timeout unchanged); status item live within 2 s of launch (SC-001); dashboard
opens < 1 s with fresh values (SC-003).

**Constraints**: no daemon/protocol changes; no new `unsafe` outside
`crates/smc` (see Constitution Check); all state shown comes from the daemon's
authoritative replies, never local optimistic state (FR-005); bounded retries
when the daemon is down (FR-006).

**Scale/Scope**: one app instance, two surfaces, ~4 new pure modules + 2
presentation modules + 1 embedded web resource. Small, single-user utility.

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

| Principle | Assessment | Status |
|---|---|---|
| I. Auto is the fallback, always | The app owns no fan state. Quitting ends the UI only; the daemon lifecycle is untouched (FR-010). The only fan mutation the UI can cause is a delegated `topfan` command, and `Off` maps to the explicit, sanctioned hand-back to auto. No new exit path touches the SMC. | ✅ Pass |
| II. Only raise, never lower | The UI has **no direct write path to the SMC** — it cannot lower anything. All writes go through the daemon's existing `SetMode` enforcement (root peer + read-back verification). "Off"/"Auto" is the explicitly allowed downward hand-back. | ✅ Pass |
| III. Bounds come from the hardware | The dashboard prints each fan against `status.fans[i].min_rpm … max_rpm` as reported per fan; the gauge mapping normalizes within each fan's own range. No constants. | ✅ Pass |
| IV. Fans only; pure governor; single-threaded daemon | No governor or `daemon.rs` changes at all. The **daemon** stays single-threaded; the new app is a separate client process and may use its own run-loop/timer — the constitution's single-thread rule is scoped to `fand`, not to clients. | ✅ Pass |
| V. Minimal unsafe, verified at the boundary | All hardware access stays in `crates/smc` (untouched). objc2 crates are safe wrappers; any `MainThreadMarker` acquisition is centralized and documented in `crates/menubar` — target is **zero new `unsafe`**, and if one is unavoidable it is a third documented exception. Hardware behaviour verified on-device is unchanged by this feature. | ✅ Pass (target: zero new unsafe) |
| VI. Honest status, no silent failure | `SurfaceState` (research D5) is a pure derivation: Live / Unavailable / Read-only rendering is driven only by polled outcomes; the checkmark and mode buttons confirm from daemon replies, never from clicks (spec: "the menu always shows the daemon's authoritative state"). | ✅ Pass |
| Workflow (test + clippy before done) | New pure modules (`state.rs`, `actions.rs`, plus existing `title` tests kept passing unchanged per FR-009) are headless-testable; CI-equivalent is `cargo test && cargo clippy --all-targets -- -D warnings`. | ✅ Pass |

No violations. Complexity Tracking section is intentionally empty.

## Project Structure

### Documentation (this feature)

```text
specs/002-desktop-ui-menubar/
├── plan.md              # This file
├── research.md          # Phase 0 output — decisions D1–D7
├── data-model.md        # Phase 1 output — read model + SurfaceState derivation
├── quickstart.md        # Phase 1 output — build/launch/verify guide (SC-001…SC-006)
├── contracts/           # Phase 1 output
│   ├── surfaces.md      # SurfaceState contract + action mapping (UI ⇄ commands)
│   ├── ipc-protocol.md  # Unchanged — reference to 001, pinned no-new-messages
│   └── cli-delegation.md# osascript ⇄ topfan privilege contract (implements D1)
├── checklists/
└── ui-design.html       # Approved visual treatment (input to the dashboard view)
```

(`tasks.md` is Phase 2 output of `/speckit-tasks`, not created here.)

### Source Code (repository root)

```text
crates/menubar/
├── Cargo.toml           # + objc2, objc2-foundation, objc2-app-kit, objc2-web-kit
└── src/
    ├── main.rs          # entry: GUI by default; `--headless` keeps the existing
    │                    #   poll-and-print loop. render_title + its tests STAY HERE.
    ├── client.rs        # extracted poll()/one-shot request helper (behaviour unchanged)
    ├── state.rs         # SurfaceState derivation (pure): poll outcome → Live | Unavailable | ReadOnly
    ├── actions.rs       # surface actions → delegation commands (pure mapping, incl. menu items)
    ├── delegate.rs      # osascript admin-authorization runner, topfan path lookup,
    │                    #   CLI fallback hint construction
    └── ui/
        ├── mod.rs       # NSApplication assembly (accessory policy), status item,
        │                #   2 s poll timer, single-instance lock socket
        ├── menu.rs      # NSMenu from actions.rs items; checkmark reflects polled mode
        └── dashboard.rs # NSWindow + WKWebView; JS bridge (WKScriptMessageHandler) → actions

crates/menubar/assets/   # embedded via include_str!
└── dashboard.html       # dashboard surface adapted from ui-design.html (no demo controls)
```

**Structure Decision**: extend the existing `menubar` crate (the headless
client already there becomes the default `--headless` mode; the GUI is added
around it). No new crates, no daemon changes — feature 001's
`contracts/ipc-protocol.md` compatibility rule ("the menubar GUI adds no new
message types; it delegates privileged commands via the CLI") already
prescribed exactly this shape.

## Complexity Tracking

> No constitution violations — table intentionally empty.

| Violation | Why Needed | Simpler Alternative Rejected Because |
|-----------|------------|-------------------------------------|
| — | — | — |