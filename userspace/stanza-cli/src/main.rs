#![deny(clippy::all)]

//! stanza-cli — Slate OS Stanza log agent
//!
//! Single personality: `stanza`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_stanza(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: stanza [COMMAND] [OPTIONS]");
        println!("Stanza v0.34 (Slate OS) — High-performance log agent");
        println!();
        println!("Commands:");
        println!("  run                Start agent");
        println!("  offsets list       List file offsets");
        println!("  offsets clear      Clear file offsets");
        println!("  graph              Display pipeline graph");
        println!("  version            Show version");
        println!();
        println!("Options:");
        println!("  --config FILE      Config file");
        println!("  --database DIR     Offset database directory");
        println!("  --plugin-dir DIR   Plugin directory");
        println!("  --log-file FILE    Log file");
        println!("  --debug            Enable debug logging");
        return 0;
    }
    if args.iter().any(|a| a == "--version" || a == "version") { println!("stanza v0.34.2 (Slate OS)"); return 0; }
    println!("Stanza v0.34.2 (Slate OS)");
    println!("  Operators: 8 active");
    println!("  File inputs: 15 monitored");
    println!("  Journald inputs: 1");
    println!("  Output: OTLP (gRPC)");
    println!("  Entries/s: 5,432");
    println!("  Errors: 0");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "stanza".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_stanza(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_stanza};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/stanza"), "stanza");
        assert_eq!(basename(r"C:\bin\stanza.exe"), "stanza.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("stanza.exe"), "stanza");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_stanza(&["--help".to_string()], "stanza"), 0);
        assert_eq!(run_stanza(&["-h".to_string()], "stanza"), 0);
        let _ = run_stanza(&["--version".to_string()], "stanza");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_stanza(&[], "stanza");
    }
}
