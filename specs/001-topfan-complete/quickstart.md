# Quickstart: validating TopFanMac completion features

Prerequisites: `cargo build --release` from repo root; this is the verified
host (Mac15,8, M3 Max, macOS 26.x). Off-device validation needs nothing —
no root, no hardware. On-device validation is ordered so no fan is ever left
unmanaged (constitution principle I) and the first SMC write is deliberate.

## 0. Off-device (always safe, run first)

```sh
cargo test                                  # all unit + governor policy tests (23+ expected)
cargo test -p fand wake                     # new: wake predicate tests (pure)
cargo test -p menubar                       # render_title / action mapping (headless)
cargo clippy --all-targets -- -D warnings   # must stay clean
cargo fmt --check
```

Expected: green, including the new tests for FR-005 (write-back verification
against MockFans), the wake predicate (FR-007/009), and install step mapping.

## 1. Daemon reachable (safe — reads only)

```sh
./target/release/topfan status      # unprivileged: mode, die temp, per-fan RPM
cargo run --bin smc-probe           # read-only full picture, exit 0 expected
```

## 2. V1 — first deliberate write (Story 1) ⚠️ the real test

Purpose: prove writes; never done before on this hardware. Do this once, with
`topfan status` visible, not incidentally.

```sh
cargo run --bin smc-probe                              # (a) BEFORE state: RPM, Md, bounds
sudo ./target/release/fand managed &                   # (b) daemon in foreground-equivalent
./target/release/topfan status                         # (c) expect fan_control_available:false
sudo ./target/release/topfan full                      # (d) THE WRITE. Expect: ok per fan…
cargo run --bin smc-probe                              # (e) expect Md=1 for BOTH fans,
                                                       #     targets ≈ 6898 and 7450 (their own max)
./target/release/topfan status                         # (f) expect fan_control_available:true
sudo ./target/release/topfan off                       # (g) hand back: Md=0 on both, RPM falls
cargo run --bin smc-probe                              # (h) confirm auto restored
```

Pass criteria: (e) shows both fans Forced at *their own* reported maxima
(different values — principle III), (f) true, (h) both Md=0.
If every key returns `result = 137` or writes misbehave → selector trap /
contract `smc-write.md` failure table first, not permissions.
Record the verdict in `CLAUDE.md` hardware facts.

## 3. V2 — wake re-assertion (Story 2) — after V1 passes

```sh
sudo ./target/release/fand managed &
sudo ./target/release/topfan full          # put fans somewhere SMC wouldn't keep after sleep
# sleep the Mac ≥ 30 s (Apple menu → Sleep, or close lid), then wake
./target/release/topfan status             # expect: Md=Auto on both fans
log stream  --or--  tail /var/log/topfan.log   # expect wake-detected + restore lines
```

Pass criteria: within one tick (~1 s) of wake, `status` shows auto; the log
names the wake transition. Repeat 3× (SC-003). (Managed mode may immediately
re-assert after wake if the user mode is Managed — auto-restore must still be
visible in the log first.)

## 4. V3 — menu bar (Story 3) — GUI session required

```sh
sudo ./target/release/fand managed &
cargo run --release -p menubar             # real app, run unprivileged
```

Checklist: status item appears with a live title matching
`topfan status` → temperature/RPM changes move the title →
click Auto / Managed / Full / Off: each triggers the admin prompt
(D1 delegation) and the *next poll* reflects the daemon's authoritative state →
decline the prompt: state unchanged, fallback instruction shown →
`launchctl bootout system/com.topfan.fand`: title flips to daemon-down, app
survives, reconnects after `bootstrap`.

Pass criteria: FR-010..013 observable; no crash with daemon down.

## 5. V4 — install / uninstall (Story 4)

```sh
sudo ./target/release/topfan install       # copies binary+plist, bootstraps, verifies status
launchctl print system/com.topfan.fand     # expect state: running
sudo ./target/release/topfan uninstall     # fans to auto FIRST, then bootout, then rm
launchctl print system/com.topfan.fand     # expect not found
```

Pass criteria: install converges in one command (SC-005); uninstall leaves
`Md=Auto` verified before the service disappears; re-running either is safe
(idempotency, FR-016).

## Failure handling — never strand the fans

At any point, `sudo ./target/release/topfan off` returns control to the SMC;
if the daemon is wedged, `sudo launchctl bootout system/com.topfan.fand`
plus a manual `sudo topfan off` (or reboot) ends any forced state. The daemon's
startup-restore + launchd `KeepAlive` guarantee auto eventually, but "off" is
the immediate lever.