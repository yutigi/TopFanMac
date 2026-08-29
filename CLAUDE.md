# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

Fan control for this MacBook Pro, in Rust. Two user-facing behaviours:

- **auto** — a daemon samples die temperature and drives the fans ahead of the SMC's
  own curve, so the machine stays cool under sustained load instead of heat-soaking first.
- **full** — pin both fans to maximum on demand.

Three surfaces: a **root daemon** (`fand`) that owns the hardware, an unprivileged **CLI**
(`topfan`), and a **menu-bar app**. The daemon is the only thing that touches the SMC.

The parent `FreetimeProjects/CLAUDE.md` describes the surrounding workspace; it is a
collection of unrelated projects, not a monorepo. `TopFanMac/` is the repo root. Not yet
a git repo.

## Hardware facts (verified on this machine, 2026-08-29)

| | |
|---|---|
| Model | `Mac15,8` — MacBook Pro 14", Apple **M3 Max** |
| macOS | 26.5.1, build 25F80, SDK 26.5 |
| Toolchain | rustc/cargo 1.93.1, clang 21.0.0 |
| SMC | 2801 keys, `FNum` = **2 fans** |
| Fan 0 | 2317–6898 RPM |
| Fan 1 | 2502–7450 RPM |
| Sensors | 46 temperature services, 20 of them `tdie*` |

The two fans have **different RPM ranges**. Never hardcode fan bounds — read `F0Mn`/`F0Mx`
per fan. Code that assumes both fans are alike will over- or under-drive one of them.

## What works, and what is still unproven

**Reads work unprivileged.** The classic Intel-era SMC protocol
(`IOServiceMatching("AppleSMC")` + `IOConnectCallStructMethod(conn, 2, …)` with the
80-byte `SMCKeyData`) is alive and well on Apple Silicon. `cargo run --bin smc-probe`
prints live fan RPM as an ordinary user. Values come back as `flt ` (4-byte LE float),
not the Intel `fpe2`; `F0Md` is `ui8`. Both encodings are implemented since the type tag
arrives with every read.

**Writes are NOT yet verified.** Nothing in this project has ever written to the SMC. The
write path (`Smc::write`, `set_mode`, `set_target_rpm`) is implemented and typed but has
never executed against hardware. First run of `sudo topfan full` is the real test, and
should be done deliberately, watching `topfan status`, not incidentally.

> **The selector trap.** `kSMCReadKey = 5` and `kSMCGetKeyInfo = 9`. Several widely-copied
> gists have these swapped. Swapped selectors do not error usefully — every key, including
> `#KEY`, returns `result = 137`, which reads exactly like a permission problem and sends
> you hunting for a privilege or Apple-Silicon-compatibility issue that does not exist.
> If every key suddenly fails, check the selectors before anything else.

Note that macOS itself runs the fans in **forced** mode (`F0Md = 1`, target pinned to
`F0Mx`) under sustained load — observed here at load average 14. So `Md = 1` on startup
does **not** mean a previous `fand` left the fans stranded; it usually means the OS is
doing its job. Do not treat it as evidence of a crash.

## Safety invariants

This writes to thermal hardware on the user's only machine. These are not style rules.

1. **Auto is the fallback, always.** Every exit path must leave fans in SMC-managed auto
   mode (`F0Md = 0`). `Drop` does not run on `SIGKILL`, so the real guarantee is twofold:
   `KeepAlive` in the launchd plist, and `fand` calling `restore()` on *startup* before it
   does anything else. `signals.rs` handles SIGINT/SIGTERM; it is a convenience on top.
2. **Only ever raise, never lower.** `daemon.rs` clamps each target to at least the fan's
   current actual RPM. This tool exists to cool the machine harder than macOS would; it
   must never be able to cool it *less*.
3. **Respect `F0Mn`/`F0Mx`.** Clamp to the bounds the hardware reports, per fan.
4. **Never touch thermal throttling or power limits.** Fan speed only.
5. **Re-assert after wake.** Sleep resets SMC state. (Not yet implemented — the daemon
   does not currently subscribe to wake notifications.)

