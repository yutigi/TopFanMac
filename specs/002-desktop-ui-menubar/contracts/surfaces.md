# Contract: UI surfaces (status item, menu, dashboard)

What each surface renders and how surface actions become commands. This is the
contract between the pure logic (`state.rs`, `actions.rs`, `title` in
`main.rs`) and the presentation layer (`ui/`, `assets/dashboard.html`).

All rules here trace to a spec FR and are enforced by headless tests on the
pure side; presentation that violates the contract is a review defect, not a
test gap.

## Surfaces and their data source

| Surface | Title/values | Controls |
|---|---|---|
| Status item (button title) | `render_title(&Status)` — **verbatim, unchanged format, unchanged tests** (FR-001) | none |
| Status menu | mode state items + Off action + Open Dashboard + Quit (FR-002) | mode actions per the mapping below |
| Dashboard window | hottest die temp, per-fan RPM **against each fan's own `min_rpm…max_rpm`**, current mode (FR-004) | same mode actions, identical behaviour |

Both surfaces render the same `SurfaceState` and the same `ModeAction` table —
there is no per-surface control logic.

## Rendering per SurfaceState

| `SurfaceState` | Status item title | Menu mode items | Dashboard readout | Dashboard controls |
|---|---|---|---|---|
| `Live(Status)` | formatted title | enabled; checkmark on the state item matching `status.mode` (FR-005) | full values; temp/RPM update every poll (1–2 s) | enabled |
| `ReadOnly(Status)` | formatted title (reads still work) | disabled with the one-line reason | live values continue | disabled with reason (FR-007) |
| `Unavailable` | unavailable presentation (no numbers) | disabled, hint shows CLI fallback | "fand unreachable"; **no stale numbers** (US3) | disabled with CLI fallback hint |

- The checkmark is never set from the click; it moves on the poll after the
  daemon reports the new mode (FR-005). Exactly one state item (Auto/Managed/
  Full) ever carries it; "Off" is an action item and never ticks (research D3).
- While a delegated command is in flight, the clicked control may show a
  transient pending affordance (disabled/…) but the authoritative state still
  comes only from the next `SurfaceState` (spec: "try one — watch the
  response, not the click").
- Dashboard window close ⇒ surfaces other than the dashboard keep running;
  reopen re-renders the current `SurfaceState` immediately (SC-003). The
  sparkline history is per-open and never restored.

## Action → command mapping (FR-003, research D3/D4)

| UI action | Delegated command | Elevated? |
|---|---|---|
| Auto | `topfan off` (with administrator privileges) | yes |
| Managed | `topfan auto` (with administrator privileges) | yes |
| Full | `topfan full` (with administrator privileges) | yes |
| Off | `topfan off` (with administrator privileges) | yes |
| Open Dashboard | local (show/focus window) | no |
| Quit | app exit | no |

Invariants (headless-tested in `actions.rs`):

1. The command set is exactly the four existing CLI verbs — the UI invents no
   command and no socket message (`SetMode` is never sent directly from the
   GUI process; the daemon would rightly reject it for non-root).
2. Every mode change from every surface uses the identical mapping — one
   behaviour, three access points (FR-003).
3. `fan_control_available == false` disables all mode actions in all surfaces.

## Degradation & recovery (FR-006, SC-004)

- Failure detection: any `Unreachable` poll outcome ⇒ the next render is
  `Unavailable` on both surfaces (within one 2 s cycle of the daemon stopping).
- Recovery: next `Reached` outcome ⇒ live rendering, no user action, no modal.
- Retry policy: fixed 2 s cadence (research D6). No unbounded spinning, no
  exponential backoff, no retry counter shown to the user.
- Elevation declined or CLI unavailable (see
  [cli-delegation.md](./cli-delegation.md)): state unchanged, one short
  non-alarming hint, no repeat prompting.

## Light/dark & accessibility (SC-005)

- The dashboard view (web) defines both palettes and follows the system
  appearance (`prefers-color-scheme`); the AppKit status item/menu follow the
  system via standard controls only (no hardcoded colors on the AppKit side).
- The status item and menu are built from standard AppKit surfaces — no custom
  drawing that could render illegibly against an unknown background.

## What this contract forbids

- No local optimistic state: no surface may display a mode, RPM, or temperature
  value that did not come from the last poll (FR-005).
- No second control path: no direct socket `SetMode` from the GUI, no extra
  message types over the daemon protocol (001 compatibility rule).
- No persistence: nothing is written to disk by the app (FR-010); the only
  user-domain artefact is the single-instance lock socket in `$TMPDIR`.
- No new `unsafe` in the UI layer (Constitution V).