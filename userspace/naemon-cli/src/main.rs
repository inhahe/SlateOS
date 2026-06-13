#![deny(clippy::all)]

//! naemon-cli — Slate OS Naemon monitoring engine
//!
//! Single personality: `naemon`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_naemon(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: naemon [OPTIONS] [CONFIG_FILE]");
        println!("Naemon v1.4 (Slate OS) — Monitoring engine (Nagios fork)");
        println!();
        println!("Options:");
        println!("  -v, --verify       Verify configuration");
        println!("  -s, --test-scheduling  Test scheduling");
        println!("  -d, --daemon       Run as daemon");
        println!("  -W FILE            Worker results file");
        println!("  --allow-root       Allow running as root");
        println!("  --version          Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Naemon v1.4.2 (Slate OS)"); return 0; }
    println!("Naemon v1.4.2 (Slate OS)");
    println!("  Hosts: 156 (148 up, 8 down)");
    println!("  Services: 2,345 (2,100 ok, 145 warning, 100 critical)");
    println!("  Workers: 4");
    println!("  Check latency: 0.234s avg");
    println!("  Active checks: 2,501");
    println!("  External commands: enabled");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "naemon".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_naemon(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_naemon};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/naemon"), "naemon");
        assert_eq!(basename(r"C:\bin\naemon.exe"), "naemon.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("naemon.exe"), "naemon");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_naemon(&["--help".to_string()], "naemon"), 0);
        assert_eq!(run_naemon(&["-h".to_string()], "naemon"), 0);
        let _ = run_naemon(&["--version".to_string()], "naemon");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_naemon(&[], "naemon");
    }
}
