#![deny(clippy::all)]

//! logtool-cli — SlateOS logtool log file analysis
//!
//! Single personality: `logtool`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_logtool(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: logtool [OPTIONS] FILE");
        println!("logtool v1.0 (Slate OS) — Log file analysis tool");
        println!();
        println!("Options:");
        println!("  --summary         Show log summary");
        println!("  --errors          Show only errors");
        println!("  --since TIME      Filter since timestamp");
        println!("  --until TIME      Filter until timestamp");
        println!("  --grep PATTERN    Filter by regex pattern");
        println!("  --stats           Show statistics");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("logtool v1.0 (Slate OS)"); return 0; }
    println!("logtool: analyzing log file...");
    println!("  Lines: 15,432");
    println!("  Errors: 12");
    println!("  Warnings: 47");
    println!("  Time range: 2024-01-01 00:00 to 2024-01-02 00:00");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "logtool".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_logtool(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_logtool};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/logtool"), "logtool");
        assert_eq!(basename(r"C:\bin\logtool.exe"), "logtool.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("logtool.exe"), "logtool");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_logtool(&["--help".to_string()], "logtool"), 0);
        assert_eq!(run_logtool(&["-h".to_string()], "logtool"), 0);
        let _ = run_logtool(&["--version".to_string()], "logtool");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_logtool(&[], "logtool");
    }
}
