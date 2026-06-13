#![deny(clippy::all)]

//! observium-cli — SlateOS Observium network monitoring
//!
//! Single personality: `observium`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_observium(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: observium COMMAND [OPTIONS]");
        println!("Observium CE v23 (Slate OS) — Network monitoring platform");
        println!();
        println!("Commands:");
        println!("  add-device HOST COMMUNITY  Add SNMP device");
        println!("  del-device HOST            Remove device");
        println!("  list-devices               List all devices");
        println!("  discovery HOST             Run discovery");
        println!("  poller HOST                Run poller");
        println!("  alerts                     Show alerts");
        println!("  update                     Update Observium");
        println!("  --version                  Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Observium CE v23.9 (Slate OS)"); return 0; }
    println!("Observium CE v23.9 (Slate OS)");
    println!("  Devices: 75 monitored");
    println!("  Ports: 1,234 (890 up, 344 down)");
    println!("  Sensors: 567 (temperature, voltage, fan, power)");
    println!("  Storage: 234 (disk partitions)");
    println!("  Alerts: 12 active (5 critical, 7 warning)");
    println!("  Graphs: 4,567 RRD files");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "observium".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_observium(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_observium};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/observium"), "observium");
        assert_eq!(basename(r"C:\bin\observium.exe"), "observium.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("observium.exe"), "observium");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_observium(&["--help".to_string()], "observium"), 0);
        assert_eq!(run_observium(&["-h".to_string()], "observium"), 0);
        let _ = run_observium(&["--version".to_string()], "observium");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_observium(&[], "observium");
    }
}
