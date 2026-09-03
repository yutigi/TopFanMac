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
collection of unrelated projects, not a monorepo. `TopFanMac/` is the repo root.

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
  menubar/   menu-bar app + desktop dashboard (AppKit via objc2)
             lib.rs      render_title + run()/headless() + the tests
             main.rs     thin binary -> menubar::run()
             client.rs   IPC poll/request against the daemon socket; set_mode
                         (the direct, promptless write path)
             state.rs    PollOutcome -> SurfaceState (Live/ReadOnly/Unavailable)
                         -> SurfaceSnapshot (the dashboard's JSON wire shape)
             actions.rs  the menu table: labels, CLI verbs, checkmark mapping
             delegate.rs the FALLBACK write path only: osascript -> `topfan
                         <verb>` with admin privileges; outcome classification,
                         120 s kill timer
             ui/mod.rs   NSApplication + NSStatusItem + timers + single-instance
                         lock socket ($TMPDIR/topfan-ui.lock)
             ui/menu.rs  NSMenu construction + state-driven update
             ui/dashboard.rs  NSWindow + WKWebView, one JS bridge
             assets/dashboard.html  the embedded dashboard page
src/main.rs   workspace-root shim binary (`topfan-mac`) so plain `cargo run`
              launches the menu-bar app; `default-members` keeps root-package
              `cargo build`/`cargo test` covering the whole workspace
packaging/com.topfan.fand.plist
```

`unsafe` outside `crates/smc` is confined to `fand`'s two documented exceptions
(`signals.rs`, `daemon.rs::peer_is_root`/`getpeereid`) and the menubar's objc2/WebKit
plumbing (`ui/mod.rs` class definition + `ui/menu.rs` target wiring + `ui/dashboard.rs`
object init/`evaluateJavaScript`), each documented at the call site. Everything else —
the governor, the whole menubar logic layer (`state.rs`, `actions.rs`, `delegate.rs`,
`client.rs`, `render_title`) — is safe and headless-tested. Do not let unsafe spread
further.

**The menu-bar app is built.** By default `menubar` is a real GUI: accessory-policy
NSApplication (no Dock icon), an NSStatusItem whose title is `render_title` verbatim,
an NSMenu (Auto/Managed/Full/Off + Open Dashboard + Quit), and a dashboard window
(NSWindow + WKWebView, page embedded via `include_str!`). `--headless` keeps the
original poll-and-print loop for scripting. Two deliberate constraints:

- **Presentation only.** All non-presentation logic lives in the pure, headless-tested
  modules listed above; the AppKit layer renders a `SurfaceState` and forwards clicks
  into an event queue. The UI never confirms a click locally — the checkmark (menu) and
  the labels (dashboard) move only on the next poll. Mode changes NEVER touch the SMC from
  the UI — they go to the daemon, on a background thread, in two steps (`ui::apply_mode`):
  first a direct `Request::SetMode` down the same socket, which the daemon accepts from the
  console user, so the normal path raises **no prompt at all**; only if that is refused *on
  authorization grounds* does it fall back to
  `/usr/bin/osascript -e 'do shell script "<topfan> <verb>" with administrator privileges'`
  (120 s kill timer). The fallback is for a pre-2026-09-03 root-only daemon or a non-console
  session, and is triggered by matching the substring `requires root` in the daemon's
  refusal — a wire contract pinned on both sides
  (`client::authorization_refusals_are_recognised_from_either_daemon`,
  `daemon::refusal_message_keeps_the_substring_the_ui_matches`). A declined/failed/missing-
  `topfan` outcome shows one short hint with the `sudo topfan` fallback and no dialogs or
  retries. `Action::mode()` (direct) and `Action::verb()` (fallback) are held in lockstep by
  `mode_and_verb_agree_for_every_action`.
- **Honest degradation.** Poll unreachable → Unavailable (no numbers, no stale data,
  dashboard sparkline cleared); `fan_control_available == false` → read-only (values
  continue, controls disabled with a one-line reason). Both recover automatically on the
  next successful poll.

One known deviation: **double-click on the status item does not open the dashboard** —
the only API path is the deprecated `popUpStatusItemMenu`, and the single-click native
menu already provides access. Everything is dashboard-openable via the menu item, direct
launch, or single-instance forward.

### Traps found during on-device validation

> **The poll budget is set by the daemon's shape, not its speed.** `fand` ticks, drains
> waiting clients with non-blocking `accept`, then sleeps `TICK` (1 s). A client that
> connects one instant after the drain is not looked at until that sleep ends, so its
> worst case is a whole `TICK` plus the next tick's sensor work — even though the reply
> itself costs microseconds. Measured here over 40 polls: median 198 ms, p90 235 ms,
> max **1071 ms**. Any client timeout at or below `TICK` therefore turns a *healthy*
> daemon into `Unreachable` on the tail polls, and the surfaces then faithfully render
> "fand unreachable" — the UI is honest, the input is wrong. A 500 ms budget flapped on
> 3 of 20 polls. `client::REQUEST_TIMEOUT` is now derived from `daemon::TICK` rather than
> picked, with tests pinning it above `TICK` and below the 2 s poll cadence.

> **`with administrator privileges` is a `do shell script` parameter, not text.**
> Written *inside* the quoted command --
> `do shell script "topfan full with administrator privileges"` -- AppleScript
> sees no clause at all: it runs `topfan full with administrator privileges` as
> the ordinary user, so three junk argv words reach clap, `topfan` exits 2, and
> **no authorization prompt is ever raised**. Every menu mode click was a silent
> no-op that surfaced only as the generic "mode change failed" hint. The clause
> must follow the closing quote (2026-09-03). `osacompile` accepts both forms,
> so syntax-checking does not catch it -- `elevation_clause_is_outside_the_command`
> in `delegate.rs` does.

> **`NSWindow` releases itself when closed.** A window built with
> `initWithContentRect:…` defaults to `releasedWhenClosed = YES`, so the close button
> drops a reference the `Retained<NSWindow>` in `Dashboard` still assumes it owns.
> Close-then-reopen was a use-after-free — **SIGSEGV, exit 139, and no crash report**,
> which reads like a mysterious silent quit. `dashboard.rs` now calls
> `setReleasedWhenClosed(false)` so the `Retained` is the sole owner, and
> `open_dashboard` rebuilds a *closed* dashboard instead of re-showing it (checked via
> `is_open()`), which is also what makes the page reload and the sparkline restart
> per-viewing as SC-003 requires.

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
0666 so `status` works without sudo; the daemon decides `SetMode` itself, accepting **root
or the console user** — whoever owns `/dev/console`, i.e. the person logged in at the
physical machine (widened 2026-09-03; it was root-only). With nobody at the console (SSH,
or the login window) that device belongs to root, so the policy collapses back to
root-only on its own.

That widening is what lets the menu bar change modes without an admin prompt per click,
and it is safe *only because* the invariants above hold: a mode change can only ever
**raise** a fan above what the SMC is already doing, is clamped to the hardware's own
`F0Mn`/`F0Mx`, and never touches throttling or power limits. The capability handed to a
local non-root process is therefore bounded by "make the fans loud, or hand them back to
macOS" — it cannot make the machine run hotter than stock. **If you ever relax invariant
1 or 2, revisit this policy first.** The pure decision is `daemon::is_authorized`, tested
by `authorization_policy`.

**The menubar app shares the daemon's thinnest-path discipline.** One target object
(`TopFanRoot`) is app delegate, menu delegate, timer target, and menu action target;
background events (delegation results, second-launch forwards) queue into a
`Mutex<VecDeque>` and are drained by a 250 ms run-loop timer, so AppKit objects are
never touched off the main thread. UI state lives in `RefCell`s behind
`MainThreadMarker`-guaranteed access, not in `Sync` types. Single-instance is a lock
socket at `$TMPDIR/topfan-ui.lock`: a second launch forwards `{"cmd":"open-dashboard"}`,
waits ≤ 2 s for the ack, and exits — there are never two status items.

## Commands

```sh
cargo build --release
cargo test                                    # 48 tests, no root, no hardware, no GUI
cargo test -p fand governor                   # just the control policy
cargo test -p fand does_not_chatter           # a single test
cargo clippy --all-targets -- -D warnings     # currently clean
cargo fmt

cargo run --bin smc-probe                     # what can we reach? (safe, read-only)
cargo run --example dump -p smc               # raw key values + encodings

cargo run                                     # the real menu-bar app (GUI); same as -p menubar
cargo run -p menubar                          # the real menu-bar app (GUI)
cargo run -p menubar -- --headless            # poll-and-print loop for scripting

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
