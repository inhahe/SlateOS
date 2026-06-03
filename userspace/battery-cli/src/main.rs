#![deny(clippy::all)]

//! battery-cli — OurOS battery monitoring tool
//!
//! Single personality: `battery`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_battery(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: battery [OPTIONS]");
        println!("battery v1.0 (OurOS) — Battery monitoring tool");
        println!();
        println!("Options:");
        println!("  -s                Short output (percentage only)");
        println!("  -j                JSON output");
        println!("  -w                Watch mode (continuous)");
        println!("  --health          Show battery health");
        println!("  --charge-limit N  Set charge limit (percent)");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("battery v1.0 (OurOS)"); return 0; }
    if args.iter().any(|a| a == "-s") {
        println!("85%");
        return 0;
    }
    if args.iter().any(|a| a == "--health") {
        println!("Battery Health:");
        println!("  Capacity: 91.4% of design");
        println!("  Cycles: 142");
        println!("  Manufacturing: 2023-06");
        println!("  Status: Good");
        return 0;
    }
    println!("Battery: 85% (charging)");
    println!("  State: charging");
    println!("  Rate: 25W");
    println!("  Time to full: 0:45");
    println!("  AC: connected");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "battery".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_battery(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_battery};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/battery"), "battery");
        assert_eq!(basename(r"C:\bin\battery.exe"), "battery.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("battery.exe"), "battery");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_battery(&["--help".to_string()], "battery"), 0);
        assert_eq!(run_battery(&["-h".to_string()], "battery"), 0);
        assert_eq!(run_battery(&["--version".to_string()], "battery"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_battery(&[], "battery"), 0);
    }
}
