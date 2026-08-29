# Tasks: Desktop fan-control UI with menu-bar presence

**Input**: Design documents from `/specs/002-desktop-ui-menubar/`

**Prerequisites**: plan.md (required), spec.md (required for user stories), research.md (D1–D8), data-model.md, contracts/ (surfaces.md, cli-delegation.md, ipc-protocol.md), quickstart.md, .specify/memory/constitution.md

**Tests**: Headless tests for the new pure modules are **explicitly requested** by plan.md ("the existing 23 tests plus new pure-module tests") and quickstart.md's test map (`state.rs`, `actions.rs`, `delegate.rs`). Presentation (`ui/`, `assets/`) is deliberately untested — do not write AppKit/WKWebView tests.

**Organization**: Tasks are grouped by user story to enable independent implementation and testing of each story. All non-presentation logic lives in pure modules per FR-009 / research D8.

## Format: `[ID] [P?] [Story] Description`

- **[P]**: Can run in parallel (different files, no dependencies)
- **[Story]**: Which user story this task belongs to (e.g., US1, US2, US3)
- Include exact file paths in descriptions

## Path Conventions

- Rust workspace; all new code extends the existing `crates/menubar` crate (plan.md Structure Decision: no new crates, no daemon changes).
- `render_title` and its tests **stay in `crates/menubar/src/main.rs`** (research D8 / CLAUDE.md).

---

## Phase 1: Setup (Shared Infrastructure)

**Purpose**: GUI dependencies in place, baseline green before refactoring

- [ ] T001 Add GUI dependencies to crates/menubar/Cargo.toml: `objc2` 0.6.4, `objc2-foundation` 0.3.x, `objc2-app-kit` 0.3.2 (features: `NSStatusBar`, `NSStatusItem`, `NSMenu`, `NSApplication`, `NSWindow`, `NSResponder`, `NSEvent`, `NSRunningApplication`, `NSApplicationDelegate`, `NSMenuItem`), `objc2-web-kit` 0.3.2 (feature `WKWebView` + `objc2-app-kit`); verify with `cargo check -p menubar` that the feature-flag set compiles headlessly (research D2 compile-check)
- [ ] T002 Record the pre-change baseline: run `cargo test` (expect all existing 23 tests green) and `cargo clippy --all-targets -- -D warnings` (expect clean) so later tasks have an unambiguous reference state (constitution workflow)

---

## Phase 2: Foundational (Blocking Prerequisites)

**Purpose**: Pure logic modules — `PollOutcome`/`SurfaceState`, action→command mapping, CLI delegation contract. MUST complete before any user story; every story renders and acts through these.

**⚠️ CRITICAL**: No user story work can begin until this phase is complete

- [ ] T003 Extract the existing poll/IPC behaviour unchanged from crates/menubar/src/main.rs into crates/menubar/src/client.rs (poll(), one-shot request helper, 250 ms timeout); keep `--headless` mode and ALL existing `render_title` tests passing unchanged (FR-009, research D8)
- [ ] T004 [P] Write failing headless tests for SurfaceState derivation in crates/menubar/src/state.rs: the full derive() truth table — Reached+available → Live, Reached+unavailable → ReadOnly, Unreachable → Unavailable; derivation is memoryless (same outcome ⇒ same state); Unavailable carries no Status data (no stale numbers); recovery happens on first successful poll (data-model.md rules, FR-006, FR-007)
- [ ] T005 Implement `PollOutcome` and `SurfaceState` with `fn derive(PollOutcome) -> SurfaceState` in crates/menubar/src/state.rs exactly per data-model.md; make the T004 tests pass
- [ ] T006 [P] Write failing headless tests for the action mapping in crates/menubar/src/actions.rs: each of the four mode items maps to exactly the existing CLI verbs per contracts/surfaces.md (Auto→`topfan off`, Managed→`topfan auto`, Full→`topfan full`, Off→`topfan off`); checkmark placement — exactly one state item (Auto/Managed/Full) matches each daemon `Mode`, Off never ticks; every mode action disabled when `fan_control_available == false`; Open Dashboard and Quit map to local actions (FR-003, FR-007, research D3)
- [ ] T007 Implement `ModeAction` as a data table (label, command, state_item) in crates/menubar/src/actions.rs per research D3; menu item list = Auto | Managed | Full | Off | Open Dashboard | Quit; make the T006 tests pass
- [ ] T008 [P] Write failing headless tests for CLI delegation in crates/menubar/src/delegate.rs: osascript command-line construction (`/usr/bin/osascript -e 'do shell script "topfan <verb> with administrator privileges"'`); topfan binary discovery with injected candidate paths (`/usr/local/bin/topfan`, then `<manifest-dir>/../../target/release/topfan`) and missing-binary ⇒ prompt-never-raised; every exit-status outcome mapped to the contracts/cli-delegation.md outcome table (Applied / Declined / Failed / topfan-missing / hung-child-after-120s) as string-typed outcomes with no process execution in tests (research D4, constitution VI)
- [ ] T009 Implement delegate.rs in crates/menubar/src/delegate.rs: topfan path lookup, command construction, outcome classification, CLI-fallback hint text ("run `sudo topfan <verb>`"); leave the actual osascript execution as a small seam to wire in T013 (no execution in unit tests per research D4); make the T008 tests pass

