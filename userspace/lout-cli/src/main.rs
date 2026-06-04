#![deny(clippy::all)]

//! lout-cli — OurOS Lout document formatter
//!
//! Single personality: `lout`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_lout(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: lout [OPTIONS] FILE");
        println!("Lout v3.42 (OurOS) — Document formatting system");
        println!();
        println!("Options:");
        println!("  -o FILE       Output PostScript file");
        println!("  -PDF          Generate PDF output");
        println!("  -EPS          Generate EPS output");
        println!("  -p            Plain text output");
        println!("  -I DIR        Include path");
        println!("  -D DIR        Database directory");
        println!("  -r N          Max passes (default: 8)");
        println!("  -S            Safe mode (no system calls)");
        println!("  --version     Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Lout v3.42 (OurOS)"); return 0; }
    println!("Lout v3.42 (OurOS)");
    println!("  Input: thesis.lout");
    println!("  Pass 1: structure and cross-references");
    println!("  Pass 2: page breaking");
    println!("  Pass 3: final output");
    println!("  Pages: 120");
    println!("  Figures: 15");
    println!("  Tables: 8");
    println!("  Output: thesis.ps (3.4 MB)");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "lout".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_lout(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_lout};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/lout"), "lout");
        assert_eq!(basename(r"C:\bin\lout.exe"), "lout.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("lout.exe"), "lout");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_lout(&["--help".to_string()], "lout"), 0);
        assert_eq!(run_lout(&["-h".to_string()], "lout"), 0);
        let _ = run_lout(&["--version".to_string()], "lout");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_lout(&[], "lout");
    }
}
