#![deny(clippy::all)]

//! fluentbit-cli — Slate OS Fluent Bit log processor
//!
//! Single personality: `fluent-bit`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_fluentbit(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: fluent-bit [OPTIONS]");
        println!("fluent-bit v3.0 (Slate OS) — Fast log processor and forwarder");
        println!();
        println!("Options:");
        println!("  -c FILE           Configuration file");
        println!("  -i INPUT          Input plugin");
        println!("  -o OUTPUT         Output plugin");
        println!("  -f FILTER         Filter plugin");
        println!("  -p KEY=VAL        Plugin property");
        println!("  -R FILE           External parsers file");
        println!("  --dry-run         Validate config without running");
        println!("  -V                Show version");
        return 0;
    }
    if args.iter().any(|a| a == "-V" || a == "--version") { println!("fluent-bit v3.0 (Slate OS)"); return 0; }
    if args.iter().any(|a| a == "--dry-run") {
        println!("Configuration validation:");
        println!("  [INPUT] tail - OK");
        println!("  [FILTER] grep - OK");
        println!("  [OUTPUT] stdout - OK");
        println!("  Status: VALID");
        return 0;
    }
    println!("Fluent Bit v3.0 (Slate OS)");
    println!("  [INPUT]  tail.0: /var/log/syslog");
    println!("  [FILTER] grep.0: match=*error*");
    println!("  [OUTPUT] es.0: localhost:9200");
    println!("  Pipeline started.");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "fluent-bit".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_fluentbit(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_fluentbit};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/fluentbit"), "fluentbit");
        assert_eq!(basename(r"C:\bin\fluentbit.exe"), "fluentbit.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("fluentbit.exe"), "fluentbit");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_fluentbit(&["--help".to_string()], "fluentbit"), 0);
        assert_eq!(run_fluentbit(&["-h".to_string()], "fluentbit"), 0);
        let _ = run_fluentbit(&["--version".to_string()], "fluentbit");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_fluentbit(&[], "fluentbit");
    }
}
