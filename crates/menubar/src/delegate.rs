//! CLI delegation (research D4 = 001 research D1, contracts/cli-delegation.md).
//!
//! Privileged mode changes run the *existing* `topfan` CLI under the system
//! admin-authorization prompt:
//!
//! ```text
//! /usr/bin/osascript -e 'do shell script "<topfan> <verb>" with administrator privileges'
//! ```
//!
//! Note where the quotes end: `with administrator privileges` is a parameter
//! of `do shell script`, not part of the command it runs.
//!
//! The app never sees a password and never touches the SMC. Everything in
//! this module except `run` is pure and headless-tested; `run` is the small
//! seam the UI wires in (no process execution in unit tests).

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// Where osascript lives, always (system path, not looked up).
pub const OSASCRIPT: &str = "/usr/bin/osascript";

/// Packaged location first, development tree second (contract "discovery").
pub fn default_topfan_candidates() -> Vec<PathBuf> {
    let dev = option_env!("CARGO_MANIFEST_DIR").map(|manifest| {
        // crates/menubar -> super -> workspace root
        PathBuf::from(manifest).join("../../target/release/topfan")
    });
    match dev {
        Some(dev) => vec![PathBuf::from("/usr/local/bin/topfan"), dev],
        None => vec![PathBuf::from("/usr/local/bin/topfan")],
    }
}

/// First existing candidate, or `None`. `None` means **no prompt is ever
/// raised** -- prompting with a command that cannot succeed would be a
/// dishonest surface (Constitution VI); the hint text below is shown instead.
pub fn find_topfan(candidates: &[PathBuf]) -> Option<PathBuf> {
    candidates
        .iter()
        .find(|p| p.exists() && p.is_file())
        .cloned()
}

/// Quote one word for the `/bin/sh` that `do shell script` runs, so a path
/// containing spaces stays a single argument.
fn sh_quote(word: &str) -> String {
    format!("'{}'", word.replace('\'', r"'\''"))
}

/// Wrap a string as an AppleScript `"..."` literal, escaping what AppleScript
/// treats specially inside one.
fn as_literal(text: &str) -> String {
    format!("\"{}\"", text.replace('\\', r"\\").replace('"', "\\\""))
}

/// The single osascript script line for one action. Uses the discovered
/// binary's full path: `do shell script` runs `/bin/sh`, whose PATH does not
/// include `/usr/local/bin`, so a bare `topfan` would find nothing.
///
/// `with administrator privileges` is a parameter of `do shell script`, so it
/// must sit **outside** the quoted command. Inside the quotes it is not an
/// AppleScript clause at all -- it becomes three more argv words handed to
/// `topfan`, which then runs unelevated, fails argument parsing, and never
/// raises an authorization prompt. That silent no-op is what this form exists
/// to prevent; `elevation_clause_is_outside_the_command` pins it.
pub fn cli_script(topfan: &Path, verb: &str) -> String {
    let command = format!("{} {verb}", sh_quote(&topfan.display().to_string()));
    format!(
        "do shell script {} with administrator privileges",
        as_literal(&command)
    )
}

/// The full argv handed to [`std::process::Command`].
pub fn cli_argv(topfan: &Path, verb: &str) -> Vec<String> {
    vec![OSASCRIPT.into(), "-e".into(), cli_script(topfan, verb)]
}

/// The fallback hint every non-applied outcome surfaces (one short
/// non-alarming line; no dialogs, no retry loops).
pub fn fallback_hint(verb: &str) -> String {
    format!("run `sudo topfan {verb}` from a terminal")
}

/// The total outcome set (contracts/cli-delegation.md table). Derived from
/// the child's exit status only -- the surfaces confirm nothing from these;
/// the next poll is the confirmation (honesty rules).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// Child exited 0. Nothing special -- the next poll shows the new mode.
    Applied,
    /// The user declined the prompt. A normal path, not an error path.
    Declined,
    /// Command failed (daemon down / SMC error / headless session).
    Failed,
    /// `topfan` was not found: no prompt, hint only.
    TopfanMissing,
    /// Killed by the 120 s safety-net timer.
    Hung,
}

/// How long a delegated command may run before the runner kills it: a
/// safety net so the UI can never hang on a stuck child (research D4).
pub const COMMAND_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(120);

