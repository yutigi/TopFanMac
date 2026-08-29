use fand::governor::Mode;

fn main() -> anyhow::Result<()> {
    // Started by launchd in Managed mode; the CLI switches it at runtime.
    let mode = match std::env::args().nth(1).as_deref() {
        Some("full") => Mode::Full,
        Some("auto") => Mode::Auto,
        _ => Mode::Managed,
    };
    fand::daemon::run(mode)
}
