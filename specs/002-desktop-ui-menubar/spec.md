# Feature Specification: Desktop fan-control UI with menu-bar presence

**Feature Branch**: `002-desktop-ui-menubar`

**Created**: 2026-08-30

**Status**: Draft

**Input**: User description: "UI on desktop app and the menu bar on the top."

## Scope statement

One unprivileged desktop app, two surfaces, both talking to the existing root
daemon through the existing control protocol:

1. **Menu bar (top of screen)** — an always-present status item showing the
   machine's thermal state at a glance, with a small dropdown menu for instant
   mode changes.
2. **Desktop window** — the app's main UI: a live dashboard (temperatures,
   per-fan RPM against their own ranges, current mode) with the mode controls,
   opened on demand from the menu bar or directly launching the app.

The daemon, CLI, and all fan-control logic are out of scope for this feature —
this is presentation over the existing interface. Curve editing / preferences /
tray-icon customization are out of scope (documented in Assumptions).

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Glanceable menu-bar status (Priority: P1)

While working normally, the user can see the machine's thermal state at a
glance: a compact status item at the top of the screen that shows the hottest
die temperature and fan speed whenever the daemon is reachable, without opening
anything. Clicking the item reveals a short menu with the current mode marked
and one-click mode changes (Auto / Managed / Full / Off) plus Open Dashboard
and Quit.

**Why this priority**: The always-visible glanceable state is the core reason
this app exists — ambient cooling awareness without any interaction.

**Independent Test**: Launch the app; a menu-bar item appears showing live
values that match `topfan status` within a poll cycle; the dropdown lets the
user switch modes; mode switches take effect (verifiable via CLI `status`).

**Acceptance Scenarios**:

1. **Given** the daemon is running, **When** the app starts, **Then** a status
   item appears at the top of the screen within ~2 s showing current
   temperature/fan values that match `topfan status`.
2. **Given** temperatures or fan speeds change, **When** the next poll lands
   (~1–2 s), **Then** the status item reflects the new values.
3. **Given** the dropdown menu open, **When** the user picks a different mode,
   **Then** the daemon applies it and the menu checkmark moves to the new mode
   on the next poll — the menu always shows the daemon's authoritative state,
   never the click.
4. **Given** the mode change needs elevated permission, **When** the user picks
   it, **Then** the system's standard admin-authorization prompt appears once,
   and declining leaves state unchanged with a clear (non-alarming) fallback
   hint; accepting applies the change.
5. **Given** a mode menu item chosen, **When** the user has picked it, **Then**
   no other app or window needs to be open for the change to happen.

---

### User Story 2 - Live desktop dashboard window (Priority: P2)

The user opens the app's main window (from the menu item "Open Dashboard", by
double-clicking the status item, or by launching the app directly) and sees a
clean live dashboard: hottest die temperature, per-fan RPM shown against each
fan's own minimum–maximum range, current mode, and mode controls identical in
behaviour to the menu. The window can be closed freely; the menu-bar presence
survives, and reopening shows current data immediately.

**Why this priority**: The dashboard gives the detail the compact menu item
cannot; it is the "real UI" of the desktop app.

**Independent Test**: Open the dashboard while the daemon reports known values;
confirm the window shows those values and its controls move RPM/mode identically
to the CLI; close and reopen without stale data.

**Acceptance Scenarios**:

1. **Given** the app running and the daemon up, **When** the dashboard is
   opened, **Then** it shows the hottest temperature, each fan's current RPM
   and its own reported range, and the current mode within ~1 s of opening.
2. **Given** the dashboard open, **When** the user activates a mode control,
   **Then** the same effect as the equivalent menu/CLI action happens, with the
   result confirmed from the daemon (not from the click).
3. **Given** the dashboard open, **When** load drives temperature/RPM changes,
   **Then** the display keeps updating roughly every 1–2 s until closed.
4. **Given** the dashboard closed, **When** the user reopens it, **Then** it
   shows current data immediately (no stale leftovers from the last viewing).

---

### User Story 3 - Honest degraded states (Priority: P3)

Both surfaces tell the truth in every situation. When the daemon is down, the
status item and dashboard show a recognizable "unavailable" state instead of
stale numbers, and the app recovers automatically when the daemon returns. When
fan control is unavailable (daemon up, write path broken), the UI shows the
read-only reality and disables mode controls rather than presenting buttons
that do nothing.

**Why this priority**: Safety-relevant honesty — this is a thermal control;
pretend-state is worse than no state.

**Independent Test**: Stop the daemon: status item flips to an unavailable
state and stays stable (no crash, no spinner forever); restart the daemon: the
item returns to live values without user intervention. With write verification
failing, mode controls show disabled.

**Acceptance Scenarios**:

1. **Given** the daemon stopped, **When** the app polls, **Then** within one
   poll cycle both surfaces show the unavailable state, and the app keeps
   running quietly (bounded retry, no crash, no user nagging).