/// Classify a finished/failed child. Pure; string-typed so tests cover the
/// outcome table without spawning anything.
///
/// `exit_code = None` means the child could not be spawned (or its status
/// was lost); osascript reports user cancellation as `-128` ("User
/// canceled") on stderr with a non-zero exit.
pub fn classify(exit_code: Option<i32>, stderr: &str, timed_out: bool) -> Outcome {
    if timed_out {
        return Outcome::Hung;
    }
    match exit_code {
        Some(0) => Outcome::Applied,
        Some(_) => {
            let declined = stderr.contains("User canceled")
                || stderr.contains("user canceled")
                || stderr.contains("-128");
            if declined {
                Outcome::Declined
            } else {
                Outcome::Failed
            }
        }
        None => Outcome::Failed,
    }
}

/// What the UI shows for each outcome. `None` = show nothing at all (Applied
/// confirms nothing; the next poll is the confirmation, honesty rules).
pub fn hint_for(outcome: Outcome, verb: &str) -> Option<String> {
    let hint = fallback_hint(verb);
    match outcome {
        Outcome::Applied => None,
        Outcome::Declined => Some(format!("mode change declined -- {hint}")),
        Outcome::Failed => Some(format!("mode change failed -- {hint}")),
        Outcome::TopfanMissing => Some(format!("topfan CLI not found -- {hint}")),
        Outcome::Hung => Some(format!("mode change timed out -- {hint}")),
    }
}

// ---------------------------------------------------------------------------
// Execution (T013). Never executed by unit tests -- this is the seam the UI
// wired through `spawn_delegation`, always from a background thread so the
// UI main loop is never blocked (contracts/cli-delegation.md).
// ---------------------------------------------------------------------------

