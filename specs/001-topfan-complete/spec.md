# Feature Specification: TopFanMac — finish fan control (write path, wake, menu bar, install)

**Feature Branch**: `001-topfan-complete`

**Created**: 2026-08-30

**Status**: Draft

**Input**: User description: "plan all the features in this project"

## Scope statement

The daemon core, CLI, governor, and SMC *read* path already exist and are tested
(23 tests). This feature covers everything between "reads work" and "the tool is
finished": a **verified write path**, **wake re-assertion**, the **real menu-bar
app**, and **frictionless LaunchDaemon installation**. Each is an independently
shippable slice.

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Proven SMC write path (Priority: P1)

The daemon can actually control the fans. Today `Smc::write` is implemented and
typed but has never executed against hardware. The user runs the first deliberate
write, observes RPM actually move, and from then on `full` and `off` are trusted
operations. While the write path is unproven, `topfan status` must expose that
fact (the existing `fan_control_available` field exists precisely for this).

**Why this priority**: Every other feature depends on writes working. `auto` and
`full` are the product; without a proven write path the daemon can only observe.

**Independent Test**: Run `sudo topfan full` while watching `topfan status`;
fans report `Md=Forced` and RPM rises toward `F0Mx`. Then `sudo topfan off`;
fans return to `Md=Auto` and SMC-managed RPM. Deliverable: an unambiguous
verdict on the write path, recorded where the project keeps hardware facts.

**Acceptance Scenarios**:

1. **Given** a running daemon, **When** root sends `SetMode{Full}`, **Then** each
   fan reaches mode Forced with target near its own reported maximum within one
   control tick, and both fans' different ranges are respected.
2. **Given** Forced mode at a raised RPM, **When** root sends `SetMode{Auto}`,
   **Then** all fans return to `Md=Auto` and the SMC regains control.
3. **Given** a daemon started under macOS load when the OS has itself forced the
   fans (`Md=1` is *normal* under sustained load), **When** `status` is read,
   **Then** the daemon reports current mode truthfully without treating the OS's
   own forcing as daemon failure.
4. **Given** a non-root peer sends `SetMode`, **When** the request is received,
   **Then** it is rejected and fans are unchanged (reads stay unprivileged).
5. **Given** the write path is broken (e.g. SMC rejects `F0Tg` writes), **When**
   any mode change is attempted, **Then** the daemon keeps sampling, reports
   `fan_control_available: false`, logs the error, and never leaves a fan
   stranded in Forced mode.

---

### User Story 2 - Wake re-assertion (Priority: P2)

Sleep resets SMC state — fans may wake up in auto (fine) or with stale targets
(not fine). The daemon must subscribe to macOS sleep/wake notifications and, on
every wake, restore SMC-managed auto *before* re-entering its control loop. The
current daemon does not do this.

**Why this priority**: A laptop sleeps and wakes many times a day; this is the
daemon's main correctness gap after the write path itself. It protects safety
invariant 5 on every wake, not just at process start.

**Independent Test**: Put the machine to sleep with the daemon in Managed/
 Forced mode; wake it; confirm within one tick that fans were reset to auto and
 the governor has resumed from a defined state, verified via `topfan status`
 and daemon log.

**Acceptance Scenarios**:

1. **Given** the daemon in Managed mode, **When** the machine sleeps and wakes,
   **Then** the daemon has called `restore_all_to_auto()` on wake before writing
   any new target.
2. **Given** any daemon mode, **When** the OS itself forces fans during heavy
   load while awake, **Then** the daemon stays responsive and does not
   misinterpret OS forcing as a wake event.
3. **Given** the daemon cannot reach the wake-notification system, **When** it
   starts, **Then** it still runs and logs the limitation rather than exiting.

---

### User Story 3 - Menu bar app with a real status item (Priority: P3)

`crates/menubar` is a headless stand-in. The user gets an actual NSStatusItem:
a live title (existing pure `render_title`, unchanged) and a menu with
**Auto / Managed / Full / Off / Quit** that sends the same `Request::SetMode`
the CLI sends. Polling and formatting stay where they are; only presentation
uses AppKit.

