# Research: 002-desktop-ui-menubar

Feature 001's research.md already decided the hard parts (D1: privilege
delegation via osascript → existing topfan CLI; objc2 over raw cocoa). This
feature adds the decisions specific to two UI surfaces. All NEEDS CLARIFICATION
items from the spec's Technical Context were resolvable locally; no on-device
research was required (this feature writes no new SMC keys).

---

## D1 — Dashboard rendering: NSWindow + WKWebView, not hand-drawn AppKit

**Decision**: The dashboard window is an `NSWindow` hosting a `WKWebView`
(`objc2-web-kit` 0.3.2, feature `WKWebView` + `objc2-app-kit`; verified
available on docs.rs). The view HTML/CSS/JS is adapted from the approved
mockup `ui-design.html` and embedded in the binary with `include_str!`.

**Rationale**:
- An approved visual treatment already exists as runnable HTML — reusing it
  gives design fidelity for near-zero cost.
- The mockup's visual core (per-fan semicircular gauges, temperature
  sparkline, semantic hot/cool color) is a custom-drawing problem. Hand-drawn
  in AppKit via objc2 (`drawRect:` + CoreGraphics), that is hundreds of lines
  of untestable view code; in a web view it already works.
- objc2-web-kit is a system-framework binding, not a new third-party UI
  framework — it does not reintroduce what feature 001's research rejected
  (tauri/tao, a whole windowing stack for one menu). The status item itself
  stays native AppKit (D2), so the OS-face parts of the app are native; only
  the in-window canvas is HTML.

**Honesty constraint** (FR-005/FR-009): the JS layer is presentation-only. All
numbers arrive via one injected call, `window.topfan.setStatus(json)`, fed
polled daemon `Status` values from Rust. Mode buttons post a message back
through `WKScriptMessageHandler` into `actions.rs` — the same mapping the
menu uses. No control logic, formatting, or state derivation moves into JS.
Buttons may show a transient pending affordance (disabled, per the mockup)
but never a locally-confirmed state; the checkmark/active-style comes only
from the next injected poll. The sparkline history accumulates JS-side and
resets when the window closes — transient presentation, and it guarantees
SC-003 (fresh data on every open, nothing stale survives a reopen).

**Alternatives considered**:
- *Pure AppKit custom views* — native and dependency-free, but duplicates an
  existing approved design as ~600+ lines of CG drawing code the constitution's
  workflow can't test headlessly. Rejected for this round.
- *SwiftUI / Swift* — needs a Swift toolchain and an app-bundle build path the
  workspace doesn't have (as in feature 001 research).
- *tauri / tao tray* — already rejected in feature 001 research.md.

---

## D2 — Status item + menu: native AppKit per CLAUDE.md's finish note

**Decision**: `NSStatusItem` with `variableLength`, button title set from the
existing `render_title(&Status)` (unchanged text, unchanged tests), `NSMenu`
with Auto / Managed / Full / Off / Open Dashboard / Quit.

**Rationale**: This is exactly the finish plan recorded in `crates/menubar`
and `CLAUDE.md`: add `objc2` 0.6.4 + `objc2-app-kit` 0.3.2, keep polling and
formatting where they are, only presentation touches AppKit. Feature flags
(`NSStatusBar`, `NSStatusItem`, `NSMenu`, `NSAppDelegate`, `NSApplication`,
`NSWindow`, `NSEvent`, `NSRunningApplication`) are a compile-check, provable
with `cargo check -p menubar` headlessly.

**Alternatives considered**: none new (raw `cocoa` crate and tauri already
rejected in feature 001).

---

## D3 — Mode labels: what "Auto" and "Off" both mean (resolves FR-002 vs. the 3-value protocol)

**Decision**: The UI presents the four FR-002 items, mapped onto the existing
3-value protocol this way:

| UI item | Daemon intent | Delegated command | Menu role |
|---|---|---|---|
| **Auto** | `Mode::Auto` — macOS/SMC drives | `sudo topfan off` | *State* item — carries the checkmark when daemon mode is `Auto` |
| **Managed** | `Mode::Managed` — fand's curve | `sudo topfan auto` | *State* item — checkmark when mode is `Managed` |
| **Full** | `Mode::Full` — both fans pinned to their own max | `sudo topfan full` | *State* item — checkmark when mode is `Full` |
| **Off** | `Mode::Auto` — hand back to macOS | `sudo topfan off` | *Action* item — the action phrasing of the auto hand-back; never checkmarked |

**Rationale**: The protocol has exactly three modes; `Mode::Auto` *is* "the
fans are handed back" (feature 001: `topfan off` sends `Mode::Auto`, "hand the
fans back to macOS"). The approved mockup already treats the pair this way —
`auto: "macos · smc curve"` (a state) vs `off: "handed back · auto"` (the
action of getting there) — so FR-002's four items and the mockup agree on the
reading above. The checkmark always reflects the daemon's polled mode (FR-005),
so exactly one state item is ever ticked; the Off action stays enabled and
tick-free. This preserves the constitution's Principle I: the always-available
"Off" is the sanctioned hand-back path, one click away from any surface.

This mapping lives in `actions.rs` as data, with headless tests asserting both
label→command and authoritative-mode→checkmark placement.

---

## D4 — Privilege flow for every mode change (implements 001 research D1)

**Decision**: All four mode commands are executed by the app as:

```
/usr/bin/osascript -e 'do shell script "topfan <mode> with administrator privileges"'
```

The admin prompt is system-drawn once per action; we never see a password.
Unprivileged reads keep the direct socket.

**Details that need pinning down now** (full contract in
`contracts/cli-delegation.md`):