/// One delegated `topfan <verb>` under the system admin prompt, bounded by
/// [`COMMAND_TIMEOUT`] (a hung child is killed rather than parking the UI).
pub fn run(topfan: &Path, verb: &str) -> Outcome {
    let argv = cli_argv(topfan, verb);
    let mut child = match Command::new(&argv[0])
        .args(&argv[1..])
        .stderr(Stdio::piped())
        .stdout(Stdio::null())
        .stdin(Stdio::null())
        .spawn()
    {
        Ok(c) => c,
        Err(_) => return Outcome::Failed,
    };

    // Piped stderr must be drained concurrently or a chatty child can fill
    // the pipe and block forever.
    let stderr_handle = child.stderr.take();
    let stderr_reader = std::thread::spawn(move || {
        let mut s = String::new();
        if let Some(mut h) = stderr_handle {
            use std::io::Read as _;
            let _ = h.read_to_string(&mut s);
        }
        s
    });

    // Bounded wait: poll try_wait() -- no extra timeout dependency, and the
    // child stays owned here so the safety-net kill is possible.
    let deadline = Instant::now() + COMMAND_TIMEOUT;
    let mut timed_out = false;
    let exit_code = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status.code(),
            Ok(None) if Instant::now() >= deadline => {
                timed_out = true;
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(50)),
            Err(_) => break None,
        }
    };

    let stderr = stderr_reader.join().unwrap_or_default();
    classify(exit_code, &stderr, timed_out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn osascript_command_line_construction() {
        let argv = cli_argv(Path::new("/usr/local/bin/topfan"), "full");
        assert_eq!(
            argv,
            [
                "/usr/bin/osascript",
                "-e",
                "do shell script \"'/usr/local/bin/topfan' full\" \
                 with administrator privileges"
            ]
        );
        // The script form embeds exactly one verb and the admin-privileges
        // clause; nothing else is ever run.
        let script = cli_script(Path::new("/usr/local/bin/topfan"), "auto");
        assert!(script.starts_with("do shell script "));
        assert!(
            script.contains(" auto\""),
            "bare verb, no mode words: {script}"
        );
        assert!(
            !script.contains("sudo "),
            "osascript does the elevation, not a shell sudo"
        );
    }

    /// The bug this pins (2026-09-03): `with administrator privileges` was
    /// built *inside* the quoted command, so AppleScript never saw a clause --
    /// it passed three extra words to `topfan`, which ran as the ordinary user,
    /// died on argument parsing, and raised no authorization prompt at all.
    /// Every menu mode click was therefore a silent no-op that only reached the
    /// surfaces as `Outcome::Failed`.
    #[test]
    fn elevation_clause_is_outside_the_command() {
        for verb in ["off", "auto", "full"] {
            let script = cli_script(Path::new("/usr/local/bin/topfan"), verb);
            assert!(
                script.ends_with("\" with administrator privileges"),
                "the clause must follow the closing quote: {script}"
            );
            // The quoted part is exactly the command: binary + one verb.
            let command = script
                .trim_start_matches("do shell script \"")
                .trim_end_matches("\" with administrator privileges");
            assert_eq!(command, format!("'/usr/local/bin/topfan' {verb}"));
            assert!(
                !command.contains("administrator"),
                "the clause must not become argv words: {command}"
            );
        }
    }

    /// A path with a space (or a quote) must stay one shell argument, or the
    /// elevated command silently mis-parses the same way.
    #[test]
    fn awkward_paths_stay_a_single_shell_argument() {
        let script = cli_script(Path::new("/Users/a b/My Tools/topfan"), "full");
        assert_eq!(
            script,
            "do shell script \"'/Users/a b/My Tools/topfan' full\" \
             with administrator privileges"
        );
        // A quote in the path is escaped for sh, and that escape's backslash
        // is itself escaped for the AppleScript literal -- so osascript hands
        // sh `'/tmp/it'\''s/topfan'`, which is one argument again.
        let quoted = cli_script(Path::new("/tmp/it's/topfan"), "off");
        assert_eq!(
            quoted,
            r#"do shell script "'/tmp/it'\\''s/topfan' off" with administrator privileges"#
        );
    }

    #[test]
    fn discovery_prefers_packaged_then_dev_then_missing() {
        // Injected candidates only -- no real filesystem calls beyond the
        // disposable temp files below.
        let id = std::process::id();
        let packaged = std::env::temp_dir().join(format!("topfan-test-packaged-{id}"));
        std::fs::write(&packaged, b"").unwrap();
        let dev = std::env::temp_dir().join(format!("topfan-test-dev-{id}"));
        std::fs::write(&dev, b"").unwrap();

        let cands = vec![packaged.clone(), dev.clone()];
        assert_eq!(
            find_topfan(&cands),
            Some(packaged),
            "packaged location wins"
        );
        assert_eq!(
            find_topfan(std::slice::from_ref(&dev)),
            Some(dev),
            "dev location second"
        );

        // Missing binary => discovery fails => no prompt raised (the runner
        // never spawns osascript; the UI shows the hint instead).
        let missing = std::env::temp_dir().join("topfan-test-does-not-exist");
        assert_eq!(find_topfan(&[missing]), None);

        // A directory, or an empty candidate list, is not a binary.
        assert_eq!(find_topfan(&[std::env::temp_dir()]), None);
        assert_eq!(find_topfan(&[]), None);
    }

    #[test]
    fn outcome_table_is_total() {
        // Exit 0 -> Applied; Applied surfaces nothing (honesty rules: the
        // next poll is the only confirmation).
        assert_eq!(classify(Some(0), "", false), Outcome::Applied);
        assert_eq!(hint_for(Outcome::Applied, "full"), None);

        // User declined the prompt (osascript: canceled, -128, non-zero exit).
        assert_eq!(
            classify(Some(1), "execution error: User canceled. (-128)", false),
            Outcome::Declined
        );
        // Command failed for real (daemon down / SMC error).
        assert_eq!(
            classify(Some(2), "Error: cannot reach the daemon", false),
            Outcome::Failed
        );
        // Headless/remote session: prompt cannot display, errors promptly --
        // same bucket as failed, never a hang.
        assert_eq!(
            classify(Some(1), "No user interaction allowed. (-1719)", false),
            Outcome::Failed
        );
        // Could not spawn at all.
        assert_eq!(classify(None, "", false), Outcome::Failed);
        // Safety-net timer fired (even a 0-exit child was killed).
        assert_eq!(classify(Some(0), "", true), Outcome::Hung);

        // Every non-applied outcome surfaces exactly one short hint with the
        // CLI fallback text, nothing alarming.
        for outcome in [
            Outcome::Declined,
            Outcome::Failed,
            Outcome::TopfanMissing,
            Outcome::Hung,
        ] {
            let hint = hint_for(outcome, "full").expect("tests call hint_for");
            assert!(
                hint.contains("sudo topfan full"),
                "CLI fallback in hint: {hint}"
            );
            assert!(
                !hint.contains("Error:"),
                "non-alarming, single line: {hint}"
            );
        }
    }
}