**Why this priority**: Convenience surface. The daemon+CLI is complete value;
the menu bar makes it ambient.

**Independent Test**: Launch the built app from an unprivileged terminal; the
status item renders within ~1 s, the title updates with `topfan status` data,
and picking ⌂Full changes RPM identically to `sudo topfan full`… with the
privilege question resolved per the requirements below.

**Acceptance Scenarios**:

1. **Given** the menu-bar app running unprivileged, **When** it polls the
   daemon, **Then** the status item title matches what `topfan status` shows.
2. **Given** the menu open, **When** the user picks a mode, **Then** the same
   `Request::SetMode` flows over the same socket, and the menu reflects the
   daemon's authoritative response (a rejected mode shows as an error, not a
   silent success).
3. **Given** the daemon is not running, **When** the app starts, **Then** the
   status item shows a daemon-down state and the app neither crashes nor
   spams reconnection.
4. **Given** AppKit is unavailable (headless/CI), **When** the app binary is
   built and its logic tested, **Then** all pure logic (title rendering,
   menu action mapping) passes without a window server.

---

### User Story 4 - One-shot LaunchDaemon installation (Priority: P4)

The plist and manual `sudo cp`/`launchctl` commands exist and work. The user
should not have to remember them: an `install` path (make target or
`topfan install`) copies the binary and plist, bootstraps the daemon, and
verifies it came up — and a matching `uninstall` bootouts and restores auto.

**Why this priority**: Packaging polish; every other story works without it.

**Independent Test**: On a clean machine state, run the install command once,
then `topfan status` succeeds and `launchctl print system/com.topfan.fand`
shows the service running. Run uninstall; the service is gone and fans are in
auto.

**Acceptance Scenarios**:

1. **Given** the daemon is not installed, **When** the user runs install, **Then**
   binary+plist are copied, the daemon is bootstrapped, and `status` works.
2. **Given** the daemon is installed and running in some mode, **When** the user
   runs uninstall, **Then** fans are restored to auto *first*, then the service
   is bootout and files removed.
3. **Given** an already-installed daemon, **When** install is run again, **Then**
   it is idempotent (replaces binary/plist, no duplicate service).

### Edge Cases

- What happens when the two fans' bounds differ (they do: 2317–6898 vs
  2502–7450)? Each fan is clamped to its *own* `F0Mn`/`F0Mx`; never shared.
- What happens when macOS itself has forced fans (`F0Md=1`, target=max) under
  load average 14? Not treated as daemon state; daemon reads truth and only
  raises further, never lowers (invariant 2).
- What happens when a previous daemon died under SIGKILL leaving Forced mode?
  Startup `restore()` + launchd `KeepAlive` together guarantee recovery.
- What happens on rapid mode changes (Full→Off→Full in <1 s)? Single-threaded
  daemon serialises; last request wins; every reply is authoritative.
- What happens when the socket client disappears mid-request? 250 ms read
  timeout cleans up; loop continues.
- What happens when wake arrives during an in-flight control tick? Wake
  handling and tick are serialised in the same thread; no torn SMC write.

## Requirements *(mandatory)*

### Functional Requirements

**Write path (Story 1)**

- **FR-001**: `sudo topfan full` MUST raise each fan to *its own* reported maximum
  within bounds read from `F0Mn`/`F0Mx`.
- **FR-002**: `sudo topfan off` MUST return every fan to `FanMode::Auto`.
- **FR-003**: The daemon MUST report `fan_control_available` truthfully in every
  `status` reply (true only once a write has succeeded against hardware).
- **FR-004**: The daemon MUST never lower a fan's target below its current actual
  RPM while driving it (invariant 2) — the raise-only clamp.
- **FR-005**: Every set_mode/set_target write MUST be followed by a read-back that
  verifies the SMC accepted it; a failed verification is logged and reflected in
  `fan_control_available`.
- **FR-006**: `SetMode` from a non-root peer MUST be rejected with an error reply.

**Wake (Story 2)** *(mechanism refined per research.md D3 — wake is detected by
time discontinuity each tick, not by an event subscription)*

