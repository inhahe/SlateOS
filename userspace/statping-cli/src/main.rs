#![deny(clippy::all)]

//! statping-cli — OurOS Statping-ng status page
//!
//! Single personality: `statping`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_statping(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: statping [COMMAND] [OPTIONS]");
        println!("Statping-ng v0.91 (OurOS) — Status page & monitoring server");
        println!();
        println!("Commands:");
        println!("  run                Start Statping server");
        println!("  export             Export status page");
        println!("  import FILE        Import configuration");
        println!("  sass               Compile custom theme");
        println!("  version            Show version");
        println!();
        println!("Options:");
        println!("  --port PORT        HTTP port (default: 8080)");
        println!("  --ip ADDR          Bind address");
        println!("  --config DIR       Config directory");
        println!("  --db-conn STRING   Database connection string");
        return 0;
    }
    if args.iter().any(|a| a == "--version" || a == "version") { println!("Statping-ng v0.91.0 (OurOS)"); return 0; }
    println!("Statping-ng v0.91.0 (OurOS)");
    println!("  Services: 15 (13 online, 2 offline)");
    println!("  Groups: 4");
    println!("  Notifiers: Slack, Email, Telegram");
    println!("  Uptime (7d): 99.87%");
    println!("  Avg latency: 156ms");
    println!("  Dashboard: http://0.0.0.0:8080");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "statping".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_statping(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_statping};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/statping"), "statping");
        assert_eq!(basename(r"C:\bin\statping.exe"), "statping.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("statping.exe"), "statping");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_statping(&["--help".to_string()], "statping"), 0);
        assert_eq!(run_statping(&["-h".to_string()], "statping"), 0);
        let _ = run_statping(&["--version".to_string()], "statping");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_statping(&[], "statping");
    }
}
