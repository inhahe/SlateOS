#![deny(clippy::all)]

//! op5monitor-cli — SlateOS OP5 Monitor
//!
//! Single personality: `op5`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_op5(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: op5 [COMMAND] [OPTIONS]");
        println!("OP5 Monitor v9.3 (Slate OS) — Network monitoring solution");
        println!();
        println!("Commands:");
        println!("  host list|add|del    Manage hosts");
        println!("  service list|add     Manage services");
        println!("  group list|add       Manage host/service groups");
        println!("  filter list|create   Manage saved filters");
        println!("  report generate      Generate reports");
        println!("  config change|save   Configuration management");
        println!("  status               Show monitoring overview");
        println!();
        println!("Options:");
        println!("  --url URL          OP5 server URL");
        println!("  --user USER        Username");
        println!("  --password PASS    Password");
        println!("  --format json|csv  Output format");
        println!("  --version          Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("OP5 Monitor v9.3.1 (Slate OS)"); return 0; }
    println!("OP5 Monitor v9.3.1 (Slate OS)");
    println!("  Hosts: 234 (220 up, 14 unreachable)");
    println!("  Services: 4,567 (4,123 ok, 234 warning, 210 critical)");
    println!("  Host groups: 15");
    println!("  Peers: 2 connected");
    println!("  SLA: 99.87% (30 day)");
    println!("  Last check: 3s ago");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "op5".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_op5(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_op5};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/op5monitor"), "op5monitor");
        assert_eq!(basename(r"C:\bin\op5monitor.exe"), "op5monitor.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("op5monitor.exe"), "op5monitor");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_op5(&["--help".to_string()], "op5monitor"), 0);
        assert_eq!(run_op5(&["-h".to_string()], "op5monitor"), 0);
        let _ = run_op5(&["--version".to_string()], "op5monitor");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_op5(&[], "op5monitor");
    }
}