**Checkpoint**: Foundation ready — all pure logic headless-green; user story implementation can now begin

---

## Phase 3: User Story 1 — Glanceable menu-bar status (Priority: P1) 🎯 MVP

**Goal**: A always-present status item showing live thermal state with a dropdown menu for one-click mode changes through the sanctioned delegation path.

**Independent Test**: Launch the app with the daemon up; a status item appears within ~2 s showing values that match `topfan status`; menu mode changes take effect and the checkmark moves only after the next poll confirms (quickstart scenarios 1–2).

### Implementation for User Story 1

- [ ] T010 [US1] Create the AppKit shell in crates/menubar/src/ui/mod.rs: NSApplication assembly with accessory activation policy at startup (no Dock icon, research D7), the 2 s poll timer on the main run loop handing each `SurfaceState` from state.rs to the surfaces, and the main-thread run loop; rework crates/menubar/src/main.rs so GUI is the default and `--headless` keeps the existing poll-and-print loop
- [ ] T011 [US1] Implement the NSStatusItem in crates/menubar/src/ui/mod.rs (variableLength): set button title from the existing `render_title(&Status)` on every poll where state is Live/ReadOnly; Unavailable renders the distinct no-numbers presentation (FR-001, FR-005)
- [ ] T012 [US1] Implement NSMenu construction in crates/menubar/src/ui/menu.rs from the actions.rs item list: mode items with the checkmark on the state item matching the polled `status.mode` (never from the click), plus Open Dashboard and Quit items (FR-002, FR-005, research D3)
- [ ] T013 [US1] Implement the execution side of delegation in crates/menubar/src/delegate.rs: spawn osascript via std::process::Command on a background thread (UI main thread never blocked), 120 s kill timer, result handed back to the main thread; wire menu mode items in ui/menu.rs to the runner through actions.rs mapping with a transient pending affordance and no locally-confirmed state (FR-003, research D4, contracts/cli-delegation.md)
- [ ] T014 [US1] Implement single-instance mediation in crates/menubar/src/ui/mod.rs: lock socket `$TMPDIR/topfan-ui.lock`; second launch connects, sends `{"cmd":"open-dashboard"}`, waits ≤ 2 s for ack, exits; first instance unlinks a dead socket and re-binds, and shows/focuses the dashboard on receipt (FR-008, research D7; window-open target may be a stub until US2)
- [ ] T015 [US1] Validate User Story 1 independently per quickstart.md scenarios 1–2 (SC-001: item live ≤ 2 s matching `topfan status`; SC-002: mode change confirmed by the daemon within one poll, checkmark moves on the poll after) and `cargo test -p menubar` + `cargo clippy --all-targets -- -D warnings` green

**Checkpoint**: User Story 1 fully functional and independently testable — MVP demo-able

---

## Phase 4: User Story 2 — Live desktop dashboard window (Priority: P2)

**Goal**: The main window: a live dashboard mirroring the approved mockup with per-fan gauges against each fan's own range and mode controls identical in behaviour to the menu.

**Independent Test**: Open the dashboard while the daemon reports known values; the window shows those values within ~1 s; its controls produce the same daemon state as the CLI; close and reopen with no stale data (quickstart scenarios 3, 4, 8, 9).

