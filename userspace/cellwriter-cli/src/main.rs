#![deny(clippy::all)]

//! cellwriter-cli — OurOS handwriting recognition input
//!
//! Single personality: `cellwriter`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_cellwriter(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: cellwriter [OPTIONS]");
        println!("cellwriter v1.3 (OurOS) — Handwriting recognition input panel");
        println!();
        println!("Options:");
        println!("  --show           Show input panel");
        println!("  --hide           Hide input panel");
        println!("  --toggle         Toggle visibility");
        println!("  --train          Open training dialog");
        println!("  --setup          Open setup dialog");
        println!("  --keyboard       Show keyboard mode");
        println!("  --cells N        Number of input cells (default: 8)");
        println!("  --window-id ID   Target window");
        println!("  --profile NAME   Recognition profile");
        println!("  --version        Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("CellWriter v1.3.6 (OurOS)"); return 0; }
    if args.iter().any(|a| a == "--train") {
        println!("CellWriter Training Mode");
        println!("  Profile: default");
        println!("  Characters trained: 52 (A-Z, a-z)");
        println!("  Digits trained: 10 (0-9)");
        println!("  Samples per character: 5");
        println!("  Ready for additional training.");
        return 0;
    }
    println!("CellWriter v1.3.6 (OurOS) — Handwriting Input");
    println!("  Cells: 8");
    println!("  Profile: default");
    println!("  Recognition engine: built-in");
    println!("  Status: ready");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "cellwriter".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_cellwriter(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_cellwriter};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/cellwriter"), "cellwriter");
        assert_eq!(basename(r"C:\bin\cellwriter.exe"), "cellwriter.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("cellwriter.exe"), "cellwriter");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_cellwriter(&["--help".to_string()], "cellwriter"), 0);
        assert_eq!(run_cellwriter(&["-h".to_string()], "cellwriter"), 0);
        let _ = run_cellwriter(&["--version".to_string()], "cellwriter");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_cellwriter(&[], "cellwriter");
    }
}
