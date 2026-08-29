# Phase 1 Data Model: TopFanMac completion features

There is no persistent store — the SMC is the state. This document describes
the in-memory/wire entities the four stories touch, their invariants, and the
state transitions. Wire formats live in [contracts/ipc-protocol.md](contracts/ipc-protocol.md).

## Entities

### Fan *(existing, read-only in this feature)*

| Field | Type | Source | Validation |
|---|---|---|---|
| `index` | u8 | 0..`FNum` | — |
| `actual_rpm` | f32 | `F#Ac` | informational truth |
| `target_rpm` | f32 | `F#Tg` | clamped to this fan's min/max on write |
| `min_rpm` | f32 | `F#Mn` | **per fan**; never shared, never constant |
| `max_rpm` | f32 | `F#Mx` | per fan |
| `mode` | FanMode | `F#Md` | Auto (0) / Forced (1) |

Invariants: `min_rpm <= actual_rpm <= max_rpm` *should* hold; code must defend
against it not holding (Principle III clamps by `clamp(min, max)` regardless).
On this machine fan 0 = 2317–6898, fan 1 = 2502–7450 — different ranges.

### Mode *(user-facing policy; existing)*

`Auto` (SMC runs its curve) | `Managed` (daemon follows governor duty) | `Full`
(per-fan max). Transitions in the daemon, single-threaded, last-writer-wins:

```text
any ──SetMode(Auto)──► Auto          any ──SetMode(Full)──> Forced@max
      (restore_all_to_auto)                (raise-only clamp still applies)
any ──SetMode(Managed)──> governor duty loop
wake ──> Auto (then Managed/Full resume on the next tick if user mode ≠ Auto)
SIGTERM/exit/uninstall ──> Auto   (signals.rs / startup-restore / uninstall order)
```

### Status *(wire; one small change)*

Unchanged fields (`mode`, `hottest_die_c`, `duty`, `fans[]`) plus the
tightened semantics of:

| Field | Old semantics | New semantics (FR-003/005) |
|---|---|---|
| `fan_control_available` | "write path unavailable" (always false — never proven) | `true` only after ≥ 1 successful **write + read-back match** this daemon process lifetime; any failed verify flips it back to `false` until a later write verifies |

Backwards compatible: same field, same type; no protocol version bump needed.

### WakeSignal *(new; in-memory only)*

Not a struct the client sees. A per-tick decision value produced by the pure
predicate, consumed by the daemon loop.

| Aspect | Value |
|---|---|
| Inputs | `gap_mono: Duration` (Instant delta of the tick), `gap_wall: Duration` (SystemTime delta of the tick) |
| Predicate | `wake_detected(gap_mono, gap_wall) := gap_wall >= Mono::from_secs(1) * 1.5 + WAKE_GAP_S` — i.e. wall advanced at least `WAKE_GAP_S` (30 s default) more than monotonic beyond normal scheduling jitter |
| Output | `bool` |
| On true | run startup sequence (restore_all_to_auto → sleep-recovery logging → re-baseline temperature), then resume tick |

Edge cases: first tick after start (no previous sample → never wake); manual
forward wall-clock step ≥30 s between ticks → spurious wake → harmless
auto-restore (documented cost of the heuristic); NTP slew → sub-second drift,
far below threshold.

### InstallTarget *(new; exists only as filesystem/launchd state, not in-process)*

| Artifact | Path | Managed by |
|---|---|---|
| daemon binary | `/usr/local/libexec/fand` | `topfan install` (copy) |
| plist | `/Library/LaunchDaemons/com.topfan.fand.plist` | `topfan install` |
| launchd service | `system/com.topfan.fand` | `launchctl bootstrap/bootout` |
| control socket | `/var/run/topfan.sock` | `fand` at runtime |

State model: `NotInstalled → Installed(Running) → Installed(Stopped) → NotInstalled`.
`install` from any state must converge to `Installed(Running)`; `uninstall` from
any state must converge to `NotInstalled` with fans in Auto **first**.

## State consistency rules (cross-cutting)

1. Daemon replies are authoritative: menu and CLI must render the `Response`,
   never their optimistic intent (Principle VI).
2. Every SMC write is paired with a read-back before its effect is reported
   (FR-005) — this is the only way `fan_control_available` stays honest.
3. Mode transitions and wake handling are serialised in the single control loop;
   there is no interleaving in which a tick can write a target without a
   `restore_all_to_auto()` having preceded it after wake.