- The app looks up the `topfan` binary at fixed candidate paths
  (`/usr/local/bin/topfan`, then the sibling `target/release/topfan` during
  development). If absent, the surfaces show the CLI fallback hint
  ("run `sudo topfan <mode>`") instead of raising a prompt that cannot work.
- Declining the prompt (user cancels → osascript exits non-zero): surfaces are
  *unchanged* and the UI shows a short non-alarming hint with that same CLI
  fallback. No retry loops, no modal errors.
- `topfan` failing (daemon down between click and command) surfaces the same
  hint; the next poll's Unavailable state also renders independently.
- Spec edge case: headless/remote session where the prompt cannot display —
  osascript fails promptly rather than hanging; the hint is shown. Bounded by
  a per-command timeout (default: none needed, but the runner kills after 120 s
  as a safety net so the UI can never hang on a stuck child).

**Rationale**: This is the already-ratified decision from feature 001
(research.md D1): one authorization model (root peer-uid at the daemon), no
setuid binary, no second protocol. The menu/dashboard adds only the system
prompt in front of the existing CLI.

---

## D5 — SurfaceState derivation (pure)

**Decision**: one pure function `fn derive(outcome: PollOutcome) -> SurfaceState`:

```rust
enum PollOutcome { Reached(Status), Unreachable(String) }
enum SurfaceState {
    Live(Status),          // reached && fan_control_available
    ReadOnly(Status),      // reached && !fan_control_available  → controls disabled + one-line reason
    Unavailable,           // unreachable → no numbers, no stale data
}
```

Both surfaces render from `SurfaceState`; the mode checkmark, enabled/disabled
controls, and unavailable presentation are all functions of it (headless
tested). No state is held across polls other than the latest valid one behind
`Live`/`ReadOnly` — the Unavailable state deliberately shows *no* numbers
(spec US3: "instead of stale numbers").

**Rationale**: This is the FR-009 seam — everything testable stays out of
AppKit, mirroring what `render_title` did for feature 001.

---

## D6 — Poll cadence & bounded retry (FR-005/FR-006)

**Decision**: Fixed 2 s poll interval, 250 ms per-request timeout (unchanged
from the existing headless client). Connection failure is cheap
(instant `ECONNREFUSED` on a Unix socket), so the bounded-retry policy *is*
the fixed rate: no exponential backoff.

**Rationale**: SC-004 requires recovery within one poll cycle of the daemon
restarting — exponential backoff (2 → 4 → 8 s …) would make recovery lag by
seconds to minutes, violating the acceptance scenario. A fixed 2 s rate is
trivially "bounded" (FR-006's actual requirement: no unbounded spinning), costs
one failed syscall per tick when the daemon is down, and satisfies the 1–2 s
surface-update cadence for both directions. No retry-state machinery to test.

---

## D7 — Single instance (FR-008), app lifecycle, and process shape

**Decision**:

- The app runs as an accessory (`NSApplication::setActivationPolicy`
  → `Accessibility`/`Provisional` at startup, the LSUIElement equivalent for an
  unbundled binary) — no Dock icon, menu-bar presence only.
- Single-instance is enforced in the user domain: a lightweight lock socket at
  `$TMPDIR/topfan-ui.lock`. First instance binds and keeps it; a second launch
  connects, sends `{"cmd":"open-dashboard"}`, waits ≤ 2 s for ack, and exits.
  The first instance forwards "open" to the main thread (shows/focuses the
  dashboard window). No file-in-use race beyond accept/connect semantics; a
  stale socket whose connect fails is unlinked and re-bound.
- The whole GUI runs on the main thread via `NSApplication::run` (poll timer =
  scheduled `NSTimer`); the lock-socket acceptor and the osascript runner run
  on short-lived background threads that hand results back to the main thread.
  This threading lives in the *app* process — constitution IV's
  single-threaded scope is the `fand` daemon, which is untouched.
- `Quit` exits the app process only. It is structurally incapable of affecting
  fans (Principle I — the UI owns no fan state). No persistence on quit
  (FR-010).

**Alternatives considered**: `NSDistributedNotification` for second-launch
signalling (heavier API plumbing in objc2 for the same result); checking a pid
file (stale-pid races); relying on `open -a` (only works for a bundled .app —
we're an unbundled binary this round; bundling is a later packaging feature).

---

## D8 — What stays where (FR-009 mapping)

**Decision**:

| Concern | Home | Testable headless |
|---|---|---|
| Title formatting (`render_title`) | `crates/menubar/src/main.rs` (stays put, per CLAUDE.md) | ✅ existing tests kept passing unchanged |

> **2026-08-30 update**: later, to make plain `cargo run` launch the GUI, the
> workspace root became a real package with a shim binary that calls
> `menubar::run()`, which required `menubar` to be lib + thin binary.
> `render_title`, its tests (byte-identical), and `run()`/`headless()` moved
> verbatim to `crates/menubar/src/lib.rs`. The intent of this D8 row — don't
> churn the tested logic while adding the GUI — is preserved.
| SurfaceState derivation | `state.rs` | ✅ new tests |
| Action → command mapping, menu structure, checkmark placement | `actions.rs` | ✅ new tests |
| topfan path lookup, command-line construction, hint text | `delegate.rs` | ✅ new tests (path lookup via injected candidates; no `osascript` execution in tests) |
| Poll/IPC | `client.rs` (extracted from `main.rs`, behaviour unchanged) | ✅ via `--headless` + protocol tests |
| AppKit/Web presentation | `ui/`, `assets/dashboard.html` | ❌ deliberately untested (presentation) |

**Rationale**: Matches the constitution's workflow gate and FR-009 verbatim:
no non-presentation logic acquires a display dependency, and the format
contract is untouched.