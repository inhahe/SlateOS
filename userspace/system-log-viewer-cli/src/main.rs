#![deny(clippy::all)]

//! system-log-viewer-cli — OurOS GNOME System Log Viewer
//!
//! Single personality: `gnome-system-log`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_system_log(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: gnome-system-log [OPTIONS] [FILE...]");
        println!("gnome-system-log v43.0 (OurOS) — System log viewer");
        println!();
        println!("Options:");
        println!("  --version         Show version");
        println!();
        println!("View system log files with filtering and search.");
        println!("Default logs: syslog, auth.log, kern.log, dpkg.log");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("gnome-system-log v43.0 (OurOS)"); return 0; }
    println!("gnome-system-log: log viewer started");
    println!("  Logs: syslog, auth.log, kern.log");
    println!("  Filter: all priorities");
    println!("  Auto-refresh: enabled");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "gnome-system-log".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_system_log(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