2. **Given** the unavailable state, **When** the daemon starts again, **Then**
   both surfaces return to live values automatically within a poll cycle.
3. **Given** the daemon reports fan control unavailable, **When** the UI
   renders, **Then** mode controls appear disabled with a short explanation,
   while live temperature/RPM display continues (reads still work).

### Edge Cases

- The app is launched while an instance is already running → only one app with
  one menu-bar item exists (second launch simply re-opens/focuses).
- Elevated action is attempted in a headless/remote session where the standard
  admin prompt cannot display → the surfaces show the CLI fallback
  ("run `sudo topfan full`") instead of hanging.
- A mode change happens outside the app (via CLI) while the dashboard is open →
  the next poll updates all surfaces to the new authoritative state.
- The daemon restarts (or wakes from sleep) while the dashboard is open → no
  stuck "unavailable" state or wrong numbers once polls resume; the wake
  auto-restore of fans is simply reflected in the live state.
- System appearance changes while the app runs → both surfaces remain legible
  in light and dark styles.

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: The app MUST present a persistent status item in the system menu
  bar that renders the live state (hottest temperature + representative fan
  speed) formatted by the project's existing shared formatting rules (the
  current status-line format and its tests are the contract).
- **FR-002**: The status item's menu MUST offer Auto / Managed / Full / Off with
  the current mode visibly marked, plus Open Dashboard and Quit.
- **FR-003**: Menu mode actions MUST behave exactly like their CLI equivalents
  over the existing daemon protocol — no second control path; elevated actions
  MUST use the system's standard admin-authorization flow.
- **FR-004**: The app MUST provide a main dashboard window reachable from the
  menu (and from launching the app), showing: hottest temperature, each fan's
  RPM and its own range, current mode, and mode controls matching FR-003.
- **FR-005**: All surfaces MUST update from polling the daemon on a 1–2 s
  cadence and MUST display the daemon's authoritative response, never local
  optimistic state.
- **FR-006**: The app MUST present a distinct unavailable state when the daemon
  is unreachable and MUST auto-recover to live values when it returns, with a
  bounded retry backoff (no unbounded spinning, no modal errors).
- **FR-007**: When the daemon reports fan control unavailable, the app MUST
  clearly disable mode controls in all surfaces while continuing to display
  live temperature/RPM.
- **FR-008**: Launching the app twice MUST result in a single instance with a
  single menu-bar item.
- **FR-009**: All non-presentation logic (status formatting, action → command
  mapping, poll/refresh cadence, unavailable-state decision) MUST remain
  testable without a display environment, and the existing format tests MUST
  keep passing unchanged.
- **FR-010**: The app MUST not keep its own durable settings/persistence this
  round; quitting ends the UI only (the daemon's lifecycle is separate).

### Key Entities *(include if feature involves data)*

- **Status** *(read model, from daemon)*: current mode, hottest die temperature,
  duty, per-fan RPM with per-fan bounds, fan-control-availability.
- **Mode** *(user intent)*: Auto | Managed | Full — selected in either surface,
  owned by the daemon.
- **SurfaceState**: what each surface renders: Live(status) | Unavailable |
  ReadOnly (control unavailable) — derived purely from polled Status and
  connection outcome.
- **AppInstance**: the single running app owning both the menu-bar item and the
  dashboard window; window open/closed is transient, not persisted state.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: The status item appears and shows live values within 2 s of app
  launch, and its content matches `topfan status` within one poll cycle.
- **SC-002**: A mode change from either surface produces the same observable
  result as `topfan <mode>` within one poll cycle, verified via CLI.
- **SC-003**: The dashboard opens in under 1 s and shows current (not stale)
  values on every open.
- **SC-004**: Stopping the daemon visibly flips both surfaces to the unavailable
  state within 2 s, with automatic recovery within one poll cycle of restart —
  no crash observed across ≥5 daemon stop/start cycles.
- **SC-005**: In a light- and a dark-themed session, all controls and text on
  both surfaces are legible (no hardcoded-on-background text).
- **SC-006**: Existing headless test suites for formatting/action mapping keep
  passing; no non-presentation logic acquires a display dependency.

## Assumptions

- Platform is macOS on this MacBook; "menu bar on the top" = the system menu
  bar, as a status item (not a floating overlay).
- One app instance owns both surfaces; launching the app directly shows the
  dashboard; the menu-bar item remains the always-visible anchor.
- The privilege model is the previously decided one (research.md D1 of feature
  001): elevated actions flow through the standard system admin-authorization
  prompt to the existing CLI; reads use the direct unprivileged daemon socket.
- Curve visualization is display-only this round: showing current behaviour —
  not editing of governor breakpoints or hysteresis (would require new daemon
  config surface; out of scope).
- No login-item/launch-on-start requirement this round (user starts the app, or
  a later packaging feature adds it).