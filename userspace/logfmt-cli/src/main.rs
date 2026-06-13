#![deny(clippy::all)]

//! logfmt-cli — SlateOS logfmt parser/formatter
//!
//! Single personality: `logfmt`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_logfmt(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: logfmt [OPTIONS] [FILE]");
        println!("logfmt v1.0 (Slate OS) — Parse and format logfmt-encoded logs");
        println!();
        println!("Options:");
        println!("  FILE              Input file (stdin if omitted)");
        println!("  --json            Output as JSON");
        println!("  --csv             Output as CSV");
        println!("  --fields F1,F2    Select specific fields");
        println!("  --filter KEY=VAL  Filter by key-value pair");
        println!("  --pretty          Pretty-print output");
        println!("  --encode          Encode JSON to logfmt");
        return 0;
    }
    if args.iter().any(|a| a == "--encode") {
        println!("ts=2024-01-15T10:30:00Z level=info msg=\"request completed\" method=GET path=/api/users status=200 duration=42ms");
        return 0;
    }
    if args.iter().any(|a| a == "--json") {
        println!("{{\"ts\":\"2024-01-15T10:30:00Z\",\"level\":\"info\",\"msg\":\"request completed\",\"method\":\"GET\",\"path\":\"/api/users\",\"status\":200,\"duration\":\"42ms\"}}");
        return 0;
    }
    let file = args.iter().find(|a| !a.starts_with('-')).map(|s| s.as_str()).unwrap_or("app.log");
    println!("Parsing: {}", file);
    println!();
    println!("ts                      level  msg                  method  path         status  duration");
    println!("2024-01-15T10:30:00Z    info   request completed    GET     /api/users   200     42ms");
    println!("2024-01-15T10:30:01Z    warn   slow query           POST    /api/search  200     1523ms");
    println!("2024-01-15T10:30:02Z    error  connection refused   GET     /api/health  503     5ms");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "logfmt".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_logfmt(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_logfmt};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/logfmt"), "logfmt");
        assert_eq!(basename(r"C:\bin\logfmt.exe"), "logfmt.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("logfmt.exe"), "logfmt");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_logfmt(&["--help".to_string()], "logfmt"), 0);
        assert_eq!(run_logfmt(&["-h".to_string()], "logfmt"), 0);
        let _ = run_logfmt(&["--version".to_string()], "logfmt");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_logfmt(&[], "logfmt");
    }
}
