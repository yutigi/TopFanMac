# packaging/

Install steps (see `../CLAUDE.md` for the full description):

```sh
sudo mkdir -p /usr/local/libexec
sudo cp target/release/fand /usr/local/libexec/
sudo cp packaging/com.topfan.fand.plist /Library/LaunchDaemons/
sudo launchctl bootstrap system /Library/LaunchDaemons/com.topfan.fand.plist
```

**Note: `com.topfan.fand.plist` must contain no XML comments.** launchd's parser
fails with `Bootstrap failed: 5: Input/output error` on commented plists, even
though `plutil -lint` accepts them. Keep prose here in this README instead.

The KeepAlive key in the plist is safety invariant 1: `Drop` does not run on
SIGKILL, but launchd restarts the daemon, and `fand` restores SMC auto mode on
startup before it does anything else.