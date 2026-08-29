//! Spike 0: does the legacy SMC protocol reach the fans on this machine?
//!
//!     cargo build && sudo ./target/debug/smc-probe
//!
//! Exit 0 means the fan path works and the project can proceed as designed.
//! Exit 2 means the SMC refused every key even as root, and the read path has
//! to be rebuilt around AppleSMCKeysEndpoint. See CLAUDE.md.

use smc::{hid::Thermals, smc::Key, FanControl, Smc};

fn main() -> std::process::ExitCode {
    let root = unsafe { libc_geteuid() } == 0;
    println!(
        "TopFanMac SMC probe -- running as {}\n",
        if root { "root" } else { "an ordinary user" }
    );

    println!("== thermal sensors (IOHIDEventSystem) ==");
    match Thermals::open() {
        Some(t) => {
            let all = t.read_all();
            let die: Vec<_> = all.iter().filter(|r| r.is_die()).collect();
            println!("  {} sensors, {} of them die sensors", all.len(), die.len());
            for r in die.iter().take(4) {
                println!("    {:<24} {:>6.2} C", r.name, r.celsius);
            }
            match t.hottest_die() {
                Some(c) => println!("  hottest die: {c:.2} C"),
                None => println!("  no usable reading"),
            }
        }
        None => println!("  FAILED to open IOHIDEventSystem client"),
    }

    println!("\n== fan control (AppleSMC) ==");
    let smc = match Smc::open() {
        Ok(s) => {
            println!("  IOServiceOpen: ok");
            s
        }
        Err(e) => {
            println!("  IOServiceOpen FAILED: {e}");
            return std::process::ExitCode::from(2);
        }
    };

    // #KEY is the key count and exists on every SMC ever shipped. If this one
    // fails, the protocol is wrong -- not the key.
    let sentinel = smc.read(Key::new(b"#KEY"));
    match &sentinel {
        Ok(v) => println!("  #KEY  -> {v:?}   (protocol works)"),
        Err(e) => println!("  #KEY  -> FAILED: {e}"),
    }

    match smc.fan_count() {
        Ok(n) => {
            println!("  FNum  -> {n} fans");
            for i in 0..n {
                match FanControl::fan(&smc, i) {
                    Ok(f) => println!(
                        "    fan {}: {:.0} rpm (target {:.0}, range {:.0}-{:.0}, {:?})",
                        f.index, f.actual_rpm, f.target_rpm, f.min_rpm, f.max_rpm, f.mode
                    ),
                    Err(e) => println!("    fan {i}: FAILED: {e}"),
                }
            }
            println!("\nSpike 0 PASSED -- the fan path is reachable.");
            std::process::ExitCode::SUCCESS
        }
        Err(e) => {
            println!("  FNum  -> FAILED: {e}");
            println!("\nSpike 0 FAILED.");
            if e.is_likely_privilege() && !root {
                println!("Re-run with sudo before concluding anything.");
            } else if root {
                println!(
                    "Refused even as root, so this is not a privilege problem:\n\
                     the legacy AppleSMC protocol does not serve keys on this machine.\n\
                     Next step is AppleSMCKeysEndpoint, not more of this protocol."
                );
            }
            std::process::ExitCode::from(2)
        }
    }
}

extern "C" {
    #[link_name = "geteuid"]
    fn libc_geteuid() -> u32;
}
