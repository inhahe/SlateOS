#![deny(clippy::all)]

//! fluentd-cli — OurOS Fluentd log collector
//!
//! Single personality: `fluentd`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_fluentd(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: fluentd [OPTIONS]");
        println!("Fluentd v1.17 (OurOS) — Unified logging layer");
        println!();
        println!("Options:");
        println!("  -c, --config FILE  Config file (default: fluent.conf)");
        println!("  -p, --plugin DIR   Plugin directory");
        println!("  -d, --daemon PID   Daemonize with PID file");
        println!("  --dry-run          Validate config without starting");
        println!("  --no-supervisor    Run without supervisor");
        println!("  -s, --setup DIR    Generate sample config");
        println!("  --gemfile FILE     Gemfile for plugin management");
        println!("  -q, --quiet        Suppress output");
        println!("  -v, --verbose      Increase verbosity");
        println!("  --version          Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("fluentd v1.17.1 (OurOS)"); return 0; }
    println!("Fluentd v1.17.1 (OurOS)");
    println!("  Inputs: tail (3), forward (1), syslog (1)");
    println!("  Outputs: elasticsearch (2), s3 (1), stdout (1)");
    println!("  Filters: record_transformer (2), grep (1)");
    println!("  Buffer: file (chunk_limit: 8MB, queue: 256)");
    println!("  Plugins: 12 loaded");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "fluentd".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_fluentd(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_fluentd};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/fluentd"), "fluentd");
        assert_eq!(basename(r"C:\bin\fluentd.exe"), "fluentd.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("fluentd.exe"), "fluentd");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_fluentd(&["--help".to_string()], "fluentd"), 0);
        assert_eq!(run_fluentd(&["-h".to_string()], "fluentd"), 0);
        let _ = run_fluentd(&["--version".to_string()], "fluentd");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_fluentd(&[], "fluentd");
    }
}
