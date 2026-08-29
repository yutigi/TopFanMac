//! `topfan` -- the CLI client.
//!
//! `status` works unprivileged. Mode changes need root, and the daemon checks
//! that by peer uid rather than trusting us.

use clap::{Parser, Subcommand};
use fand::governor::Mode;
use fand::proto::{Request, Response, Status, SOCKET_PATH};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;

#[derive(Parser)]
#[command(name = "topfan", about = "Fan control for Apple Silicon MacBooks")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Show temperature, fan speeds, and current mode.
    Status,
    /// Follow the temperature curve, ahead of the SMC (needs root).
    Auto,
    /// Pin both fans to maximum (needs root).
    Full,
    /// Hand the fans back to macOS (needs root).
    Off,
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let request = match cli.command {
        Command::Status => Request::Status,
        Command::Auto => Request::SetMode {
            mode: Mode::Managed,
        },
        Command::Full => Request::SetMode { mode: Mode::Full },
        Command::Off => Request::SetMode { mode: Mode::Auto },
    };

    let stream = UnixStream::connect(SOCKET_PATH).map_err(|e| {
        anyhow::anyhow!(
            "cannot reach the daemon at {SOCKET_PATH} ({e}).\n\
             Is it running?  sudo launchctl bootstrap system \
             /Library/LaunchDaemons/com.topfan.fand.plist"
        )
    })?;

    let mut writer = stream.try_clone()?;
    let mut line = serde_json::to_string(&request)?;
    line.push('\n');
    writer.write_all(line.as_bytes())?;
    writer.flush()?;

    let mut reader = BufReader::new(stream);
    let mut response = String::new();
    reader.read_line(&mut response)?;

    match serde_json::from_str::<Response>(&response)? {
        Response::Status(s) => print_status(&s),
        Response::Ok => println!("ok"),
        Response::Error { message } => {
            eprintln!("topfan: {message}");
            std::process::exit(1);
        }
    }
    Ok(())
}

fn print_status(s: &Status) {
    match s.hottest_die_c {
        Some(c) => println!("temp   {c:.1} C (hottest die)"),
        None => println!("temp   unavailable"),
    }
    println!("mode   {:?}", s.mode);
    println!("duty   {:.0}%", s.duty * 100.0);
    if !s.fan_control_available {
        println!("\nfan control UNAVAILABLE -- temperatures only.");
        println!("Run `sudo smc-probe` to see why (see CLAUDE.md, Spike 0).");
        return;
    }
    for f in &s.fans {
        println!(
            "fan {}  {:>5.0} rpm  target {:>5.0}  [{:>5.0}-{:<5.0}]  {:?}",
            f.index, f.actual_rpm, f.target_rpm, f.min_rpm, f.max_rpm, f.mode
        );
    }
}