### Implementation for User Story 2

- [ ] T016 [P] [US2] Create crates/menubar/assets/dashboard.html adapted from the approved mockup specs/002-desktop-ui-menubar/ui-design.html: hottest die temperature, per-fan semicircular gauges normalized against each fan's own `min_rpm…max_rpm` from the injected status (no constants, Constitution III), current mode, mode buttons; `prefers-color-scheme` light/dark palettes (SC-005); JS is presentation-only — one inbound `window.topfan.setStatus(json)`, outbound `{kind:"mode",value:...}` messages; sparkline history per-open only; **remove any demo/fixture controls** (FR-004, research D1, data-model.md DashboardBridge)
- [ ] T017 [US2] Implement crates/menubar/src/ui/dashboard.rs: NSWindow hosting a WKWebView (embed dashboard.html via include_str!), inject the serialized current SurfaceState on every poll via `window.topfan.setStatus(json)`, re-render current state immediately on open so reopened windows show fresh data (SC-003, FR-005)
- [ ] T018 [US2] Implement the JS bridge in crates/menubar/src/ui/dashboard.rs: WKScriptMessageHandler receiving `{kind:"mode",value}` messages and handing them to the same actions.rs mapping as the menu (T013 runner); pending-button affordance cleared on every setStatus; buttons never self-confirm (FR-003, FR-005, research D1 honesty constraint)
- [ ] T019 [US2] Wire open paths in crates/menubar/src/ui/mod.rs and crates/menubar/src/ui/menu.rs: "Open Dashboard" menu item, double-click on the status item, direct launch, and the single-instance `open-dashboard` forward from T014; closing the window leaves the app and status item running (FR-004, FR-008)
- [ ] T020 [US2] Validate User Story 2 independently per quickstart.md scenarios 3, 4, 8, 9 (SC-003: opens < 1 s with current values, gauges use each fan's own range 2317–6898 / 2502–7450; external CLI mode change reflected on next poll; single instance) with `cargo test` + `cargo clippy --all-targets -- -D warnings` green

**Checkpoint**: User Stories 1 AND 2 both work independently

---

## Phase 5: User Story 3 — Honest degraded states (Priority: P3)

**Goal**: Both surfaces tell the truth when the daemon is down or write control is unavailable, and recover automatically.

**Independent Test**: Stop the daemon — both surfaces flip to unavailable within a poll cycle, stay stable, and auto-recover on restart; with `fan_control_available == false` mode controls are disabled in both surfaces while live readout continues (quickstart scenarios 5–7).

### Implementation for User Story 3

- [ ] T021 [US3] Implement degraded rendering for the menu in crates/menubar/src/ui/menu.rs driven purely by SurfaceState: Unavailable ⇒ all mode items disabled with the CLI fallback hint from delegate.rs; ReadOnly ⇒ all mode items disabled with the one-line reason; live values (title) continue in ReadOnly; checkmark never shown while unavailable (contracts/surfaces.md rendering table, FR-006, FR-007)
- [ ] T022 [US3] Implement degraded rendering for the dashboard in crates/menubar/src/ui/dashboard.rs and crates/menubar/assets/dashboard.html from the same SurfaceState: Unavailable ⇒ "fand unreachable" presentation with **no stale numbers** and sparkline cleared; ReadOnly ⇒ live temperature/RPM continue with mode controls visibly disabled plus reason; auto-recovery renders on the first successful poll with no user action (US3, FR-006, FR-007)
- [ ] T023 [US3] Implement delegation failure UX in crates/menubar/src/ui/menu.rs and crates/menubar/src/ui/dashboard.rs: declined/failed/missing-topfan outcomes from delegate.rs surface one short non-alarming hint with the CLI fallback text, state unchanged, no dialogs, no retry loops, no repeat prompting (contracts/cli-delegation.md outcome table, spec edge case for headless/remote sessions)
- [ ] T024 [US3] Validate User Story 3 independently per quickstart.md scenarios 5–7: daemon stop → unavailable within ~2 s on both surfaces; restart → auto-recovery within one poll cycle; ≥5 stop/start cycles with an open dashboard, no crash and no stuck state; simulated `fan_control_available: false` disables controls on both surfaces while reads continue (SC-004, FR-006, FR-007)

**Checkpoint**: All user stories independently functional

---

## Phase 6: Polish & Cross-Cutting Concerns

**Purpose**: Documentation truth, automated-gate hygiene, full on-device validation

- [ ] T025 [P] Update CLAUDE.md: replace the "menu bar is not built yet / HEADLESS STAND-IN" section with the real layout (client.rs/state.rs/actions.rs/delegate.rs/ui/, assets/dashboard.html), build/run commands from quickstart.md, and record the first deliberate delegation-prompt observation per constitution workflow (one command, one observed outcome)
- [ ] T026 Run the full automated gate: `cargo test` (existing 23 + new pure-module tests all green), `cargo clippy --all-targets -- -D warnings` clean, `cargo fmt` applied; confirm `render_title` tests are byte-identical to the pre-change baseline from T002 (FR-009, SC-006, constitution workflow)
- [ ] T027 Run the complete quickstart.md validation matrix (scenarios 1–11) on-device, deliberately, including the single-instance relaunch check (FR-008) and the Quit-leaves-daemon-unharmed check (FR-010, Constitution I); record results and any deviations

---

## Dependencies & Execution Order

### Phase Dependencies

- **Setup (Phase 1)**: No dependencies — can start immediately
- **Foundational (Phase 2)**: Depends on Setup — **BLOCKS all user stories**. T003 first (client.rs extraction must not change behaviour); T004–T009 each pair tests-before-implementation
- **User Stories (Phases 3–5)**: Depend on Foundational. US1 → US2 → US3 in priority order; US2's wire-up (T019) builds on US1's menu (T012) and single-instance stub (T014); US3 is pure presentation variation on the same data path and could start after US1, but sequential delivery keeps each increment honest
- **Polish (Phase 6)**: Depends on all desired user stories complete

### User Story Dependencies

- **User Story 1 (P1)**: Can start after Foundational — no dependencies on other stories
- **User Story 2 (P2)**: Extends US1's app shell; independently testable once T016–T018 done
- **User Story 3 (P3)**: Renders from the same `SurfaceState`/`ModeAction` foundation; depends on US1's surfaces existing

### Within Each User Story

- `assets/dashboard.html` (T016) before `dashboard.rs` (T017) — the embed target must exist
- Pure-module changes always before their presentation wiring
- Story complete before moving to next priority

### Parallel Opportunities

- T004/T005 blocked internally (same file), but the three pure-module tracks may interleave: (state.rs) ∥ (actions.rs) ∥ (delegate.rs) touch different files
- T016 [P] can run during Phase 3 (different files — assets vs ui/)
- T025 [P] documentation can proceed anytime after US3

---

## Parallel Example: Foundational Phase

```bash
# Three independent pure-module tracks (different files):
Task: "Write failing headless tests for SurfaceState derivation in crates/menubar/src/state.rs"
Task: "Write failing headless tests for the action mapping in crates/menubar/src/actions.rs"
Task: "Write failing headless tests for CLI delegation in crates/menubar/src/delegate.rs"
# Then each track's implementation task, tests before code, same file ⇒ sequential
```

---

## Implementation Strategy

### MVP First (User Story 1 Only)

1. Complete Phase 1: Setup
2. Complete Phase 2: Foundational (CRITICAL — blocks all stories)
3. Complete Phase 3: User Story 1
4. **STOP and VALIDATE**: the menu-bar item alone delivers the core "glanceable" value (SC-001, SC-002)
5. Demo if ready

### Incremental Delivery

1. Setup + Foundational → pure logic headless-green, zero display dependencies
2. Add User Story 1 → validate quickstart 1–2 → MVP (glanceable menu bar)
3. Add User Story 2 → validate quickstart 3, 4, 8, 9 → full desktop app
4. Add User Story 3 → validate quickstart 5–7 → honest degraded states
5. Polish → docs + full on-device matrix

### Notes

- Zero new `unsafe` (constitution V): objc2 is a safe binding; `MainThreadMarker` acquisition centralized in `ui/mod.rs` and documented
- No direct socket `SetMode` from the GUI, no new protocol messages, no persistence (FR-010, 001 compatibility rule)
- The daemon is never rebuilt or touched by this feature
- Verify tests fail before implementing; commit after each task or logical group
- Stop at any checkpoint to validate the story independently