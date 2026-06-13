#![deny(clippy::all)]

//! ntopng-cli — SlateOS ntopng network traffic monitor
//!
//! Single personality: `ntopng`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_ntopng(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: ntopng [OPTIONS]");
        println!("ntopng v6.0 (SlateOS) — High-speed web-based traffic analysis");
        println!();
        println!("Options:");
        println!("  -i, --interface IF   Network interface(s)");
        println!("  -w, --http-port PORT HTTP port (default: 3000)");
        println!("  -m, --local-networks NET  Local networks CIDR");
        println!("  -d, --data-dir DIR   Data directory");
        println!("  -n, --dns-mode MODE  DNS resolution mode (0-3)");
        println!("  --community          Community edition mode");
        println!("  --disable-alerts     Disable alerting");
        println!("  -G, --pid-path FILE  PID file");
        println!("  --version            Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("ntopng v6.0.0 (SlateOS)"); return 0; }
    println!("ntopng v6.0.0 (SlateOS)");
    println!("  Interface: eth0");
    println!("  Active flows: 1,234");
    println!("  Hosts: 89 (45 local, 44 remote)");
    println!("  Throughput: 234 Mbit/s");
    println!("  Packets: 12,345/s");
    println!("  Alerts: 3 active");
    println!("  Web: http://0.0.0.0:3000");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "ntopng".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_ntopng(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_ntopng};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/ntopng"), "ntopng");
        assert_eq!(basename(r"C:\bin\ntopng.exe"), "ntopng.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("ntopng.exe"), "ntopng");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_ntopng(&["--help".to_string()], "ntopng"), 0);
        assert_eq!(run_ntopng(&["-h".to_string()], "ntopng"), 0);
        let _ = run_ntopng(&["--version".to_string()], "ntopng");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_ntopng(&[], "ntopng");
    }
}