## Layout

```
crates/
  smc/       IOKit FFI + safe API. ALL unsafe lives here, except fand's two
             documented exceptions. ffi.rs declares the externs by hand.
    hid.rs     thermal sensors (IOHIDEventSystem) -- verified, unprivileged
    smc.rs     SMC key protocol (fan read/write) -- reads verified, writes not
    lib.rs     FanControl trait + MockFans
    bin/probe.rs   `smc-probe`: prints everything it can reach, exit 2 on failure
    examples/dump.rs   raw key dump for debugging encodings
  fand/      root daemon: governor (pure logic), IPC server, signal handling
  topfan/    unprivileged CLI client
  menubar/   status-bar app -- HEADLESS STAND-IN, see below
packaging/com.topfan.fand.plist
```

`unsafe` outside `crates/smc` is confined to two places in `fand`, both documented at the
call site: `signals.rs` (installing two handlers) and `daemon.rs::peer_is_root`
(`getpeereid`). Do not let it spread further — the governor must stay pure.

**The menu bar is not built yet.** `crates/menubar` is a working headless client that
polls the daemon and prints what the status item would show. `render_title` is pure and
tested. To finish: add `objc2` 0.6.4 + `objc2-app-kit` 0.3.2, make an `NSStatusItem`,
set its button title from `render_title`, hang an `NSMenu` with Auto/Full/Off items that
send the same `Request::SetMode`. Keep polling and formatting where they are — only
presentation should need AppKit.

## Architecture notes

**The governor is pure and that is deliberate.** `fand::governor` has no IOKit, no root,
no clock. It turns a temperature into a duty and nothing else, so the whole control policy
is testable off-device. Fan oscillation is the likeliest defect in a controller like this,
and finding it by melting a real M3 Max is an expensive way to find it. Keep new policy
in the governor, not in `daemon.rs`.

Response is deliberately **asymmetric**: duty rises the instant temperature calls for it,
but falls only after temperature drops `hysteresis_c` (default 4 °C) below the level that
last raised it. `does_not_chatter_across_a_breakpoint` pins this — a 2 °C flicker across a
curve breakpoint 200 times must produce at most one duty change.

**The daemon is single-threaded on purpose.** Clients are handled inline on the control
loop with a 250 ms timeout (well under the 1 s tick). This is why the raw IOKit handles
never need to be `Sync`. Reaching for `thread::spawn` per client forces `Sync` onto them
and is what an earlier draft got wrong — don't reintroduce it.

**Authorisation is by peer uid** (`getpeereid`), not by socket permissions. The socket is
0666 so `status` works without sudo; the daemon rejects `SetMode` from non-root itself.

## Commands

```sh
cargo build --release
cargo test                                    # 23 tests, no root, no hardware
cargo test -p fand governor                   # just the control policy
cargo test -p fand does_not_chatter           # a single test
cargo clippy --all-targets -- -D warnings     # currently clean
cargo fmt

cargo run --bin smc-probe                     # what can we reach? (safe, read-only)
cargo run --example dump -p smc               # raw key values + encodings

sudo ./target/release/fand managed            # run the daemon in the foreground
./target/release/topfan status                # unprivileged
sudo ./target/release/topfan full             # UNVERIFIED WRITE PATH -- see above
sudo ./target/release/topfan off              # hand back to macOS

sudo cp target/release/fand /usr/local/libexec/
sudo cp packaging/com.topfan.fand.plist /Library/LaunchDaemons/
sudo launchctl bootstrap system /Library/LaunchDaemons/com.topfan.fand.plist
sudo launchctl bootout system/com.topfan.fand
log stream --predicate 'process == "fand"'
```

## Testing without cooking the laptop

Hardware access sits behind the `FanControl` trait; `MockFans::two()` implements it with
plausible bounds. Test control policy against the mock and the curve directly — reserve
on-device runs for the thin FFI layer. `smc-probe` is read-only and safe to run any time.
