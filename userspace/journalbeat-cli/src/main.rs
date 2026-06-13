#![deny(clippy::all)]

//! journalbeat-cli — SlateOS Journalbeat log shipper
//!
//! Single personality: `journalbeat`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_journalbeat(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: journalbeat [OPTIONS]");
        println!("Journalbeat v8.14 (SlateOS) — Journal log shipper");
        println!();
        println!("Options:");
        println!("  -c, --config FILE     Config file");
        println!("  -e                    Log to stderr");
        println!("  --path.data DIR       Data directory");
        println!("  --path.logs DIR       Logs directory");
        println!("  --setup               Run initial setup");
        println!("  --strict.perms        Strict config permissions");
        println!("  test config           Test configuration");
        println!("  test output           Test output connectivity");
        println!("  --version             Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("journalbeat v8.14.3 (SlateOS)"); return 0; }
    println!("Journalbeat v8.14.3 (SlateOS)");
    println!("  Journal units: 45 monitored");
    println!("  Output: elasticsearch");
    println!("  Events/s: 234");
    println!("  Queue: memory (4096 events max)");
    println!("  Backoff: 1s");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "journalbeat".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_journalbeat(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_journalbeat};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/journalbeat"), "journalbeat");
        assert_eq!(basename(r"C:\bin\journalbeat.exe"), "journalbeat.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("journalbeat.exe"), "journalbeat");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_journalbeat(&["--help".to_string()], "journalbeat"), 0);
        assert_eq!(run_journalbeat(&["-h".to_string()], "journalbeat"), 0);
        let _ = run_journalbeat(&["--version".to_string()], "journalbeat");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_journalbeat(&[], "journalbeat");
    }
}
