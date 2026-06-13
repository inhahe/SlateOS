#![deny(clippy::all)]

//! lapce-cli — Slate OS Lapce code editor
//!
//! Single personality: `lapce`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_lapce(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: lapce [OPTIONS] [PATH...]");
        println!("Lapce 0.4.2 (Slate OS) — Lightning-fast and powerful code editor");
        println!();
        println!("Options:");
        println!("  -n, --new              New window");
        println!("  -w, --wait             Wait for window to close");
        println!("  --config-dir DIR       Config directory");
        println!("  --data-dir DIR         Data directory");
        println!("  -V, --version          Show version");
        println!();
        println!("Arguments:");
        println!("  [PATH...]   Files or directories to open");
        return 0;
    }
    if args.iter().any(|a| a == "-V" || a == "--version") {
        println!("Lapce 0.4.2 (Slate OS)");
        return 0;
    }
    let paths: Vec<&str> = args.iter()
        .filter(|a| !a.starts_with('-'))
        .map(|s| s.as_str())
        .collect();
    if paths.is_empty() {
        println!("lapce: Opening welcome tab...");
    } else {
        for p in &paths {
            println!("lapce: Opening '{}'", p);
        }
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "lapce".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_lapce(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_lapce};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/lapce"), "lapce");
        assert_eq!(basename(r"C:\bin\lapce.exe"), "lapce.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("lapce.exe"), "lapce");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_lapce(&["--help".to_string()], "lapce"), 0);
        assert_eq!(run_lapce(&["-h".to_string()], "lapce"), 0);
        let _ = run_lapce(&["--version".to_string()], "lapce");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_lapce(&[], "lapce");
    }
}
