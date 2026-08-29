# Contract: SMC write path (crates/smc)

The one place `unsafe` hardware mutation is allowed. Read side is verified;
write side has never executed — this contract governs its proof.

## Operations (`FanControl` trait, implemented by `Smc`)

| Operation | SMC keys | Semantics |
|---|---|---|
| `set_mode(i, FanMode)` | write `F#Md` | write `Md`'s natural encoding (`flo`/`ui8` by type tag), value `1.0`=Forced / `0.0`=Auto |
| `set_target_rpm(i, rpm)` | read `F#Mn`,`F#Mx`; write `F#Tg` | clamp **to that fan's own bounds** before write |
| `restore_all_to_auto()` | all fans `Md←0` | the safe state; called at daemon startup, on exit, on wake, and before uninstall bootout |

Internal command selectors (already in code, pinned here):
`SMC_CMD_READ_BYTES=5`, `SMC_CMD_READ_KEYINFO=9`, `SMC_CMD_WRITE_BYTES=6`,
transport `IOConnectCallStructMethod(conn, 2, 80-byte SMCKeyData…)`.

## Verification requirement (new — FR-005)

Every write is followed by a read-back of the same key within the same daemon
operation. Write is **not considered successful** unless the read-back equals
the value written (for `Md`: identical 0/1; for `Tg`: RPM within a small epsilon
— target is what the write carried, actual RPM lags and must NOT be compared).

Failure classification:

| Symptom | Meaning | Action |
|---|---|---|
| write returns IOResult ≠ 0 | call rejected (possibly by SMC policy) | error; `fan_control_available=false` |
| write ok, read-back ≠ value | SMC silently refused/overwrote (e.g. OS re-forcing) | treat as failed; restore Auto for that fan; error reply |
| every key (even `#KEY`) returns `result = 137` | the selector trap — selectors are wrong, *not* a permissions problem | fix selectors; do not chase privileges (CLAUDE.md warning) |

## Testing contract

- All governor/policy tests run against `MockFans` (`FanControl` impl) — no
  hardware, no root; `MockFans::two()` keeps two fans with distinct bounds and
  the mock records every write for assertion (extend if needed).
- The on-device proof (quickstart.md V1) exercises only this thin layer:
  probe → `full` → probe + `status` → `off` → probe. One session, observed,
  deliberate — then the result is recorded in `CLAUDE.md` hardware facts.

## Safety invariants restated (violation = revert the change)

1. Any operation that cannot complete verified leaves fans in Auto, not Forced.
2. Raise-only while driving; the single downward path is the explicit auto hand-back.
3. Bounds are per-fan, from hardware, never constants.
4. Nothing outside fan keys (`F#Md`, `F#Tg`) is written. Ever.