- **FR-007**: The daemon MUST detect wake in every control tick (monotonic vs
  wall-clock gap) and MUST call `restore_all_to_auto()` on detected wake before
  the next tick writes a target.
- **FR-008**: Wake detection MUST be a pure predicate in the governor; a missed
  or noisy detection MUST be non-fatal and logged.
- **FR-009**: The wake path MUST be testable in isolation: a pure
  `on_wake(state)` step over the `FanControl` trait.

**Menu bar (Story 3)**

- **FR-010**: The menu-bar app MUST render its title exclusively through the
  existing pure `render_title` (no AppKit state in formatting).
- **FR-011**: Menu actions MUST send `Request::SetMode` over the existing socket;
  no second protocol.
- **FR-012**: Menu actions requiring root MUST surface the daemon's Error reply
  in the UI (non-privileged menu-bar user must see that driving fans was
  rejected — see Assumptions for the intended resolution).
- **FR-013**: The app MUST degrade gracefully when the daemon is unreachable
  (title shows unknown state; bounded reconnect backoff).
- **FR-014**: All menu-bar logic except NSStatusItem/NSMenu presentation MUST be
  testable headlessly (keep existing tests green).

**Install (Story 4)**

- **FR-015**: Install MUST copy the release binary and plist, bootstrap the
  daemon, and verify liveness via `status`; uninstall MUST restore auto, bootout,
  and remove files, in that order.
- **FR-016**: Install/uninstall MUST be idempotent and MUST refuse or correctly
  handle the "already installed" case.

### Key Entities *(include if feature involves data)*

- **Fan**: per-fan state read from SMC (`actual`, `target`, `min`, `max`, `mode`);
  bounds are per-fan and are never hard-coded.
- **Mode**: Auto | Managed | Full — the user-visible control policy, mapped onto
  SMC fan modes by the governor.
- **Request / Response**: line-delimited JSON over `/var/run/topfan.sock`;
  `Status | SetMode{mode}` → `Status | Ok | Error{message}`.
- **WakeEvent**: sleep/wake signal consumed by a pure `on_wake` step.
- **DaemonRuntime**: single-threaded control loop that owns all IOKit handles,
  serialises client requests, ticks (~1 s), and wake events.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: One deliberate hardware session proves writes: `full` moves both
  fans to ≥90% of reported max (different maxima respected) and `off` returns
  `Md=Auto` on both — observed in `topfan status`, once, with intent.
- **SC-002**: After write proof, `fan_control_available` stays `true` across a
  ≥24 h daemon uptime including at least one sleep/wake cycle.
- **SC-003**: Sleep then wake under Managed mode leaves fans in auto within one
  control tick (≤1 s of wake), verified via status + log, ≥3 consecutive cycles.
- **SC-004**: Menu-bar app launched unprivileged shows a live title and
  successfully switches to Managed mode without sudo for the *curve-following*
  mode (privilege model per Assumptions).
- **SC-005**: A fresh install command yields a working daemon (`status` OK,
  launchctl shows running) with no manual editing of paths; uninstall reverses it
  with fans left in auto.
- **SC-006**: `cargo test`, `cargo clippy --all-targets -- -D warnings` remain
  clean, with 0 `unsafe` outside `crates/smc` and the two documented exceptions
  in `fand`.

## Assumptions

- **Privilege model (Story 3)**: SetMode requires root by peer-uid today.
  Resolved by research.md D1: the menu bar delegates privileged commands
  through the system admin-privilege prompt (`osascript` → existing `topfan`
  CLI); reads stay direct-socket. Revisit only if the per-command password
  prompt is unacceptable (then: root proxy agent).
- Daemon tick ≈1 s with 250 ms client timeout, unchanged.
- The machine is a Mac15,8 M3 Max, macOS 26.5.1 — hardware facts in CLAUDE.md
  remain the source of truth for SMC key behaviour and fan ranges.
- No persistence beyond what launchd provides; no config file this round
  (hysteresis/curve stay compiled-in defaults).
- Thermal/power-limit management is out of scope (invariant 4): fans only.