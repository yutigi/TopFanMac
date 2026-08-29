# Quickstart: 002-desktop-ui-menubar

How to build, test, and validate the desktop UI end to end. References
[surfaces.md](./contracts/surfaces.md) for rendering rules and
[data-model.md](./data-model.md) for the derivation.

## Prerequisites

- Rust 1.93 (workspace default), macOS on this MacBook, no special privileges
  for build/test.
- For live validation: the daemon running (root). The UI itself always runs
  unprivileged.
- One-time packaging for delegation (the UI looks up the CLI):
  ```sh
  sudo cp target/release/topfan /usr/local/bin/
  ```

## Build & automated checks (no display, no root, no daemon)

```sh
cargo build --release
cargo test                                   # existing 23 tests + new pure-module tests
cargo test -p menubar                        # just this feature's logic
cargo clippy --all-targets -- -D warnings    # must stay clean
```

New headless tests map to the contracts:

- `state.rs` — `derive()` truth table: Live / ReadOnly / Unavailable, no-stale
  rendering, recovery-on-first-success (FR-006/007, data-model transition table).
- `actions.rs` — mode labels ⇒ exactly the four existing CLI verbs; checkmark
  placement for each `Mode`; controls disabled when `fan_control_available ==
  false` (FR-003, FR-007).
- `delegate.rs` — command-line construction, binary discovery fallback, all
  exit-status outcomes mapped to the outcome table (no execution in tests).
- `main.rs` — **existing `render_title` tests pass unchanged** (FR-009, SC-006).
- Protocol round-trips (unchanged from 001) still pass.

## Run it

```sh
# 1. daemon (root, foreground is fine for testing)
sudo cargo run --release -p fand -- managed   # or the launchd route

# 2. the UI (ordinary user)
cargo run --release -p menubar                # GUI; menu-bar item appears
cargo run --release -p menubar -- --headless  # old behaviour kept for scripting
```

## Validation scenarios (map to Success Criteria)

| # | Steps | Expected | Criteria |
|---|---|---|---|
| 1 | Launch the UI with the daemon up; within ~2 s run `./target/release/topfan status` | a menu-bar item exists; its text matches the CLI's temp/RPM; values refresh when temperature changes | SC-001, FR-005 |
| 2 | Use the menu: Managed → Full. Immediately run `sudo ./target/release/topfan status` (and watch the item) | menu checkmark confirmed from the daemon within one poll; CLI shows the new mode; UI never showed a tick before the poll | SC-002, FR-005 |
| 3 | Menu "Open Dashboard" (also: double-click item; also: launch the binary again → should focus the existing instance) | dashboard opens < 1 s with current values; per-fan gauges use each fan's own range (2317–6898 / 2502–7450); only one instance/item exists | SC-003, FR-004, FR-008 |
| 4 | Dashboard mode control vs CLI | produces exactly the same daemon state as `sudo topfan <mode>`; checkmark/active control move only after the poll confirms | SC-002 |
| 5 | `sudo launchctl bootout system/com.topfan.fand` (stop daemon) → watch both surfaces for ~2 s | "unreachable"/no stale numbers; app keeps running; click a mode item → prompt, then accept → applied + next poll confirms; accept/decline both leave a sane UI | FR-003, FR-004, US1-4 |
| 6 | Start the daemon again | both surfaces recover automatically within a poll cycle; repeat stop/start 5+ times across an open dashboard — no crash, no stuck state | SC-004, FR-006 |
| 7 | While a live `status` shows values, simulate a broken write path (or read `fan_control_available: false`) | mode controls disabled on both surfaces with a one-line reason; temperature/RPM display continues | FR-007 (US3-3) |
| 8 | Close the dashboard → wait a few seconds → reopen | current values immediately, no leftovers from the previous viewing (sparkline restarts) | SC-003 |
| 9 | Change mode via CLI while the dashboard is open | next poll updates the dashboard's mode indicator (external-change scenario) | FR-005 |
| 10 | Toggle system light/dark appearance | both surfaces legible in both themes (dashboard web view follows; AppKit chrome follows) | SC-005 |
| 11 | Quit the app | app exits only; `topfan status` still works; daemon unharmed; fans still auto if left alone | FR-010, Constitution I |

## Safety notes

- Nothing here writes SMC keys from the UI. The only writes are the daemon's
  `SetMode` path via `topfan`, already covered by feature 001's write
  verification.
- First on-device run of the delegation prompt is deliberate: one command, one
  observed outcome (per constitution workflow), result recorded in CLAUDE.md.
  Suggested first exercise: menu **Full** with `topfan status` in a second
  window, then **Off** to hand back.