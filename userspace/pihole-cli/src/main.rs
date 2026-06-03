#![deny(clippy::all)]

//! pihole-cli — OurOS Pi-hole DNS sinkhole
//!
//! Single personality: `pihole`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_pihole(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: pihole COMMAND [OPTIONS]");
        println!("Pi-hole v5.18 (OurOS) — Network-wide ad blocking");
        println!();
        println!("Commands:");
        println!("  status          Show blocking status");
        println!("  enable          Enable blocking");
        println!("  disable [TIME]  Disable blocking (optionally for TIME seconds)");
        println!("  restartdns      Restart DNS resolver");
        println!("  -g              Update gravity (blocklists)");
        println!("  -q DOMAIN       Query blocklists for domain");
        println!("  -t              Tail the pihole log");
        println!("  -c              Chronometer (stats dashboard)");
        println!("  -w DOMAIN       Whitelist domain");
        println!("  -b DOMAIN       Blacklist domain");
        println!("  --version       Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Pi-hole v5.18.2 (OurOS)"); return 0; }
    println!("Pi-hole v5.18.2 (OurOS)");
    println!("  Status: enabled");
    println!("  Domains on blocklist: 234,567");
    println!("  DNS queries today: 45,678");
    println!("  Queries blocked: 12,345 (27.0%)");
    println!("  Clients: 15");
    println!("  Top blocked: ads.example.com (1,234 hits)");
    println!("  Upstream DNS: 1.1.1.1, 9.9.9.9");
    println!("  FTL DNS: running (PID 1234)");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "pihole".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_pihole(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_pihole};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/pihole"), "pihole");
        assert_eq!(basename(r"C:\bin\pihole.exe"), "pihole.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("pihole.exe"), "pihole");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_pihole(&["--help".to_string()], "pihole"), 0);
        assert_eq!(run_pihole(&["-h".to_string()], "pihole"), 0);
        assert_eq!(run_pihole(&["--version".to_string()], "pihole"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_pihole(&[], "pihole"), 0);
    }
}
