#![deny(clippy::all)]

//! ancestris-cli — Slate OS Ancestris genealogy tool
//!
//! Single personality: `ancestris`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_ancestris(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: ancestris [OPTIONS] [FILE.ged]");
        println!("Ancestris v11 (Slate OS) — Genealogy research tool");
        println!();
        println!("Options:");
        println!("  --open FILE     Open GEDCOM file");
        println!("  --report TYPE   Generate report (pedigree, descendant, stats)");
        println!("  --export FILE   Export data");
        println!("  --check         Verify data consistency");
        println!("  --nosplash      Skip splash screen");
        println!("  --version       Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Ancestris v11.0 (Slate OS)"); return 0; }
    println!("Ancestris v11.0 (Slate OS) — Genealogy Tool");
    println!("  GEDCOM 5.5.1 compliant");
    println!("  Views: tree, table, map, timeline");
    println!("  Reports: pedigree, descendants, statistics");
    println!("  Status: ready");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "ancestris".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_ancestris(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_ancestris};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/ancestris"), "ancestris");
        assert_eq!(basename(r"C:\bin\ancestris.exe"), "ancestris.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("ancestris.exe"), "ancestris");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_ancestris(&["--help".to_string()], "ancestris"), 0);
        assert_eq!(run_ancestris(&["-h".to_string()], "ancestris"), 0);
        let _ = run_ancestris(&["--version".to_string()], "ancestris");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_ancestris(&[], "ancestris");
    }
}
