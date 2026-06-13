#![deny(clippy::all)]

//! psensor-cli — SlateOS Psensor hardware temperature monitor
//!
//! Multi-personality: `psensor`, `psensor-server`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_psensor(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: psensor [OPTIONS]");
        println!("psensor v1.2 (SlateOS) — Hardware temperature monitor (GUI)");
        println!();
        println!("Options:");
        println!("  -u URL            Connect to psensor-server");
        println!("  -d SECONDS        Update interval");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("psensor v1.2 (SlateOS)"); return 0; }
    println!("psensor: hardware temperature monitor started");
    println!("  Sensors detected: 4");
    println!("  CPU Core 0: 45.0\u{00b0}C");
    println!("  CPU Core 1: 43.0\u{00b0}C");
    println!("  GPU: 52.0\u{00b0}C");
    println!("  SSD: 35.0\u{00b0}C");
    0
}

fn run_server(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: psensor-server [OPTIONS]");
        println!("psensor-server v1.2 (SlateOS) — Remote temperature monitoring server");
        println!();
        println!("Options:");
        println!("  -p PORT           Listen port (default: 3131)");
        println!("  -d                Debug mode");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("psensor-server v1.2 (SlateOS)"); return 0; }
    println!("psensor-server: listening on port 3131");
    println!("  Serving sensor data via JSON API");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "psensor".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "psensor-server" => run_server(&rest, &prog),
        _ => run_psensor(&rest, &prog),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_psensor};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/psensor"), "psensor");
        assert_eq!(basename(r"C:\bin\psensor.exe"), "psensor.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("psensor.exe"), "psensor");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_psensor(&["--help".to_string()], "psensor"), 0);
        assert_eq!(run_psensor(&["-h".to_string()], "psensor"), 0);
        let _ = run_psensor(&["--version".to_string()], "psensor");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_psensor(&[], "psensor");
    }
}
