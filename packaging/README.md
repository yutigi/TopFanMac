# packaging/

Everything needed to turn `cargo build --release` output into a shippable
`.dmg`, plus the launchd plist the daemon runs from.

| File | Role |
|---|---|
| `Info.plist` | App bundle metadata. `__VERSION__` is substituted at build time. |
| `make-app.sh` | Assembles `dist/TopFan.app` from built binaries and signs it. |
| `make-dmg.sh` | Wraps the bundle in `dist/TopFan-<version>-arm64.dmg`. |
| `install-daemon.sh` | Installs the root LaunchDaemon. Ships inside the app. |
| `uninstall-daemon.sh` | Removes it and hands the fans back to macOS. |
| `com.topfan.fand.plist` | The LaunchDaemon definition. |

## Building the installer

```sh
cargo build --release
./packaging/make-app.sh --version 0.1.0
./packaging/make-dmg.sh --version 0.1.0
```

`.github/workflows/ci.yml` runs exactly these two scripts on every push and
uploads the result as a workflow artifact, so packaging breakage surfaces on the
commit that caused it rather than at tag time.

## Releasing

Releases are cut by tag. The tag and the `[workspace.package]` version in
`Cargo.toml` must agree — `release.yml` fails the build if they do not, because
otherwise the `.dmg` filename and the version inside `Info.plist` would disagree
with the release itself.

```sh
# bump [workspace.package] version in Cargo.toml first, then:
git commit -am "release 0.2.0"
git tag v0.2.0
git push origin main --tags
```

Signing is optional and additive. With no secrets set the workflow ad-hoc signs
the bundle and says so in the release notes. Setting `MACOS_CERT_P12`,
`MACOS_CERT_PASSWORD` and `MACOS_SIGN_IDENTITY` switches it to Developer ID;
adding `APPLE_ID`, `APPLE_TEAM_ID` and `APPLE_APP_PASSWORD` also notarizes and
staples. No other change is needed.

## Why the daemon is not installed by drag-and-drop

Dragging an app into `/Applications` cannot write `/Library/LaunchDaemons` or
`/usr/local/libexec`, and it cannot `launchctl bootstrap` anything. So the `.dmg`
carries the app *and* `install-daemon.sh`, which the user runs once.

This is not a workaround so much as a consequence of the design: the app is
unprivileged and only ever talks to the daemon over a socket. An app that could
install its own root helper silently would be a bigger capability than the whole
tool needs. Until the daemon is installed the app degrades to read-only, which is
the behaviour `state.rs` already implements for an unreachable daemon.

## Traps

> **`TopFan` and `topfan` are the same filename.** Stock macOS APFS is
> case-insensitive. An app bundle whose `CFBundleExecutable` is `TopFan` cannot
> also carry the CLI as `Contents/MacOS/topfan` — the second `install` silently
> overwrites the first, and the bundle ships with the *CLI* as its executable.
> It builds, signs, passes `codesign --verify --strict`, and then launches to
> print clap usage and exit, with no status item and no error. The bundle
> executable is therefore named `TopFanMenuBar`, and `make-app.sh` asserts both
> that three distinct files survive the copy and that the bundle executable still
> hashes equal to the `menubar` binary.

> **`com.topfan.fand.plist` must contain no XML comments.** launchd's parser
> fails with `Bootstrap failed: 5: Input/output error` on commented plists, even
> though `plutil -lint` accepts them. Keep prose in this README instead.

> **Copies off a downloaded `.dmg` inherit `com.apple.quarantine`,** and launchd
> will not run a quarantined daemon. `install-daemon.sh` strips the attribute
> from the installed copies.

The `KeepAlive` key in the plist is safety invariant 1: `Drop` does not run on
SIGKILL, but launchd restarts the daemon, and `fand` restores SMC auto mode on
startup before it does anything else.

## Manual install from a source checkout

```sh
sudo cp target/release/fand /usr/local/libexec/
sudo cp packaging/com.topfan.fand.plist /Library/LaunchDaemons/
sudo launchctl bootstrap system /Library/LaunchDaemons/com.topfan.fand.plist
```

`install-daemon.sh` also resolves a `target/release` payload, so
`sudo ./packaging/install-daemon.sh` does the same thing with the quarantine and
ownership handling included.
