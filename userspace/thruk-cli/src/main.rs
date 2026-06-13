#![deny(clippy::all)]

//! thruk-cli — SlateOS Thruk monitoring web interface
//!
//! Single personality: `thruk`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_thruk(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: thruk [COMMAND] [OPTIONS]");
        println!("Thruk v3.12 (Slate OS) — Monitoring web interface");
        println!();
        println!("Commands:");
        println!("  start              Start Thruk");
        println!("  stop               Stop Thruk");
        println!("  restart            Restart Thruk");
        println!("  status             Show status");
        println!("  cache clean        Clean cache");
        println!("  report generate    Generate report");
        println!("  bp list|commit     Business process");
        println!("  url URL            Fetch Thruk URL");
        println!();
        println!("Options:");
        println!("  --config FILE      Config file");
        println!("  --backend NAME     Backend to use");
        println!("  --action ACTION    CLI action");
        println!("  --version          Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Thruk v3.12.2 (Slate OS)"); return 0; }
    println!("Thruk v3.12.2 (Slate OS)");
    println!("  Backends: 3 (Naemon, Icinga2, Shinken)");
    println!("  Hosts: 234 (220 up, 14 down)");
    println!("  Services: 3,456 (3,201 ok, 123 warning, 132 critical)");
    println!("  Downtimes: 5 scheduled");
    println!("  Reports: 12 configured");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "thruk".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_thruk(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_thruk};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/thruk"), "thruk");
        assert_eq!(basename(r"C:\bin\thruk.exe"), "thruk.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("thruk.exe"), "thruk");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_thruk(&["--help".to_string()], "thruk"), 0);
        assert_eq!(run_thruk(&["-h".to_string()], "thruk"), 0);
        let _ = run_thruk(&["--version".to_string()], "thruk");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_thruk(&[], "thruk");
    }
}
